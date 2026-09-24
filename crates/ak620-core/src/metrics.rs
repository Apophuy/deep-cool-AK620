//! Validated values accepted by the fixed-function display.

use std::{error::Error, fmt};

const MAX_DISPLAY_TEMPERATURE: f32 = 255.0;

/// Temperature unit encoded in the HID report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureUnit {
    /// Degrees Celsius.
    Celsius,
    /// Degrees Fahrenheit.
    Fahrenheit,
}

impl TemperatureUnit {
    pub(crate) const fn protocol_value(self) -> u8 {
        match self {
            Self::Celsius => 0,
            Self::Fahrenheit => 1,
        }
    }
}

/// A finite, non-negative temperature representable by the display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Temperature {
    degrees: f32,
    unit: TemperatureUnit,
}

impl Temperature {
    /// Creates a temperature already expressed in the requested display unit.
    pub fn new(degrees: f32, unit: TemperatureUnit) -> Result<Self, MetricError> {
        if !degrees.is_finite() {
            return Err(MetricError::TemperatureNotFinite);
        }
        if !(0.0..=MAX_DISPLAY_TEMPERATURE).contains(&degrees) {
            return Err(MetricError::TemperatureOutOfRange { degrees });
        }
        Ok(Self { degrees, unit })
    }

    /// Converts a Celsius sensor value to the selected unit and rounds to the integer shown by
    /// the fixed-function display.
    pub fn from_celsius(celsius: f32, unit: TemperatureUnit) -> Result<Self, MetricError> {
        if !celsius.is_finite() {
            return Err(MetricError::TemperatureNotFinite);
        }

        let degrees = match unit {
            TemperatureUnit::Celsius => celsius,
            TemperatureUnit::Fahrenheit => celsius.mul_add(1.8, 32.0),
        };
        Self::new(degrees.round(), unit)
    }

    /// Numeric value written to the report.
    pub const fn degrees(self) -> f32 {
        self.degrees
    }

    /// Unit written to the report.
    pub const fn unit(self) -> TemperatureUnit {
        self.unit
    }
}

/// CPU package power rounded to whole watts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerWatts(u16);

impl PowerWatts {
    /// Validates and creates a power value.
    pub fn new(watts: u32) -> Result<Self, MetricError> {
        u16::try_from(watts)
            .map(Self)
            .map_err(|_| MetricError::PowerOutOfRange { watts })
    }

    /// Whole watts written to the report.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Aggregate CPU utilization in the inclusive range 0–100 percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuUtilization(u8);

impl CpuUtilization {
    /// Validates and creates a utilization value.
    pub fn new(percent: u8) -> Result<Self, MetricError> {
        if percent > 100 {
            return Err(MetricError::UtilizationOutOfRange { percent });
        }
        Ok(Self(percent))
    }

    /// Whole percent written to the report.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Highest observed CPU-core frequency rounded to MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFrequencyMhz(u16);

impl CpuFrequencyMhz {
    /// Validates and creates a frequency value.
    pub fn new(mhz: u32) -> Result<Self, MetricError> {
        u16::try_from(mhz)
            .map(Self)
            .map_err(|_| MetricError::FrequencyOutOfRange { mhz })
    }

    /// Whole MHz written to the report.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// One coherent set of values displayed by the cooler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayMetrics {
    power: PowerWatts,
    temperature: Temperature,
    utilization: CpuUtilization,
    frequency: CpuFrequencyMhz,
}

impl DisplayMetrics {
    /// Groups values sampled for a single display update.
    pub const fn new(
        power: PowerWatts,
        temperature: Temperature,
        utilization: CpuUtilization,
        frequency: CpuFrequencyMhz,
    ) -> Self {
        Self {
            power,
            temperature,
            utilization,
            frequency,
        }
    }

    /// CPU package power included in this snapshot.
    pub const fn power(self) -> PowerWatts {
        self.power
    }

    /// CPU temperature included in this snapshot.
    pub const fn temperature(self) -> Temperature {
        self.temperature
    }

    /// Aggregate CPU utilization included in this snapshot.
    pub const fn utilization(self) -> CpuUtilization {
        self.utilization
    }

    /// Highest observed core frequency included in this snapshot.
    pub const fn frequency(self) -> CpuFrequencyMhz {
        self.frequency
    }
}

/// Invalid value supplied to the display metric model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricError {
    /// Temperature was NaN or infinite.
    TemperatureNotFinite,
    /// Temperature does not fit the supported non-negative display domain.
    TemperatureOutOfRange {
        /// Rejected numeric value in the requested unit.
        degrees: f32,
    },
    /// Power does not fit the protocol's unsigned 16-bit field.
    PowerOutOfRange {
        /// Rejected whole-watt value.
        watts: u32,
    },
    /// Utilization exceeded 100 percent.
    UtilizationOutOfRange {
        /// Rejected percentage.
        percent: u8,
    },
    /// Frequency does not fit the protocol's unsigned 16-bit field.
    FrequencyOutOfRange {
        /// Rejected whole-MHz value.
        mhz: u32,
    },
}

impl fmt::Display for MetricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TemperatureNotFinite => formatter.write_str("temperature must be finite"),
            Self::TemperatureOutOfRange { degrees } => write!(
                formatter,
                "temperature {degrees} is outside the display range 0..={MAX_DISPLAY_TEMPERATURE}"
            ),
            Self::PowerOutOfRange { watts } => {
                write!(formatter, "power {watts} W does not fit a 16-bit field")
            }
            Self::UtilizationOutOfRange { percent } => {
                write!(formatter, "CPU utilization {percent}% exceeds 100%")
            }
            Self::FrequencyOutOfRange { mhz } => {
                write!(
                    formatter,
                    "CPU frequency {mhz} MHz does not fit a 16-bit field"
                )
            }
        }
    }
}

impl Error for MetricError {}

#[cfg(test)]
mod tests {
    use super::{
        CpuFrequencyMhz, CpuUtilization, MetricError, PowerWatts, Temperature, TemperatureUnit,
    };

    #[test]
    fn temperature_rejects_non_finite_and_out_of_range_values() {
        assert_eq!(
            Temperature::new(f32::NAN, TemperatureUnit::Celsius),
            Err(MetricError::TemperatureNotFinite)
        );
        assert_eq!(
            Temperature::new(-0.1, TemperatureUnit::Celsius),
            Err(MetricError::TemperatureOutOfRange { degrees: -0.1 })
        );
        assert_eq!(
            Temperature::new(255.1, TemperatureUnit::Fahrenheit),
            Err(MetricError::TemperatureOutOfRange { degrees: 255.1 })
        );
    }

    #[test]
    fn celsius_conversion_rounds_in_the_requested_unit() {
        let celsius = Temperature::from_celsius(80.4, TemperatureUnit::Celsius).unwrap();
        let fahrenheit = Temperature::from_celsius(80.4, TemperatureUnit::Fahrenheit).unwrap();

        assert_eq!(celsius.degrees(), 80.0);
        assert_eq!(celsius.unit(), TemperatureUnit::Celsius);
        assert_eq!(fahrenheit.degrees(), 177.0);
        assert_eq!(fahrenheit.unit(), TemperatureUnit::Fahrenheit);
    }

    #[test]
    fn utilization_accepts_boundaries_only() {
        assert_eq!(CpuUtilization::new(0).unwrap().get(), 0);
        assert_eq!(CpuUtilization::new(100).unwrap().get(), 100);
        assert_eq!(
            CpuUtilization::new(101),
            Err(MetricError::UtilizationOutOfRange { percent: 101 })
        );
    }

    #[test]
    fn unsigned_sixteen_bit_metrics_are_checked() {
        assert_eq!(PowerWatts::new(u16::MAX.into()).unwrap().get(), u16::MAX);
        assert_eq!(
            PowerWatts::new(u32::from(u16::MAX) + 1),
            Err(MetricError::PowerOutOfRange { watts: 65_536 })
        );
        assert_eq!(
            CpuFrequencyMhz::new(u32::from(u16::MAX) + 1),
            Err(MetricError::FrequencyOutOfRange { mhz: 65_536 })
        );
    }
}
