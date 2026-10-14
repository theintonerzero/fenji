#!/usr/bin/env python3
"""Driver watchdog for scripts/spike/run-client.sh (M1 Task 9; first used locally in M0's runs 5–9).

Host side, best effort. While a sandboxed run is active it polls about every 0.5 s. At the first
sign of driver activity it freezes every process in the sandbox (SIGSTOP), records what it saw,
kills the whole sandbox (SIGKILL: the PID namespace's init, every member, then bwrap) and exits 3.
That is a gate event: stop all runs and report BLOCKED. It never reads or analyses a driver file
beyond stat() and a SHA-256 for identification.

  watchdog.py baseline --root S --out OUT [--prefix PFX]
      Snapshot of every *.sys under S and of the service keys in PFX/system.reg (default: the
      prefix at S/compat/pfx) -> OUT/watchdog-baseline.json.
  watchdog.py watch --root S --out OUT --sandbox-pid PID --log FILE [--log FILE] [--prefix PFX]
      [--interval 0.5]
      PID is the isolated-run.sh wrapper started in the background (it execs bwrap). Once the
      sandbox's PID namespace exists the watchdog arms itself and creates OUT/.go, which the command
      inside waits for before it starts the client. Exit 0: the sandbox ended, no sign seen.
      Exit 3: gate event. Exit 4: the watchdog itself failed (the sandbox is killed then too).
      --log names each client's Wine log, once per client, in the order the clients start (M4 world
      plan, Amendment B: the runner starts them in that order, at least 2 s apart); two naming one
      file are refused.
  watchdog.py watch ... [--go FILE]
      FILE is the start signal (default OUT/.go); it is created exclusively and never through a link.
  watchdog.py watch ... [--tmpfs DIR]... [--bind PATH]...
      The sandbox's own tmpfs mounts S1 scans (default: isolated-run.sh's, and the root's own
      entries), and the paths bound into the sandbox, which that scan skips. run-bare.sh names its
      namespace's /run, /tmp and /dev/shm, and what it binds into them from the host.
  watchdog.py collect --src DIR --dest DIR [--name N]... [--exclude N]...
      Copies DIR's regular files into DEST without following any link (links and special files are
      listed in DEST/collect-notes.txt): how the host takes evidence from directories the sandbox
      can write.
  watchdog.py scanlog --root S --out OUT FILE...
      Runs only the log rules over finished logs (to test them against r1-r4), prints the hits.

Clients: every gujianol.exe in the namespace is a client, named at first sight or later (#142);
each poll reads again the name of every live process not yet known as one. Clients are numbered
by their start, and client n's log is the n-th --log. A client beyond the logs given has no log of
its own: exit 4.

Signs (any one triggers):
  S1 a *.sys under S that isn't in the baseline, or a baseline one whose identity changed: a regular
     file's size, mtime or inode, or a symlink's target or the target file's size, mtime or inode
     (sys_identity; a link merely re-created by Proton's prefix re-sync is not a change); any *.sys
     in the sandbox's own tmpfs mounts (--tmpfs), seen through /proc/<pid>/root of a sandbox
     process, bar the paths bound into them (--bind).
  S2 a winedevice.exe process in the sandbox's PID namespace that started after the earliest
     client (gujianol.exe) started, whichever client's start brought it. Those started before it
     host Wine's own drivers (the baseline).
  S3 in the Wine log, read incrementally: a .sys module (any letter case) loaded native, not as a
     Wine builtin; a native module loaded from a drivers directory or into a winedevice.exe process
     (Wine's builtin drivers and their imports are all builtin); a resource lookup of the named type
     "PE" (the embedded driver's type, PE/144; needs +resource); Nt/ZwLoadDriver for a service that
     isn't in the baseline, or for a baseline service that isn't of a driver type (discord-bridge);
     a service key created by services.exe that isn't in the baseline (needs +reg, r1-r6);
     CreateService for a service that isn't in the baseline, or for a baseline service with an image
     path other than its baseline one or none readable (needs +service, r7 on); ChangeServiceConfigW
     (needs +service; issue #202) that sets a type other than a user-mode one or a driver image, or
     none readable, or that changes the type or image while its process has opened (OpenServiceW,
     CreateServiceW) a baseline driver service, a service that isn't a user-mode baseline one, or
     none the log names (sechost's line has the service's handle, not its name); services.exe's
     svcctl_ChangeServiceConfigW, which logs only its name, with no ChangeServiceConfigW before it
     (a caller the log doesn't trace); the strings ProcessProtect, AGTDriver or AurogonGameTrust.
     The logs are scanned as one stream with one rule state (Amendment B): the prefix's one
     wineserver has processes writing to each log (services.exe, which client 1's session starts,
     answers client 2's calls in log 1), so the pending changes, the services each Wine pid opened,
     services.exe's and winedevice.exe's pids and the earliest client creation in any log are
     shared. Lines are scanned in the order of Wine's timestamps, which every Wine process takes
     from one clock: a line waits until each other log has shown a later one, or was read to its
     end in a later poll, and holds back nothing of its own that is earlier. A log's lines keep their
     order, a line without a stamp goes with the line before it, and at equal stamps services.exe's
     svcctl line goes after the caller's, which it answers. A split LoadDriver message is completed
     from its own log only.
  S4 the prefix's system.reg (--prefix; re-read when it changes; Wine saves it periodically and at
     exit): a new service key of a driver type (kernel or file system) or with a driver image, or a
     baseline service whose type or image path changed, or a system.reg that is no longer a regular
     file.
  Fail safe (exit 4, the sandbox killed): client n was found 20 s ago, whether or not it still
  runs, log n has been read to its end, and log n still hasn't shown a client's creation, so the
  LoadDriver sign would be blind for the lines only that client's log carries. Each log answers
  for its own client: one log's creation never vouches for another's (Amendment B).
  Notes only (no trigger): a builtin .sys outside the baseline set; a new service of another type;
  CreateService re-registering a baseline service with its baseline image (Proton's discord-bridge,
  at every start); ChangeServiceConfigW that keeps the type and image, or that changes them in a
  process that has opened only user-mode baseline services; LoadDriver for a baseline service of a
  driver type (Wine's own winebth fails to load in every run), the name read from the same line or,
  when a nested Wine trace split the message, from a header-less continuation in the next 5 lines
  (r7); LoadDriver logged before the client's process creation (controller ruling after r7).
  Everything else, including a LoadDriver with no readable name after the client's creation,
  triggers.
"""
import collections
import contextlib
import argparse
import hashlib
import itertools
import json
import math
import os
import re
import signal
import stat
import sys
import time

HOST_HOME = os.path.realpath(os.path.expanduser("~"))
CLIENT = "gujianol.exe"
WINEDEVICE = "winedevice.exe"
BASELINE_SYS = {
    "win32k.sys", "dxgkrnl.sys", "dxgmms1.sys", "mountmgr.sys", "nsiproxy.sys", "ndis.sys",
    "hidparse.sys", "winebus.sys", "hidclass.sys", "winehid.sys", "wineusb.sys",
}

# Every pattern starts with a literal, so the regex engine can skip ahead fast (logs can be GBs).
# Every module load (+loaddll): the path as Wine logs it, and "native" or "builtin".
R_LOADED = re.compile(rb'Loaded L"([^"\n]*)" at [0-9A-Fa-f]+: (\w+)')
# A resource lookup of the type (or name) "PE": kernelbase's FindResource*, or any +resource line.
R_RES_PE = re.compile(rb'FindResource\w*\s+[0-9A-Fa-f]+\s+L?"[Pp][Ee]"|:resource:[^\n]*L?"[Pp][Ee]"')
R_LOADDRIVER = re.compile(rb"LoadDriver[^\n]*")
R_SVC_NAME = re.compile(rb'Services(?:\\)+([^"\\\n]+)', re.I)
DRIVER_STRINGS = (b"processprotect", b"agtdriver", b"aurogongametrust")
R_SERVICES_PID = re.compile(rb'NtCreateUserProcess L"[^"\n]*\\services\.exe" pid ([0-9a-f]+) tid')
R_WINEDEVICE_PID = re.compile(rb'NtCreateUserProcess L"[^"\n]*\\winedevice\.exe" pid ([0-9a-f]+) tid', re.I)
R_CREATEKEY = re.compile(rb':trace:reg:NtCreateKey \(0x[0-9a-f]+,L"([^"\\\n]+)"')  # needs +reg
R_HEADER = re.compile(rb"\d+\.\d+:[0-9a-f]+:[0-9a-f]+:")  # "<time>:<pid>:<tid>:" (+timestamp,+pid,+tid)
# services.exe's side of a service call (svcctl_*), which answers a caller's line in the caller's log.
R_ANSWER = re.compile(rb"trace:service:svcctl_")
# The client's creation (+process): steam.exe's CreateProcessInternalW, or NtCreateUserProcess.
R_CLIENT_CREATE = re.compile(rb"(?:CreateProcessInternalW|NtCreateUserProcess) [^\n]*gujianol\.exe", re.I)
# +service (r7 on, replacing +reg): sechost's "CreateServiceW <scm> L"name" ..." and services.exe's
# "svcctl_CreateServiceW (L"name", ...)". The first quoted string on the line is the service name.
R_CREATESERVICE = re.compile(rb'CreateService\w*[^"\n]*"([^"\n]*)"|CreateService\w*[^\n]*')
# services.exe's line ends with the image path, after the access mask: `0xf01ff, L"<path>")`
# (m1-accept2). The display name before it can be a bare (null) (issue #148). Escapes kept.
R_SVCCTL_IMAGE = re.compile(rb', 0x[0-9a-f]+, L"((?:[^"\\\n]|\\.)*)"\)\s*$')
# +service, ChangeServiceConfig (Wine 11.0, issue #202): sechost's opens, which name the services a
# process holds handles to (the manager's handle, then the name); its ChangeServiceConfigW, which has
# the service's handle but not its name; and services.exe's svcctl_ChangeServiceConfigW, which logs
# nothing but its name. ChangeServiceConfigA's line is followed by ChangeServiceConfigW's.
R_SVC_CALL = re.compile(
    rb"trace:service:(OpenServiceW|CreateServiceW|ChangeServiceConfigW|svcctl_ChangeServiceConfigW) ([^\n]*)")
R_OPEN_NAME = re.compile(rb'[0-9A-Fa-f]+ L"((?:[^"\\\n]|\\.)*)"')
# ChangeServiceConfigW's "%p %ld %ld %ld %s ...": the handle; the new type, start type and error
# control (-1, SERVICE_NO_CHANGE, keeps one); the new image ((null) keeps it; `"...` is one cut short).
R_CHANGE = re.compile(rb'[0-9A-Fa-f]+ (-?\d+) -?\d+ -?\d+ '
                      rb'(?:\(null\)|L"((?:[^"\\\n]|\\.)*)"(?!\.\.\.))(?: |$)')
# The sandbox's tmpfs mounts (isolated-run.sh): walked whole, and the root's own entries.
TMPFS_SUBS = ("/tmp", "/dev/shm", "/run", "/var", "/etc", HOST_HOME)
TMPFS_SHALLOW = ("/",)
# Seconds the client may run before the log must have shown its creation (review M3).
CLIENT_LOG_GRACE = 20.0
DRIVER_TYPES = {1, 2}  # SERVICE_KERNEL_DRIVER, SERVICE_FILE_SYSTEM_DRIVER
USER_MODE_TYPES = {0x10, 0x20}  # SERVICE_WIN32_OWN_PROCESS, SERVICE_WIN32_SHARE_PROCESS
SERVICE_INTERACTIVE_PROCESS = 0x100  # may be added to a user-mode type
SERVICE_NO_CHANGE = 0xFFFFFFFF


def now():
    return time.strftime("%Y-%m-%dT%H:%M:%S") + ("%.3f" % (time.time() % 1))[1:]


class Log:
    def __init__(self, path):
        self.f = open(path, "a", buffering=1)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        try:
            self.f.close()
        except OSError:
            # Closing flushes again what a failed write left in the buffer: on a full disk that
            # fails too, and must not turn a gate event into an uncaught error either.
            pass

    def __call__(self, msg):
        line = f"{now()} {msg}\n"
        try:
            self.f.write(line)
        except OSError:
            # A full disk must not turn a gate event into an uncaught error: fall back to stderr
            # (the runner's watchdog.err), and drop the line if that fails too.
            try:
                sys.stderr.write(line)
            except OSError:
                pass


# --- *.sys files -------------------------------------------------------------------------------
def sys_identity(path):
    """What a *.sys entry is, for comparing snapshots. A symlink is its target plus the target
    file's size, mtime and inode: Proton re-creates its links to Wine's built-in drivers at every
    prefix re-sync, which renews the link itself but not what it points to (the owner's run look-1,
    2026-10-12). A regular file is its own size, mtime and inode."""
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode):
        target = os.readlink(path)
        try:
            t = os.stat(path)
        except OSError:
            return ["link", target, None, None, None]
        return ["link", target, t.st_size, t.st_mtime_ns, t.st_ino]
    return ["file", None, st.st_size, st.st_mtime_ns, st.st_ino]


def scan_sys(root, prune=()):
    """Each *.sys under `root`, with its identity; the paths in `prune` (directories or files) are
    skipped."""
    found = {}
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        if prune:
            dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in prune]
        for f in filenames:
            if f.lower().endswith(".sys"):
                p = os.path.join(dirpath, f)
                if p in prune:
                    continue
                try:
                    found[p] = sys_identity(p)
                except OSError:
                    continue
    return found


def compare_sys(base, cur):
    """S1 findings between two scan_sys() snapshots: new *.sys entries, and changed ones."""
    out = []
    for p, ident in sorted(cur.items()):
        if p not in base:
            out.append(("S1 new .sys file", p, ident))
        elif base[p] != ident:
            out.append(("S1 changed .sys file", p, {"before": base[p], "after": ident}))
    return out


def open_regular(path):
    """A read-only file object for `path`, refusing a symlink (O_NOFOLLOW) or anything but a
    regular file: the sandbox can write the directories this reads, and a planted link must never
    make the host read one of its own files. The open doesn't block (O_NONBLOCK), so a planted FIFO
    with no writer is refused instead of stopping the watch."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        raise OSError(f"not a regular file: {path}")
    os.set_blocking(fd, True)
    return os.fdopen(fd, "rb")


# --- the prefix's services (system.reg) ----------------------------------------------------------
R_REG_SERVICE = re.compile(r"\[System\\\\ControlSet00\d\\\\Services\\\\([^\]\\]+)\]")
R_REG_TYPE = re.compile(r'"Type"=dword:([0-9a-fA-F]{8})')
R_REG_IMAGE = re.compile(r'"ImagePath"=(?:str\(2\):)?"((?:[^"\\]|\\.)*)"')


def norm_image(text):
    """A service's image path for comparison: escapes undone (system.reg and Wine's debugstr_w both
    write a backslash as two), no case, no surrounding blanks."""
    return re.sub(r"\\(.)", r"\1", text).strip().lower()


def parse_services(system_reg):
    """Each service key in a prefix's system.reg: {name: {"type": int or None, "image": str or
    None}}, names lowercased and images normalised (norm_image). Subkeys are skipped. Reads through
    open_regular, and raises if system.reg is a link or not a regular file: the sandbox can write the
    prefix."""
    services, cur = {}, None
    with open_regular(system_reg) as fh:
        for raw in fh:
            line = raw.decode("utf-8", "replace")
            if line.startswith("["):
                m = R_REG_SERVICE.match(line)
                cur = services.setdefault(m.group(1).lower(), {"type": None, "image": None}) if m else None
            elif cur is not None:
                m = R_REG_TYPE.match(line)
                if m:
                    cur["type"] = int(m.group(1), 16)
                m = R_REG_IMAGE.match(line)
                if m:
                    cur["image"] = norm_image(m.group(1))
    return services


def is_driver_service(info):
    image = info.get("image") or ""
    return info.get("type") in DRIVER_TYPES or image.endswith(".sys") or "\\drivers\\" in image


def compare_services(base, cur):
    """S4 findings between two parse_services() results, and notes: (rule, name, detail) each."""
    trig, notes = [], []
    for name, info in sorted(cur.items()):
        before = base.get(name)
        if before is None:
            if is_driver_service(info):
                trig.append(("S4 new driver service in the registry", name, info))
            else:
                notes.append(("new service in the registry", name, info))
        elif before != info:
            trig.append(("S4 baseline service changed in the registry", name,
                         {"before": before, "after": info}))
    return trig, notes


def client_creation_overdue(client_seen, now, client_ts, caught_up, grace=CLIENT_LOG_GRACE):
    """Whether the client has run `grace` seconds (host side, monotonic clock) while the log, read
    up to its end, still hasn't shown the client's creation. Every LoadDriver would then count as
    logged before the client, so the LoadDriver sign is blind (review M3)."""
    return client_seen is not None and client_ts is None and caught_up and now - client_seen > grace


def sha256(path):
    h = hashlib.sha256()
    with open_regular(path) as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def describe_file(path):
    """`sha256 …` for a regular file; for a link, its target, never what it points to."""
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode):
        return f"symlink to {os.readlink(path)}"
    if not stat.S_ISREG(st.st_mode):
        return f"not a regular file (mode {oct(st.st_mode)})"
    return f"sha256 {sha256(path)}"


def create_exclusive(path):
    """Creates the empty file `path`; fails if anything, a link included, is already there."""
    os.close(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644))


def scan_tmpfs(root, subs, binds, shallow=()):
    """S1 over the sandbox's own mounts as seen at `root` (/proc/PID/root): *.sys under each of
    `subs`, skipping the directories bound into the sandbox (`binds`): the read-write ones are
    scanned on the host side, and a read-only one (GE-Proton) can't gain a file from inside. Plus
    the *.sys entries directly in each of `shallow` (the sandbox's root, whose other directories
    are mount points)."""
    prune = {root + d for d in binds}
    hits = {}
    for sub in subs:
        hits.update(scan_sys(root + sub, prune=prune))
    for d in shallow:
        top = os.path.join(root + d, "")
        try:
            names = os.listdir(top)
        except OSError:
            continue
        for n in names:
            if n.lower().endswith(".sys"):
                try:
                    hits[top + n] = sys_identity(top + n)
                except OSError:
                    continue
    return hits


def collect(src, dest, names=None, exclude=()):
    """Copies the regular files under `src` (all of it, or only its top-level `names`, minus
    `exclude`) into `dest`, never following a link: links and special files are listed in the
    returned notes instead. `src` may be written by the sandbox; `dest` is the host's evidence."""
    notes = []
    os.makedirs(dest, exist_ok=True)
    try:
        top = sorted(os.listdir(src))
    except FileNotFoundError:
        return notes
    for name in top:
        if (names is not None and name not in names) or name in exclude:
            continue
        stack = [name]
        while stack:
            rel = stack.pop()
            p = os.path.join(src, rel)
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                notes.append(f"link {rel!r} -> {os.readlink(p)!r} (not followed)")
            elif stat.S_ISDIR(st.st_mode):
                os.makedirs(os.path.join(dest, rel), exist_ok=True)
                stack.extend(os.path.join(rel, c) for c in sorted(os.listdir(p)))
            elif stat.S_ISREG(st.st_mode):
                with open_regular(p) as fin, open(os.path.join(dest, rel), "xb") as fout:
                    for block in iter(lambda: fin.read(1 << 20), b""):
                        fout.write(block)
            else:
                notes.append(f"special {rel!r} mode {oct(st.st_mode)} (not copied)")
    return notes


# --- processes ---------------------------------------------------------------------------------
def read(path, mode="r"):
    with open(path, mode) as fh:
        return fh.read()


def pidns(pid):
    try:
        return os.readlink(f"/proc/{pid}/ns/pid")
    except OSError:
        return None


def children(pid):
    try:
        return [int(c) for c in read(f"/proc/{pid}/task/{pid}/children").split()]
    except (OSError, ValueError):
        return []


def proc_info(pid):
    try:
        stat = read(f"/proc/{pid}/stat")
        comm = read(f"/proc/{pid}/comm").strip()
        argv = [a.decode("utf-8", "replace") for a in read(f"/proc/{pid}/cmdline", "rb").split(b"\0") if a]
    except OSError:
        return None
    fields = stat[stat.rfind(")") + 2:].split()
    return {"pid": pid, "state": fields[0], "ppid": int(fields[1]), "start": int(fields[19]),
            "comm": comm, "argv": argv}


def is_named(info, name):
    a0 = info["argv"][0] if info["argv"] else ""
    return info["comm"].lower() == name or re.split(r"[\\/]", a0)[-1].lower() == name


def renamed_to(seen, name, info_of):
    """The live processes in `seen` that carry `name` now though they didn't when first seen;
    their records are updated in place. Wine starts its processes as wine64 and renames them, so a
    name read when a pid appeared can be stale (#142). A pid now held by another process, which has
    another start time, is left alone."""
    found = []
    for p, info in seen.items():
        if not info.get("alive") or is_named(info, name):
            continue
        fresh = info_of(p)
        if not fresh or fresh["start"] != info["start"] or not is_named(fresh, name):
            continue
        info.update(comm=fresh["comm"], argv=fresh["argv"])
        found.append((p, info))
    return found


def track_clients(seen, clients, info_of):
    """One poll's late naming and rule S2, over `seen` (pid to record, as check_procs keeps it) and
    `clients` (pid to start, the clients known so far). Every live process not yet known as a
    client has its name read again, at every poll (#142; Amendment B: a second client may be named
    late too), and the ones now named the client are returned as (start, pid), in start order. The
    names of the processes started after the earliest client, known or found now, are read again
    too, and S2's hits are the winedevice.exe among them, running or gone: a client found late must
    not miss a short-lived one, and whichever client's start brought one, it is a sign. Returns
    ([(start, pid)], [(pid, record)])."""
    others = {p: i for p, i in seen.items() if p not in clients}
    found = sorted((info["start"], p) for p, info in renamed_to(others, CLIENT, info_of))
    starts = list(clients.values()) + [start for start, _ in found]
    hits = []
    if starts:
        earliest = min(starts)
        later = {p: i for p, i in seen.items() if i["start"] > earliest}
        renamed_to(later, WINEDEVICE, info_of)
        hits = [(p, i) for p, i in later.items() if is_named(i, WINEDEVICE)]
    return found, hits


def ppid_map():
    m = {}
    for d in os.listdir("/proc"):
        if d.isdigit():
            try:
                st = read(f"/proc/{d}/stat")
                m[int(d)] = int(st[st.rfind(")") + 2:].split()[1])
            except (OSError, ValueError, IndexError):
                pass
    return m


def descendants(root):
    """root and every process below it. The sandbox's init (bwrap's child, PID 1 in the new PID
    namespace) reaps every orphan inside, so its subtree is the whole sandbox."""
    kids = {}
    for p, pp in ppid_map().items():
        kids.setdefault(pp, []).append(p)
    out, stack = [], [root]
    while stack:
        p = stack.pop()
        out.append(p)
        stack += kids.get(p, [])
    return out


def alive(pid):
    try:
        return read(f"/proc/{pid}/stat").split(") ", 1)[1][0] != "Z"
    except (OSError, IndexError):
        return False


# --- log rules ---------------------------------------------------------------------------------
class LogRules:
    def __init__(self, baseline_services, images=None, drivers=None):
        self.services = set(baseline_services)
        self.images = dict(images or {})  # baseline service -> its normalised image path
        self.drivers = set(drivers or ())  # the baseline services of a driver type (is_driver_service)
        # The baseline services known to be user-mode: those whose registry entry the baseline has
        # (images) and that aren't drivers. A baseline without service_info (M0's) knows none.
        self.user_mode = (self.services & set(self.images)) - self.drivers
        self.opened = {}  # pid -> the services its OpenServiceW and CreateServiceW lines named
        self.changes_pending = 0  # ChangeServiceConfigW lines that services.exe's line hasn't followed yet
        self.services_pids = set()
        self.winedevice_pids = set()
        self.client_ts = None  # Wine timestamp of the client's process creation, once logged

    def change_verdict(self, names, args):
        """S3 for sechost's ChangeServiceConfigW line, whose text after the function's name is
        `args`: (rule, whether it triggers). The line has the service's handle but not its name, so
        `names`, every service its process had opened or created, stand for it (issue #202)."""
        a = R_CHANGE.match(args)
        if not a:
            return "S3 ChangeServiceConfig with no readable type or image", True
        stype, image = int(a.group(1)) & 0xFFFFFFFF, a.group(2)
        if stype == SERVICE_NO_CHANGE and image is None:
            return "ChangeServiceConfig that keeps the type and image", False
        if stype != SERVICE_NO_CHANGE and stype & ~SERVICE_INTERACTIVE_PROCESS not in USER_MODE_TYPES:
            return f"S3 ChangeServiceConfig sets service type {stype:#x}, not a user-mode one", True
        if image is not None and is_driver_service({"image": norm_image(image.decode("utf-8", "replace"))}):
            return "S3 ChangeServiceConfig sets a driver image", True
        what = "ChangeServiceConfig changes the type or image of"
        if not names:
            return f"S3 {what} a service the log doesn't name", True
        drivers, others = names & self.drivers, names - self.user_mode
        if drivers:
            return f"S3 {what} baseline driver service {' or '.join(sorted(drivers))}", True
        if others:
            return f"S3 {what} {' or '.join(sorted(others))}, not a user-mode baseline service", True
        return f"{what} user-mode baseline service {' or '.join(sorted(names))}", False

    def scan(self, chunk, look=b""):
        """Returns (triggers, notes): lists of (rule, line). look: the text after chunk, if any
        (held back by Tail), used only as look-ahead for messages split across lines."""
        trig, notes = [], []

        def raw_line_at(pos):
            s = chunk.rfind(b"\n", 0, pos) + 1
            e = chunk.find(b"\n", pos)
            return chunk[s:e if e >= 0 else len(chunk)]

        def line_at(pos):
            return raw_line_at(pos).decode("utf-8", "replace")

        def pid_at(pos):  # "<time>:<pid>:<tid>:..." at the start of the line
            s = chunk.rfind(b"\n", 0, pos) + 1
            parts = chunk[s:pos].split(b":", 3)
            return parts[1].decode() if len(parts) > 2 else None

        def ts_at(pos):  # the Wine timestamp at the start of the line, or None
            s = chunk.rfind(b"\n", 0, pos) + 1
            h = R_HEADER.match(chunk, s)
            return float(chunk[s:chunk.index(b":", s)]) if h else None

        for m in R_SERVICES_PID.finditer(chunk):
            self.services_pids.add(m.group(1).decode())
        for m in R_WINEDEVICE_PID.finditer(chunk):
            self.winedevice_pids.add(m.group(1).decode().lower())
        for m in R_CLIENT_CREATE.finditer(chunk):
            ts = ts_at(m.start())
            if ts is not None and (self.client_ts is None or ts < self.client_ts):
                self.client_ts = ts
        for m in R_LOADED.finditer(chunk):
            path = m.group(1).decode("utf-8", "replace").replace("\\\\", "\\").lower()
            name = path.split("\\")[-1]
            native = m.group(2).lower() != b"builtin"
            if name.endswith(".sys"):
                if native:
                    trig.append(("S3 .sys module loaded native", line_at(m.start())))
                elif name not in BASELINE_SYS:
                    notes.append(("builtin .sys outside the baseline set", line_at(m.start())))
            elif native and "\\drivers\\" in path:
                trig.append(("S3 native module loaded from a drivers directory", line_at(m.start())))
            elif native and pid_at(m.start()) in self.winedevice_pids:
                # Wine's own drivers and everything they import are builtin; a native module in a
                # driver host is a driver whatever its name (review I2).
                trig.append(("S3 native module loaded into winedevice.exe", line_at(m.start())))
        for m in R_RES_PE.finditer(chunk):
            trig.append(('S3 FindResource of type "PE" (embedded driver resource type)', line_at(m.start())))
        for m in R_LOADDRIVER.finditer(chunk):
            # Controller ruling (r7): a driver-load line logged before the client's process
            # creation can't be the client's. The log is appended in write order, so while the
            # creation line hasn't appeared, every earlier line predates the client.
            ts = ts_at(m.start())
            if ts is not None and (self.client_ts is None or ts < self.client_ts):
                notes.append((f"LoadDriver logged at {ts:.3f}, before the client's process creation "
                              f"({'not yet logged' if self.client_ts is None else f'{self.client_ts:.3f}'})",
                              line_at(m.start())))
                continue
            n = R_SVC_NAME.search(m.group(0))
            split = False
            if not n:
                # A debug message that Wine emits while formatting another one (a nested trace)
                # splits it: the outer header ends up joined to the nested text, and the outer
                # text follows on a later line with no header (r7: Wine's own winebth error).
                after = chunk[m.end():m.end() + 8192] + look[:8192]
                for ln in after.split(b"\n")[1:6]:
                    if ln and not R_HEADER.match(ln):
                        n = R_SVC_NAME.search(ln)
                        if n:
                            split = True
                            break
            svc = n.group(1).decode("utf-8", "replace").lower() if n else None
            how = " (message split across lines)" if split else ""
            if svc and svc in self.drivers:
                notes.append((f"LoadDriver for baseline service {svc}{how}", line_at(m.start())))
            elif svc and svc in self.services:
                # A user-mode service (discord-bridge, Type 0x10) never goes through LoadDriver: its
                # key now names a driver (issue #148).
                trig.append((f"S3 LoadDriver for user-mode baseline service {svc}{how}", line_at(m.start())))
            else:
                trig.append((f"S3 LoadDriver for non-baseline service {svc}{how}", line_at(m.start())))
        low = None
        for kw in DRIVER_STRINGS:
            if low is None:
                low = chunk.lower()
            i = low.find(kw)
            while i >= 0:
                trig.append(("S3 driver string in log", line_at(i)))
                i = low.find(kw, i + 1)
        for m in R_CREATESERVICE.finditer(chunk):
            name = m.group(1)
            if name is None or not name.strip():
                notes.append(("CreateService line without a service name", line_at(m.start())))
                continue
            svc = name.decode("utf-8", "replace")
            if svc.lower() not in self.services:
                trig.append((f"S3 CreateService for non-baseline service {svc!r}", line_at(m.start())))
                continue
            # services.exe's line carries the image path after the name, the display name and the
            # (unquoted) access mask (m1-accept2). Proton re-registers its own services at every
            # start with their baseline image; another image is a driver under a borrowed name
            # (review I1), and so is a line whose image can't be read. sechost's line has no path:
            # a note, as services.exe's line follows.
            raw = raw_line_at(m.start())
            if b"svcctl_CreateService" in raw:
                found = R_SVCCTL_IMAGE.search(raw)
                if not found:
                    trig.append((f"S3 CreateService for baseline service {svc}, with no readable image",
                                 line_at(m.start())))
                    continue
                image = norm_image(found.group(1).decode("utf-8", "replace"))
                if image != self.images.get(svc.lower()):
                    trig.append((f"S3 CreateService re-points baseline service {svc}", line_at(m.start())))
                    continue
                notes.append((f"CreateService for baseline service {svc}, with its baseline image",
                              line_at(m.start())))
            else:
                notes.append((f"CreateService for baseline service {svc}", line_at(m.start())))
        for m in R_SVC_CALL.finditer(chunk):
            call, args, pid = m.group(1), m.group(2), pid_at(m.start())
            if call in (b"OpenServiceW", b"CreateServiceW"):
                n = R_OPEN_NAME.match(args)
                name = n.group(1).decode("utf-8", "replace").lower() if n else "(an unreadable name)"
                self.opened.setdefault(pid, set()).add(name)
            elif call == b"ChangeServiceConfigW":
                self.changes_pending += 1
                rule, triggers = self.change_verdict(self.opened.get(pid, set()), args)
                (trig if triggers else notes).append((rule, line_at(m.start())))
            elif self.changes_pending:  # services.exe's line for the change sechost's line showed
                self.changes_pending -= 1
            else:
                # A caller whose sechost logs nothing (another environment, or the RPC called
                # directly): services.exe's line is all there is, and it can't say what changed.
                trig.append(("S3 services.exe changed a service's configuration, with no "
                             "ChangeServiceConfigW before it", line_at(m.start())))
        if self.services_pids:
            for m in R_CREATEKEY.finditer(chunk):
                key = m.group(1).decode("utf-8", "replace")
                if pid_at(m.start()) in self.services_pids and key.lower() not in self.services:
                    trig.append((f"S3 services.exe created key {key!r}, not a baseline service",
                                 line_at(m.start())))
        return trig, notes


class Tail:
    HOLD = 8  # complete lines held back as look-ahead (see LogRules.scan), released on a quiet poll

    def __init__(self, path):
        self.path, self.off, self.rest, self.fh = path, 0, b"", None
        self.at_eof = False  # whether the last read reached the end of the log as it was then
        self.quiet = False  # whether the last read found nothing new

    def close(self):
        if self.fh is not None:
            self.fh.close()
            self.fh = None

    def replaced(self):
        """Whether the path now names another file than the open one, or this one shrank below
        what was read of it. A moment with no file at the path is neither: the old one is still
        being read."""
        try:
            st = os.lstat(self.path)
        except FileNotFoundError:
            return False
        cur = os.fstat(self.fh.fileno())
        return (st.st_dev, st.st_ino) != (cur.st_dev, cur.st_ino) or st.st_size < self.off

    def read(self, limit=16 << 20, final=False):
        """Returns (chunk, look): complete lines to scan now, and the text held back after them,
        which is look-ahead only (it is scanned on a later read). With final=True, or when nothing
        new arrived (a quiet poll), the held-back complete lines are released too."""
        if self.fh is None:
            try:
                self.fh = open_regular(self.path)
            except FileNotFoundError:
                self.at_eof = self.quiet = True
                return b"", b""
        self.fh.seek(self.off)
        new = self.fh.read(limit)
        self.off += len(new)
        self.at_eof = len(new) < limit
        self.quiet = not new
        if self.at_eof and self.replaced():
            # GE-Proton removes and recreates its log for each session (umu's createprefix, then
            # the client), and a log can be truncated: the old file is read to its end, its last
            # partial line counts as complete, and the next read starts the new file at byte 0.
            data = self.rest + new
            if data and not data.endswith(b"\n"):
                data += b"\n"
            self.fh.close()
            self.fh, self.off, self.rest, self.at_eof = None, 0, b"", False
            return data, b""
        data = self.rest + new
        if final:
            self.rest = b""
            return data, b""
        if not new:
            cut = data.rfind(b"\n")
            self.rest = data[cut + 1:]
            return data[:cut + 1], b""
        cut = len(data)
        for _ in range(self.HOLD + 1):
            cut = data.rfind(b"\n", 0, cut)
            if cut < 0:
                break
        if cut < 0:
            self.rest = data
            return b"", b""
        self.rest = data[cut + 1:]
        return data[:cut + 1], self.rest


def stamp_of(line):
    """The Wine timestamp that begins `line` (+timestamp), or None."""
    return float(line[:line.index(b":")]) if R_HEADER.match(line) else None


class Follow:
    """One client's log in a LogStream: its Tail; the lines read and not yet scanned, each as
    [the stamp it goes by, the poll that read it, whether it is services.exe's answer, its text];
    and what the log has shown."""

    def __init__(self, path):
        self.tail = Tail(path)
        self.units = collections.deque()
        self.held = b""  # what the Tail holds back after the last read: read, not yet queued
        self.held_min = None  # the earliest stamp in it, which holds the other logs' later lines back
        self.shown = -math.inf  # the latest stamp read
        self.last = -math.inf  # the stamp a line without one goes by
        self.eof_poll = -1  # the last poll that read the log to its end
        self.created = None  # the stamp of the first client creation the log showed

    def ingest(self, chunk, poll):
        """Queues the lines of `chunk`: a line without a stamp goes with the line before it."""
        lines = chunk.split(b"\n")
        if not lines[-1]:
            lines.pop()
        for line in lines:
            text = line + b"\n"
            stamp = stamp_of(line)
            if stamp is None:
                if self.units:
                    self.units[-1][3] += text
                    continue
                stamp, answer = self.last, False
            else:
                answer = bool(R_ANSWER.search(line))
                if self.created is None and R_CLIENT_CREATE.search(line):
                    self.created = stamp
            self.units.append([stamp, poll, answer, text])
            self.last = stamp
            self.shown = max(self.shown, stamp)

    def hold(self, look, stalled=False):
        """Notes what the Tail holds back: read, so it counts as shown, and not yet queued, so a line
        of another log stamped at or after it waits for it. A last line still partial after a poll
        that found nothing new (`stalled`) waits for a writer that stopped mid-line, and no longer
        holds the others back."""
        self.held = look
        lines = look.split(b"\n")
        if stalled and lines[-1]:
            lines.pop()
        stamps, prev = [], self.last
        for line in lines:
            stamp = stamp_of(line)
            if stamp is not None:
                prev = stamp
            if line:
                stamps.append(prev)
        self.held_min = min(stamps) if stamps else None
        if stamps:
            self.shown = max(self.shown, max(stamps))


class LogStream:
    """The clients' Wine logs read as one stream, in the order of Wine's timestamps (M4 world plan,
    Amendment B). Every Wine process takes its stamps from one clock, the host's time since boot,
    so they compare across logs (Task 13's rehearsal: one exchange stamped alike in both). Each
    poll reads every log once; a line is released once each other log has shown a later stamp, or
    was read to its end in a later poll, and holds back no earlier line, queued or still in its
    Tail: so at most a poll late, or a few while another log trickles in. A log's last line, left
    partial through a poll with nothing new, holds no line back (its writer stopped mid-line). A
    log's lines keep their order, and at equal stamps services.exe's answer goes after the caller's
    line. With one log, every line is released as its Tail releases it."""

    LOOK = 8192  # look-ahead for a run, as LogRules.scan uses it

    def __init__(self, paths):
        self.logs = [Follow(p) for p in paths]
        self.polls = 0

    def close(self):
        for f in self.logs:
            f.tail.close()

    def ready(self, i, unit):
        """Whether `unit`, at the head of log i's queue, may be scanned now."""
        stamp, poll = unit[0], unit[1]
        for j, f in enumerate(self.logs):
            if j == i:
                continue
            if f.held_min is not None and f.held_min <= stamp:
                return False
            if not (f.shown > stamp or f.eof_poll > poll):
                return False
        return True

    def read(self, final=False):
        """One poll: reads every log, then returns the lines to scan now as runs of one log's lines,
        in time order: [(log index, chunk, look)], where look is what follows the run in its own log
        (look-ahead only). With final=True, everything is read and released."""
        self.polls += 1
        poll = self.polls
        for f in self.logs:
            if final:
                while True:
                    chunk, _ = f.tail.read(final=True)
                    if not chunk:
                        break
                    f.ingest(chunk, poll)
                f.hold(b"")
                f.eof_poll = poll
            else:
                chunk, _ = f.tail.read()
                f.ingest(chunk, poll)
                # All the Tail holds back, not only the look-ahead it returned: with fewer complete
                # lines than it keeps, it returns none, and those lines must hold the others back too.
                f.hold(f.tail.rest, stalled=f.tail.quiet)
                if f.tail.at_eof:
                    f.eof_poll = poll
        released = []
        while True:
            best = None
            for i, f in enumerate(self.logs):
                if f.units:
                    u = f.units[0]
                    key = (u[0], u[2], i)
                    if best is None or key < best[0]:
                        best = (key, i)
            if best is None:
                break
            i = best[1]
            if not final and not self.ready(i, self.logs[i].units[0]):
                break
            released.append((i, self.logs[i].units.popleft()))
        return self.runs(released)

    def runs(self, released):
        runs = []
        for i, u in released:
            if runs and runs[-1][0] == i:
                runs[-1][1].append(u[3])
            else:
                runs.append((i, [u[3]]))
        texts = {}
        for i, u in released:
            texts.setdefault(i, []).append(u[3])
        pos = dict.fromkeys(texts, 0)
        out = []
        for i, chunk in runs:
            pos[i] += len(chunk)
            ahead = texts[i]
            following = (ahead[k] for k in range(pos[i], len(ahead)))
            look, size = [], 0
            queued = (u[3] for u in self.logs[i].units)
            for text in itertools.chain(following, queued, (self.logs[i].held,)):
                if size >= self.LOOK:
                    break
                look.append(text)
                size += len(text)
            out.append((i, b"".join(chunk), b"".join(look)))
        return out


# --- modes -------------------------------------------------------------------------------------
def system_reg(prefix):
    return os.path.join(prefix, "system.reg")


def cmd_baseline(a):
    info = parse_services(system_reg(a.prefix))
    base = {"taken": now(), "root": a.root, "prefix": a.prefix, "sys": scan_sys(a.root),
            "services": sorted(info), "service_info": info}
    with open(os.path.join(a.out, "watchdog-baseline.json"), "w") as fh:
        json.dump(base, fh, indent=1)
    print(f"watchdog baseline: {len(base['sys'])} .sys files, {len(base['services'])} service keys")


def load_baseline(out):
    with open(os.path.join(out, "watchdog-baseline.json")) as fh:
        return json.load(fh)


def baseline_rules(base):
    """LogRules for a baseline: its service names, each one's image path (S3 CreateService), and
    which of them are drivers (S3 LoadDriver)."""
    info = base.get("service_info", {})
    return LogRules(base["services"], {n: i["image"] for n, i in info.items()},
                    {n for n, i in info.items() if is_driver_service(i)})


def cmd_scanlog(a):
    base = load_baseline(a.out)
    rc = 0
    for f in a.files:
        rules = baseline_rules(base)  # per file: the client's creation time is per run
        with open(f, "rb") as fh:
            trig, notes = rules.scan(fh.read())
        for kind, line in trig:
            print(f"{f}: TRIGGER {kind}: {line}")
            rc = 3
        for kind, line in notes:
            print(f"{f}: note {kind}: {line}")
        print(f"{f}: {len(trig)} triggers, {len(notes)} notes")
    return rc


def cmd_watch(a):
    with Log(os.path.join(a.out, "watchdog.log")) as log, contextlib.closing(LogStream(a.log)) as stream:
        return watch(a, log, stream)


def watch(a, log, stream):
    base = load_baseline(a.out)
    base_sys = base["sys"]
    rules = baseline_rules(base)
    reg_path, reg_sig = system_reg(a.prefix), None
    spid = a.sandbox_pid
    ns = None
    init = None
    seen = {}
    clients = {}  # pid -> {"start": clock ticks, "found": monotonic time first seen}; never removed
    notes_seen = set()
    polls = 0
    tmpfs_warned = False
    subs, shallow = (a.tmpfs, ()) if a.tmpfs else (TMPFS_SUBS, TMPFS_SHALLOW)
    log(f"watch start: sandbox pid {spid}, root {a.root}, prefix {a.prefix}, logs {a.log}, "
        f"interval {a.interval}s, baseline {len(base_sys)} .sys / {len(base['services'])} services, "
        f"tmpfs scan of {' '.join(subs)}{' and / (its own entries)' if shallow else ''}")

    def sandbox_root_scan():
        nonlocal tmpfs_warned
        hits = {}
        cands = [p for p in ([init] if init else []) + list(seen) if alive(p)]
        for p in cands:
            r = f"/proc/{p}/root"  # the sandbox's mount namespace, if we may look (ptrace read)
            try:
                os.listdir(r + "/tmp")
            except OSError:
                continue
            return scan_tmpfs(r, subs, [a.root] + a.binds, shallow=shallow)
        if cands and not tmpfs_warned:
            log("note: can't read any sandbox process's /proc/PID/root; tmpfs mounts not scanned")
            tmpfs_warned = True
        return hits

    def check_sys():
        out = compare_sys(base_sys, scan_sys(a.root))
        if init:
            for p, st in sandbox_root_scan().items():
                out.append(("S1 .sys file in a sandbox tmpfs mount", p, st))
        return out

    def check_reg():
        """S4: re-reads system.reg when its lstat() changes (never through a link)."""
        nonlocal reg_sig
        try:
            st = os.lstat(reg_path)
        except FileNotFoundError:
            return []
        sig = (st.st_mode, st.st_ino, st.st_size, st.st_mtime_ns)
        if sig == reg_sig:
            return []
        reg_sig = sig
        try:
            cur = parse_services(reg_path)
        except OSError as e:
            return [("S4 the prefix's system.reg is not a regular file any more", reg_path, str(e))]
        trig, notes = compare_services(base.get("service_info", {}), cur)
        for k, name, detail in notes:
            if (k, name) not in notes_seen:
                notes_seen.add((k, name))
                log(f"registry note: {k}: {name} {json.dumps(detail)}")
        return trig

    def by_start():
        return sorted(clients.items(), key=lambda pc: pc[1]["start"])

    def new_client(p, start, how):
        """Client `p` is found: numbered by its start, tied to that log, counted after it ends."""
        clients[p] = {"start": start, "found": time.monotonic()}
        n = [q for q, _ in by_start()].index(p) + 1
        log(f"client {n} detected{how}: pid {p} start {start}")
        if len(clients) > len(stream.logs):
            raise RuntimeError(f"{len(clients)} clients, but {len(stream.logs)} log(s): client pid {p} has no "
                               "log of its own (one --log per client, in the order they start)")

    def check_procs():
        out = []
        if not init:
            return out
        cur = set(descendants(init))
        for p in cur:
            if p not in seen:
                info = proc_info(p)
                if not info:
                    continue
                info["alive"] = True
                seen[p] = info
                log(f"proc + pid {p} ppid {info['ppid']} start {info['start']} comm {info['comm']!r} "
                    f"argv {info['argv'][:4]!r}")
                if is_named(info, CLIENT):
                    new_client(p, info["start"], "")
        for p, info in seen.items():
            if info.get("alive") and p not in cur:
                info["alive"] = False
                log(f"proc - pid {p} comm {info['comm']!r}")
        found, hits = track_clients(seen, {p: c["start"] for p, c in clients.items()}, proc_info)
        for start, p in found:
            new_client(p, start, " after a rename")
        out += [("S2 winedevice.exe started after the client", p, info) for p, info in hits]
        return out

    def check_log(final=False):
        out = []
        for i, chunk, look in stream.read(final=final):
            trig, notes = rules.scan(chunk, look)
            where = f" (log {i + 1})" if len(stream.logs) > 1 else ""
            out += [(k + where, ln, None) for k, ln in trig]
            for k, ln in notes:
                if (k, ln) not in notes_seen:
                    notes_seen.add((k, ln))
                    log(f"log note{where}: {k}: {ln}")
        return out

    def check_overdue():
        """The fail safe: client n's own log must show a client's creation (Amendment B)."""
        now_m = time.monotonic()
        for n, (p, c) in enumerate(by_start(), 1):
            f = stream.logs[n - 1]
            if client_creation_overdue(c["found"], now_m, f.created, f.tail.at_eof):
                raise RuntimeError(f"client {n} (pid {p}) has run {CLIENT_LOG_GRACE:.0f} s, but log {n} never "
                                   "showed a client's creation: the LoadDriver sign would be blind")

    def freeze_kill_record(found):
        t_trig = time.time()
        mem = descendants(init) if init else []
        for p in mem:
            try:
                os.kill(p, signal.SIGSTOP)
            except OSError:
                pass
        snap = []
        for p in mem:
            info = proc_info(p) or {"pid": p}
            try:
                maps = sorted({ln.split(None, 5)[5].strip() for ln in read(f"/proc/{p}/maps").splitlines()
                               if len(ln.split(None, 5)) == 6 and ln.lower().rstrip().endswith((".sys", ".exe"))})
            except OSError:
                maps = []
            info["maps_sys_exe"] = maps
            snap.append(info)
        for p in ([init] if init else []) + mem:
            try:
                os.kill(p, signal.SIGKILL)
            except OSError:
                pass
        try:
            os.kill(spid, signal.SIGKILL)
        except OSError:
            pass
        left = []
        for _ in range(50):
            left = [p for p in descendants(init) if alive(p)] if init else []
            if not left and not alive(spid):
                break
            for p in left:
                try:
                    os.kill(p, signal.SIGKILL)
                except OSError:
                    pass
            time.sleep(0.1)
        # The record is best effort: failing to write it must not turn exit 3 into 4.
        try:
            with open(os.path.join(a.out, "watchdog-trigger.txt"), "w") as fh:
                fh.write(f"GATE EVENT at {time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(t_trig))}\n")
                for n, (p, c) in enumerate(by_start(), 1):
                    fh.write(f"client {n} pid {p} start {c['start']} (clock ticks since boot)\n")
                fh.write("\n")
                for kind, what, detail in found:
                    fh.write(f"{kind}: {what}\n")
                    if isinstance(what, str) and what.startswith("/"):
                        try:
                            fh.write(f"  {describe_file(what)}\n")
                        except OSError as e:
                            fh.write(f"  unreadable: {e}\n")
                    if detail is not None:
                        fh.write(f"  {json.dumps(detail, default=str)}\n")
                fh.write("\nsandbox processes at the trigger (frozen with SIGSTOP, then SIGKILL):\n")
                for info in snap:
                    fh.write(json.dumps(info, default=str) + "\n")
                fh.write(f"\nprocesses left after the kill: {left}; bwrap alive: {alive(spid)}\n")
        except OSError as e:
            log(f"could not write {a.out}/watchdog-trigger.txt: {e}")
        log(f"GATE EVENT: {found[0][0]}: {found[0][1]} -> sandbox frozen and killed; left {left}")

    try:
        while True:
            t = time.monotonic()
            polls += 1
            sandbox_alive = alive(spid)
            if init is None and sandbox_alive:
                # The wrapper execs bwrap under the same PID; bwrap's child (also named bwrap) is
                # PID 1 of the new PID namespace. Other children (the wrapper's process
                # substitution for the seccomp filter) are host processes.
                try:
                    comm = read(f"/proc/{spid}/comm").strip()
                except OSError:
                    comm = ""
                if comm == "bwrap":
                    for c in children(spid):
                        try:
                            if read(f"/proc/{c}/comm").strip() == "bwrap":
                                init = c
                                break
                        except OSError:
                            pass
                if init:
                    ns = pidns(init) or "unreadable (init not dumpable)"
                    log(f"armed: bwrap pid {spid}, sandbox init pid {init}, pid namespace {ns}")
                    try:
                        create_exclusive(a.go)
                    except OSError as e:
                        log(f"WATCHDOG FAILURE: can't create the start signal {a.go}: {e}")
                        return 4  # the runner kills the sandbox when the watchdog ends first
            found = check_sys() + check_procs() + check_log() + (check_reg() if polls % 4 == 0 else [])
            if found:
                freeze_kill_record(found)
                return 3
            check_overdue()
            if not sandbox_alive:
                found = check_sys() + check_log(final=True) + check_reg()
                if found:
                    log("sign found after the sandbox had already ended")
                    freeze_kill_record(found)
                    return 3
                log(f"sandbox ended; no sign of driver activity in {polls} polls; client pids "
                    f"{[p for p, _ in by_start()]}; log bytes scanned {[f.tail.off for f in stream.logs]}; "
                    f"processes seen {len(seen)}")
                return 0
            if polls % 20 == 0:
                live = [f"{p}:{i['comm']}" for p, i in seen.items() if i.get("alive")]
                log(f"heartbeat: poll {polls}, logs {[f.tail.off for f in stream.logs]} bytes, live {live}")
            time.sleep(max(0.0, a.interval - (time.monotonic() - t)))
    except BaseException as e:  # fail safe: no watchdog, no run. Kill first, then say why.
        for p in ((descendants(init) if init else []) + [spid]):
            try:
                os.kill(p, signal.SIGKILL)
            except OSError:
                pass
        log(f"WATCHDOG FAILURE {type(e).__name__}: {e}; the sandbox was killed")
        raise SystemExit(4)


def parser():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="mode", required=True)
    b = sub.add_parser("baseline")
    w = sub.add_parser("watch")
    s = sub.add_parser("scanlog")
    for p in (b, w, s):
        p.add_argument("--root", required=True)
        p.add_argument("--out", required=True)
    for p in (b, w):
        p.add_argument("--prefix", help="the Wine prefix whose system.reg S4 reads (default: ROOT/compat/pfx)")
    w.add_argument("--sandbox-pid", type=int, required=True)
    w.add_argument("--log", action="append", required=True,
                   help="a client's Wine log; once per client, in the order the clients start")
    w.add_argument("--interval", type=float, default=0.5)
    w.add_argument("--bind", dest="binds", action="append", default=[],
                   help="other paths bound into the sandbox (skipped by the tmpfs scan)")
    w.add_argument("--go", help="the start signal to create once armed (default: OUT/.go)")
    w.add_argument("--tmpfs", action="append",
                   help="a tmpfs mount of the sandbox's own to scan, instead of isolated-run.sh's (a bare "
                        "run's /run, /tmp, /dev/shm: its namespace sees the host's other files, whose own "
                        "Proton prefixes would trip S1)")
    s.add_argument("files", nargs="+")
    c = sub.add_parser("collect", help="copy a sandbox-writable tree's regular files, never following a link")
    c.add_argument("--src", required=True)
    c.add_argument("--dest", required=True)
    c.add_argument("--name", dest="names", action="append", help="only these top-level names")
    c.add_argument("--exclude", action="append", default=[], help="skip these top-level names")
    return ap


def main():
    ap = parser()
    a = ap.parse_args()
    if a.mode == "collect":
        notes = collect(a.src, a.dest, names=a.names, exclude=a.exclude)
        if notes:
            with open(os.path.join(a.dest, "collect-notes.txt"), "a") as fh:
                fh.write("".join(n + "\n" for n in notes))
        return 0
    a.root = os.path.realpath(a.root)
    a.out = os.path.realpath(a.out)
    if a.mode in ("baseline", "watch"):
        a.prefix = a.prefix or os.path.join(a.root, "compat", "pfx")
    if a.mode == "baseline":
        cmd_baseline(a)
        return 0
    if a.mode == "scanlog":
        return cmd_scanlog(a)
    a.binds = [os.path.realpath(x) for x in a.binds]
    a.go = a.go or os.path.join(a.out, ".go")
    if len({os.path.realpath(x) for x in a.log}) != len(a.log):
        ap.error("two --log name one file: each client has its own log")
    return cmd_watch(a)


if __name__ == "__main__":
    sys.exit(main())
