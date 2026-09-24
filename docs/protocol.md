# AK620 DIGITAL PRO protocol notes

## Identity

- USB vendor ID: `0x3633`
- USB product ID: `0x0012`
- Observed USB manufacturer string: `DC`
- Observed USB product string: `AK620-DIGITAL-PRO`
- Linux `HID_NAME` combines them as `DC AK620-DIGITAL-PRO`
- Transport: USB HID through the internal USB 2.0 header

## Known report shape

The device accepts a 64-byte HID output report. Multi-byte numeric values use big-endian byte order.

| Offset | Size | Meaning |
| ---: | ---: | --- |
| 0 | 1 | HID report identifier / fixed value `0x10` |
| 1–7 | 7 | Fixed prefix `68 01 04 0d 01 02 08` |
| 8–9 | 2 | CPU package power, whole watts, unsigned 16-bit |
| 10 | 1 | Temperature unit: `0` Celsius, `1` Fahrenheit |
| 11–14 | 4 | Temperature, IEEE-754 `f32` |
| 15 | 1 | Aggregate CPU utilization, whole percent |
| 16–17 | 2 | Highest observed core frequency, whole MHz, unsigned 16-bit |
| 18 | 1 | Wrapping sum of bytes 1 through 17 |
| 19 | 1 | Terminator `0x16` |
| 20–63 | 44 | Zero padding |

`ak620-core` now expresses this layout independently through named fields and validated domain
types. A complete golden report plus boundary tests lock down endian encoding, checksum coverage,
temperature units, and zero padding.

The screen is treated as a fixed-function status display, not a general bitmap framebuffer.
Temperature warning thresholds appear to be device-controlled.

## Sources

- DeepCool product page and manual for the displayed metrics and USB connection:
  <https://www.deepcool.com/products/Cooling/cpuaircoolers/AK620-Digital-Pro-Cooler-With-Multi-line-Display-1851-1700-AM5/2024/18681.shtml>
- Community device list and protocol research:
  <https://github.com/Nortank12/deepcool-digital-linux>
- AK620 Pro protocol reference consulted during feasibility research:
  <https://github.com/Nortank12/deepcool-digital-linux/blob/main/src/devices/ak620_pro.rs>

The community project is GPL-3.0. Its source was used only to confirm observable field positions and
units. This repository expresses those facts independently with original types, structure, errors,
documentation, and tests; it does not copy that implementation.
