//! Validated HID discovery and report transport.

use std::{error::Error, fmt};

use ak620_core::{DisplayReport, REPORT_LENGTH, USB_PRODUCT_ID, USB_VENDOR_ID};
use hidapi::{DeviceInfo, HidApi, HidDevice, HidError};

/// USB vendor identifier observed for the AK620 DIGITAL PRO controller.
pub const AK620_VENDOR_ID: u16 = USB_VENDOR_ID;
/// USB product identifier observed for the AK620 DIGITAL PRO controller.
pub const AK620_PRODUCT_ID: u16 = USB_PRODUCT_ID;
/// Manufacturer string exposed by hidapi for the supported display controller.
pub const AK620_MANUFACTURER_STRING: &str = "DC";
/// Product string exposed by hidapi for the supported display controller.
pub const AK620_PRODUCT_STRING: &str = "AK620-DIGITAL-PRO";

/// Diagnostic identity of the exact HID interface opened by the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    path: String,
    manufacturer: Option<String>,
    product: Option<String>,
    serial_number: Option<String>,
    interface_number: i32,
}

impl DeviceIdentity {
    /// OS-specific hidraw path used to open the device.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// USB manufacturer string, when the kernel and device expose one.
    pub fn manufacturer(&self) -> Option<&str> {
        self.manufacturer.as_deref()
    }

    /// USB product string, when the kernel and device expose one.
    pub fn product(&self) -> Option<&str> {
        self.product.as_deref()
    }

    /// USB serial number, when present.
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// HID interface number reported by the operating system.
    pub const fn interface_number(&self) -> i32 {
        self.interface_number
    }
}

/// Failure to discover, open, or write to the supported device.
#[derive(Debug)]
pub enum DeviceError {
    /// No HID interface with the exact supported VID/PID is present.
    NotFound,
    /// More than one matching HID interface was found, so choosing one would be unsafe.
    Ambiguous { paths: Vec<String> },
    /// VID/PID matched but the manufacturer/product strings did not identify this cooler.
    UnexpectedIdentity {
        manufacturer: Option<String>,
        product: Option<String>,
    },
    /// The HID backend failed.
    Hid(HidError),
    /// The kernel accepted fewer bytes than one complete output report.
    ShortWrite { expected: usize, actual: usize },
}

impl fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(
                formatter,
                "AK620 DIGITAL PRO {:04x}:{:04x} was not found",
                AK620_VENDOR_ID, AK620_PRODUCT_ID
            ),
            Self::Ambiguous { paths } => write!(
                formatter,
                "found {} AK620 DIGITAL PRO HID interfaces; refusing an ambiguous write: {}",
                paths.len(),
                paths.join(", ")
            ),
            Self::UnexpectedIdentity {
                manufacturer,
                product,
            } => write!(
                formatter,
                "USB {:04x}:{:04x} has unexpected manufacturer/product strings {:?}/{:?}; expected {:?}/{:?}",
                AK620_VENDOR_ID,
                AK620_PRODUCT_ID,
                manufacturer,
                product,
                AK620_MANUFACTURER_STRING,
                AK620_PRODUCT_STRING
            ),
            Self::Hid(error) => write!(formatter, "HID operation failed: {error}"),
            Self::ShortWrite { expected, actual } => write!(
                formatter,
                "incomplete HID report write: expected {expected} bytes, wrote {actual}"
            ),
        }
    }
}

impl Error for DeviceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Hid(error) => Some(error),
            Self::NotFound
            | Self::Ambiguous { .. }
            | Self::UnexpectedIdentity { .. }
            | Self::ShortWrite { .. } => None,
        }
    }
}

impl From<HidError> for DeviceError {
    fn from(error: HidError) -> Self {
        Self::Hid(error)
    }
}

/// Open handle whose discovery path was validated against the supported VID/PID.
pub struct Ak620Device {
    device: HidDevice,
    identity: DeviceIdentity,
}

impl Ak620Device {
    /// Enumerates HID devices and opens the sole exact VID/PID match.
    pub fn connect() -> Result<Self, DeviceError> {
        Self::connect_after_identity(|_| {})
    }

    /// Opens the sole exact match after exposing its validated identity for diagnostics.
    pub fn connect_after_identity(
        before_open: impl FnOnce(&DeviceIdentity),
    ) -> Result<Self, DeviceError> {
        let api = HidApi::new()?;
        let candidates = matching_candidates(&api);
        let candidate = select_single(candidates)?;
        let identity = identity(&candidate);
        before_open(&identity);
        let device = candidate.open_device(&api)?;

        Ok(Self { device, identity })
    }

    /// Returns the identity captured before opening the device.
    pub const fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Writes exactly one complete, already encoded display report.
    pub fn write_report(&self, report: &DisplayReport) -> Result<(), DeviceError> {
        write_complete(&self.device, report)
    }
}

fn matching_candidates(api: &HidApi) -> Vec<DeviceInfo> {
    api.device_list()
        .filter(|info| is_supported(info.vendor_id(), info.product_id()))
        .cloned()
        .collect()
}

const fn is_supported(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == AK620_VENDOR_ID && product_id == AK620_PRODUCT_ID
}

fn select_single(mut candidates: Vec<DeviceInfo>) -> Result<DeviceInfo, DeviceError> {
    for candidate in &candidates {
        if !has_expected_identity(candidate.manufacturer_string(), candidate.product_string()) {
            return Err(DeviceError::UnexpectedIdentity {
                manufacturer: candidate.manufacturer_string().map(str::to_owned),
                product: candidate.product_string().map(str::to_owned),
            });
        }
    }
    match candidates.len() {
        0 => Err(DeviceError::NotFound),
        1 => Ok(candidates.swap_remove(0)),
        _ => Err(DeviceError::Ambiguous {
            paths: candidates
                .iter()
                .map(|candidate| candidate.path().to_string_lossy().into_owned())
                .collect(),
        }),
    }
}

fn has_expected_identity(manufacturer: Option<&str>, product: Option<&str>) -> bool {
    manufacturer == Some(AK620_MANUFACTURER_STRING) && product == Some(AK620_PRODUCT_STRING)
}

fn identity(info: &DeviceInfo) -> DeviceIdentity {
    DeviceIdentity {
        path: info.path().to_string_lossy().into_owned(),
        manufacturer: info.manufacturer_string().map(str::to_owned),
        product: info.product_string().map(str::to_owned),
        serial_number: info.serial_number().map(str::to_owned),
        interface_number: info.interface_number(),
    }
}

trait ReportWriter {
    fn write(&self, bytes: &[u8]) -> Result<usize, HidError>;
}

impl ReportWriter for HidDevice {
    fn write(&self, bytes: &[u8]) -> Result<usize, HidError> {
        HidDevice::write(self, bytes)
    }
}

fn write_complete(writer: &impl ReportWriter, report: &DisplayReport) -> Result<(), DeviceError> {
    let actual = writer.write(report.as_bytes())?;
    if actual != REPORT_LENGTH {
        return Err(DeviceError::ShortWrite {
            expected: REPORT_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use ak620_core::{
        CpuFrequencyMhz, CpuUtilization, DisplayMetrics, PowerWatts, Temperature, TemperatureUnit,
    };

    use super::{
        AK620_MANUFACTURER_STRING, AK620_PRODUCT_ID, AK620_PRODUCT_STRING, AK620_VENDOR_ID,
        DeviceError, HidError, REPORT_LENGTH, ReportWriter, has_expected_identity, is_supported,
        write_complete,
    };

    #[derive(Default)]
    struct FakeWriter {
        writes: RefCell<Vec<Vec<u8>>>,
        written_length: Option<usize>,
    }

    impl ReportWriter for FakeWriter {
        fn write(&self, bytes: &[u8]) -> Result<usize, HidError> {
            self.writes.borrow_mut().push(bytes.to_vec());
            Ok(self.written_length.unwrap_or(bytes.len()))
        }
    }

    fn report() -> ak620_core::DisplayReport {
        ak620_core::DisplayReport::encode(DisplayMetrics::new(
            PowerWatts::new(105).unwrap(),
            Temperature::new(67.0, TemperatureUnit::Celsius).unwrap(),
            CpuUtilization::new(42).unwrap(),
            CpuFrequencyMhz::new(5_658).unwrap(),
        ))
    }

    #[test]
    fn passes_the_complete_encoded_report_to_the_backend() {
        let writer = FakeWriter::default();
        let report = report();

        write_complete(&writer, &report).unwrap();

        let writes = writer.writes.borrow();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0], report.as_bytes());
        assert_eq!(writes[0].len(), REPORT_LENGTH);
        assert_eq!(writes[0][0], 0x10);
    }

    #[test]
    fn rejects_a_short_hid_write() {
        let writer = FakeWriter {
            written_length: Some(REPORT_LENGTH - 1),
            ..FakeWriter::default()
        };

        let error = write_complete(&writer, &report()).unwrap_err();

        assert!(matches!(
            error,
            DeviceError::ShortWrite {
                expected: REPORT_LENGTH,
                actual: 63
            }
        ));
    }

    #[test]
    fn accepts_only_the_exact_supported_usb_identity() {
        assert!(is_supported(AK620_VENDOR_ID, AK620_PRODUCT_ID));
        assert!(!is_supported(AK620_VENDOR_ID, AK620_PRODUCT_ID + 1));
        assert!(!is_supported(AK620_VENDOR_ID + 1, AK620_PRODUCT_ID));
        assert!(has_expected_identity(
            Some(AK620_MANUFACTURER_STRING),
            Some(AK620_PRODUCT_STRING)
        ));
        assert!(!has_expected_identity(
            Some("another manufacturer"),
            Some(AK620_PRODUCT_STRING)
        ));
        assert!(!has_expected_identity(
            Some(AK620_MANUFACTURER_STRING),
            Some("another product")
        ));
        assert!(!has_expected_identity(None, Some(AK620_PRODUCT_STRING)));
    }
}
