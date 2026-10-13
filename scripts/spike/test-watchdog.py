#!/usr/bin/env python3
"""Tests for scripts/spike/watchdog.py's pure rules: the S1 *.sys comparison and the S3 Wine-log
rules. Stdlib only, no sandbox and no Wine: python3 -I scripts/spike/test-watchdog.py"""
import importlib.util
import pathlib
import sys
import tempfile
import time
import unittest

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("watchdog", HERE / "watchdog.py")
wd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wd)


class S1(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        (self.root / "proton").mkdir()
        (self.root / "pfx").mkdir()
        self.target = self.root / "proton" / "winebth.sys"
        self.target.write_bytes(b"Wine builtin DLL")
        self.link = self.root / "pfx" / "winebth.sys"
        self.link.symlink_to(self.target)
        self.regular = self.root / "pfx" / "mountmgr.sys"
        self.regular.write_bytes(b"Wine builtin DLL")
        self.base = wd.scan_sys(str(self.root))

    def tearDown(self):
        self._tmp.cleanup()

    def rules(self):
        return [f[0] for f in wd.compare_sys(self.base, wd.scan_sys(str(self.root)))]

    def test_nothing_changed(self):
        self.assertEqual(self.rules(), [])

    def test_recreated_link_with_the_same_target_is_quiet(self):
        time.sleep(0.01)
        self.link.unlink()
        self.link.symlink_to(self.target)  # what Proton's prefix re-sync does (look-1)
        self.assertEqual(self.rules(), [])

    def test_link_to_a_different_file_triggers(self):
        other = self.root / "proton" / "agt.sys"
        other.write_bytes(b"MZ driver")
        self.link.unlink()
        self.link.symlink_to(other)
        self.assertIn("S1 changed .sys file", self.rules())

    def test_changed_link_target_triggers(self):
        time.sleep(0.01)
        self.target.write_bytes(b"MZ a different driver")
        self.assertIn("S1 changed .sys file", self.rules())

    def test_new_driver_file_triggers(self):
        (self.root / "pfx" / "agt.sys").write_bytes(b"MZ")
        self.assertEqual(self.rules(), ["S1 new .sys file"])

    def test_rewritten_regular_file_triggers(self):
        time.sleep(0.01)
        self.regular.write_bytes(b"MZ something else")
        self.assertEqual(self.rules(), ["S1 changed .sys file"])


CLIENT = r'1.000:0020:0024:trace:process:CreateProcessInternalW app (null) cmdline L"\"Z:\\c\\bin64\\gujianol.exe\""'
AGT_LOADDRIVER = r'2.000:0098:00a4:trace:ntoskrnl:ZwLoadDriver (L"\\Registry\\Machine\\System\\CurrentControlSet\\Services\\AGT")'


WINEDEVICE = (r'1.500:0030:0034:trace:process:NtCreateUserProcess L"\\??\\C:\\windows\\system32\\winedevice.exe" '
              "pid 003c tid 0040 handles 0xf8/0xfc")
DISCORD = r"c:\\windows\\system32\\discord\\bridge.exe --service"


class S3(unittest.TestCase):
    def scan(self, *lines):
        rules = wd.LogRules(["winebth", "mountmgr", "discord-bridge"],
                            {"discord-bridge": wd.norm_image(DISCORD)})
        trig, notes = rules.scan(("\n".join(lines) + "\n").encode())
        return [t[0] for t in trig], [n[0] for n in notes]

    def test_split_winebth_line_after_the_client_is_a_note(self):  # run 7
        trig, notes = self.scan(
            CLIENT,
            "2.000:0098:00a4:err:ntoskrnl:ZwLoadDriver flags 0x1, load_path 0000000000000000, "
            "name 0000000000A1F840, base 0000000000A1F838.",
            '2.000:0098:00a4:trace:module:LdrGetDllHandleEx L"ntdll.dll" -> 00006FFFFFEF0000 (load path (null))',
            r'failed to create driver L"\\Registry\\Machine\\System\\CurrentControlSet\\Services\\winebth": c0000142',
        )
        self.assertEqual(trig, [])
        self.assertEqual(notes, ["LoadDriver for baseline service winebth (message split across lines)"])

    def test_loaddriver_before_the_client_exists_is_a_note(self):
        trig, notes = self.scan(AGT_LOADDRIVER.replace("2.000:", "1.000:", 1))
        self.assertEqual(trig, [])
        self.assertTrue(notes[0].startswith("LoadDriver logged at 1.000, before the client's process creation"))

    def test_non_baseline_loaddriver_after_the_client_triggers(self):
        trig, _ = self.scan(CLIENT, AGT_LOADDRIVER)
        self.assertEqual(trig, ["S3 LoadDriver for non-baseline service agt"])

    def test_native_sys_load_triggers(self):
        trig, _ = self.scan(
            r'2.000:0134:0138:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\drivers\\agt.sys" '
            "at 0000000140000000: native"
        )
        self.assertEqual(trig, ["S3 .sys module loaded native"])

    def test_pe_resource_lookup_triggers(self):
        trig, _ = self.scan('2.000:0134:0138:trace:resource:FindResourceExW 140000000 L"PE" #0090 0')
        self.assertEqual(trig, ['S3 FindResource of type "PE" (embedded driver resource type)'])

    def test_driver_strings_trigger(self):
        trig, _ = self.scan('2.000:0134:0138:warn:debugstr:OutputDebugStringA "ProcessProtect ready"')
        self.assertEqual(trig, ["S3 driver string in log"])

    def test_createservice_for_an_unknown_service_triggers(self):
        trig, _ = self.scan('2.000:0134:0138:trace:service:CreateServiceW 0000000000000001 L"AGT" L"AGT"')
        self.assertEqual(trig, ["S3 CreateService for non-baseline service 'AGT'"])


    def test_native_sys_load_in_any_letter_case_triggers(self):  # review I2
        trig, _ = self.scan(
            r'2.000:0134:0138:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\drivers\\agt.sYs" '
            "at 0000000140000000: native"
        )
        self.assertEqual(trig, ["S3 .sys module loaded native"])

    def test_native_module_from_a_drivers_directory_triggers(self):  # review I2
        trig, _ = self.scan(
            r'2.000:0134:0138:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\DRIVERS\\agt.dll" '
            "at 0000000140000000: native"
        )
        self.assertEqual(trig, ["S3 native module loaded from a drivers directory"])

    def test_native_module_in_a_winedevice_process_triggers(self):  # review I2
        trig, _ = self.scan(
            WINEDEVICE,
            r'2.000:003c:0040:trace:loaddll:build_module Loaded L"Z:\\c\\bin64\\agt.dll" at 0000000180000000: native',
        )
        self.assertEqual(trig, ["S3 native module loaded into winedevice.exe"])

    def test_the_clients_own_native_modules_are_quiet(self):  # m1-accept3's 16 native loads
        trig, notes = self.scan(
            WINEDEVICE,
            CLIENT,
            r'1.459:0020:0024:trace:process:NtCreateUserProcess L"\\??\\Z:\\c\\bin64\\gujianol.exe" pid 0134 tid 0138',
            r'1.463:0134:0138:trace:loaddll:build_module Loaded L"Z:\\c\\bin64\\gameforge_api64.dll" at 00006FFFF99D0000: native',
            r'1.470:003c:0040:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\ntoskrnl.exe" at 0000000140000000: builtin',
        )
        self.assertEqual((trig, notes), ([], []))

    def test_pe_resource_lookup_through_ntdll_triggers(self):
        trig, _ = self.scan(
            '2.000:0134:0138:trace:resource:LdrFindResource_U module 0000000140000000 type L"PE" name 0090 lang 0000 level 3'
        )
        self.assertEqual(trig, ['S3 FindResource of type "PE" (embedded driver resource type)'])

    def test_createservice_re_registering_a_baseline_service_is_a_note(self):  # m1-accept2's discord-bridge
        trig, notes = self.scan(
            '2.000:0020:0024:trace:service:CreateServiceW 000000000080C950 L"discord-bridge" L"Wine Discord RPC Bridge"',
            '2.000:0030:0048:trace:service:svcctl_CreateServiceW (L"discord-bridge", L"Wine Discord RPC Bridge", '
            f'0xf01ff, L"{DISCORD}")',
        )
        self.assertEqual(trig, [])
        self.assertEqual(len(notes), 2)

    def test_createservice_that_re_points_a_baseline_service_triggers(self):  # review I1
        trig, _ = self.scan(
            '2.000:0030:0048:trace:service:svcctl_CreateServiceW (L"discord-bridge", L"Wine Discord RPC Bridge", '
            r'0xf01ff, L"C:\\agt\\agt.sys")'
        )
        self.assertEqual(trig, ["S3 CreateService re-points baseline service discord-bridge"])

    def test_a_loaddriver_without_a_name_after_the_client_triggers(self):  # review test gap
        trig, _ = self.scan(
            CLIENT,
            "2.000:0098:00a4:err:ntoskrnl:ZwLoadDriver flags 0x1, load_path 0000000000000000, "
            "name 0000000000A1F840, base 0000000000A1F838.",
        )
        self.assertEqual(trig, ["S3 LoadDriver for non-baseline service None"])


SYSTEM_REG = r"""WINE REGISTRY Version 2
[System\\ControlSet001\\Services\\discord-bridge] 1791759044
#time=1dd59d2f2ae0cb0
"DisplayName"="Wine Discord RPC Bridge"
"ImagePath"=str(2):"c:\\windows\\system32\\discord\\bridge.exe --service"
"Type"=dword:00000010

[System\\ControlSet001\\Services\\winebth] 1789520096
"ImagePath"=str(2):"C:\\windows\\system32\\drivers\\winebth.sys"
"Type"=dword:00000001

[System\\ControlSet001\\Services\\winebth\\Parameters] 1789520096
"Type"=dword:00000002
"""


class S4Registry(unittest.TestCase):
    """The prefix's service keys, compared with the baseline (review I1, M7)."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.reg = self.root / "system.reg"
        self.reg.write_text(SYSTEM_REG)
        self.base = wd.parse_services(str(self.reg))

    def tearDown(self):
        self._tmp.cleanup()

    def findings(self, text):
        self.reg.write_text(text)
        trig, notes = wd.compare_services(self.base, wd.parse_services(str(self.reg)))
        return [t[0] for t in trig], [n[0] for n in notes]

    def test_services_are_parsed_with_type_and_image(self):
        self.assertEqual(
            self.base,
            {
                "discord-bridge": {"type": 0x10, "image": "c:\\windows\\system32\\discord\\bridge.exe --service"},
                "winebth": {"type": 1, "image": "c:\\windows\\system32\\drivers\\winebth.sys"},
            },
        )

    def test_an_unchanged_registry_is_quiet(self):
        self.assertEqual(self.findings(SYSTEM_REG), ([], []))

    def test_a_new_driver_service_triggers(self):
        agt = '\n[System\\\\ControlSet001\\\\Services\\\\AGT] 1\n"ImagePath"="C:\\\\agt.bin"\n"Type"=dword:00000001\n'
        self.assertEqual(self.findings(SYSTEM_REG + agt)[0], ["S4 new driver service in the registry"])

    def test_a_new_ordinary_service_is_a_note(self):
        svc = '\n[System\\\\ControlSet001\\\\Services\\\\Updater] 1\n"ImagePath"="C:\\\\u.exe"\n"Type"=dword:00000010\n'
        self.assertEqual(self.findings(SYSTEM_REG + svc), ([], ["new service in the registry"]))

    def test_a_re_pointed_baseline_service_triggers(self):
        changed = SYSTEM_REG.replace("drivers\\\\winebth.sys", "drivers\\\\agt.dll")
        self.assertEqual(self.findings(changed)[0], ["S4 baseline service changed in the registry"])

    def test_a_linked_registry_file_is_refused(self):
        secret = self.root / "secret"
        secret.write_text(SYSTEM_REG)
        self.reg.unlink()
        self.reg.symlink_to(secret)
        with self.assertRaises(OSError):
            wd.parse_services(str(self.reg))


class ClientRename(unittest.TestCase):  # issue #142
    """Wine starts the client's process as wine64 and renames it once the exe is loaded, so the
    name read when its pid first appeared can be stale."""

    def proc(self, pid, start, comm, argv, alive=True):
        return {"pid": pid, "start": start, "comm": comm, "argv": argv, "alive": alive}

    def test_a_process_renamed_to_the_client_is_found(self):
        seen = {
            7: self.proc(7, 100, "wine64", ["/usr/bin/wine64", "C:\\SoLO\\bin64\\gujianol.exe"]),
            8: self.proc(8, 90, "wineserver", ["wineserver"]),
            9: self.proc(9, 95, "wine64", ["wine64"], alive=False),
        }
        now = {
            7: self.proc(7, 100, "gujianol.exe", ["C:\\SoLO\\bin64\\gujianol.exe"]),
            8: self.proc(8, 90, "wineserver", ["wineserver"]),
            9: self.proc(9, 95, "gujianol.exe", ["gujianol.exe"]),
        }
        found = wd.renamed_to(seen, wd.CLIENT, now.get)
        self.assertEqual([p for p, _ in found], [7])
        self.assertEqual(seen[7]["comm"], "gujianol.exe")
        self.assertEqual(wd.renamed_to(seen, wd.CLIENT, now.get), [], "found once")

    def test_a_pid_now_held_by_another_process_is_not_the_client(self):
        seen = {7: self.proc(7, 100, "wine64", ["wine64"])}
        now = {7: self.proc(7, 250, "gujianol.exe", ["gujianol.exe"])}
        self.assertEqual(wd.renamed_to(seen, wd.CLIENT, now.get), [])

    def test_a_process_that_is_gone_is_skipped(self):
        seen = {7: self.proc(7, 100, "wine64", ["wine64"])}
        self.assertEqual(wd.renamed_to(seen, wd.CLIENT, lambda p: None), [])


class ClientCreationOverdue(unittest.TestCase):
    """Review M3: if the log never shows the client's creation, every LoadDriver would count as
    'before the client'; the watchdog stops the run instead of going blind."""

    def test_overdue_only_after_the_grace_period_with_the_log_caught_up(self):
        f = wd.client_creation_overdue
        self.assertFalse(f(None, 100.0, None, True))  # client not seen yet
        self.assertFalse(f(10.0, 25.0, None, True))  # within the grace period
        self.assertTrue(f(10.0, 31.0, None, True))
        self.assertFalse(f(10.0, 31.0, 5.0, True))  # the creation was logged
        self.assertFalse(f(10.0, 31.0, None, False))  # the log scan is behind

class SandboxWritableFiles(unittest.TestCase):
    """Everything the sandbox can write may be a planted link: the host side never follows one."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.secret = self.root / "host-secret"
        self.secret.write_text("do not copy")
        self.src = self.root / "io"
        (self.src / "sub").mkdir(parents=True)
        (self.src / "proton.out").write_text("log")
        (self.src / "sub" / "stub.log").write_text("stub")
        (self.src / "link").symlink_to(self.secret)
        (self.src / "linkdir").symlink_to(self.root)
        self.dest = self.root / "evidence"

    def tearDown(self):
        self._tmp.cleanup()

    def test_collect_copies_regular_files_and_records_links(self):
        notes = wd.collect(str(self.src), str(self.dest))
        self.assertEqual((self.dest / "proton.out").read_text(), "log")
        self.assertEqual((self.dest / "sub" / "stub.log").read_text(), "stub")
        self.assertFalse((self.dest / "link").exists() or (self.dest / "link").is_symlink())
        self.assertFalse((self.dest / "linkdir").exists())
        self.assertEqual(sorted(n.split(" ")[0] for n in notes), ["link", "link"])
        self.assertNotIn("do not copy", "".join(p.read_text() for p in self.dest.rglob("*") if p.is_file()))

    def test_collect_can_take_only_some_names(self):
        wd.collect(str(self.src), str(self.dest), names=["proton.out", "link"])
        self.assertEqual(sorted(p.name for p in self.dest.iterdir()), ["proton.out"])

    def test_the_go_file_is_created_exclusively(self):
        go = self.root / ".go"
        wd.create_exclusive(str(go))
        self.assertTrue(go.is_file())
        planted = self.root / ".go2"
        planted.symlink_to(self.root / "elsewhere")
        with self.assertRaises(OSError):
            wd.create_exclusive(str(planted))
        self.assertFalse((self.root / "elsewhere").exists())

    def test_the_log_is_never_read_through_a_link(self):
        log = self.src / "steam-1418100.log"
        log.symlink_to(self.secret)
        with self.assertRaises(OSError):
            wd.Tail(str(log)).read()

    def test_a_planted_link_is_described_not_hashed(self):
        self.assertTrue(wd.describe_file(str(self.src / "link")).startswith("symlink to "))
        self.assertTrue(wd.describe_file(str(self.src / "proton.out")).startswith("sha256 "))

    def test_the_tmpfs_scan_skips_bound_directories(self):
        # The sandbox's view of a tmpfs $HOME holding a read-only bind of GE-Proton (m1-accept).
        r = self.root / "proc-root"
        proton = r / "home" / "u" / "tools" / "GE-Proton" / "files"
        proton.mkdir(parents=True)
        (proton / "winebth.sys").write_bytes(b"Wine builtin")
        (r / "tmp").mkdir()
        (r / "tmp" / "agt.sys").write_bytes(b"MZ")
        hits = wd.scan_tmpfs(str(r), ["/tmp", "/home/u"], binds=["/home/u/tools/GE-Proton"])
        self.assertEqual(sorted(pathlib.Path(p).name for p in hits), ["agt.sys"])

    def test_the_tmpfs_scan_covers_the_sandbox_root_and_etc(self):  # review M2
        r = self.root / "proc-root"
        (r / "etc" / "x").mkdir(parents=True)
        (r / "usr").mkdir()
        (r / "top.sys").write_bytes(b"MZ")
        (r / "etc" / "x" / "deep.sys").write_bytes(b"MZ")
        (r / "usr" / "ro-bound.sys").write_bytes(b"not scanned: a read-only bind")
        hits = wd.scan_tmpfs(str(r), wd.TMPFS_SUBS, binds=[], shallow=wd.TMPFS_SHALLOW)
        self.assertEqual(sorted(pathlib.Path(p).name for p in hits), ["deep.sys", "top.sys"])

    def test_collect_notes_escape_names(self):  # review M6
        (self.src / "evil\nspecial forged (not copied)").symlink_to(self.secret)
        notes = wd.collect(str(self.src), str(self.dest))
        self.assertTrue(all("\n" not in n for n in notes), notes)


class GateEventRecord(unittest.TestCase):
    """Final review Minor 5: a trigger record that can't be written must not turn a gate event
    (exit 3) into a watchdog failure (exit 4)."""

    def test_a_gate_event_stays_exit_3_when_its_record_cant_be_written(self):
        import json
        import subprocess
        import types
        with tempfile.TemporaryDirectory() as tmp:
            root, out = pathlib.Path(tmp, "root"), pathlib.Path(tmp, "out")
            root.mkdir()
            out.mkdir()
            (out / "watchdog-baseline.json").write_text(json.dumps({"sys": {}, "services": [], "service_info": {}}))
            (root / "agt.sys").write_bytes(b"MZ")  # S1 at the first poll
            (out / "watchdog-trigger.txt").mkdir()  # so the record can't be written
            child = subprocess.Popen(["sleep", "30"])  # stands in for the sandbox; the watchdog kills it
            try:
                a = types.SimpleNamespace(root=str(root), out=str(out), sandbox_pid=child.pid,
                                          log=str(out / "none.log"), interval=0.05, binds=[], go=str(out / ".go"))
                rc = wd.cmd_watch(a)
            finally:
                child.kill()
                child.wait()
            self.assertEqual(rc, 3)
            self.assertIn("GATE EVENT", (out / "watchdog.log").read_text())

class LogNeverRaises(unittest.TestCase):
    """Scoped re-review: a full disk must not make the watchdog's own log raise, or a gate event
    would end as an uncaught error (exit 1) instead of exit 3."""

    def test_a_failed_log_write_falls_back_and_does_not_raise(self):
        class Full:
            def write(self, _):
                raise OSError(28, "No space left on device")

        with tempfile.TemporaryDirectory() as tmp:
            log = wd.Log(str(pathlib.Path(tmp, "watchdog.log")))
            log.f.close()
            log.f = Full()
            log("GATE EVENT: S1 new .sys file: /x/agt.sys")  # must not raise

if __name__ == "__main__":
    sys.exit(0 if unittest.main(exit=False, verbosity=2).result.wasSuccessful() else 1)
