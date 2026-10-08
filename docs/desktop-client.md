# Desktop client

`ak620-control` is a native `eframe`/`egui` application with Wayland and X11 support. Wayland is
the supported and manually tested target; the compiled X11 fallback has not received equivalent
desktop acceptance testing. It talks only
to `io.github.ak620linux.Daemon1` on the system D-Bus; it never opens USB, hidraw, procfs, hwmon,
or powercap.

The window has four pages: Monitoring, System information, Device, and Settings. Its compact left
rail uses keyboard-focusable vector icons; localized page names are available as hover tooltips.
Monitoring follows the vendor dashboard layout: paired CPU/GPU cards use concentric load and
temperature arcs with three detail values, while memory, storage, and network occupy the lower
row. Optional sections remain hidden when their source is unavailable. System information shows
the readable unprivileged host inventory. Device uses the packaged cooler artwork and repeats live
CPU values plus fan RPM when a motherboard hwmon channel exists. Display and interface controls
remain on the bottom Settings page.

The window polls the selected HID path, connection errors, applied settings, display metrics, and
optional host telemetry every second. Its D-Bus proxy disables property caching because the daemon
publishes a fresh snapshot on every poll rather than emitting a change signal for every metric.
Temperature unit changes and the bounded refresh interval are
sent to the daemon, which validates and persists them. The dashboard uses a wide default viewport
and stacks its cards vertically when the available content width becomes narrow. The refresh
interval uses localized presets, and its explanatory copy is available from the adjacent tooltip.
English and Russian plus System, Light, and Dark themes are selectable; System is the default.
Both explicit themes use application palettes for panels, cards, controls, and interaction states.
Presentation choices are per-user and do not affect shared daemon settings. Optional telemetry is
never rendered as a fabricated unavailable value: absent hardware channels are omitted from the
relevant page.
The diagnostics footer converts the daemon's Unix update timestamp to the fixed-width local
`HH:MM:ss` representation. The pinned `time` crate performs the conversion using the host timezone;
it requires no additional runtime service and supports the workspace MSRV.

On KDE Plasma, `ksni` publishes a StatusNotifierItem with live summary text, a red attention icon
for errors, localized error tooltips, Open settings, and Quit actions. The client supplies embedded
22, 32, and 48 pixel ARGB pixmaps directly instead of relying on the desktop icon-theme cache. The
tray process owns the StatusNotifierItem and launches
the settings window as a child process. Closing settings closes that window normally and leaves the
existing tray item running. The tray reuses a running window rather than launching another one; once
the window has closed, the next Open settings action starts one replacement window. The package
autostarts `ak620-control --tray`, which starts without a settings window.
D-Bus polling and tray updates use worker threads and do not block the egui event loop.

The Wayland application ID is `io.github.ak620linux.Control` and matches the packaged desktop file.
The KDE autostart entry starts the tray application after the panel. Users who do not want the tray
at login can omit or disable that autostart file without affecting `ak620d`.
