# Desktop client

`ak620-control` is a native `eframe`/`egui` application with Wayland and X11 support. Wayland is
the supported and manually tested target; the compiled X11 fallback has not received equivalent
desktop acceptance testing. It talks only
to `io.github.ak620linux.Daemon1` on the system D-Bus; it never opens USB, hidraw, procfs, hwmon,
or powercap.

The window polls the four values sent to the cooler, the selected HID path, connection errors, and
the applied settings every second. Its D-Bus proxy disables property caching because the daemon
publishes a fresh snapshot on every poll rather than emitting a change signal for every metric.
Temperature unit changes and the bounded refresh interval are
sent to the daemon, which validates and persists them. The native viewport resizes in both
dimensions to the localized content instead of retaining a fixed-width or blank region. The refresh
interval uses localized presets, and its explanatory copy is available from the adjacent tooltip.
English and Russian plus System, Light, and Dark themes are selectable; System is the default.
Both explicit themes use application palettes for panels, cards, controls, and interaction states.
Presentation choices are per-user and do not affect shared daemon settings.
The diagnostics footer converts the daemon's Unix update timestamp to the fixed-width local
`HH:MM:ss` representation. The pinned `time` crate performs the conversion using the host timezone;
it requires no additional runtime service and supports the workspace MSRV.

On KDE Plasma, `ksni` publishes a StatusNotifierItem with live summary text, a red attention icon
for errors, localized error tooltips, Open settings, and Quit actions. The tray process owns the StatusNotifierItem and launches
the settings window as a child process. Closing settings closes that window normally and leaves the
existing tray item running. The tray reuses a running window rather than launching another one; once
the window has closed, the next Open settings action starts one replacement window. The package
autostarts `ak620-control --tray`, which starts without a settings window.
D-Bus polling and tray updates use worker threads and do not block the egui event loop.

The Wayland application ID is `io.github.ak620linux.Control` and matches the packaged desktop file.
The KDE autostart entry starts the tray application after the panel. Users who do not want the tray
at login can omit or disable that autostart file without affecting `ak620d`.
