use std::{
    fs,
    path::{Path, PathBuf},
};

use super::MetricsError;

const DEFAULT_HWMON_ROOT: &str = "/sys/class/hwmon";
const SUPPORTED_DRIVERS: [&str; 2] = ["k10temp", "zenpower"];

/// One temperature reading in the Linux hwmon millidegree-Celsius unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemperatureReading {
    millidegrees_celsius: i64,
}

impl TemperatureReading {
    /// Returns the unmodified hwmon value.
    pub const fn millidegrees_celsius(self) -> i64 {
        self.millidegrees_celsius
    }

    /// Converts the checked reading to degrees Celsius.
    pub fn degrees_celsius(self) -> f32 {
        self.millidegrees_celsius as f32 / 1_000.0
    }
}

/// Selected AMD CPU temperature input and its diagnostic identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemperatureSensor {
    input_path: PathBuf,
    driver: String,
    label: String,
}

impl TemperatureSensor {
    /// Reads the current hwmon value.
    pub fn read(&self) -> Result<TemperatureReading, MetricsError> {
        let contents = fs::read_to_string(&self.input_path)
            .map_err(|error| MetricsError::io(&self.input_path, error))?;
        let value = contents.trim().parse::<i64>().map_err(|error| {
            MetricsError::invalid(
                "hwmon temperature",
                format!("invalid value in {}: {error}", self.input_path.display()),
            )
        })?;
        if !(-273_150..=1_000_000).contains(&value) {
            return Err(MetricsError::invalid(
                "hwmon temperature",
                format!("value {value} m°C is outside a plausible parse domain"),
            ));
        }
        Ok(TemperatureReading {
            millidegrees_celsius: value,
        })
    }

    /// Path selected for metric reads.
    pub fn input_path(&self) -> &Path {
        &self.input_path
    }

    /// Kernel hwmon driver name.
    pub fn driver(&self) -> &str {
        &self.driver
    }

    /// Channel label, normally `Tdie` or `Tctl`.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// Discovers the preferred AMD package temperature in the real hwmon tree.
pub fn discover_temperature_sensor() -> Result<TemperatureSensor, MetricsError> {
    discover_temperature_sensor_in(Path::new(DEFAULT_HWMON_ROOT))
}

/// Discovers the preferred AMD package temperature below a supplied hwmon root.
pub fn discover_temperature_sensor_in(root: &Path) -> Result<TemperatureSensor, MetricsError> {
    let mut hwmon_dirs = directory_paths(root)?;
    hwmon_dirs.sort();

    let mut candidates = Vec::new();
    for directory in hwmon_dirs {
        let name_path = directory.join("name");
        let Ok(driver) = fs::read_to_string(&name_path) else {
            continue;
        };
        let driver = driver.trim();
        if !SUPPORTED_DRIVERS.contains(&driver) {
            continue;
        }

        for label_path in directory_paths(&directory)? {
            let Some(file_name) = label_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(channel) = file_name
                .strip_prefix("temp")
                .and_then(|name| name.strip_suffix("_label"))
                .filter(|channel| {
                    !channel.is_empty() && channel.bytes().all(|byte| byte.is_ascii_digit())
                })
            else {
                continue;
            };
            let Ok(label) = fs::read_to_string(&label_path) else {
                continue;
            };
            let label = label.trim();
            let rank = match label {
                "Tdie" => 0,
                "Tctl" => 1,
                _ => continue,
            };
            let input_path = directory.join(format!("temp{channel}_input"));
            if input_path.is_file() {
                candidates.push((rank, input_path, driver.to_owned(), label.to_owned()));
            }
        }

        let fallback = directory.join("temp1_input");
        if fallback.is_file() {
            candidates.push((2, fallback, driver.to_owned(), "temp1".to_owned()));
        }
    }

    candidates.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    candidates
        .into_iter()
        .next()
        .map(|(_, input_path, driver, label)| TemperatureSensor {
            input_path,
            driver,
            label,
        })
        .ok_or(MetricsError::TemperatureSensorNotFound)
}

fn directory_paths(root: &Path) -> Result<Vec<PathBuf>, MetricsError> {
    fs::read_dir(root)
        .map_err(|error| MetricsError::io(root, error))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| MetricsError::io(root, error))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{MetricsError, discover_temperature_sensor_in};
    use crate::metrics::test_support::FixtureDir;

    #[test]
    fn prefers_tdie_over_tctl_and_ignores_gpu_hwmon() {
        let fixture = FixtureDir::new("temperature-priority");
        fixture.write("hwmon0/name", "amdgpu\n");
        fixture.write("hwmon0/temp1_label", "edge\n");
        fixture.write("hwmon0/temp1_input", "40000\n");
        fixture.write("hwmon1/name", "k10temp\n");
        fixture.write("hwmon1/temp1_label", "Tctl\n");
        fixture.write("hwmon1/temp1_input", "71000\n");
        fixture.write("hwmon1/temp2_label", "Tdie\n");
        fixture.write("hwmon1/temp2_input", "69000\n");

        let sensor = discover_temperature_sensor_in(fixture.path()).unwrap();

        assert_eq!(sensor.driver(), "k10temp");
        assert_eq!(sensor.label(), "Tdie");
        assert_eq!(sensor.read().unwrap().millidegrees_celsius(), 69_000);
    }

    #[test]
    fn falls_back_to_tctl_then_documented_temp1() {
        let fixture = FixtureDir::new("temperature-fallback");
        fixture.write("hwmon0/name", "k10temp\n");
        fixture.write("hwmon0/temp1_label", "Tctl\n");
        fixture.write("hwmon0/temp1_input", "55250\n");

        let sensor = discover_temperature_sensor_in(fixture.path()).unwrap();
        assert_eq!(sensor.label(), "Tctl");
        assert_eq!(sensor.read().unwrap().degrees_celsius(), 55.25);

        let fixture = FixtureDir::new("temperature-unlabelled");
        fixture.write("hwmon0/name", "k10temp\n");
        fixture.write("hwmon0/temp1_input", "50000\n");
        assert_eq!(
            discover_temperature_sensor_in(fixture.path())
                .unwrap()
                .label(),
            "temp1"
        );
    }

    #[test]
    fn reports_missing_or_malformed_temperature() {
        let fixture = FixtureDir::new("temperature-errors");
        fixture.write("hwmon0/name", "amdgpu\n");
        assert!(matches!(
            discover_temperature_sensor_in(fixture.path()),
            Err(MetricsError::TemperatureSensorNotFound)
        ));

        fixture.write("hwmon1/name", "k10temp\n");
        fixture.write("hwmon1/temp1_input", "not-a-number\n");
        let sensor = discover_temperature_sensor_in(fixture.path()).unwrap();
        assert!(matches!(
            sensor.read(),
            Err(MetricsError::InvalidData { .. })
        ));
    }
}
