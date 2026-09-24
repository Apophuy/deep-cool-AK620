# Roadmap

## 0. Foundation

- Rust workspace and crate boundaries.
- Repository and nested agent guidance.
- Project Rust/HID and hardware-testing skills.
- Formatting, Clippy, unit tests, rustdoc, CI, MSRV, and scoped Context7 MCP.

## 1. Protocol core

- [x] Document the complete 64-byte report layout.
- [x] Add validated metric types, encoder, checksum, and golden fixtures.
- [x] Keep the crate platform-independent and fuzz-friendly.

## 2. Linux metrics

- [x] Parse aggregate CPU utilization and highest observed core frequency.
- [x] Discover AMD `k10temp` package/Tctl/Tdie with diagnostic identity.
- [x] Discover AMD package energy/powercap when the kernel exposes a real counter.
- [x] Cover fixture-based sysfs/procfs trees in hardware-independent tests.

## 3. HID daemon

- [x] Exact `3633:0012` discovery and single-daemon ownership through the D-Bus name.
- [x] Periodic writes, bounded reconnect, structured logging, and graceful shutdown.
- [x] Versioned configuration and D-Bus status/settings API.
- [x] Exact-match udev rule and systemd user unit.

## 4. Desktop client

- [x] Native settings/status window.
- [x] KDE Wayland StatusNotifierItem with operation when no tray host exists.
- [x] Autostart integration and actionable device/sensor diagnostics.

## 5. Packaging and hardware acceptance

- [x] Reproducible local Debian binary package and clean install/uninstall paths.
- [x] Opt-in identity/permission/sensor inspection and one-report golden helper.
- Hardware-in-the-loop matrix on the Ryzen 9 9900X host.
- Suspend/resume, unplug/replug, daemon restart, and long-running stability tests.
- User documentation and first signed release.
