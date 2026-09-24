//! Explicit, bounded hardware acceptance helper. Never run from ordinary CI.

use std::{error::Error, process::ExitCode, thread, time::Duration};

use ak620_core::{
    CpuFrequencyMhz, CpuUtilization, DisplayMetrics, DisplayReport, PowerWatts, Temperature,
    TemperatureUnit,
};
use ak620d::{hid::Ak620Device, metrics::LinuxMetricSampler, service::UpdateEngine};

const WRITE_OPT_IN: &str = "--i-understand-this-writes-one-known-report";
const LIVE_OPT_IN: &str = "--i-understand-this-writes-30-live-reports";
const LIVE_UPDATE_COUNT: u8 = 30;
const LIVE_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Inspect,
    WriteGolden,
    Live,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hardware check failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match parse_action(&arguments)? {
        Action::Inspect => inspect(),
        Action::WriteGolden => write_golden(),
        Action::Live => live(),
    }
}

fn parse_action(arguments: &[String]) -> Result<Action, String> {
    match arguments {
        [command] if command == "inspect" => Ok(Action::Inspect),
        [command, opt_in] if command == "write-golden" && opt_in == WRITE_OPT_IN => {
            Ok(Action::WriteGolden)
        }
        [command, opt_in] if command == "live" && opt_in == LIVE_OPT_IN => Ok(Action::Live),
        [command] if command == "write-golden" => Err(format!(
            "write-golden requires the explicit {WRITE_OPT_IN} argument"
        )),
        [command] if command == "live" => {
            Err(format!("live requires the explicit {LIVE_OPT_IN} argument"))
        }
        _ => Err(format!(
            "usage:\n  ak620-hwtest inspect\n  ak620-hwtest write-golden {WRITE_OPT_IN}\n  ak620-hwtest live {LIVE_OPT_IN}"
        )),
    }
}

fn inspect() -> Result<(), Box<dyn Error>> {
    println!("kernel: {}", read_kernel_release()?);
    let _device = Ak620Device::connect_after_identity(print_identity)?;
    println!("unprivileged read/write open: passed (no report was sent)");

    let sampler = LinuxMetricSampler::discover(TemperatureUnit::Celsius)?;
    println!(
        "temperature source: {}:{} ({})",
        sampler.temperature_sensor().driver(),
        sampler.temperature_sensor().label(),
        sampler.temperature_sensor().input_path().display()
    );
    println!(
        "package energy source: {} ({})",
        sampler.energy_sensor().source_name(),
        sampler.energy_sensor().input_path().display()
    );
    println!("inspection complete: zero USB reports written");
    Ok(())
}

fn write_golden() -> Result<(), Box<dyn Error>> {
    let report = unmistakable_golden_report()?;
    println!("bounded test: one 64-byte known report, then close the HID handle");
    println!("expected display: 42°C, 73%, 88 W, and 4321 MHz");
    println!("report: {:02x?}", report.as_bytes());
    let device = Ak620Device::connect_after_identity(print_identity)?;
    device.write_report(&report)?;
    println!("one complete report written; verify 42°C, 73%, 88 W, and 4321 MHz");
    Ok(())
}

fn live() -> Result<(), Box<dyn Error>> {
    println!("kernel: {}", read_kernel_release()?);
    println!(
        "bounded live test: {LIVE_UPDATE_COUNT} reports at one-second intervals, then close the HID handle"
    );

    let sampler = LinuxMetricSampler::discover(TemperatureUnit::Celsius)?;
    println!(
        "temperature source: {}:{} ({})",
        sampler.temperature_sensor().driver(),
        sampler.temperature_sensor().label(),
        sampler.temperature_sensor().input_path().display()
    );
    println!(
        "package energy source: {} ({})",
        sampler.energy_sensor().source_name(),
        sampler.energy_sensor().input_path().display()
    );

    let device = Ak620Device::connect_after_identity(print_identity)?;
    let mut engine = UpdateEngine::new(sampler, device);
    for sequence in 1..=LIVE_UPDATE_COUNT {
        thread::sleep(LIVE_UPDATE_INTERVAL);
        let metrics = engine.update_once()?;
        println!(
            "update {sequence:02}/{LIVE_UPDATE_COUNT}: {:.0}°C, {}%, {} W, {} MHz",
            metrics.temperature().degrees(),
            metrics.utilization().get(),
            metrics.power().get(),
            metrics.frequency().get()
        );
    }

    println!("bounded live test complete: {LIVE_UPDATE_COUNT} reports written; HID handle closed");
    Ok(())
}

fn unmistakable_golden_report() -> Result<DisplayReport, ak620_core::MetricError> {
    Ok(DisplayReport::encode(DisplayMetrics::new(
        PowerWatts::new(88)?,
        Temperature::new(42.0, TemperatureUnit::Celsius)?,
        CpuUtilization::new(73)?,
        CpuFrequencyMhz::new(4_321)?,
    )))
}

fn print_identity(identity: &ak620d::hid::DeviceIdentity) {
    println!("target VID:PID: 3633:0012");
    println!(
        "manufacturer: {}",
        identity.manufacturer().unwrap_or("<missing>")
    );
    println!("product: {}", identity.product().unwrap_or("<missing>"));
    println!("hidraw path: {}", identity.path());
    println!("interface: {}", identity.interface_number());
}

fn read_kernel_release() -> Result<String, std::io::Error> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Action, LIVE_OPT_IN, parse_action, unmistakable_golden_report};

    #[test]
    fn hardware_fixture_is_a_complete_known_report() {
        let report = unmistakable_golden_report().unwrap();
        assert_eq!(report.as_bytes().len(), 64);
        assert_eq!(&report.as_bytes()[8..10], &[0x00, 0x58]);
        assert_eq!(report.as_bytes()[15], 73);
        assert_eq!(&report.as_bytes()[16..18], &[0x10, 0xe1]);
    }

    #[test]
    fn live_mode_requires_the_exact_bounded_write_opt_in() {
        let accepted = vec!["live".to_owned(), LIVE_OPT_IN.to_owned()];
        assert_eq!(parse_action(&accepted), Ok(Action::Live));

        assert!(parse_action(&["live".to_owned()]).is_err());
        assert!(parse_action(&["live".to_owned(), "--allow-writes".to_owned()]).is_err());
    }
}
