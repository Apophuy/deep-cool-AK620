"""Run maintainer scripts against fake commands and a temporary sysfs tree only."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


PACKAGING = Path(__file__).resolve().parents[1]


class MaintainerScripts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / "commands"
        self.hidraw = self.root / "hidraw"
        self.env = dict(
            os.environ,
            PATH=f"{self.root}:/usr/bin:/bin",
            LOG=str(self.log),
            AK620_HIDRAW_SYSFS=str(self.hidraw),
        )
        for command in ("getent", "groupadd", "useradd", "udevadm", "systemctl", "chmod", "chown"):
            stub = self.root / command
            stub.write_text(
                '#!/bin/sh\nprintf "%s\\n" "${0##*/} $*" >> "$LOG"\n'
                'if [ "${0##*/}" = getent ]; then exit "${GROUP_MISSING:-0}"; fi\n'
                'if [ "$*" = "control --reload-rules" ]; then exit "${RELOAD_FAIL:-0}"; fi\n'
            )
            stub.chmod(0o755)

    def run_script(self, script, action):
        source = (PACKAGING / script).read_text()
        source = source.replace("/sys/class/powercap/", f"{self.root}/powercap/")
        subprocess.run(["/bin/sh", "-c", source, script, action], env=self.env, check=True)
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_existing_group_is_preserved_and_trigger_is_exact(self):
        device = self.hidraw / "hidraw0" / "device"
        device.mkdir(parents=True)
        (device / "uevent").write_text("HID_ID=0003:00003633:00000012\n")
        self.assertEqual(self.run_script("postinst", "configure"), [
            "getent group ak620", "getent passwd ak620", "udevadm control --reload-rules",
            "udevadm trigger --action=change --subsystem-match=hidraw --sysname-match=hidraw0",
            "udevadm trigger --action=change --subsystem-match=powercap --attr-match=name=package-0",
            "udevadm settle --timeout=10",
            "systemctl daemon-reload", "systemctl enable --now ak620d.service",
        ])

    def test_missing_group_is_created(self):
        self.env["GROUP_MISSING"] = "2"
        commands = self.run_script("postinst", "configure")
        self.assertIn("groupadd --system ak620", commands)
        self.assertIn("useradd --system --gid ak620 --home-dir /nonexistent --shell /usr/sbin/nologin ak620", commands)

    def test_failed_reload_does_not_trigger_stale_rules(self):
        self.env["RELOAD_FAIL"] = "1"
        self.assertEqual(self.run_script("postinst", "configure"), [
            "getent group ak620", "getent passwd ak620", "udevadm control --reload-rules",
            "systemctl daemon-reload", "systemctl enable --now ak620d.service",
        ])

    def test_only_the_exact_usb_hid_identity_is_retriggered(self):
        for name, identity in (("hidraw0", "HID_ID=0003:00003633:00000012\n"), ("hidraw1", "HID_ID=0003:00001234:00005678\n")):
            device = self.hidraw / name / "device"
            device.mkdir(parents=True)
            (device / "uevent").write_text(identity)
        commands = self.run_script("postinst", "configure")
        self.assertIn("udevadm trigger --action=change --subsystem-match=hidraw --sysname-match=hidraw0", commands)
        self.assertNotIn("udevadm trigger --action=change --subsystem-match=hidraw --sysname-match=hidraw1", commands)

    def test_hid_rule_reasserts_only_the_target_daemon_acl_after_uaccess(self):
        rule = (PACKAGING.parent / "udev" / "99-ak620-digital-pro.rules").read_text()
        self.assertIn('ATTRS{idVendor}=="3633"', rule)
        self.assertIn('ATTRS{idProduct}=="0012"', rule)
        self.assertIn('TAG+="uaccess"', rule)
        self.assertIn('RUN+="/usr/bin/chgrp ak620 /dev/%k"', rule)
        self.assertIn('RUN+="/usr/bin/setfacl -m g::rw,m::rw /dev/%k"', rule)

    def test_upgrade_and_abort_do_not_revoke_access(self):
        for action in ("upgrade", "failed-upgrade", "abort-install", "abort-upgrade"):
            self.assertEqual(self.run_script("postrm", action), [])
        self.assertEqual(self.run_script("postinst", "abort-upgrade"), [])

    def test_remove_and_purge_only_revoke_package_zero(self):
        for zone, name in (("socket", "package-0"), ("other", "package-1"), ("core", "core")):
            path = self.root / "powercap" / zone
            path.mkdir(parents=True)
            (path / "name").write_text(name + "\n")
            (path / "energy_uj").write_text("123\n")
        for action in ("remove", "purge"):
            self.log.unlink(missing_ok=True)
            commands = self.run_script("postrm", action)
            energy = self.root / "powercap/socket/energy_uj"
            self.assertEqual(commands, [
                "systemctl disable --now ak620d.service", "systemctl daemon-reload",
                "udevadm control --reload-rules", "udevadm settle --timeout=10",
                f"chmod 0400 {energy}", f"chown root:root {energy}",
            ])

    def test_remove_without_powercap_is_safe(self):
        self.assertEqual(self.run_script("postrm", "remove"), [
            "systemctl disable --now ak620d.service", "systemctl daemon-reload",
            "udevadm control --reload-rules", "udevadm settle --timeout=10",
        ])


if __name__ == "__main__":
    unittest.main()
