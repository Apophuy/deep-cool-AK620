use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use ak620_core::PowerWatts;

use super::MetricsError;

const DEFAULT_POWERCAP_ROOT: &str = "/sys/class/powercap";
const DEFAULT_HWMON_ROOT: &str = "/sys/class/hwmon";
const MAX_POWERCAP_DEPTH: usize = 4;

/// One cumulative package-energy reading in microjoules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnergyReading {
    microjoules: u64,
}

impl EnergyReading {
    /// Creates a reading from a Linux energy counter.
    pub const fn new(microjoules: u64) -> Self {
        Self { microjoules }
    }

    /// Returns the unmodified cumulative counter.
    pub const fn microjoules(self) -> u64 {
        self.microjoules
    }
}

/// Selected CPU package/socket energy counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnergySensor {
    input_path: PathBuf,
    source_name: String,
    max_range_microjoules: Option<u64>,
}

impl EnergySensor {
    /// Reads the current cumulative energy value.
    pub fn read(&self) -> Result<EnergyReading, MetricsError> {
        let contents = fs::read_to_string(&self.input_path)
            .map_err(|error| MetricsError::io(&self.input_path, error))?;
        let microjoules = contents.trim().parse::<u64>().map_err(|error| {
            MetricsError::invalid(
                "CPU energy",
                format!("invalid value in {}: {error}", self.input_path.display()),
            )
        })?;
        Ok(EnergyReading::new(microjoules))
    }

    /// Path selected for metric reads.
    pub fn input_path(&self) -> &Path {
        &self.input_path
    }

    /// Kernel-provided power-zone or hwmon label.
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    /// Counter range used to recognize wraparound, when provided by the kernel.
    pub const fn max_range_microjoules(&self) -> Option<u64> {
        self.max_range_microjoules
    }

    /// Calculates power between readings using this counter's wrap semantics.
    pub fn power_between(
        &self,
        previous: EnergyReading,
        current: EnergyReading,
        elapsed: Duration,
    ) -> Result<PowerWatts, MetricsError> {
        power_from_energy(previous, current, elapsed, self.max_range_microjoules)
    }
}

/// Discovers a package/socket energy counter in the real Linux trees.
pub fn discover_energy_sensor() -> Result<EnergySensor, MetricsError> {
    discover_energy_sensor_in(
        Path::new(DEFAULT_POWERCAP_ROOT),
        Path::new(DEFAULT_HWMON_ROOT),
    )
}

/// Discovers a package/socket energy counter below supplied fixture or sysfs roots.
pub fn discover_energy_sensor_in(
    powercap_root: &Path,
    hwmon_root: &Path,
) -> Result<EnergySensor, MetricsError> {
    let mut powercap_candidates = Vec::new();
    if powercap_root.is_dir() {
        collect_powercap_candidates(powercap_root, MAX_POWERCAP_DEPTH, &mut powercap_candidates)?;
    }
    powercap_candidates.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    if let Some((_, input_path, source_name, max_range_microjoules)) =
        powercap_candidates.into_iter().next()
    {
        return Ok(EnergySensor {
            input_path,
            source_name,
            max_range_microjoules,
        });
    }

    discover_hwmon_energy_sensor(hwmon_root)
}

/// Derives rounded whole watts from cumulative microjoule readings and elapsed time.
pub fn power_from_energy(
    previous: EnergyReading,
    current: EnergyReading,
    elapsed: Duration,
    max_range_microjoules: Option<u64>,
) -> Result<PowerWatts, MetricsError> {
    let elapsed_microseconds = elapsed.as_micros();
    if elapsed_microseconds == 0 {
        return Err(MetricsError::NoProgress {
            context: "energy sampling clock",
        });
    }

    let delta = if current.microjoules >= previous.microjoules {
        current.microjoules - previous.microjoules
    } else if let Some(maximum) = max_range_microjoules {
        if previous.microjoules > maximum || current.microjoules > maximum {
            return Err(MetricsError::invalid(
                "CPU energy",
                "counter value exceeds max_energy_range_uj",
            ));
        }
        maximum
            .checked_sub(previous.microjoules)
            .and_then(|remaining| remaining.checked_add(current.microjoules))
            .ok_or_else(|| MetricsError::invalid("CPU energy", "wrapped delta overflowed"))?
    } else {
        return Err(MetricsError::CounterRegressed {
            previous: previous.microjoules,
            current: current.microjoules,
        });
    };

    // microjoules / microseconds is watts. Add half the divisor for integer rounding.
    let rounded_watts = (u128::from(delta) + elapsed_microseconds / 2) / elapsed_microseconds;
    let watts = u32::try_from(rounded_watts)
        .map_err(|_| MetricsError::invalid("CPU energy", "derived power exceeds u32"))?;
    PowerWatts::new(watts).map_err(Into::into)
}

type PowercapCandidate = (usize, PathBuf, String, Option<u64>);

fn collect_powercap_candidates(
    directory: &Path,
    remaining_depth: usize,
    candidates: &mut Vec<PowercapCandidate>,
) -> Result<(), MetricsError> {
    let energy_path = directory.join("energy_uj");
    if energy_path.is_file() {
        let name = fs::read_to_string(directory.join("name"))
            .unwrap_or_else(|_| {
                directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("powercap")
                    .to_owned()
            })
            .trim()
            .to_owned();
        let lower_name = name.to_ascii_lowercase();
        let rank = if lower_name.contains("package") || lower_name.contains("socket") {
            0
        } else if lower_name.contains("core") {
            2
        } else {
            1
        };
        let max_range_microjoules = fs::read_to_string(directory.join("max_energy_range_uj"))
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok());
        candidates.push((rank, energy_path, name, max_range_microjoules));
    }

    if remaining_depth == 0 {
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(|error| MetricsError::io(directory, error))?;
    for entry in entries {
        let path = entry
            .map_err(|error| MetricsError::io(directory, error))?
            .path();
        if path.is_dir() {
            collect_powercap_candidates(&path, remaining_depth - 1, candidates)?;
        }
    }
    Ok(())
}

fn discover_hwmon_energy_sensor(root: &Path) -> Result<EnergySensor, MetricsError> {
    if !root.is_dir() {
        return Err(MetricsError::EnergySensorNotFound);
    }
    let mut directories = fs::read_dir(root)
        .map_err(|error| MetricsError::io(root, error))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();

    for directory in directories {
        let Ok(driver) = fs::read_to_string(directory.join("name")) else {
            continue;
        };
        if !["amd_energy", "zenergy"].contains(&driver.trim()) {
            continue;
        }

        let mut labels = fs::read_dir(&directory)
            .map_err(|error| MetricsError::io(&directory, error))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        labels.sort();
        for label_path in labels {
            let Some(file_name) = label_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(channel) = file_name
                .strip_prefix("energy")
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
            if !label.starts_with("Esocket") {
                continue;
            }
            let input_path = directory.join(format!("energy{channel}_input"));
            if input_path.is_file() {
                return Ok(EnergySensor {
                    input_path,
                    source_name: format!("{}:{label}", driver.trim()),
                    max_range_microjoules: None,
                });
            }
        }
    }

    Err(MetricsError::EnergySensorNotFound)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{EnergyReading, MetricsError, discover_energy_sensor_in, power_from_energy};
    use crate::metrics::test_support::FixtureDir;

    #[test]
    fn discovers_package_powercap_before_core_and_reads_range() {
        let powercap = FixtureDir::new("powercap-priority");
        let hwmon = FixtureDir::new("powercap-empty-hwmon");
        powercap.write("intel-rapl:0:0/name", "core\n");
        powercap.write("intel-rapl:0:0/energy_uj", "100\n");
        powercap.write("amd-rapl:0/name", "package-0\n");
        powercap.write("amd-rapl:0/energy_uj", "42000000\n");
        powercap.write("amd-rapl:0/max_energy_range_uj", "262143328850\n");

        let sensor = discover_energy_sensor_in(powercap.path(), hwmon.path()).unwrap();

        assert_eq!(sensor.source_name(), "package-0");
        assert_eq!(sensor.read().unwrap().microjoules(), 42_000_000);
        assert_eq!(sensor.max_range_microjoules(), Some(262_143_328_850));
    }

    #[test]
    fn falls_back_to_amd_socket_energy_in_hwmon() {
        let powercap = FixtureDir::new("energy-empty-powercap");
        let hwmon = FixtureDir::new("amd-energy");
        hwmon.write("hwmon0/name", "amd_energy\n");
        hwmon.write("hwmon0/energy1_label", "Ecore0\n");
        hwmon.write("hwmon0/energy1_input", "10\n");
        hwmon.write("hwmon0/energy25_label", "Esocket0\n");
        hwmon.write("hwmon0/energy25_input", "65000000\n");

        let sensor = discover_energy_sensor_in(powercap.path(), hwmon.path()).unwrap();

        assert_eq!(sensor.source_name(), "amd_energy:Esocket0");
        assert_eq!(sensor.read().unwrap().microjoules(), 65_000_000);
        assert_eq!(sensor.max_range_microjoules(), None);
    }

    #[test]
    fn calculates_power_with_rounding_and_wraparound() {
        let watts = power_from_energy(
            EnergyReading::new(1_000_000),
            EnergyReading::new(66_400_000),
            Duration::from_secs(1),
            None,
        )
        .unwrap();
        assert_eq!(watts.get(), 65);

        let wrapped = power_from_energy(
            EnergyReading::new(990),
            EnergyReading::new(40),
            Duration::from_secs(1),
            Some(1_000),
        )
        .unwrap();
        assert_eq!(wrapped.get(), 0);
    }

    #[test]
    fn rejects_regression_without_range_and_zero_time() {
        assert!(matches!(
            power_from_energy(
                EnergyReading::new(100),
                EnergyReading::new(90),
                Duration::from_secs(1),
                None
            ),
            Err(MetricsError::CounterRegressed { .. })
        ));
        assert!(matches!(
            power_from_energy(
                EnergyReading::new(100),
                EnergyReading::new(200),
                Duration::ZERO,
                None
            ),
            Err(MetricsError::NoProgress { .. })
        ));
    }

    #[test]
    fn reports_when_no_package_energy_source_exists() {
        let powercap = FixtureDir::new("no-energy-powercap");
        let hwmon = FixtureDir::new("no-energy-hwmon");
        hwmon.write("hwmon0/name", "k10temp\n");

        assert!(matches!(
            discover_energy_sensor_in(powercap.path(), hwmon.path()),
            Err(MetricsError::EnergySensorNotFound)
        ));
    }
}
