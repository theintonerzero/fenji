#!/usr/bin/env python3
"""Tests for scripts/spike/watchdog.py's pure rules: the S1 *.sys comparison and the S3 Wine-log
rules. Stdlib only, no sandbox and no Wine: python3 -I scripts/spike/test-watchdog.py"""
import contextlib
import importlib.util
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import types
import unittest

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("watchdog", HERE / "watchdog.py")
wd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wd)


class Blocked(Exception):
    """Raised by the alarm in tests that must not block; not an OSError, so assertRaises(OSError)
    can't take it for the refusal."""


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

# +service's lines for ChangeServiceConfigW (Wine 11.0, GE-Proton11-7; issue #202). sechost's, in the
# calling process, is "%p %ld %ld %ld %s %s %p %p %s %s %s": the service's handle; the new type, start
# type and error control, -1 for SERVICE_NO_CHANGE; the image, group, tag and dependencies pointers,
# account, password and display name. services.exe's svcctl_ChangeServiceConfigW logs its name only.
# Neither names the service: the caller's OpenServiceW, before them, does.
SVCCTL_CHANGE = "2.000:0030:0048:trace:service:svcctl_ChangeServiceConfigW "
UNSEEN_CHANGE = "S3 services.exe changed a service's configuration, with no ChangeServiceConfigW before it"
USER_MODE_NOTE = "ChangeServiceConfig changes the type or image of user-mode baseline service discord-bridge"


def open_service(name, pid="0134"):
    return f'2.000:{pid}:0138:trace:service:OpenServiceW 0000000000A1F3D0 L"{name}" 0xf01ff'


def change_config(stype=-1, image=None, start=-1, pid="0134", shown=None, fn="ChangeServiceConfigW"):
    """sechost's line; `image` is the new image as the log escapes it, `shown` the field verbatim."""
    if shown is None:
        shown = "(null)" if image is None else f'L"{image}"'
    return (f"2.000:{pid}:0138:trace:service:{fn} 0000000000A1F420 {stype} {start} -1 {shown} (null) "
            "0000000000000000 0000000000000000 (null) (null) (null)")


def changes(names):
    return f"S3 ChangeServiceConfig changes the type or image of {names}"


class S3(unittest.TestCase):
    def rules(self):
        return wd.LogRules(["winebth", "mountmgr", "discord-bridge"],
                           {"discord-bridge": wd.norm_image(DISCORD)}, drivers=["winebth", "mountmgr"])

    def scan(self, *lines):
        trig, notes = self.rules().scan(("\n".join(lines) + "\n").encode())
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

    def test_a_re_point_with_a_null_display_name_triggers(self):  # issue #148
        trig, _ = self.scan(
            '2.000:0030:0048:trace:service:svcctl_CreateServiceW (L"discord-bridge", (null), '
            r'0xf01ff, L"C:\\agt\\agt.sys")'
        )
        self.assertEqual(trig, ["S3 CreateService re-points baseline service discord-bridge"])

    def test_a_null_display_name_with_the_baseline_image_is_a_note(self):  # issue #148
        trig, notes = self.scan(
            '2.000:0030:0048:trace:service:svcctl_CreateServiceW (L"discord-bridge", (null), '
            f'0xf01ff, L"{DISCORD}")'
        )
        self.assertEqual(trig, [])
        self.assertEqual(notes, ["CreateService for baseline service discord-bridge, with its baseline image"])

    def test_a_createservice_line_without_a_readable_image_triggers(self):  # issue #148
        trig, _ = self.scan(
            '2.000:0030:0048:trace:service:svcctl_CreateServiceW (L"discord-bridge", '
            'L"Wine Discord RPC Bridge", 0xf01ff, (null))'
        )
        self.assertEqual(trig, ["S3 CreateService for baseline service discord-bridge, with no readable image"])

    def test_loaddriver_for_a_user_mode_baseline_service_after_the_client_triggers(self):  # issue #148
        trig, notes = self.scan(CLIENT, AGT_LOADDRIVER.replace(r"Services\\AGT", r"Services\\discord-bridge"))
        self.assertEqual(trig, ["S3 LoadDriver for user-mode baseline service discord-bridge"])
        self.assertEqual(notes, [])

    def test_changeserviceconfig_of_a_baseline_driver_service_triggers(self):  # issue #202
        for change in (change_config(image=r"C:\\agt\\agt.bin"), change_config(stype=16)):
            trig, _ = self.scan(open_service("winebth"), change, SVCCTL_CHANGE)
            self.assertEqual(trig, [changes("baseline driver service winebth")], change)

    def test_changeserviceconfig_of_a_user_mode_baseline_service_is_a_note(self):  # issue #202
        change = change_config(stype=32, image=DISCORD.replace("--service", "--other"))
        trig, notes = self.scan(open_service("discord-bridge"), change, SVCCTL_CHANGE)
        self.assertEqual(trig, [])
        self.assertEqual(notes, [USER_MODE_NOTE])

    def test_changeserviceconfig_that_makes_a_service_a_driver_triggers(self):  # issue #202
        trig, _ = self.scan(open_service("discord-bridge"), change_config(stype=1), SVCCTL_CHANGE)
        self.assertEqual(trig, ["S3 ChangeServiceConfig sets service type 0x1, not a user-mode one"])
        trig, _ = self.scan(open_service("discord-bridge"), change_config(image=r"C:\\agt\\AGT.SYS"),
                            SVCCTL_CHANGE)
        self.assertEqual(trig, ["S3 ChangeServiceConfig sets a driver image"])

    def test_changeserviceconfig_stands_for_every_service_its_process_opened(self):  # issue #202
        # The line has the service's handle, not its name: any service the process opened may be it.
        trig, _ = self.scan(open_service("winebth"), open_service("discord-bridge"), change_config(stype=16),
                            SVCCTL_CHANGE)
        self.assertEqual(trig, [changes("baseline driver service winebth")])

    def test_changeserviceconfig_for_a_service_the_log_doesnt_name_triggers(self):  # issue #202
        # A service handle is its own process's: an open in another process names nothing.
        trig, _ = self.scan(open_service("discord-bridge", pid="0020"), change_config(stype=16),
                            SVCCTL_CHANGE)
        self.assertEqual(trig, [changes("a service the log doesn't name")])

    def test_changeserviceconfig_for_a_service_outside_the_baseline_triggers(self):  # issue #202
        trig, _ = self.scan(open_service("AGT"), change_config(stype=16), SVCCTL_CHANGE)
        self.assertEqual(trig, [changes("agt, not a user-mode baseline service")])
        unreadable = open_service("x").replace('L"x"', "(invalid)")
        trig, _ = self.scan(unreadable, open_service("discord-bridge"), change_config(stype=16), SVCCTL_CHANGE)
        self.assertEqual(trig, [changes("(an unreadable name), not a user-mode baseline service")])

    def test_changeserviceconfig_that_keeps_the_type_and_image_is_a_note(self):  # issue #202
        trig, notes = self.scan(open_service("winebth"), change_config(start=2), SVCCTL_CHANGE)
        self.assertEqual(trig, [])
        self.assertEqual(notes, ["ChangeServiceConfig that keeps the type and image"])

    def test_changeserviceconfig_with_an_image_it_cant_read_triggers(self):  # issue #202
        # debugstr_w's forms for a bad pointer, and for a string it cut short.
        for shown in ("(invalid)", 'L"C:\\\\agt\\\\' + "a" * 280 + '"...'):
            trig, _ = self.scan(open_service("discord-bridge"), change_config(shown=shown), SVCCTL_CHANGE)
            self.assertEqual(trig, ["S3 ChangeServiceConfig with no readable type or image"], shown)

    def test_a_services_exe_change_with_no_call_before_it_triggers(self):  # issue #202
        # A caller whose sechost logs nothing (another environment, or the RPC called directly) leaves
        # only services.exe's line. ChangeServiceConfigA's line is followed by W's, which is the one.
        self.assertEqual(self.scan(SVCCTL_CHANGE)[0], [UNSEEN_CHANGE])
        a_then_w = (change_config(stype=16, fn="ChangeServiceConfigA"), change_config(stype=16))
        trig, notes = self.scan(open_service("discord-bridge"), *a_then_w, SVCCTL_CHANGE, SVCCTL_CHANGE)
        self.assertEqual(trig, [UNSEEN_CHANGE])
        self.assertEqual(notes, [USER_MODE_NOTE])

    def test_the_open_and_the_change_can_come_in_separate_reads(self):  # issue #202: the watch reads chunks
        rules, found = self.rules(), []
        for line in (open_service("winebth"), change_config(stype=16), SVCCTL_CHANGE):
            found += [t[0] for t in rules.scan((line + "\n").encode())[0]]
        self.assertEqual(found, [changes("baseline driver service winebth")])


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

    def test_a_baselines_log_rules_exempt_only_its_driver_services(self):  # issue #148
        rules = wd.baseline_rules({"services": sorted(self.base), "service_info": self.base})
        lines = [CLIENT] + [AGT_LOADDRIVER.replace(r"Services\\AGT", "Services\\\\" + s) for s in ("winebth", "discord-bridge")]
        trig, notes = rules.scan(("\n".join(lines) + "\n").encode())
        self.assertEqual([t[0] for t in trig], ["S3 LoadDriver for user-mode baseline service discord-bridge"])
        self.assertEqual([n[0] for n in notes], ["LoadDriver for baseline service winebth"])

    def test_a_baselines_log_rules_know_its_user_mode_services(self):  # issue #202
        rules = wd.baseline_rules({"services": sorted(self.base), "service_info": self.base})
        lines = [open_service("discord-bridge", pid="0020"), change_config(stype=16, pid="0020"),
                 open_service("winebth"), change_config(stype=16)]
        trig, notes = rules.scan(("\n".join(lines) + "\n").encode())
        self.assertEqual([t[0] for t in trig], [changes("baseline driver service winebth")])
        self.assertEqual([n[0] for n in notes], [USER_MODE_NOTE])

    def test_a_baseline_without_service_info_knows_no_user_mode_service(self):  # issue #202
        rules = wd.baseline_rules({"services": ["discord-bridge"]})  # M0's baselines (r5-r9, look-1, look-2)
        trig, _ = rules.scan(f"{open_service('discord-bridge')}\n{change_config(stype=16)}\n".encode())
        self.assertEqual([t[0] for t in trig], [changes("discord-bridge, not a user-mode baseline service")])


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


class TrackClient(unittest.TestCase):  # issue #142: both of S2's operands can be named late
    def proc(self, pid, start, comm, alive=True):
        return {"pid": pid, "start": start, "comm": comm, "argv": [comm], "alive": alive}

    def test_a_client_found_late_is_the_earliest_renamed(self):
        seen = {7: self.proc(7, 100, "wine64"), 9: self.proc(9, 90, "wine64")}
        now = {p: self.proc(p, seen[p]["start"], "gujianol.exe") for p in seen}
        found, hits = wd.track_client(seen, None, now.get)
        self.assertEqual(found, (90, 9))
        self.assertEqual(hits, [])

    def test_s2_sees_a_winedevice_renamed_after_the_client(self):
        seen = {11: self.proc(11, 150, "wine64")}
        now = {11: self.proc(11, 150, "winedevice.exe")}
        found, hits = wd.track_client(seen, 100, now.get)
        self.assertIsNone(found)
        self.assertEqual([p for p, _ in hits], [11])

    def test_s2_counts_a_winedevice_that_is_gone_already(self):
        seen = {12: self.proc(12, 150, "winedevice.exe", alive=False)}
        self.assertEqual([p for p, _ in wd.track_client(seen, 100, lambda p: None)[1]], [12])

    def test_a_winedevice_older_than_the_client_is_no_sign(self):
        seen = {13: self.proc(13, 50, "winedevice.exe")}
        self.assertEqual(wd.track_client(seen, 100, lambda p: None)[1], [])

    def test_no_client_means_no_s2(self):
        seen = {14: self.proc(14, 150, "winedevice.exe")}
        self.assertEqual(wd.track_client(seen, None, lambda p: None), (None, []))


# The stand-ins for the whole-loop tests below. Each runs through a symlink to this Python, so it
# starts with that name, as a Wine process starts as wine64.
_PROC = """import ctypes, sys, time
name, rename_at, die_at = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
def until(t):
    if t > time.time():
        time.sleep(t - time.time())
if name != "-":
    until(rename_at)
    ctypes.CDLL(None).prctl(15, name.encode(), 0, 0, 0)  # PR_SET_NAME, as Wine renames
until(die_at)
"""
_INIT = """import json, os, subprocess, sys, time
d, plan, go = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
deadline = time.time() + 15  # a watchdog that never arms fails the test, not the CI job
while not os.path.exists(go):
    if time.time() > deadline:
        sys.exit(9)
    time.sleep(0.005)
t0 = time.time()
procs = []
for at, binary, rename, rename_t, die_t in plan:
    if t0 + at > time.time():
        time.sleep(t0 + at - time.time())
    procs.append(subprocess.Popen([f"{d}/bin/{binary}", f"{d}/proc.py", rename, str(t0 + rename_t), str(t0 + die_t)]))
for q in procs:
    q.wait()
"""
_OUTER = """import subprocess, sys
subprocess.Popen([f"{sys.argv[1]}/bin/bwrap", f"{sys.argv[1]}/init.py"] + sys.argv[1:]).wait()
"""
# A sandbox whose bwrap never gets its init (a child named bwrap): one stand-in that ends in 1.5 s.
_OUTER_NO_INIT = """import subprocess, sys, time
subprocess.Popen([f"{sys.argv[1]}/bin/wine64", f"{sys.argv[1]}/proc.py", "-", "0", str(time.time() + 1.5)]).wait()
"""


class WholeLoop(unittest.TestCase):
    """cmd_watch itself over a fake sandbox of stand-in processes renamed with prctl: no Wine and
    no client. Times are seconds after the start signal, with margins far above the 0.05 s polls."""

    def watch(self, plan, outer_script=_OUTER, **extra):
        with tempfile.TemporaryDirectory() as tmp:
            d = pathlib.Path(tmp)
            (d / "bin").mkdir()
            for name in ("bwrap", "wine64", "gujianol.exe", "winedevice.exe"):
                (d / "bin" / name).symlink_to(sys.executable)
            for name, text in (("proc.py", _PROC), ("init.py", _INIT), ("outer.py", outer_script)):
                (d / name).write_text(text)
            out = d / "out"
            (out / "root").mkdir(parents=True)
            (out / "watchdog-baseline.json").write_text(json.dumps({"sys": {}, "services": [], "service_info": {}}))
            outer = subprocess.Popen([str(d / "bin" / "bwrap"), str(d / "outer.py"), str(d), json.dumps(plan),
                                      str(out / ".go")], start_new_session=True)
            a = types.SimpleNamespace(root=str(out / "root"), out=str(out), sandbox_pid=outer.pid,
                                      log=str(out / "none.log"), interval=0.05, binds=[], go=str(out / ".go"),
                                      tmpfs=None, prefix=str(out / "root" / "compat" / "pfx"))
            vars(a).update(extra)
            saved = wd.TMPFS_SUBS, wd.TMPFS_SHALLOW
            wd.TMPFS_SUBS = wd.TMPFS_SHALLOW = ()  # the host's own files are not this test's sandbox
            try:
                rc = wd.cmd_watch(a)
            except SystemExit as e:  # a failure (exit 4) leaves through SystemExit
                rc = e.code
            finally:
                wd.TMPFS_SUBS, wd.TMPFS_SHALLOW = saved
                try:
                    os.killpg(outer.pid, signal.SIGKILL)
                except OSError:
                    pass
                outer.wait()
            self.go_created = (out / ".go").exists()
            trig = out / "watchdog-trigger.txt"
            return rc, trig.read_text() if trig.exists() else ""


@unittest.skipUnless(sys.platform.startswith("linux"), "needs /proc and prctl")
class RenameWholeLoop(WholeLoop):  # issue #142

    def test_a_client_renamed_late_stays_found_and_arms_s2(self):
        # The winedevice starts after the rename, so only a client that stayed found sees it.
        rc, trig = self.watch([[0.0, "wine64", "gujianol.exe", 0.6, 6.0],
                               [1.4, "winedevice.exe", "-", 0, 5.0]])
        self.assertEqual(rc, 3)
        self.assertIn("S2", trig)

    def test_a_client_found_late_rearms_the_log_check(self):
        # Found late, the client starts the clock for "the log never showed its creation".
        saved = wd.client_creation_overdue.__defaults__
        wd.client_creation_overdue.__defaults__ = (0.5,)
        try:
            rc, _ = self.watch([[0.0, "wine64", "gujianol.exe", 0.6, 6.0]])
        finally:
            wd.client_creation_overdue.__defaults__ = saved
        self.assertEqual(rc, 4)

    def test_a_winedevice_renamed_late_still_trips_s2(self):
        rc, trig = self.watch([[0.0, "gujianol.exe", "-", 0, 6.0],
                               [0.4, "wine64", "winedevice.exe", 0.9, 5.0]])
        self.assertEqual(rc, 3)
        self.assertIn("S2", trig)

    def test_a_winedevice_older_than_a_late_named_client_is_no_sign(self):
        rc, _ = self.watch([[0.0, "winedevice.exe", "-", 0, 2.0],
                            [0.4, "wine64", "gujianol.exe", 0.8, 2.5]])
        self.assertEqual(rc, 0)


@unittest.skipUnless(sys.platform.startswith("linux"), "needs /proc and prctl")
class BareWholeLoop(WholeLoop):  # issue #148
    """A bare run's own tmpfs mounts (--tmpfs), seen through /proc/PID/root, and its prefix
    (--prefix). The fake sandbox shares the host's mounts, so a host directory stands in for one."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.ns = pathlib.Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_a_sys_file_in_a_named_tmpfs_mount_triggers(self):
        (self.ns / "agt.sys").write_bytes(b"MZ")
        rc, trig = self.watch([[0.0, "gujianol.exe", "-", 0, 4.0]], tmpfs=[str(self.ns)])
        self.assertEqual(rc, 3)
        self.assertIn("S1 .sys file in a sandbox tmpfs mount", trig)

    def test_what_the_namespace_binds_into_a_tmpfs_mount_is_skipped(self):
        (self.ns / "pulse").mkdir()
        (self.ns / "pulse" / "agt.sys").write_bytes(b"the host's")
        (self.ns / "X99.sys").write_bytes(b"the host's")
        rc, _ = self.watch([[0.0, "gujianol.exe", "-", 0, 1.0]], tmpfs=[str(self.ns)],
                           binds=[str(self.ns / "pulse"), str(self.ns / "X99.sys")])
        self.assertEqual(rc, 0)

    def test_s4_reads_the_named_prefix(self):
        (self.ns / "system.reg").write_text(SYSTEM_REG)  # winebth: a driver service the baseline lacks
        rc, trig = self.watch([[0.0, "gujianol.exe", "-", 0, 1.0]], prefix=str(self.ns))
        self.assertEqual(rc, 3)
        self.assertIn("S4 new driver service in the registry", trig)

    def test_no_start_signal_while_the_namespaces_init_is_unknown(self):  # the client-start guard
        rc, _ = self.watch([], outer_script=_OUTER_NO_INIT)
        self.assertEqual(rc, 0)
        self.assertFalse(self.go_created)


class BareMetal(unittest.TestCase):  # issues #139, #148
    """A bare-metal run's namespace sees the host's files, whose own Proton prefixes would trip the
    sandbox's tmpfs scan; its own tmpfs mounts (/run, /tmp, /dev/shm) are named with --tmpfs. Its
    prefix is umu's, whose pfx is a link to the prefix, so the runner names it with --prefix."""

    WATCH = ["watch", "--root", "/r", "--out", "/o", "--sandbox-pid", "1", "--log", "/l"]
    BASELINE = ["baseline", "--root", "/r", "--out", "/o"]

    def test_the_tmpfs_mounts_are_the_sandboxes_unless_named(self):
        self.assertIsNone(wd.parser().parse_args(self.WATCH).tmpfs)
        self.assertEqual(wd.parser().parse_args(self.WATCH + ["--tmpfs", "/tmp", "--tmpfs", "/run"]).tmpfs,
                         ["/tmp", "/run"])

    def test_baseline_and_watch_take_the_prefix(self):
        for args in (self.WATCH, self.BASELINE):
            self.assertIsNone(wd.parser().parse_args(args).prefix)
            self.assertEqual(wd.parser().parse_args(args + ["--prefix", "/p"]).prefix, "/p")

    def baseline_services(self, *extra):
        with tempfile.TemporaryDirectory() as tmp:
            root, out = pathlib.Path(tmp, "root"), pathlib.Path(tmp, "out")
            (root / "compat" / "pfx").mkdir(parents=True)
            (root / "compat" / "pfx" / "system.reg").write_text(SYSTEM_REG)  # run-client.sh's layout
            (root / "umu").mkdir()
            (root / "umu" / "system.reg").write_text(SYSTEM_REG.split("\n[System\\\\ControlSet001\\\\Services\\\\winebth]")[0])
            out.mkdir()
            argv = ["watchdog.py", "baseline", "--root", str(root), "--out", str(out)]
            argv += [x.replace("ROOT", str(root)) for x in extra]
            saved = sys.argv
            sys.argv = argv
            try:
                with open(os.devnull, "w") as quiet, contextlib.redirect_stdout(quiet):
                    self.assertEqual(wd.main(), 0)
            finally:
                sys.argv = saved
            return json.loads((out / "watchdog-baseline.json").read_text())["services"]

    def test_the_baseline_reads_root_compat_pfx_by_default(self):
        self.assertEqual(self.baseline_services(), ["discord-bridge", "winebth"])

    def test_the_baseline_reads_the_named_prefix(self):
        self.assertEqual(self.baseline_services("--prefix", "ROOT/umu"), ["discord-bridge"])


class TailFollowsAReplacedLog(unittest.TestCase):  # issue #144
    """GE-Proton removes and recreates its log for each session (umu's createprefix, then the
    client), so the watchdog must leave the old file at its end and read the new one."""

    def drain(self, t, polls=4):
        out = b""
        for _ in range(polls):
            chunk, _ = t.read()
            out += chunk
        return out

    def test_a_replaced_log_is_read_to_its_end_then_the_new_one_from_its_start(self):
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d) / "steam-1418100.log"
            p.write_bytes(b"old 1\nold 2\nold partial")
            t = wd.Tail(str(p))
            self.addCleanup(t.close)
            got = self.drain(t)
            p.unlink()
            p.write_bytes(b"new 1\nnew 2\n")
            got += self.drain(t)
            self.assertEqual(got, b"old 1\nold 2\nold partial\nnew 1\nnew 2\n")

    def test_a_truncated_log_is_read_again_from_its_start(self):
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d) / "steam-1418100.log"
            p.write_bytes(b"first session, a long line\n")
            t = wd.Tail(str(p))
            self.addCleanup(t.close)
            got = self.drain(t)
            with open(p, "r+b") as fh:
                fh.truncate(0)
                fh.write(b"again\n")
            got += self.drain(t)
            self.assertEqual(got, b"first session, a long line\nagain\n")

    def test_a_fifo_at_the_log_path_is_refused_without_blocking(self):
        # Review of #144: anything in the namespace can write the run's io directory, and opening a
        # FIFO that has no writer blocks, which would stop the whole watch, not just the log rules.
        def blocked(signum, frame):
            raise Blocked("the watchdog blocked opening a FIFO")
        old = signal.signal(signal.SIGALRM, blocked)
        signal.alarm(5)
        try:
            with tempfile.TemporaryDirectory() as d:
                p = pathlib.Path(d) / "steam-1418100.log"
                p.write_bytes(b"first session\n")
                t = wd.Tail(str(p))
                self.addCleanup(t.close)
                self.assertEqual(self.drain(t), b"first session\n")
                p.unlink()
                os.mkfifo(p)
                with self.assertRaises(OSError):
                    self.drain(t)
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, old)


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
            with contextlib.closing(wd.Tail(str(log))) as t:
                t.read()

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

    def test_the_tmpfs_scan_skips_bound_files(self):  # issue #148: a bare run's sockets, its auth file
        r = self.root / "proc-root"
        (r / "tmp").mkdir(parents=True)
        (r / "tmp" / "bound.sys").write_bytes(b"the host's")
        (r / "tmp" / "agt.sys").write_bytes(b"MZ")
        hits = wd.scan_tmpfs(str(r), ["/tmp"], binds=["/tmp/bound.sys"])
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
                                          log=str(out / "none.log"), interval=0.05, binds=[], go=str(out / ".go"),
                                          tmpfs=None, prefix=str(root / "compat" / "pfx"))
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

    @unittest.skipUnless(os.path.exists("/dev/full"), "needs /dev/full")
    def test_a_gate_event_stays_exit_3_when_closing_the_log_fails(self):
        # A failed write leaves its line in the file's buffer, and closing the file flushes it again.
        with tempfile.TemporaryDirectory() as tmp:
            root, out = pathlib.Path(tmp, "root"), pathlib.Path(tmp, "out")
            root.mkdir()
            out.mkdir()
            (out / "watchdog-baseline.json").write_text(json.dumps({"sys": {}, "services": [], "service_info": {}}))
            (root / "agt.sys").write_bytes(b"MZ")  # S1 at the first poll
            (out / "watchdog.log").symlink_to("/dev/full")  # every write fails, as on a full disk
            child = subprocess.Popen(["sleep", "30"])  # stands in for the sandbox; the watchdog kills it
            try:
                a = types.SimpleNamespace(root=str(root), out=str(out), sandbox_pid=child.pid,
                                          log=str(out / "none.log"), interval=0.05, binds=[], go=str(out / ".go"),
                                          tmpfs=None, prefix=str(root / "compat" / "pfx"))
                with open(os.devnull, "w") as quiet, contextlib.redirect_stderr(quiet):  # the log's fallback
                    rc = wd.cmd_watch(a)
            finally:
                child.kill()
                child.wait()
            self.assertEqual(rc, 3)

if __name__ == "__main__":
    sys.exit(0 if unittest.main(exit=False, verbosity=2).result.wasSuccessful() else 1)
