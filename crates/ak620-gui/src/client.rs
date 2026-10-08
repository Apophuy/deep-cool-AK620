use std::{
    sync::{Arc, RwLock, mpsc},
    thread,
    time::Duration,
};

use zbus::{Proxy, proxy::CacheProperties};

use crate::model::{DaemonSnapshot, TemperatureChoice};

const BUS_NAME: &str = "io.github.ak620linux.Daemon";
const OBJECT_PATH: &str = "/io/github/ak620linux/Daemon";
const INTERFACE_NAME: &str = "io.github.ak620linux.Daemon1";
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const LIVE_PROPERTY_CACHE: CacheProperties = CacheProperties::No;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ClientCommand {
    SetTemperatureUnit(TemperatureChoice),
    SetUpdateIntervalMs(u64),
}

#[derive(Clone)]
pub(crate) struct SharedSnapshot(Arc<RwLock<DaemonSnapshot>>);

impl Default for SharedSnapshot {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(DaemonSnapshot::default())))
    }
}

impl SharedSnapshot {
    pub(crate) fn get(&self) -> DaemonSnapshot {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, snapshot: DaemonSnapshot) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
    }
}

pub(crate) fn spawn_worker(shared: SharedSnapshot) -> mpsc::Sender<ClientCommand> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("ak620-dbus-client".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    shared.replace(DaemonSnapshot::unavailable(format!(
                        "Could not start the D-Bus runtime: {error}"
                    )));
                    return;
                }
            };
            runtime.block_on(worker_loop(&shared, &receiver));
        })
        .expect("could not start D-Bus client thread");
    sender
}

async fn worker_loop(shared: &SharedSnapshot, receiver: &mpsc::Receiver<ClientCommand>) {
    loop {
        let connection = match zbus::Connection::system().await {
            Ok(connection) => connection,
            Err(error) => {
                shared.replace(DaemonSnapshot::unavailable(format!(
                    "Could not connect to the system bus: {error}"
                )));
                tokio::time::sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        };
        // The daemon exposes live state as ordinary properties but does not emit a
        // PropertiesChanged signal for every sampling cycle. Polling must therefore bypass zbus'
        // default lazy property cache or the first snapshot remains visible indefinitely.
        let proxy = match zbus::proxy::Builder::<Proxy<'_>>::new(&connection)
            .destination(BUS_NAME)
            .and_then(|builder| builder.path(OBJECT_PATH))
            .and_then(|builder| builder.interface(INTERFACE_NAME))
            .map(|builder| builder.cache_properties(LIVE_PROPERTY_CACHE))
        {
            Ok(builder) => match builder.build().await {
                Ok(proxy) => proxy,
                Err(error) => {
                    shared.replace(DaemonSnapshot::unavailable(format!(
                        "Could not create the daemon proxy: {error}"
                    )));
                    tokio::time::sleep(RECONNECT_INTERVAL).await;
                    continue;
                }
            },
            Err(error) => {
                shared.replace(DaemonSnapshot::unavailable(format!(
                    "Could not create the daemon proxy: {error}"
                )));
                tokio::time::sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        };

        loop {
            while let Ok(command) = receiver.try_recv() {
                if let Err(error) = send_command(&proxy, command).await {
                    shared.replace(DaemonSnapshot::unavailable(format!(
                        "Could not update daemon settings: {error}"
                    )));
                    break;
                }
            }

            match read_snapshot(&proxy).await {
                Ok(snapshot) => shared.replace(snapshot),
                Err(error) => {
                    shared.replace(DaemonSnapshot::unavailable(format!(
                        "ak620d is unavailable: {error}"
                    )));
                    break;
                }
            }

            match receiver.recv_timeout(REFRESH_INTERVAL) {
                Ok(command) => {
                    if let Err(error) = send_command(&proxy, command).await {
                        shared.replace(DaemonSnapshot::unavailable(format!(
                            "Could not update daemon settings: {error}"
                        )));
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        tokio::time::sleep(RECONNECT_INTERVAL).await;
    }
}

async fn send_command(proxy: &Proxy<'_>, command: ClientCommand) -> zbus::Result<()> {
    match command {
        ClientCommand::SetTemperatureUnit(unit) => {
            proxy
                .call("SetTemperatureUnit", &(unit.as_dbus_str(),))
                .await
        }
        ClientCommand::SetUpdateIntervalMs(milliseconds) => {
            proxy.call("SetUpdateIntervalMs", &(milliseconds,)).await
        }
    }
}

async fn read_snapshot(proxy: &Proxy<'_>) -> zbus::Result<DaemonSnapshot> {
    let api_version = proxy.get_property("ApiVersion").await?;
    let unit: String = proxy.get_property("TemperatureUnit").await?;
    let temperature_unit = TemperatureChoice::from_dbus_str(&unit).ok_or_else(|| {
        zbus::Error::Failure(format!(
            "daemon returned an unknown temperature unit {unit:?}"
        ))
    })?;

    let mut snapshot = DaemonSnapshot {
        api_version,
        connection_state: proxy.get_property("ConnectionState").await?,
        device_path: proxy.get_property("DevicePath").await?,
        last_error: proxy.get_property("LastError").await?,
        has_metrics: proxy.get_property("HasMetrics").await?,
        power_watts: proxy.get_property("PowerWatts").await?,
        temperature_degrees: proxy.get_property("TemperatureDegrees").await?,
        utilization_percent: proxy.get_property("UtilizationPercent").await?,
        frequency_mhz: proxy.get_property("FrequencyMhz").await?,
        last_update_unix_seconds: proxy.get_property("LastUpdateUnixSeconds").await?,
        update_interval_ms: proxy.get_property("UpdateIntervalMs").await?,
        temperature_unit,
        ..DaemonSnapshot::default()
    };
    if api_version >= 2 {
        snapshot.has_telemetry = proxy.get_property("HasTelemetry").await?;
        snapshot.host_name = proxy.get_property("HostName").await?;
        snapshot.operating_system = proxy.get_property("OperatingSystem").await?;
        snapshot.cpu_name = proxy.get_property("CpuName").await?;
        snapshot.gpu_name = proxy.get_property("GpuName").await?;
        snapshot.motherboard_name = proxy.get_property("MotherboardName").await?;
        snapshot.memory_description = proxy.get_property("MemoryDescription").await?;
        snapshot.drive_models = proxy.get_property("DriveModels").await?;
        snapshot.has_gpu_metrics = proxy.get_property("HasGpuMetrics").await?;
        snapshot.gpu_utilization_percent = proxy.get_property("GpuUtilizationPercent").await?;
        snapshot.gpu_frequency_mhz = proxy.get_property("GpuFrequencyMhz").await?;
        snapshot.gpu_temperature_celsius = proxy.get_property("GpuTemperatureCelsius").await?;
        snapshot.gpu_memory_used_bytes = proxy.get_property("GpuMemoryUsedBytes").await?;
        snapshot.gpu_memory_total_bytes = proxy.get_property("GpuMemoryTotalBytes").await?;
        snapshot.memory_used_bytes = proxy.get_property("MemoryUsedBytes").await?;
        snapshot.memory_total_bytes = proxy.get_property("MemoryTotalBytes").await?;
        snapshot.storage_labels = proxy.get_property("StorageLabels").await?;
        snapshot.storage_used_bytes = proxy.get_property("StorageUsedBytes").await?;
        snapshot.storage_total_bytes = proxy.get_property("StorageTotalBytes").await?;
        snapshot.storage_read_bytes_per_second =
            proxy.get_property("StorageReadBytesPerSecond").await?;
        snapshot.storage_write_bytes_per_second =
            proxy.get_property("StorageWriteBytesPerSecond").await?;
        snapshot.network_receive_bytes_per_second =
            proxy.get_property("NetworkReceiveBytesPerSecond").await?;
        snapshot.network_transmit_bytes_per_second =
            proxy.get_property("NetworkTransmitBytesPerSecond").await?;
        snapshot.has_fan_rpm = proxy.get_property("HasFanRpm").await?;
        snapshot.fan_rpm = proxy.get_property("FanRpm").await?;
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::{CacheProperties, LIVE_PROPERTY_CACHE};

    #[test]
    fn live_metric_polling_never_uses_the_property_cache() {
        assert_eq!(LIVE_PROPERTY_CACHE, CacheProperties::No);
    }
}
