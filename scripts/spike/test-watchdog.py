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


class S3(unittest.TestCase):
    def scan(self, *lines):
        rules = wd.LogRules(["winebth", "mountmgr"])
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


if __name__ == "__main__":
    sys.exit(0 if unittest.main(exit=False, verbosity=2).result.wasSuccessful() else 1)
