#!/usr/bin/env python3
"""Driver watchdog for scripts/spike/run-client.sh (M1 Task 9; first used locally in M0's runs 5–9).

Host side, best effort. While a sandboxed run is active it polls about every 0.5 s. At the first
sign of driver activity it freezes every process in the sandbox (SIGSTOP), records what it saw,
kills the whole sandbox (SIGKILL: the PID namespace's init, every member, then bwrap) and exits 3.
That is a gate event: stop all runs and report BLOCKED. It never reads or analyses a driver file
beyond stat() and a SHA-256 for identification.

  watchdog.py baseline --root S --out OUT
      Snapshot of every *.sys under S and of the prefix's service keys -> OUT/watchdog-baseline.json.
  watchdog.py watch --root S --out OUT --sandbox-pid PID --log FILE [--interval 0.5]
      PID is the isolated-run.sh wrapper started in the background (it execs bwrap). Once the
      sandbox's PID namespace exists the watchdog arms itself and creates OUT/.go, which the command
      inside waits for before it starts the client. Exit 0: the sandbox ended, no sign seen.
      Exit 3: gate event. Exit 4: the watchdog itself failed (the sandbox is killed then too).
  watchdog.py watch ... [--go FILE]
      FILE is the start signal (default OUT/.go); it is created exclusively and never through a link.
  watchdog.py collect --src DIR --dest DIR [--name N]... [--exclude N]...
      Copies DIR's regular files into DEST without following any link (links and special files are
      listed in DEST/collect-notes.txt): how the host takes evidence from directories the sandbox
      can write.
  watchdog.py scanlog --root S --out OUT FILE...
      Runs only the log rules over finished logs (to test them against r1-r4), prints the hits.

Signs (any one triggers):
  S1 a *.sys under S that isn't in the baseline, or a baseline one whose identity changed: a regular
     file's size, mtime or inode, or a symlink's target or the target file's size, mtime or inode
     (sys_identity; a link merely re-created by Proton's prefix re-sync is not a change); any *.sys
     in the sandbox's own tmpfs mounts, seen through /proc/<pid>/root of a sandbox process.
  S2 a winedevice.exe process in the sandbox's PID namespace that started after the client
     (gujianol.exe) started. Those started before it host Wine's own drivers (the baseline).
  S3 in the Wine log, read incrementally: a .sys module (any letter case) loaded native, not as a
     Wine builtin; a native module loaded from a drivers directory or into a winedevice.exe process
     (Wine's builtin drivers and their imports are all builtin); a resource lookup of the named type
     "PE" (the embedded driver's type, PE/144; needs +resource); Nt/ZwLoadDriver for a service that
     isn't in the baseline; a service key created by services.exe that isn't in the baseline (needs
     +reg, r1-r6); CreateService for a service that isn't in the baseline, or for a baseline service
     with an image path other than its baseline one (needs +service, r7 on); the strings
     ProcessProtect, AGTDriver or AurogonGameTrust.
  S4 the prefix's system.reg (re-read when it changes; Wine saves it periodically and at exit): a new
     service key of a driver type (kernel or file system) or with a driver image, or a baseline
     service whose type or image path changed, or a system.reg that is no longer a regular file.
  Fail safe (exit 4, the sandbox killed): the client has run 20 s, the log scan has caught up, and
  the log still hasn't shown the client's creation, so the LoadDriver sign would be blind.
  Notes only (no trigger): a builtin .sys outside the baseline set; a new service of another type;
  CreateService re-registering a baseline service with its baseline image (Proton's discord-bridge,
  at every start); LoadDriver for a baseline service
  (Wine's own winebth fails to load in every run), the name read from the same line or, when a nested
  Wine trace split the message, from a header-less continuation in the next 5 lines (r7); LoadDriver
  logged before the client's process creation (controller ruling after r7). Everything else,
  including a LoadDriver with no readable name after the client's creation, triggers.
"""
import argparse
import hashlib
import json
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
# The client's creation (+process): steam.exe's CreateProcessInternalW, or NtCreateUserProcess.
R_CLIENT_CREATE = re.compile(rb"(?:CreateProcessInternalW|NtCreateUserProcess) [^\n]*gujianol\.exe", re.I)
# +service (r7 on, replacing +reg): sechost's "CreateServiceW <scm> L"name" ..." and services.exe's
# "svcctl_CreateServiceW (L"name", ...)". The first quoted string on the line is the service name.
R_CREATESERVICE = re.compile(rb'CreateService\w*[^"\n]*"([^"\n]*)"|CreateService\w*[^\n]*')
R_QUOTED = re.compile(rb'"((?:[^"\\\n]|\\.)*)"')  # a debugstr_w string's text, escapes kept
# The sandbox's tmpfs mounts (isolated-run.sh): walked whole, and the root's own entries.
TMPFS_SUBS = ("/tmp", "/dev/shm", "/run", "/var", "/etc", HOST_HOME)
TMPFS_SHALLOW = ("/",)
# Seconds the client may run before the log must have shown its creation (review M3).
CLIENT_LOG_GRACE = 20.0
DRIVER_TYPES = {1, 2}  # SERVICE_KERNEL_DRIVER, SERVICE_FILE_SYSTEM_DRIVER


def now():
    return time.strftime("%Y-%m-%dT%H:%M:%S") + ("%.3f" % (time.time() % 1))[1:]


class Log:
    def __init__(self, path):
        self.f = open(path, "a", buffering=1)

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
    found = {}
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        if prune:
            dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in prune]
        for f in filenames:
            if f.lower().endswith(".sys"):
                p = os.path.join(dirpath, f)
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


def track_client(seen, client_start, info_of):
    """One poll's late naming and rule S2, over `seen` (pid to record, as check_procs keeps it).
    While no client is known (`client_start` is None), every live process's name is read again, and
    the earliest-started one now named the client is returned as (start, pid). Once a client is
    known, the names of the processes started after it are read again too, and S2's hits are the
    winedevice.exe among them, running or gone: a client found late must not miss a short-lived
    one. Returns (the client found this poll or None, [(pid, record)])."""
    found = None
    if client_start is None:
        named = renamed_to(seen, CLIENT, info_of)
        if named:
            p, info = min(named, key=lambda pi: pi[1]["start"])
            found = (info["start"], p)
            client_start = info["start"]
    hits = []
    if client_start is not None:
        later = {p: i for p, i in seen.items() if i["start"] > client_start}
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
    def __init__(self, baseline_services, images=None):
        self.services = set(baseline_services)
        self.images = dict(images or {})  # baseline service -> its normalised image path
        self.services_pids = set()
        self.winedevice_pids = set()
        self.client_ts = None  # Wine timestamp of the client's process creation, once logged

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
            if svc and svc in self.services:
                notes.append((f"LoadDriver for baseline service {svc}{how}", line_at(m.start())))
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
            # (review I1). sechost's line has no path: a note, as services.exe's line follows.
            raw = raw_line_at(m.start())
            quoted = R_QUOTED.findall(raw)
            if b"svcctl_CreateService" in raw and len(quoted) >= 3:
                image = norm_image(quoted[2].decode("utf-8", "replace"))
                if image != self.images.get(svc.lower()):
                    trig.append((f"S3 CreateService re-points baseline service {svc}", line_at(m.start())))
                    continue
                notes.append((f"CreateService for baseline service {svc}, with its baseline image",
                              line_at(m.start())))
            else:
                notes.append((f"CreateService for baseline service {svc}", line_at(m.start())))
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
                self.at_eof = True
                return b"", b""
        self.fh.seek(self.off)
        new = self.fh.read(limit)
        self.off += len(new)
        self.at_eof = len(new) < limit
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


# --- modes -------------------------------------------------------------------------------------
def system_reg(root):
    return os.path.join(root, "compat/pfx/system.reg")


def cmd_baseline(a):
    info = parse_services(system_reg(a.root))
    base = {"taken": now(), "root": a.root, "sys": scan_sys(a.root),
            "services": sorted(info), "service_info": info}
    with open(os.path.join(a.out, "watchdog-baseline.json"), "w") as fh:
        json.dump(base, fh, indent=1)
    print(f"watchdog baseline: {len(base['sys'])} .sys files, {len(base['services'])} service keys")


def load_baseline(out):
    with open(os.path.join(out, "watchdog-baseline.json")) as fh:
        return json.load(fh)


def baseline_rules(base):
    """LogRules for a baseline: its service names, and each one's image path (S3 CreateService)."""
    return LogRules(base["services"], {n: i["image"] for n, i in base.get("service_info", {}).items()})


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
    log = Log(os.path.join(a.out, "watchdog.log"))
    base = load_baseline(a.out)
    base_sys = base["sys"]
    rules = baseline_rules(base)
    reg_path, reg_sig = system_reg(a.root), None
    tail = Tail(a.log)
    spid = a.sandbox_pid
    ns = None
    init = None
    seen = {}
    client_start = None
    client_pid = None
    client_seen = None  # monotonic time the client was first seen running
    notes_seen = set()
    polls = 0
    tmpfs_warned = False
    log(f"watch start: sandbox pid {spid}, root {a.root}, log {a.log}, interval {a.interval}s, "
        f"baseline {len(base_sys)} .sys / {len(base['services'])} services")
    if a.no_tmpfs_scan:
        log("tmpfs scan off: a bare-metal run, whose namespace sees the host's own files; "
            "S1 covers the root")

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
            return scan_tmpfs(r, TMPFS_SUBS, [a.root] + a.binds, shallow=TMPFS_SHALLOW)
        if cands and not tmpfs_warned:
            log("note: can't read any sandbox process's /proc/PID/root; tmpfs mounts not scanned")
            tmpfs_warned = True
        return hits

    def check_sys():
        out = compare_sys(base_sys, scan_sys(a.root))
        if init and not a.no_tmpfs_scan:
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

    def check_procs():
        nonlocal client_start, client_pid, client_seen
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
                if is_named(info, CLIENT) and (client_start is None or info["start"] < client_start):
                    client_start, client_pid = info["start"], p
                    client_seen = client_seen or time.monotonic()
                    log(f"client detected: pid {p} start {client_start}")
        for p, info in seen.items():
            if info.get("alive") and p not in cur:
                info["alive"] = False
                log(f"proc - pid {p} comm {info['comm']!r}")
        found, hits = track_client(seen, client_start, proc_info)
        if found:
            client_start, client_pid = found
            client_seen = client_seen or time.monotonic()
            log(f"client detected after a rename: pid {client_pid} start {client_start}")
        out += [("S2 winedevice.exe started after the client", p, info) for p, info in hits]
        return out

    def check_log(final=False):
        out = []
        while True:
            chunk, look = tail.read(final=final)
            if not chunk:
                break
            trig, notes = rules.scan(chunk, look)
            out += [(k, l, None) for k, l in trig]
            for k, l in notes:
                if (k, l) not in notes_seen:
                    notes_seen.add((k, l))
                    log(f"log note: {k}: {l}")
            if not final:
                break
        return out

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
                fh.write(f"client pid {client_pid} start {client_start} (clock ticks since boot)\n\n")
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
            if client_creation_overdue(client_seen, time.monotonic(), rules.client_ts, tail.at_eof):
                raise RuntimeError(f"the client has run {CLIENT_LOG_GRACE:.0f} s, but the log never showed "
                                   "its creation: the LoadDriver sign would be blind")
            if not sandbox_alive:
                found = check_sys() + check_log(final=True) + check_reg()
                if found:
                    log("sign found after the sandbox had already ended")
                    freeze_kill_record(found)
                    return 3
                log(f"sandbox ended; no sign of driver activity in {polls} polls; client pid {client_pid}; "
                    f"log bytes scanned {tail.off}; processes seen {len(seen)}")
                return 0
            if polls % 20 == 0:
                live = [f"{p}:{i['comm']}" for p, i in seen.items() if i.get("alive")]
                log(f"heartbeat: poll {polls}, log {tail.off} bytes, live {live}")
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
    w.add_argument("--sandbox-pid", type=int, required=True)
    w.add_argument("--log", required=True)
    w.add_argument("--interval", type=float, default=0.5)
    w.add_argument("--bind", dest="binds", action="append", default=[],
                   help="other dirs bound into the sandbox (pruned from the tmpfs scan)")
    w.add_argument("--go", help="the start signal to create once armed (default: OUT/.go)")
    w.add_argument("--no-tmpfs-scan", action="store_true",
                   help="a bare-metal run (run-bare.sh): no sandbox tmpfs mounts to scan; the "
                        "namespace sees the host's files, whose own Proton prefixes would trip S1")
    s.add_argument("files", nargs="+")
    c = sub.add_parser("collect", help="copy a sandbox-writable tree's regular files, never following a link")
    c.add_argument("--src", required=True)
    c.add_argument("--dest", required=True)
    c.add_argument("--name", dest="names", action="append", help="only these top-level names")
    c.add_argument("--exclude", action="append", default=[], help="skip these top-level names")
    return ap


def main():
    a = parser().parse_args()
    if a.mode == "collect":
        notes = collect(a.src, a.dest, names=a.names, exclude=a.exclude)
        if notes:
            with open(os.path.join(a.dest, "collect-notes.txt"), "a") as fh:
                fh.write("".join(n + "\n" for n in notes))
        return 0
    a.root = os.path.realpath(a.root)
    a.out = os.path.realpath(a.out)
    if a.mode == "baseline":
        cmd_baseline(a)
        return 0
    if a.mode == "scanlog":
        return cmd_scanlog(a)
    a.binds = [os.path.realpath(x) for x in a.binds]
    a.go = a.go or os.path.join(a.out, ".go")
    return cmd_watch(a)


if __name__ == "__main__":
    sys.exit(main())
