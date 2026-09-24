# Architecture

## Goals

Provide reliable display updates, user-visible diagnostics, settings, and KDE Wayland tray
integration with a boot-started system service that does not run the hardware process as root.

## Process boundary

```text
Linux hwmon / procfs / powercap
              |
              v
          ak620d  ------->  HID 3633:0012
              ^
              | system D-Bus
              v
       ak620-control
       window + tray
```

`ak620d` exclusively owns the HID handle. systemd starts it at boot as the dedicated unprivileged
`ak620` system account. It samples metrics, validates and encodes a display
report through `ak620-core`, writes at a bounded interval, and reconnects with bounded backoff.
`ak620-control` is a session application; it never opens `/dev/hidraw*` directly.

## Crate boundaries

- `ak620-core` has no Linux, HID, GUI, or async-runtime dependency. Packet encoding must be pure.
- `ak620d` contains Linux adapters, device discovery, the update loop, and D-Bus service.
- `ak620-gui` contains the settings model, D-Bus client, native window, and StatusNotifierItem.

## Initial technology choices

- HID: `hidapi` with the Linux hidraw backend.
- IPC: `zbus` on the system bus, with a narrowly scoped D-Bus policy for local desktop users.
- GUI: `eframe`/`egui`, with Wayland enabled and X11 fallback.
- Tray: a StatusNotifierItem implementation compatible with KDE Plasma.
- Configuration: versioned TOML under systemd-managed `/var/lib/ak620-linux/` state.

The daemon locks `hidapi` to its native basic-udev backend and uses `zbus` on the session bus.
`zbus` and `zbus_macros` are pinned together at 5.11 because newer macro releases generate code
which is incompatible with that MSRV-compatible library release. The client uses `eframe`/`egui`
0.31 with Wayland and X11 enabled, plus `ksni` for KDE StatusNotifierItem. D-Bus and tray work run
on worker threads rather than the UI event loop.

## Permissions

Installation provides a late exact-match udev rule for `3633:0012`. It retains `TAG+="uaccess"`
and then reasserts the `ak620` owning-group ACL after uaccess has granted the active session its
ACL. This lets the dedicated system service open only the target device while retaining active
session integration. World-writable `MODE="0666"` rules and root hardware services are not
acceptable defaults.
