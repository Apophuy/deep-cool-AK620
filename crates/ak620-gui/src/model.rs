/// Temperature choice accepted by the daemon's version-one D-Bus API.
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
