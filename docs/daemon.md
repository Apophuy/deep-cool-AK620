# Daemon operation

## Build dependencies

On Debian 13, install Rust 1.85 or newer, `build-essential`, and `pkg-config`:

```bash
sudo apt install build-essential pkg-config
cargo build --release -p ak620d
```

`hidapi` uses its Rust Linux basic-udev backend; no system HID development library is required.
At runtime the normal kernel `hidraw`, hwmon and powercap interfaces are required.

## System service and permissions

Packages install `/usr/lib/systemd/system/ak620d.service` and enable it during installation. It is
therefore started at boot independently of which desktop user subsequently logs in. systemd runs
the process as the dedicated unprivileged `ak620` user and group, never as root. The system manager
is root-managed; the daemon itself retains only the least privilege needed to read sensors and open
the exact AK620 HID node.

The late HID udev rule matches only USB `3633:0012`, retains `TAG+="uaccess"`, and then uses
`chgrp` plus `setfacl g::rw,m::rw` to restore the `ak620` group ACL after uaccess has processed the
active session. It never grants access to an arbitrary HID device. The package also creates the
`ak620` group and user if absent. The powercap rule gives this group read-only access to the exact
`package-0` energy counter; no users need to be manually added to that group.

During installation the maintainer script reloads udev rules and retriggers only existing hidraw
nodes whose kernel `HID_ID` is the USB identity `0003:00003633:00000012`. This also fixes a cooler
that was connected before the package was installed, without sending a report or touching another
HID device.

After manually installing a package, reload systemd state if necessary:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now ak620d.service
sudo systemctl status ak620d.service
```

Follow logs with `journalctl -u ak620d.service -f`.

### Upgrade from the old user service

Release 0.1.x used `systemctl --user` and may still own the HID handle in the logged-in session.
Before installing 0.2.3, run `systemctl --user disable --now ak620d.service`; then use
`sudo apt install ./dist/ak620-linux_0.2.3_amd64.deb`. `apt` performs the package upgrade itself;
do not purge the old package unless a clean removal is specifically required.

## Configuration

The daemon stores a validated versioned configuration at
`/var/lib/ak620-linux/config.toml`, a directory created by systemd with ownership for the `ak620`
account. A missing file uses:

```toml
version = 1
update_interval_ms = 1000
temperature_unit = "celsius"
```

The interval is restricted to 250–10000 ms. Settings submitted by any desktop client are validated
by the daemon and atomically persisted. Since the display is hardware-global, its settings are
shared among local users. GUI language/theme choices remain per-user.

## D-Bus API version 1

- Bus: system bus
- Name: `io.github.ak620linux.Daemon`
- Object path: `/io/github/ak620linux/Daemon`
- Interface: `io.github.ak620linux.Daemon1`

The package D-Bus policy allows local desktop users to read status and submit the two validated
settings methods. Read-only properties include connection state, device path, last error, the four
latest display metrics, current settings, and last update time. The GUI refreshes them every
second, so an early permission error is replaced by current daemon state without reopening the
window.

## Packages

Build both installer formats after every version change:

```bash
make package-deb
make package-rpm
```

The Debian package is written to `dist/ak620-linux_<version>_<architecture>.deb`; RPM packages are
written under `dist/`. The RPM builder requires `rpmbuild` and the usual rpm build macros.
