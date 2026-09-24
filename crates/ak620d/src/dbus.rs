//! Versioned system D-Bus boundary shared by the daemon and desktop clients.

use std::{
    sync::{Arc, RwLock, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use ak620_core::DisplayMetrics;
use zbus::{connection, fdo, interface};

use crate::{
    config::{Config, ConfigTemperatureUnit, MAX_UPDATE_INTERVAL_MS, MIN_UPDATE_INTERVAL_MS},
    hid::DeviceIdentity,
};

/// Well-known name which also prevents two daemons owning the cooler.
pub const BUS_NAME: &str = "io.github.ak620linux.Daemon";
/// Stable object path for the version-one API.
pub const OBJECT_PATH: &str = "/io/github/ak620linux/Daemon";

/// Settings requests delivered from D-Bus to the hardware loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonCommand {
    /// Change the unit and persist it after validation.
    SetTemperatureUnit(ConfigTemperatureUnit),
    /// Change the bounded refresh interval and persist it after validation.
    SetUpdateIntervalMs(u64),
}

/// Coarse connection state exposed to the tray application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Daemon started and has not completed its first connection attempt.
    Starting,
    /// The exact target is open and the last display update succeeded.
    Connected,
    /// Device discovery or the most recent update failed.
    Disconnected,
}

impl ConnectionState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Copyable metric values suited to the D-Bus scalar boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricStatus {
    /// CPU package power in whole watts.
    pub power_watts: u16,
    /// Temperature rounded to the configured display unit.
    pub temperature_degrees: f64,
    /// Aggregate CPU utilization percentage.
    pub utilization_percent: u8,
    /// Highest observed core frequency in MHz.
    pub frequency_mhz: u16,
}

impl From<DisplayMetrics> for MetricStatus {
    fn from(metrics: DisplayMetrics) -> Self {
        Self {
            power_watts: metrics.power().get(),
            temperature_degrees: f64::from(metrics.temperature().degrees()),
            utilization_percent: metrics.utilization().get(),
            frequency_mhz: metrics.frequency().get(),
        }
    }
}

/// Immutable status snapshot read by the D-Bus interface.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    /// Current connection lifecycle state.
    pub connection_state: ConnectionState,
    /// hidraw path of the open target.
    pub device_path: Option<String>,
    /// Last actionable error, cleared by a successful update.
    pub last_error: Option<String>,
    /// Last successfully displayed metric values.
    pub metrics: Option<MetricStatus>,
    /// Unix timestamp of the last successful display update.
    pub last_update_unix_seconds: u64,
    /// Settings currently applied by the hardware loop.
    pub config: Config,
}

/// Poison-tolerant synchronized status shared with zbus worker threads.
#[derive(Clone)]
pub struct StatusStore(Arc<RwLock<StatusSnapshot>>);

impl StatusStore {
    /// Creates initial status for a loaded configuration.
    pub fn new(config: Config) -> Self {
        Self(Arc::new(RwLock::new(StatusSnapshot {
            connection_state: ConnectionState::Starting,
            device_path: None,
            last_error: None,
            metrics: None,
            last_update_unix_seconds: 0,
            config,
        })))
    }

    /// Returns a coherent clone for IPC serialization or diagnostics.
    pub fn snapshot(&self) -> StatusSnapshot {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Records a failed discovery, metric read, or HID write.
    pub fn record_disconnected(&self, error: impl Into<String>) {
        let mut status = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        status.connection_state = ConnectionState::Disconnected;
        status.device_path = None;
        status.last_error = Some(error.into());
    }

    /// Records a non-connection failure while preserving the current device state.
    pub fn record_error(&self, error: impl Into<String>) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last_error = Some(error.into());
    }

    /// Records a metric failure while retaining the identity of an open HID device.
    pub fn record_metric_failure(&self, error: impl Into<String>) {
        let mut status = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        status.connection_state = ConnectionState::Disconnected;
        status.last_error = Some(error.into());
    }

    /// Records the exact device selected by safe discovery.
    pub fn record_device_opened(&self, identity: &DeviceIdentity) {
        let mut status = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        status.device_path = Some(identity.path().to_owned());
    }

    /// Records a complete display update and clears a stale failure.
    pub fn record_update(&self, metrics: DisplayMetrics) {
        let mut status = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        status.connection_state = ConnectionState::Connected;
        status.last_error = None;
        status.metrics = Some(metrics.into());
        status.last_update_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
    }

    /// Records settings only after the hardware loop has applied and saved them.
    pub fn record_config(&self, config: Config) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .config = config;
    }
}

/// Owns the dedicated Tokio runtime that serves the system D-Bus interface.
pub struct SystemService {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    executor_thread: thread::JoinHandle<()>,
}

impl SystemService {
    /// Stops the D-Bus executor before the daemon process exits.
    pub fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        let _ = self.executor_thread.join();
    }
}

/// Starts the version-one service on the system bus for every local desktop user.
pub fn start_system_service(
    status: StatusStore,
    commands: mpsc::Sender<DaemonCommand>,
) -> zbus::Result<SystemService> {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (shutdown_sender, mut shutdown_receiver) = tokio::sync::oneshot::channel();
    let executor_thread = thread::Builder::new()
        .name("ak620-dbus-executor".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_sender.send(Err(zbus::Error::Failure(format!(
                        "could not start D-Bus runtime: {error}"
                    ))));
                    return;
                }
            };
            runtime.block_on(async move {
                let builder = match connection::Builder::system()
                    .and_then(|builder| builder.name(BUS_NAME))
                    .and_then(|builder| {
                        builder.serve_at(OBJECT_PATH, DaemonInterface { status, commands })
                    }) {
                    Ok(builder) => builder,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                let connection = match builder.build().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                if ready_sender.send(Ok(())).is_err() {
                    return;
                }
                loop {
                    tokio::select! {
                        _ = connection.executor().tick() => {}
                        _ = &mut shutdown_receiver => break,
                    }
                }
            });
        })
        .map_err(|error| {
            zbus::Error::Failure(format!("could not start D-Bus executor: {error}"))
        })?;

    match ready_receiver.recv() {
        Ok(Ok(())) => Ok(SystemService {
            shutdown: Some(shutdown_sender),
            executor_thread,
        }),
        Ok(Err(error)) => {
            let _ = executor_thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = executor_thread.join();
            Err(zbus::Error::Failure(format!(
                "D-Bus executor ended before startup completed: {error}"
            )))
        }
    }
}

struct DaemonInterface {
    status: StatusStore,
    commands: mpsc::Sender<DaemonCommand>,
}

#[interface(name = "io.github.ak620linux.Daemon1")]
impl DaemonInterface {
    #[zbus(property)]
    fn api_version(&self) -> u32 {
        1
    }

    #[zbus(property)]
    fn connection_state(&self) -> String {
        self.status.snapshot().connection_state.as_str().to_owned()
    }

    #[zbus(property)]
    fn device_path(&self) -> String {
        self.status.snapshot().device_path.unwrap_or_default()
    }

    #[zbus(property)]
    fn last_error(&self) -> String {
        self.status.snapshot().last_error.unwrap_or_default()
    }

    #[zbus(property)]
    fn has_metrics(&self) -> bool {
        self.status.snapshot().metrics.is_some()
    }

    #[zbus(property)]
    fn power_watts(&self) -> u16 {
        self.status
            .snapshot()
            .metrics
            .map_or(0, |metrics| metrics.power_watts)
    }

    #[zbus(property)]
    fn temperature_degrees(&self) -> f64 {
        self.status
            .snapshot()
            .metrics
            .map_or(0.0, |metrics| metrics.temperature_degrees)
    }

    #[zbus(property)]
    fn utilization_percent(&self) -> u8 {
        self.status
            .snapshot()
            .metrics
            .map_or(0, |metrics| metrics.utilization_percent)
    }

    #[zbus(property)]
    fn frequency_mhz(&self) -> u16 {
        self.status
            .snapshot()
            .metrics
            .map_or(0, |metrics| metrics.frequency_mhz)
    }

    #[zbus(property)]
    fn last_update_unix_seconds(&self) -> u64 {
        self.status.snapshot().last_update_unix_seconds
    }

    #[zbus(property)]
    fn update_interval_ms(&self) -> u64 {
        self.status.snapshot().config.update_interval_ms()
    }

    #[zbus(property)]
    fn temperature_unit(&self) -> String {
        match self.status.snapshot().config.temperature_unit() {
            ConfigTemperatureUnit::Celsius => "celsius",
            ConfigTemperatureUnit::Fahrenheit => "fahrenheit",
        }
        .to_owned()
    }

    fn set_update_interval_ms(&self, milliseconds: u64) -> fdo::Result<()> {
        if !(MIN_UPDATE_INTERVAL_MS..=MAX_UPDATE_INTERVAL_MS).contains(&milliseconds) {
            return Err(fdo::Error::InvalidArgs(format!(
                "interval must be within {MIN_UPDATE_INTERVAL_MS}..={MAX_UPDATE_INTERVAL_MS} ms"
            )));
        }
        self.send(DaemonCommand::SetUpdateIntervalMs(milliseconds))
    }

    fn set_temperature_unit(&self, unit: &str) -> fdo::Result<()> {
        let unit = match unit {
            "celsius" => ConfigTemperatureUnit::Celsius,
            "fahrenheit" => ConfigTemperatureUnit::Fahrenheit,
            _ => {
                return Err(fdo::Error::InvalidArgs(
                    "unit must be 'celsius' or 'fahrenheit'".to_owned(),
                ));
            }
        };
        self.send(DaemonCommand::SetTemperatureUnit(unit))
    }
}

impl DaemonInterface {
    fn send(&self, command: DaemonCommand) -> fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| fdo::Error::Failed("daemon update loop is not running".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{DaemonCommand, DaemonInterface, StatusStore};
    use crate::config::{Config, ConfigTemperatureUnit};

    #[test]
    fn settings_methods_validate_and_forward_commands() {
        let (sender, receiver) = mpsc::channel();
        let interface = DaemonInterface {
            status: StatusStore::new(Config::default()),
            commands: sender,
        };

        interface.set_update_interval_ms(750).unwrap();
        interface.set_temperature_unit("fahrenheit").unwrap();

        assert_eq!(
            receiver.recv().unwrap(),
            DaemonCommand::SetUpdateIntervalMs(750)
        );
        assert_eq!(
            receiver.recv().unwrap(),
            DaemonCommand::SetTemperatureUnit(ConfigTemperatureUnit::Fahrenheit)
        );
        assert!(interface.set_update_interval_ms(10).is_err());
        assert!(interface.set_temperature_unit("kelvin").is_err());
    }
}
