#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd -- "$repository_root"

project_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' Cargo.toml)
if [[ -z "$project_version" ]]; then
    echo "could not read workspace version" >&2
    exit 1
fi

architecture=$(dpkg --print-architecture)
package_name="ak620-linux_${project_version}_${architecture}"
temporary_root=$(mktemp -d -t ak620-linux-package.XXXXXXXX)
package_root="$temporary_root/$package_name"
trap 'rm -rf -- "$temporary_root"' EXIT
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"

cargo build --locked --release --workspace

install -Dm755 target/release/ak620d "$package_root/usr/bin/ak620d"
install -Dm755 target/release/ak620-control "$package_root/usr/bin/ak620-control"
install -Dm644 packaging/udev/99-ak620-digital-pro.rules \
    "$package_root/usr/lib/udev/rules.d/99-ak620-digital-pro.rules"
install -Dm644 packaging/udev/70-ak620-powercap.rules \
    "$package_root/usr/lib/udev/rules.d/70-ak620-powercap.rules"
install -Dm644 packaging/systemd/system/ak620d.service \
    "$package_root/usr/lib/systemd/system/ak620d.service"
install -Dm644 packaging/dbus-1/system.d/io.github.ak620linux.Daemon.conf \
    "$package_root/usr/share/dbus-1/system.d/io.github.ak620linux.Daemon.conf"
install -Dm644 packaging/applications/io.github.ak620linux.Control.desktop \
    "$package_root/usr/share/applications/io.github.ak620linux.Control.desktop"
install -Dm644 packaging/autostart/io.github.ak620linux.Control.desktop \
    "$package_root/etc/xdg/autostart/io.github.ak620linux.Control.desktop"
for icon_size in 16 22 32 48 64 128 256; do
    install -Dm644 "packaging/icons/hicolor/${icon_size}x${icon_size}/apps/io.github.ak620linux.Control.png" \
        "$package_root/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/io.github.ak620linux.Control.png"
    install -Dm644 "packaging/icons/hicolor/${icon_size}x${icon_size}/apps/io.github.ak620linux.Control-attention.png" \
        "$package_root/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/io.github.ak620linux.Control-attention.png"
done
install -Dm644 README.md "$package_root/usr/share/doc/ak620-linux/README.md"
install -Dm644 LICENSE "$package_root/usr/share/doc/ak620-linux/copyright"
install -Dm644 docs/daemon.md "$package_root/usr/share/doc/ak620-linux/daemon.md"
install -Dm644 docs/desktop-client.md \
    "$package_root/usr/share/doc/ak620-linux/desktop-client.md"

install -d "$package_root/DEBIAN"
sed \
    -e "s/@VERSION@/$project_version/g" \
    -e "s/@ARCHITECTURE@/$architecture/g" \
    packaging/debian/control.in >"$package_root/DEBIAN/control"
install -m755 packaging/debian/postinst "$package_root/DEBIAN/postinst"
install -m755 packaging/debian/postrm "$package_root/DEBIAN/postrm"

mkdir -p dist
dpkg-deb --root-owner-group --build "$package_root" "dist/$package_name.deb"
echo "created dist/$package_name.deb"
