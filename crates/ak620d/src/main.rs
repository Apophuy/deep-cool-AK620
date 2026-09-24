//! AK620 DIGITAL PRO background service entry point.

use std::{
    error::Error,
    path::Path,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use ak620_core::{DisplayReport, TemperatureUnit};
use ak620d::{
    config::{Config, default_config_path},
    dbus::{DaemonCommand, StatusStore, start_system_service},
    hid::Ak620Device,
    metrics::LinuxMetricSampler,
    service::ReconnectBackoff,
};
use signal_hook::consts::{SIGINT, SIGTERM};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn main() -> ExitCode {
    init_logging();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "daemon stopped with a fatal error");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config_path = default_config_path()?;
    let mut config = Config::load_or_default(&config_path)?;
    let status = StatusStore::new(config.clone());
    let (command_sender, command_receiver) = mpsc::channel();
    // Keep the connection alive for the complete update loop. Dropping it releases the
    // well-known name and stops zbus from serving the desktop client.
    let dbus_service = start_system_service(status.clone(), command_sender)?;
    let shutdown = install_signal_handlers()?;

    info!(
        config_path = %config_path.display(),
        interval_ms = config.update_interval_ms(),
        "AK620 daemon started"
    );

    let mut sampler = None;
    let mut device = None;
    let mut backoff = ReconnectBackoff::default();

    while !shutdown.load(Ordering::Relaxed) {
        apply_pending_commands(
            &command_receiver,
            &mut config,
            &config_path,
            &mut sampler,
            &status,
        );

        if sampler.is_none() {
            match LinuxMetricSampler::discover(TemperatureUnit::from(config.temperature_unit())) {
                Ok(discovered) => {
                    info!(
                        temperature_driver = discovered.temperature_sensor().driver(),
                        temperature_label = discovered.temperature_sensor().label(),
                        temperature_path = %discovered.temperature_sensor().input_path().display(),
                        energy_source = discovered.energy_sensor().source_name(),
                        energy_path = %discovered.energy_sensor().input_path().display(),
                        "Linux CPU metric sources discovered"
                    );
                    sampler = Some(discovered);
                    wait_with_commands(
                        Duration::from_millis(config.update_interval_ms()),
                        &shutdown,
                        &command_receiver,
                        &mut config,
                        &config_path,
                        &mut sampler,
                        &status,
                    );
                    continue;
                }
                Err(error) => {
                    let message = format!("metric discovery failed: {error}");
                    warn!(error = %error, "metric discovery failed; will retry");
                    status.record_disconnected(message);
                    let delay = backoff.next_delay();
                    wait_with_commands(
                        delay,
                        &shutdown,
                        &command_receiver,
                        &mut config,
                        &config_path,
                        &mut sampler,
                        &status,
                    );
                    continue;
                }
            }
        }

        if device.is_none() {
            match Ak620Device::connect() {
                Ok(connected) => {
                    info!(
                        path = connected.identity().path(),
                        manufacturer = connected.identity().manufacturer().unwrap_or("unknown"),
                        product = connected.identity().product().unwrap_or("unknown"),
                        interface = connected.identity().interface_number(),
                        "AK620 DIGITAL PRO connected"
                    );
                    status.record_device_opened(connected.identity());
                    device = Some(connected);
                }
                Err(error) => {
                    let message = format!("device connection failed: {error}");
                    warn!(error = %error, "device connection failed; will retry");
                    status.record_disconnected(message);
                    let delay = backoff.next_delay();
                    wait_with_commands(
                        delay,
                        &shutdown,
                        &command_receiver,
                        &mut config,
                        &config_path,
                        &mut sampler,
                        &status,
                    );
                    continue;
                }
            }
        }

        let metrics = match sampler.as_mut().expect("sampler initialized").sample() {
            Ok(metrics) => metrics,
            Err(error) => {
                let message = format!("metric update failed: {error}");
                warn!(error = %error, "metric read failed; rediscovering sensors");
                status.record_metric_failure(message);
                sampler = None;
                wait_with_commands(
                    backoff.next_delay(),
                    &shutdown,
                    &command_receiver,
                    &mut config,
                    &config_path,
                    &mut sampler,
                    &status,
                );
                continue;
            }
        };

        let report = DisplayReport::encode(metrics);
        if let Err(error) = device
            .as_ref()
            .expect("device initialized")
            .write_report(&report)
        {
            let message = format!("display update failed: {error}");
            warn!(error = %error, "HID write failed; reconnecting");
            status.record_disconnected(message);
            device = None;
            wait_with_commands(
                backoff.next_delay(),
                &shutdown,
                &command_receiver,
                &mut config,
                &config_path,
                &mut sampler,
                &status,
            );
            continue;
        }

        backoff.reset();
        status.record_update(metrics);
        wait_with_commands(
            Duration::from_millis(config.update_interval_ms()),
            &shutdown,
            &command_receiver,
            &mut config,
            &config_path,
            &mut sampler,
            &status,
        );
    }

    dbus_service.shutdown();
    info!("shutdown signal received");
    Ok(())
}

fn apply_pending_commands(
    receiver: &mpsc::Receiver<DaemonCommand>,
    config: &mut Config,
    config_path: &Path,
    sampler: &mut Option<LinuxMetricSampler>,
    status: &StatusStore,
) {
    while let Ok(command) = receiver.try_recv() {
        apply_command(command, config, config_path, sampler, status);
    }
}

fn apply_command(
    command: DaemonCommand,
    config: &mut Config,
    config_path: &Path,
    sampler: &mut Option<LinuxMetricSampler>,
    status: &StatusStore,
) {
    let mut updated = config.clone();
    let result = match command {
        DaemonCommand::SetTemperatureUnit(unit) => {
            updated.set_temperature_unit(unit);
            Ok(())
        }
        DaemonCommand::SetUpdateIntervalMs(milliseconds) => {
            updated.set_update_interval_ms(milliseconds)
        }
    };
    if let Err(error) = result.and_then(|()| updated.save(config_path)) {
        warn!(error = %error, "could not apply configuration update");
        status.record_error(format!("configuration update failed: {error}"));
        return;
    }

    if let Some(sampler) = sampler {
        sampler.set_temperature_unit(TemperatureUnit::from(updated.temperature_unit()));
    }
    *config = updated.clone();
    status.record_config(updated);
    info!(
        interval_ms = config.update_interval_ms(),
        temperature_unit = ?config.temperature_unit(),
        "configuration updated"
    );
}

#[allow(clippy::too_many_arguments)]
fn wait_with_commands(
    duration: Duration,
    shutdown: &AtomicBool,
    receiver: &mpsc::Receiver<DaemonCommand>,
    config: &mut Config,
    config_path: &Path,
    sampler: &mut Option<LinuxMetricSampler>,
    status: &StatusStore,
) {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::Relaxed) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        match receiver.recv_timeout(remaining.min(SIGNAL_POLL_INTERVAL)) {
            Ok(command) => apply_command(command, config, config_path, sampler, status),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn install_signal_handlers() -> Result<Arc<AtomicBool>, std::io::Error> {
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&shutdown))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown))?;
    Ok(shutdown)
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ak620d=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
