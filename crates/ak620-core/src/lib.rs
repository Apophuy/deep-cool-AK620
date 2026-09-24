//! Pure domain and protocol types for the AK620 DIGITAL PRO display.

mod metrics;
mod protocol;

pub use metrics::{
    CpuFrequencyMhz, CpuUtilization, DisplayMetrics, MetricError, PowerWatts, Temperature,
    TemperatureUnit,
};
pub use protocol::{DisplayReport, REPORT_LENGTH};

/// DeepCool's USB vendor identifier for this device family.
pub const USB_VENDOR_ID: u16 = 0x3633;

/// USB product identifier for the AK620 DIGITAL PRO.
pub const USB_PRODUCT_ID: u16 = 0x0012;

#[cfg(test)]
mod tests {
    use super::{USB_PRODUCT_ID, USB_VENDOR_ID};

    #[test]
    fn target_identity_matches_observed_hardware() {
        assert_eq!(USB_VENDOR_ID, 0x3633);
        assert_eq!(USB_PRODUCT_ID, 0x0012);
    }
}
