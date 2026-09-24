use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use ak620_core::{DisplayMetrics, Temperature, TemperatureUnit};

use super::{
    CpuTimes, EnergyReading, EnergySensor, MetricsError, TemperatureSensor, discover_energy_sensor,
    discover_temperature_sensor, parse_cpu_times, parse_highest_frequency_mhz,
};

const PROC_STAT_PATH: &str = "/proc/stat";
const PROC_CPUINFO_PATH: &str = "/proc/cpuinfo";

/// Stateful Linux collector that derives one coherent display snapshot per interval.
pub struct LinuxMetricSampler {
    temperature_sensor: TemperatureSensor,
    energy_sensor: EnergySensor,
    proc_stat_path: PathBuf,
    proc_cpuinfo_path: PathBuf,
    temperature_unit: TemperatureUnit,
    previous_cpu: CpuTimes,
    previous_energy: EnergyReading,
    previous_instant: Instant,
}

impl LinuxMetricSampler {
    /// Discovers AMD package sensors and captures the initial counter baseline.
    pub fn discover(temperature_unit: TemperatureUnit) -> Result<Self, MetricsError> {
        Self::from_parts(
            discover_temperature_sensor()?,
            discover_energy_sensor()?,
            PathBuf::from(PROC_STAT_PATH),
            PathBuf::from(PROC_CPUINFO_PATH),
            temperature_unit,
            Instant::now(),
        )
    }

    fn from_parts(
        temperature_sensor: TemperatureSensor,
        energy_sensor: EnergySensor,
        proc_stat_path: PathBuf,
        proc_cpuinfo_path: PathBuf,
        temperature_unit: TemperatureUnit,
        now: Instant,
    ) -> Result<Self, MetricsError> {
        let previous_cpu = read_cpu_times(&proc_stat_path)?;
        let previous_energy = energy_sensor.read()?;

        Ok(Self {
            temperature_sensor,
            energy_sensor,
            proc_stat_path,
            proc_cpuinfo_path,
            temperature_unit,
            previous_cpu,
            previous_energy,
            previous_instant: now,
        })
    }

    /// Reads current Linux metrics and advances counter baselines after a successful sample.
    pub fn sample(&mut self) -> Result<DisplayMetrics, MetricsError> {
        self.sample_at(Instant::now())
    }

    fn sample_at(&mut self, now: Instant) -> Result<DisplayMetrics, MetricsError> {
        let elapsed = now
            .checked_duration_since(self.previous_instant)
            .ok_or_else(|| MetricsError::invalid("sampling clock", "monotonic time regressed"))?;
        let cpu = read_cpu_times(&self.proc_stat_path)?;
        let utilization = self.previous_cpu.utilization_since(cpu)?;
        let energy = self.energy_sensor.read()?;
        let power = self
            .energy_sensor
            .power_between(self.previous_energy, energy, elapsed)?;
        let temperature = Temperature::from_celsius(
            self.temperature_sensor.read()?.degrees_celsius(),
            self.temperature_unit,
        )?;
        let cpuinfo = fs::read_to_string(&self.proc_cpuinfo_path)
            .map_err(|error| MetricsError::io(&self.proc_cpuinfo_path, error))?;
        let frequency = parse_highest_frequency_mhz(&cpuinfo)?;

        self.previous_cpu = cpu;
        self.previous_energy = energy;
        self.previous_instant = now;

        Ok(DisplayMetrics::new(
            power,
            temperature,
            utilization,
            frequency,
        ))
    }

    /// Changes the unit used for subsequent display reports.
    pub const fn set_temperature_unit(&mut self, temperature_unit: TemperatureUnit) {
        self.temperature_unit = temperature_unit;
    }

    /// Selected temperature sensor, exposed for actionable diagnostics.
    pub const fn temperature_sensor(&self) -> &TemperatureSensor {
        &self.temperature_sensor
    }

    /// Selected energy sensor, exposed for actionable diagnostics.
    pub const fn energy_sensor(&self) -> &EnergySensor {
        &self.energy_sensor
    }
}

fn read_cpu_times(path: &Path) -> Result<CpuTimes, MetricsError> {
    let contents = fs::read_to_string(path).map_err(|error| MetricsError::io(path, error))?;
    parse_cpu_times(&contents)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ak620_core::TemperatureUnit;

    use super::LinuxMetricSampler;
    use crate::metrics::{
        discover_energy_sensor_in, discover_temperature_sensor_in, test_support::FixtureDir,
    };

    #[test]
    fn combines_counter_deltas_and_instantaneous_metrics() {
        let fixture = FixtureDir::new("metric-sampler");
        let empty_hwmon = FixtureDir::new("metric-sampler-empty-hwmon");
        fixture.write("hwmon/hwmon0/name", "k10temp\n");
        fixture.write("hwmon/hwmon0/temp1_label", "Tctl\n");
        fixture.write("hwmon/hwmon0/temp1_input", "68500\n");
        fixture.write("powercap/amd-rapl:0/name", "package-0\n");
        fixture.write("powercap/amd-rapl:0/energy_uj", "100000000\n");
        fixture.write("powercap/amd-rapl:0/max_energy_range_uj", "1000000000\n");
        fixture.write("proc/stat", "cpu 100 0 0 900\n");
        fixture.write("proc/cpuinfo", "cpu MHz : 5657.600\n");

        let temperature = discover_temperature_sensor_in(&fixture.path().join("hwmon")).unwrap();
        let energy =
            discover_energy_sensor_in(&fixture.path().join("powercap"), empty_hwmon.path())
                .unwrap();
        let started = std::time::Instant::now();
        let mut sampler = LinuxMetricSampler::from_parts(
            temperature,
            energy,
            fixture.path().join("proc/stat"),
            fixture.path().join("proc/cpuinfo"),
            TemperatureUnit::Celsius,
            started,
        )
        .unwrap();

        fixture.write("powercap/amd-rapl:0/energy_uj", "195000000\n");
        fixture.write("proc/stat", "cpu 125 0 25 950\n");

        let metrics = sampler.sample_at(started + Duration::from_secs(1)).unwrap();

        assert_eq!(metrics.power().get(), 95);
        assert_eq!(metrics.temperature().degrees(), 69.0);
        assert_eq!(metrics.utilization().get(), 50);
        assert_eq!(metrics.frequency().get(), 5_658);
    }

    #[test]
    fn can_switch_temperature_units_without_rediscovery() {
        let fixture = FixtureDir::new("metric-sampler-units");
        let empty_hwmon = FixtureDir::new("metric-sampler-units-empty-hwmon");
        fixture.write("hwmon/hwmon0/name", "k10temp\n");
        fixture.write("hwmon/hwmon0/temp1_input", "50000\n");
        fixture.write("powercap/package/name", "package-0\n");
        fixture.write("powercap/package/energy_uj", "0\n");
        fixture.write("proc/stat", "cpu 0 0 0 1\n");
        fixture.write("proc/cpuinfo", "cpu MHz : 4000.0\n");

        let temperature = discover_temperature_sensor_in(&fixture.path().join("hwmon")).unwrap();
        let energy =
            discover_energy_sensor_in(&fixture.path().join("powercap"), empty_hwmon.path())
                .unwrap();
        let started = std::time::Instant::now();
        let mut sampler = LinuxMetricSampler::from_parts(
            temperature,
            energy,
            fixture.path().join("proc/stat"),
            fixture.path().join("proc/cpuinfo"),
            TemperatureUnit::Celsius,
            started,
        )
        .unwrap();
        sampler.set_temperature_unit(TemperatureUnit::Fahrenheit);
        fixture.write("powercap/package/energy_uj", "10000000\n");
        fixture.write("proc/stat", "cpu 1 0 0 10\n");

        let metrics = sampler.sample_at(started + Duration::from_secs(1)).unwrap();

        assert_eq!(metrics.temperature().unit(), TemperatureUnit::Fahrenheit);
        assert_eq!(metrics.temperature().degrees(), 122.0);
    }
}
