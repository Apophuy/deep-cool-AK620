/// Temperature choice accepted by the daemon's version-two D-Bus API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemperatureChoice {
    Celsius,
    Fahrenheit,
}

impl TemperatureChoice {
    pub(crate) const fn as_dbus_str(self) -> &'static str {
        match self {
            Self::Celsius => "celsius",
            Self::Fahrenheit => "fahrenheit",
        }
    }

    pub(crate) fn from_dbus_str(value: &str) -> Option<Self> {
        match value {
            "celsius" => Some(Self::Celsius),
            "fahrenheit" => Some(Self::Fahrenheit),
            _ => None,
        }
    }

    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
}

/// Complete UI-facing snapshot read from the daemon.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DaemonSnapshot {
    pub api_version: u32,
    pub connection_state: String,
    pub device_path: String,
    pub last_error: String,
    pub has_metrics: bool,
    pub power_watts: u16,
    pub temperature_degrees: f64,
    pub utilization_percent: u8,
    pub frequency_mhz: u16,
    pub last_update_unix_seconds: u64,
    pub update_interval_ms: u64,
    pub temperature_unit: TemperatureChoice,
    pub has_telemetry: bool,
    pub host_name: String,
    pub operating_system: String,
    pub cpu_name: String,
    pub gpu_name: String,
    pub motherboard_name: String,
    pub memory_description: String,
    pub drive_models: Vec<String>,
    pub has_gpu_metrics: bool,
    pub gpu_utilization_percent: u8,
    pub gpu_frequency_mhz: u32,
    pub gpu_temperature_celsius: f64,
    pub gpu_memory_used_bytes: u64,
    pub gpu_memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub storage_labels: Vec<String>,
    pub storage_used_bytes: Vec<u64>,
    pub storage_total_bytes: Vec<u64>,
    pub storage_read_bytes_per_second: u64,
    pub storage_write_bytes_per_second: u64,
    pub network_receive_bytes_per_second: u64,
    pub network_transmit_bytes_per_second: u64,
    pub has_fan_rpm: bool,
    pub fan_rpm: u32,
}

impl Default for DaemonSnapshot {
    fn default() -> Self {
        Self {
            api_version: 0,
            connection_state: "unavailable".to_owned(),
            device_path: String::new(),
            last_error: "Waiting for ak620d on the session bus".to_owned(),
            has_metrics: false,
            power_watts: 0,
            temperature_degrees: 0.0,
            utilization_percent: 0,
            frequency_mhz: 0,
            last_update_unix_seconds: 0,
            update_interval_ms: 1_000,
            temperature_unit: TemperatureChoice::Celsius,
            has_telemetry: false,
            host_name: String::new(),
            operating_system: String::new(),
            cpu_name: String::new(),
            gpu_name: String::new(),
            motherboard_name: String::new(),
            memory_description: String::new(),
            drive_models: Vec::new(),
            has_gpu_metrics: false,
            gpu_utilization_percent: 0,
            gpu_frequency_mhz: 0,
            gpu_temperature_celsius: 0.0,
            gpu_memory_used_bytes: 0,
            gpu_memory_total_bytes: 0,
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            storage_labels: Vec::new(),
            storage_used_bytes: Vec::new(),
            storage_total_bytes: Vec::new(),
            storage_read_bytes_per_second: 0,
            storage_write_bytes_per_second: 0,
            network_receive_bytes_per_second: 0,
            network_transmit_bytes_per_second: 0,
            has_fan_rpm: false,
            fan_rpm: 0,
        }
    }
}

impl DaemonSnapshot {
    pub(crate) fn connected(&self) -> bool {
        self.connection_state == "connected"
    }

    pub(crate) fn unavailable(error: impl Into<String>) -> Self {
        Self {
            last_error: error.into(),
            ..Self::default()
        }
    }
}

/// Editable settings that only resynchronize after the daemon reports an applied change.
pub(crate) struct SettingsDraft {
    pub interval_ms: u64,
    pub temperature_unit: TemperatureChoice,
    last_daemon: (u64, TemperatureChoice),
}

impl SettingsDraft {
    pub(crate) fn new(snapshot: &DaemonSnapshot) -> Self {
        Self {
            interval_ms: snapshot.update_interval_ms,
            temperature_unit: snapshot.temperature_unit,
            last_daemon: (snapshot.update_interval_ms, snapshot.temperature_unit),
        }
    }

    pub(crate) fn sync(&mut self, snapshot: &DaemonSnapshot) {
        let current = (snapshot.update_interval_ms, snapshot.temperature_unit);
        if current != self.last_daemon {
            self.interval_ms = current.0;
            self.temperature_unit = current.1;
            self.last_daemon = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonSnapshot, SettingsDraft, TemperatureChoice};

    #[test]
    fn temperature_choices_round_trip_dbus_values() {
        for choice in [TemperatureChoice::Celsius, TemperatureChoice::Fahrenheit] {
            assert_eq!(
                TemperatureChoice::from_dbus_str(choice.as_dbus_str()),
                Some(choice)
            );
        }
        assert_eq!(TemperatureChoice::from_dbus_str("kelvin"), None);
    }

    #[test]
    fn settings_draft_preserves_edits_until_daemon_state_changes() {
        let initial = DaemonSnapshot::default();
        let mut draft = SettingsDraft::new(&initial);
        draft.interval_ms = 750;
        draft.sync(&initial);
        assert_eq!(draft.interval_ms, 750);

        let applied = DaemonSnapshot {
            update_interval_ms: 750,
            temperature_unit: TemperatureChoice::Fahrenheit,
            ..initial
        };
        draft.sync(&applied);
        assert_eq!(draft.interval_ms, 750);
        assert_eq!(draft.temperature_unit, TemperatureChoice::Fahrenheit);
    }
}
