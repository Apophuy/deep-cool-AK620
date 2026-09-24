# Test strategy

## Levels

1. Unit tests validate packet bytes, checksum behavior, conversions, parsing, and fallback policy.
2. Integration tests connect fake metric sources, clocks, HID transports, and D-Bus boundaries.
3. Process tests exercise daemon configuration and failure reporting without a physical device.
4. Hardware-in-the-loop tests target exactly `3633:0012` and are always explicitly enabled.

## CI contract

`make check` must be deterministic, require neither root nor a graphical session, and never open a
real HID device. CI runs it on stable Rust and verifies the Debian 13 MSRV separately.

`make check` also requires Python 3 for packaging tests. These execute maintainer scripts with
fake system commands and a temporary powercap tree, covering existing/missing system accounts,
exact trigger arguments, service enablement, failed reloads, removal/purge, and upgrades without
touching host permissions.
Before installation, validate both rules with `udevadm verify packaging/udev/*.rules` and inspect
the package contents. A read-only `udevadm test` against the package powercap device should show
only the intended `chgrp` and `chmod` RUN commands; `udevadm test` does not execute RUN programs.

## Hardware safety gates

A future hardware test command must:

- require an explicit opt-in flag or environment variable;
- enumerate and print the selected path, VID, PID, manufacturer, and product;
- refuse zero, multiple, or non-matching devices unless a unique explicit path is supplied;
- send only known reports for a bounded duration;
- restore normal daemon operation or close the device on cancellation;
- never manipulate fan control, ARGB, kernel modules, or unrelated USB devices.

Hardware results should record kernel version, firmware-visible USB identity, sensor source, packet
fixture, observed display values, and any rounding or refresh delay.

## Opt-in hardware helper

Stop `ak620d` before using the helper so it can own the HID handle. Stage 1–4 inspection opens only
the uniquely matched `3633:0012` / manufacturer `DC` / product `AK620-DIGITAL-PRO` device,
validates unprivileged access and
discovers sensors, but sends no report:

```bash
cargo run --locked -p ak620d --bin ak620-hwtest -- inspect
```

Only after reviewing that output, the stage-5 command sends one documented 64-byte fixture and
immediately closes the handle:

```bash
cargo run --locked -p ak620d --bin ak620-hwtest -- \
  write-golden --i-understand-this-writes-one-known-report
```

It should show exactly 42 °C, 73%, 88 W, and 4321 MHz. Never run this command through `sudo`.
Capture results under `/tmp` or another untracked artifact directory and do not commit a host's
hidraw path.

After the single-report fixture has been visually confirmed, the stage-6 helper sends exactly 30
live metric reports at one-second intervals and then closes the HID handle:

```bash
cargo run --locked -p ak620d --bin ak620-hwtest -- \
  live --i-understand-this-writes-30-live-reports
```

The command prints the selected sensor sources and all four values for every report. It stops on
the first sampling or HID error and never retries a failed write.

## Linux metric fixtures

Daemon tests construct isolated temporary procfs/sysfs-shaped trees. They verify `Tdie` over `Tctl`,
ignore GPU hwmon entries, select package/socket energy over core energy, exercise counter wrap and
regression, and parse aggregate utilization and maximum observed `cpu MHz`. These tests use only
files created by the test process and cannot access `/dev/hidraw*`.

The ordinary daemon tests also use a fake report writer, a fake metric source, bounded-backoff
tests, TOML round trips, and direct D-Bus interface method tests. Starting `ak620d` itself is not
part of `make check`, because the binary intentionally attempts real sensor and HID discovery.

GUI unit tests cover D-Bus temperature values, metric presentation, and draft-setting
resynchronization without starting a window server, system bus, or tray watcher. Native window and
StatusNotifierItem acceptance remain manual desktop tests because CI is intentionally headless.
