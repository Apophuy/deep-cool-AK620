# Contributing

## Host setup

The current daemon uses pure-Rust D-Bus and native hidraw discovery, so it does not require
`libhidapi-dev`, `libudev-dev`, or `libdbus-1-dev`. On Debian 13 install the basic compiler tools:

```bash
sudo apt install build-essential pkg-config
```

The eframe Wayland/X11 build dynamically loads the relevant display libraries and compiled in the
Debian 13 KDE development environment without additional dev packages. At runtime a normal KDE
installation already supplies Wayland/X11, EGL/OpenGL, XKB, and the StatusNotifierWatcher. If a
minimal installation lacks runtime libraries, install `libwayland-client0`, `libxkbcommon0`,
`libegl1`, and `libgl1` rather than unrelated `-dev` packages.

Debian 13 provides Rust 1.85.1. A newer stable toolchain may be used locally, but all code must
continue to compile on the workspace MSRV.

With rustup, install the exact local verification toolchain separately:

```bash
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 check --workspace --all-targets
```

## Before submitting a change

Run:

```bash
make check
git diff --check
```

Do not require attached hardware for ordinary CI. Follow `docs/testing.md` for explicit
hardware-in-the-loop checks.

## Dependencies

Before adding a production crate, document why the standard library or an existing dependency is
insufficient, confirm MSRV compatibility, and note any Debian runtime/build packages it needs.
Use the project Context7 MCP connection for current crate APIs when available, then verify against
the exact locked version with `cargo doc` and tests.
