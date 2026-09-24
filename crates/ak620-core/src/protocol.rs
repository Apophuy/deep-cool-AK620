//! Binary report encoder for the AK620 DIGITAL PRO.

use crate::DisplayMetrics;

/// Length of an AK620 DIGITAL PRO HID output report.
pub const REPORT_LENGTH: usize = 64;

const FIXED_PREFIX: [u8; 8] = [0x10, 0x68, 0x01, 0x04, 0x0d, 0x01, 0x02, 0x08];
const POWER_RANGE: std::ops::Range<usize> = 8..10;
const TEMPERATURE_UNIT_OFFSET: usize = 10;
const TEMPERATURE_RANGE: std::ops::Range<usize> = 11..15;
const UTILIZATION_OFFSET: usize = 15;
const FREQUENCY_RANGE: std::ops::Range<usize> = 16..18;
const CHECKSUM_OFFSET: usize = 18;
const TERMINATOR_OFFSET: usize = 19;
const TERMINATOR: u8 = 0x16;

/// Complete output report ready for a validated HID transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayReport([u8; REPORT_LENGTH]);

impl DisplayReport {
    /// Encodes one validated metric snapshot.
    pub fn encode(metrics: DisplayMetrics) -> Self {
        let mut bytes = [0_u8; REPORT_LENGTH];
        bytes[..FIXED_PREFIX.len()].copy_from_slice(&FIXED_PREFIX);
        bytes[POWER_RANGE].copy_from_slice(&metrics.power().get().to_be_bytes());

        let temperature = metrics.temperature();
        bytes[TEMPERATURE_UNIT_OFFSET] = temperature.unit().protocol_value();
        bytes[TEMPERATURE_RANGE].copy_from_slice(&temperature.degrees().to_be_bytes());
        bytes[UTILIZATION_OFFSET] = metrics.utilization().get();
        bytes[FREQUENCY_RANGE].copy_from_slice(&metrics.frequency().get().to_be_bytes());
        bytes[CHECKSUM_OFFSET] = checksum(&bytes);
        bytes[TERMINATOR_OFFSET] = TERMINATOR;

        Self(bytes)
    }

    /// Borrows all 64 bytes, including the HID report identifier at byte zero.
    pub const fn as_bytes(&self) -> &[u8; REPORT_LENGTH] {
        &self.0
    }

    /// Consumes the report and returns its bytes.
    pub const fn into_bytes(self) -> [u8; REPORT_LENGTH] {
        self.0
    }
}

fn checksum(report: &[u8; REPORT_LENGTH]) -> u8 {
    report[1..CHECKSUM_OFFSET]
        .iter()
        .fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

#[cfg(test)]
mod tests {
    use crate::{
        CpuFrequencyMhz, CpuUtilization, DisplayMetrics, PowerWatts, Temperature, TemperatureUnit,
    };

    use super::{CHECKSUM_OFFSET, DisplayReport, REPORT_LENGTH, checksum};

    fn metrics(unit: TemperatureUnit) -> DisplayMetrics {
        DisplayMetrics::new(
            PowerWatts::new(142).unwrap(),
            Temperature::new(68.0, unit).unwrap(),
            CpuUtilization::new(73).unwrap(),
            CpuFrequencyMhz::new(5_658).unwrap(),
        )
    }

    #[test]
    fn encodes_complete_celsius_golden_report() {
        let expected: [u8; REPORT_LENGTH] = [
            0x10, 0x68, 0x01, 0x04, 0x0d, 0x01, 0x02, 0x08, 0x00, 0x8e, 0x00, 0x42, 0x88, 0x00,
            0x00, 0x49, 0x16, 0x1a, 0x56, 0x16, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(
            DisplayReport::encode(metrics(TemperatureUnit::Celsius)).into_bytes(),
            expected
        );
    }

    #[test]
    fn fahrenheit_unit_changes_only_the_unit_for_same_numeric_value() {
        let celsius = DisplayReport::encode(metrics(TemperatureUnit::Celsius)).into_bytes();
        let fahrenheit = DisplayReport::encode(metrics(TemperatureUnit::Fahrenheit)).into_bytes();

        assert_eq!(fahrenheit[10], 1);
        assert_eq!(fahrenheit[11..18], celsius[11..18]);
        assert_eq!(fahrenheit[CHECKSUM_OFFSET], celsius[CHECKSUM_OFFSET] + 1);
    }

    #[test]
    fn encodes_u16_fields_in_network_byte_order() {
        let metrics = DisplayMetrics::new(
            PowerWatts::new(u16::MAX.into()).unwrap(),
            Temperature::new(0.0, TemperatureUnit::Celsius).unwrap(),
            CpuUtilization::new(0).unwrap(),
            CpuFrequencyMhz::new(0x1234).unwrap(),
        );
        let bytes = DisplayReport::encode(metrics).into_bytes();

        assert_eq!(&bytes[8..10], &[0xff, 0xff]);
        assert_eq!(&bytes[16..18], &[0x12, 0x34]);
    }

    #[test]
    fn checksum_excludes_report_id_and_wraps_at_one_byte() {
        let mut bytes = DisplayReport::encode(metrics(TemperatureUnit::Celsius)).into_bytes();
        let expected = bytes[CHECKSUM_OFFSET];

        bytes[0] = bytes[0].wrapping_add(1);
        assert_eq!(checksum(&bytes), expected);

        bytes[1] = 0xff;
        assert_eq!(checksum(&bytes), 0xed);
    }

    #[test]
    fn bytes_after_terminator_remain_zero() {
        let report = DisplayReport::encode(metrics(TemperatureUnit::Celsius));
        assert!(report.as_bytes()[20..].iter().all(|byte| *byte == 0));
    }
}
