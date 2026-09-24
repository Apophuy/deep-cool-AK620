//! CPU metric collection from Linux procfs, hwmon, and powercap interfaces.

mod cpu;
mod error;
mod power;
mod sampler;
mod temperature;

pub use cpu::{CpuTimes, parse_cpu_times, parse_highest_frequency_mhz};
pub use error::MetricsError;
pub use power::{
    EnergyReading, EnergySensor, discover_energy_sensor, discover_energy_sensor_in,
    power_from_energy,
};
pub use sampler::LinuxMetricSampler;
pub use temperature::{
    TemperatureReading, TemperatureSensor, discover_temperature_sensor,
    discover_temperature_sensor_in,
};

#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct FixtureDir(PathBuf);

    impl FixtureDir {
        pub(crate) fn new(name: &str) -> Self {
            let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("ak620d-{name}-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }

        pub(crate) fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
