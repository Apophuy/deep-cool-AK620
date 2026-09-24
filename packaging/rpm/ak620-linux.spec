Name:           ak620-linux
%global debug_package %{nil}
Version:        @VERSION@
Release:        1%{?dist}
Summary:        Linux display support for DeepCool AK620 DIGITAL PRO
License:        GPL-3.0-only
URL:            https://github.com/ak620-linux/ak620-linux
Source0:        ak620-linux-%{version}.tar.gz
Requires:       systemd
Requires:       dbus
Requires:       acl

%description
System-managed, least-privilege display daemon and KDE-compatible tray client
for the DeepCool AK620 DIGITAL PRO. ARGB control is intentionally excluded.

%prep
%setup -q

%build
cargo build --locked --release --workspace

%install
install -Dpm755 target/release/ak620d %{buildroot}/usr/bin/ak620d
install -Dpm755 target/release/ak620-control %{buildroot}/usr/bin/ak620-control
install -Dpm644 packaging/udev/99-ak620-digital-pro.rules %{buildroot}/usr/lib/udev/rules.d/99-ak620-digital-pro.rules
install -Dpm644 packaging/udev/70-ak620-powercap.rules %{buildroot}/usr/lib/udev/rules.d/70-ak620-powercap.rules
install -Dpm644 packaging/systemd/system/ak620d.service %{buildroot}/usr/lib/systemd/system/ak620d.service
install -Dpm644 packaging/dbus-1/system.d/io.github.ak620linux.Daemon.conf %{buildroot}/usr/share/dbus-1/system.d/io.github.ak620linux.Daemon.conf
install -Dpm644 packaging/applications/io.github.ak620linux.Control.desktop %{buildroot}/usr/share/applications/io.github.ak620linux.Control.desktop
install -Dpm644 packaging/autostart/io.github.ak620linux.Control.desktop %{buildroot}/etc/xdg/autostart/io.github.ak620linux.Control.desktop
for size in 16 22 32 48 64 128 256; do install -Dpm644 packaging/icons/hicolor/${size}x${size}/apps/io.github.ak620linux.Control.png %{buildroot}/usr/share/icons/hicolor/${size}x${size}/apps/io.github.ak620linux.Control.png; install -Dpm644 packaging/icons/hicolor/${size}x${size}/apps/io.github.ak620linux.Control-attention.png %{buildroot}/usr/share/icons/hicolor/${size}x${size}/apps/io.github.ak620linux.Control-attention.png; done

%pre
getent group ak620 >/dev/null || groupadd -r ak620
getent passwd ak620 >/dev/null || useradd -r -g ak620 -d /nonexistent -s /sbin/nologin ak620

%post
systemctl daemon-reload || :
systemctl enable --now ak620d.service || :
udevadm control --reload-rules || :

%preun
if [ "$1" -eq 0 ]; then systemctl disable --now ak620d.service || :; fi

%postun
systemctl daemon-reload || :

%files
%attr(0755,root,root) /usr/bin/ak620d
%attr(0755,root,root) /usr/bin/ak620-control
/usr/lib/udev/rules.d/99-ak620-digital-pro.rules
/usr/lib/udev/rules.d/70-ak620-powercap.rules
/usr/lib/systemd/system/ak620d.service
/usr/share/dbus-1/system.d/io.github.ak620linux.Daemon.conf
/usr/share/applications/io.github.ak620linux.Control.desktop
%config(noreplace) /etc/xdg/autostart/io.github.ak620linux.Control.desktop
/usr/share/icons/hicolor/*/apps/io.github.ak620linux.Control*.png
%doc README.md LICENSE

%changelog
* Thu Sep 24 2026 AK620 Linux contributors <noreply@example.invalid> - @VERSION@-1
- System daemon, localized interface, and theme selection.
