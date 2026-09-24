# Security policy

This project writes status reports to a USB HID device. Please report issues involving unintended
device access, overly broad udev permissions, malformed packet handling, privilege escalation, or
unsafe D-Bus exposure privately to the repository owner until a public security contact exists.

Automated tests must never write to a physical HID device. Real-device tests are opt-in and must
validate VID `0x3633` and PID `0x0012` immediately before every test session.

