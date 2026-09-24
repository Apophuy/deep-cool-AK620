# AK620 Linux / AK620 Linux

## English

Native Linux support for the fixed-function display on the DeepCool AK620 DIGITAL PRO
(USB `3633:0012`). It deliberately does not control ARGB lighting.

> **Supported model: DeepCool AK620 DIGITAL PRO only.** This project is not compatible with
> other AK620-series coolers; DeepCool sells several distinct AK620 models with different hardware.

`ak620d` is installed as a system service, enabled during package installation, and starts at
boot. systemd starts and supervises it as the dedicated unprivileged `ak620` account; it does
**not** run the hardware process as root. The service is the sole HID owner and exposes status
through the system D-Bus. Every desktop user can start the tray/settings client and see the same
live values. The package grants the `ak620` account access only to the exact supported HID device.

The target is Debian 13, KDE Plasma, Wayland, and AMD Ryzen 9 9900X. The client is built with both
Wayland and X11 backends. Wayland is the supported target; the X11 fallback is compiled in but has
not received equivalent desktop acceptance testing.

### Dependencies

Runtime dependencies supplied by the Debian package: `libc6`, `libgcc-s1`, `libwayland-client0`,
`libxkbcommon0`, `libegl1`, `libgl1`, `passwd`, `systemd`, `udev`, and D-Bus. The running system
also needs the kernel `hidraw`, hwmon and powercap interfaces, plus a StatusNotifierItem-capable
tray host (KDE Plasma has one).

To build from source on Debian 13 install Rust 1.85 or newer, `build-essential`, `pkg-config`,
Python 3, `dpkg-dev` for `.deb`, and `rpm`/`rpmbuild` for `.rpm`:

```bash
sudo apt install build-essential pkg-config python3 dpkg-dev rpm
make check
make package-deb
make package-rpm
```

When upgrading from 0.1.x, first stop its old per-user daemon to release the HID handle:

```bash
systemctl --user disable --now ak620d.service
sudo apt install ./dist/ak620-linux_0.2.3_amd64.deb
```

The second command upgrades `ak620-linux`; it does not require a separate removal. Reconnect the
cooler after installation if it was already attached. The tray client is placed in the KDE
autostart directory for each desktop login; it can also be started as `ak620-control --tray`.

The GUI offers English/Russian, System/Light/Dark theme selection (System is the default), live
status, display settings, and diagnostics. A connection failure turns the tray icon red and shows
the localized actionable error in its tooltip.

Before every release or user-visible update, increment the workspace version in `Cargo.toml` and
produce both installer formats with `make package-deb` and `make package-rpm`.

See [architecture](docs/architecture.md), [daemon operation](docs/daemon.md),
[desktop client](docs/desktop-client.md), and [testing](docs/testing.md).

### License

GPL-3.0-only. This is an independent implementation and is not affiliated with DeepCool.

---

## Русский

Нативное приложение Linux для фиксированного дисплея DeepCool AK620 DIGITAL PRO
(USB `3633:0012`). Управление ARGB-подсветкой намеренно не реализовано.

> **Поддерживаемая модель: только DeepCool AK620 DIGITAL PRO.** Проект несовместим с другими
> кулерами серии AK620: DeepCool выпускает несколько разных моделей с отличающимся оборудованием.

`ak620d` устанавливается как общесистемная служба, включается при установке пакета и запускается
вместе с ОС. systemd запускает и контролирует её от выделенной непривилегированной учётной записи
`ak620`; сам процесс работы с оборудованием **не** запускается от root. Только служба открывает
HID-устройство и публикует состояние в системном D-Bus. Любой пользователь рабочего стола может
запустить трей/настройки и увидеть одинаковые актуальные данные. Пакет даёт учётной записи `ak620`
доступ только к точно соответствующему поддерживаемому HID-устройству.

Целевые платформа и окружение: Debian 13, KDE Plasma, Wayland и AMD Ryzen 9 9900X. Клиент
собирается с бэкендами Wayland и X11. Wayland — поддерживаемый вариант; fallback для X11 собран,
но не проходил эквивалентное ручное тестирование рабочего стола.

### Зависимости

Зависимости времени выполнения из Debian-пакета: `libc6`, `libgcc-s1`, `libwayland-client0`,
`libxkbcommon0`, `libegl1`, `libgl1`, `passwd`, `systemd`, `udev` и D-Bus. В системе также нужны
интерфейсы ядра `hidraw`, hwmon и powercap, а для трея — хост StatusNotifierItem (он есть в KDE
Plasma).

Для сборки из исходников на Debian 13 установите Rust 1.85 или новее, `build-essential`,
`pkg-config`, Python 3, `dpkg-dev` для `.deb` и `rpm`/`rpmbuild` для `.rpm`:

```bash
sudo apt install build-essential pkg-config python3 dpkg-dev rpm
make check
make package-deb
make package-rpm
```

При обновлении с 0.1.x сначала остановите старую пользовательскую службу, чтобы она освободила
HID-устройство:

```bash
systemctl --user disable --now ak620d.service
sudo apt install ./dist/ak620-linux_0.2.3_amd64.deb
```

Вторая команда обновляет `ak620-linux`, отдельно удалять старый пакет не нужно. Если кулер уже был
подключён, после установки переподключите его. Трей-клиент добавляется в KDE autostart для каждого
входа в рабочий стол; его также можно запустить командой `ak620-control --tray`.

В GUI есть английский и русский языки, системная/светлая/тёмная темы (по умолчанию — системная),
актуальное состояние, настройки дисплея и диагностика. При ошибке подключения иконка в трее
становится красной, а tooltip показывает локализованное понятное сообщение.

Перед каждым релизом или видимым пользователю обновлением увеличивайте версию workspace в
`Cargo.toml` и собирайте оба установщика: `make package-deb` и `make package-rpm`.

См. [архитектуру](docs/architecture.md), [работу службы](docs/daemon.md),
[клиент рабочего стола](docs/desktop-client.md) и [тестирование](docs/testing.md).

### Лицензия

GPL-3.0-only. Это независимая реализация, не связанная с DeepCool.
