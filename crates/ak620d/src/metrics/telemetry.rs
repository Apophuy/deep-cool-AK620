use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use ak620_core::{GpuTelemetry, HostTelemetry, StorageVolume};

use super::MetricsError;

const PROC_ROOT: &str = "/proc";
const SYS_ROOT: &str = "/sys";
const ETC_ROOT: &str = "/etc";
const PCI_IDS_PATHS: [&str; 2] = ["/usr/share/misc/pci.ids", "/usr/share/hwdata/pci.ids"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct IoCounters {
    first: u64,
    second: u64,
}

#[derive(Debug, Clone)]
struct GpuSensor {
    device_path: PathBuf,
    hwmon_path: PathBuf,
}

/// Stateful best-effort collector for metrics which are not written to the cooler display.
pub struct LinuxTelemetrySampler {
    proc_root: PathBuf,
    inventory: HostTelemetry,
    gpu_sensor: Option<GpuSensor>,
    fan_input: Option<PathBuf>,
    previous_disk: IoCounters,
    previous_network: IoCounters,
    previous_instant: Instant,
}

impl LinuxTelemetrySampler {
    /// Discovers readable Linux telemetry without opening a HID device.
    pub fn discover() -> Result<Self, MetricsError> {
        Self::discover_in(
            Path::new(PROC_ROOT),
            Path::new(SYS_ROOT),
            Path::new(ETC_ROOT),
            Instant::now(),
        )
    }

    fn discover_in(
        proc_root: &Path,
        sys_root: &Path,
        etc_root: &Path,
        now: Instant,
    ) -> Result<Self, MetricsError> {
        let diskstats_path = proc_root.join("diskstats");
        let net_dev_path = proc_root.join("net/dev");
        let previous_disk = parse_diskstats(&read(&diskstats_path)?)?;
        let previous_network = parse_net_dev(&read(&net_dev_path)?)?;
        let gpu_sensor = discover_gpu(sys_root);
        let fan_input = discover_fan_input(&sys_root.join("class/hwmon"));
        let inventory = inventory(proc_root, sys_root, etc_root, gpu_sensor.as_ref());

        Ok(Self {
            proc_root: proc_root.to_owned(),
            inventory,
            gpu_sensor,
            fan_input,
            previous_disk,
            previous_network,
            previous_instant: now,
        })
    }

    /// Samples available telemetry. Optional hardware readings disappear when they cannot be read.
    pub fn sample(&mut self) -> Result<HostTelemetry, MetricsError> {
        self.sample_at(Instant::now())
    }

    fn sample_at(&mut self, now: Instant) -> Result<HostTelemetry, MetricsError> {
        let elapsed = now
            .checked_duration_since(self.previous_instant)
            .ok_or_else(|| MetricsError::invalid("telemetry clock", "monotonic time regressed"))?;
        let disk = parse_diskstats(&read(&self.proc_root.join("diskstats"))?)?;
        let network = parse_net_dev(&read(&self.proc_root.join("net/dev"))?)?;
        let (memory_total_bytes, memory_used_bytes) =
            parse_meminfo(&read(&self.proc_root.join("meminfo"))?)?;

        let mut snapshot = self.inventory.clone();
        snapshot.memory_total_bytes = memory_total_bytes;
        snapshot.memory_used_bytes = memory_used_bytes;
        snapshot.storage_volumes = mounted_volumes(&self.proc_root.join("self/mounts"));
        snapshot.storage_read_bytes_per_second =
            rate(disk.first.saturating_sub(self.previous_disk.first), elapsed);
        snapshot.storage_write_bytes_per_second = rate(
            disk.second.saturating_sub(self.previous_disk.second),
            elapsed,
        );
        snapshot.network_receive_bytes_per_second = rate(
            network.first.saturating_sub(self.previous_network.first),
            elapsed,
        );
        snapshot.network_transmit_bytes_per_second = rate(
            network.second.saturating_sub(self.previous_network.second),
            elapsed,
        );
        snapshot.gpu = self.gpu_sensor.as_ref().and_then(sample_gpu);
        snapshot.fan_rpm = self.fan_input.as_ref().and_then(|path| read_u32(path).ok());

        self.previous_disk = disk;
        self.previous_network = network;
        self.previous_instant = now;
        Ok(snapshot)
    }
}

fn inventory(
    proc_root: &Path,
    sys_root: &Path,
    etc_root: &Path,
    gpu: Option<&GpuSensor>,
) -> HostTelemetry {
    let memory_total = read(&proc_root.join("meminfo"))
        .ok()
        .and_then(|contents| parse_meminfo(&contents).ok())
        .map_or(0, |(total, _)| total);
    HostTelemetry {
        host_name: read_trimmed(&proc_root.join("sys/kernel/hostname")).unwrap_or_default(),
        operating_system: read(&etc_root.join("os-release"))
            .ok()
            .and_then(|contents| os_pretty_name(&contents))
            .unwrap_or_default(),
        cpu_name: read(&proc_root.join("cpuinfo"))
            .ok()
            .and_then(|contents| cpu_name(&contents))
            .unwrap_or_default(),
        gpu_name: gpu.and_then(gpu_name).unwrap_or_default(),
        motherboard_name: motherboard_name(&sys_root.join("class/dmi/id")),
        memory_description: if memory_total == 0 {
            String::new()
        } else {
            format!("{} GB", installed_memory_gib(memory_total))
        },
        drive_models: drive_models(&sys_root.join("class/block")),
        ..HostTelemetry::default()
    }
}

fn discover_gpu(sys_root: &Path) -> Option<GpuSensor> {
    let drm_root = sys_root.join("class/drm");
    let mut cards = directory_paths(&drm_root).ok()?;
    cards.sort();
    for card in cards {
        let card_name = card.file_name()?.to_str()?;
        if !card_name.strip_prefix("card").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            continue;
        }
        let device_path = card.join("device");
        let uevent = read(&device_path.join("uevent")).ok()?;
        if !uevent.lines().any(|line| line == "DRIVER=amdgpu") {
            continue;
        }
        let hwmon_root = device_path.join("hwmon");
        let hwmon_path = directory_paths(&hwmon_root)
            .ok()?
            .into_iter()
            .find(|path| read_trimmed(&path.join("name")).as_deref() == Some("amdgpu"))?;
        return Some(GpuSensor {
            device_path,
            hwmon_path,
        });
    }
    None
}

fn sample_gpu(sensor: &GpuSensor) -> Option<GpuTelemetry> {
    let utilization = read_u32(&sensor.device_path.join("gpu_busy_percent")).ok()?;
    let utilization_percent = u8::try_from(utilization.min(100)).ok()?;
    let frequency_mhz = read(&sensor.device_path.join("pp_dpm_sclk"))
        .ok()
        .and_then(|contents| parse_current_gpu_clock(&contents))
        .or_else(|| {
            read_u64(&sensor.hwmon_path.join("freq1_input"))
                .ok()
                .and_then(|hertz| u32::try_from(hertz / 1_000_000).ok())
        })?;
    let temperature_celsius =
        f64::from(read_u32(&sensor.hwmon_path.join("temp1_input")).ok()?) / 1_000.0;
    let memory_used_bytes = read_u64(&sensor.device_path.join("mem_info_vram_used")).ok()?;
    let memory_total_bytes = read_u64(&sensor.device_path.join("mem_info_vram_total")).ok()?;
    Some(GpuTelemetry {
        utilization_percent,
        frequency_mhz,
        temperature_celsius,
        memory_used_bytes,
        memory_total_bytes,
    })
}

fn gpu_name(sensor: &GpuSensor) -> Option<String> {
    let uevent = read(&sensor.device_path.join("uevent")).ok()?;
    let pci_id = uevent
        .lines()
        .find_map(|line| line.strip_prefix("PCI_ID="))?;
    let (vendor, device) = pci_id.split_once(':')?;
    let prefix = if vendor.eq_ignore_ascii_case("1002") {
        "AMD Radeon "
    } else {
        ""
    };
    for path in PCI_IDS_PATHS {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(name) = pci_device_name(&contents, vendor, device) {
            return Some(format!("{prefix}{name}"));
        }
    }
    Some(format!("PCI {vendor}:{device}"))
}

fn pci_device_name(contents: &str, vendor: &str, device: &str) -> Option<String> {
    let vendor = vendor.to_ascii_lowercase();
    let device = device.to_ascii_lowercase();
    let mut in_vendor = false;
    for line in contents.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line
                .split_ascii_whitespace()
                .next()
                .is_some_and(|id| id.eq_ignore_ascii_case(&vendor));
            continue;
        }
        if !in_vendor || line.starts_with("\t\t") {
            continue;
        }
        let mut fields = line.split_ascii_whitespace();
        if fields
            .next()
            .is_some_and(|id| id.eq_ignore_ascii_case(&device))
        {
            return Some(fields.collect::<Vec<_>>().join(" "));
        }
    }
    None
}

fn discover_fan_input(hwmon_root: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for directory in directory_paths(hwmon_root).ok()? {
        let driver = read_trimmed(&directory.join("name")).unwrap_or_default();
        if driver == "amdgpu" {
            continue;
        }
        for path in directory_paths(&directory).ok()? {
            let name = path.file_name()?.to_str()?;
            let Some(channel) = name
                .strip_prefix("fan")
                .and_then(|value| value.strip_suffix("_input"))
                .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                continue;
            };
            let label = read_trimmed(&directory.join(format!("fan{channel}_label")))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let rank = if label.contains("cpu") { 0 } else { 1 };
            candidates.push((rank, path));
        }
    }
    candidates.sort();
    candidates.into_iter().map(|(_, path)| path).next()
}

fn parse_meminfo(contents: &str) -> Result<(u64, u64), MetricsError> {
    let total_kib = meminfo_value(contents, "MemTotal")?;
    let available_kib = meminfo_value(contents, "MemAvailable")?;
    let total = total_kib
        .checked_mul(1024)
        .ok_or_else(|| MetricsError::invalid("/proc/meminfo", "total memory overflowed"))?;
    let available = available_kib
        .checked_mul(1024)
        .ok_or_else(|| MetricsError::invalid("/proc/meminfo", "available memory overflowed"))?;
    Ok((total, total.saturating_sub(available)))
}

fn meminfo_value(contents: &str, name: &str) -> Result<u64, MetricsError> {
    let value = contents
        .lines()
        .find_map(|line| line.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix(':'))
        .and_then(|rest| rest.split_ascii_whitespace().next())
        .ok_or_else(|| MetricsError::invalid("/proc/meminfo", format!("{name} is missing")))?;
    value.parse::<u64>().map_err(|error| {
        MetricsError::invalid("/proc/meminfo", format!("invalid {name} value: {error}"))
    })
}

fn parse_diskstats(contents: &str) -> Result<IoCounters, MetricsError> {
    let mut result = IoCounters::default();
    for line in contents.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 || !is_physical_disk(fields[2]) {
            continue;
        }
        let read_sectors = parse_counter("/proc/diskstats", fields[5])?;
        let written_sectors = parse_counter("/proc/diskstats", fields[9])?;
        result.first = result
            .first
            .saturating_add(read_sectors.saturating_mul(512));
        result.second = result
            .second
            .saturating_add(written_sectors.saturating_mul(512));
    }
    Ok(result)
}

fn is_physical_disk(name: &str) -> bool {
    (name.starts_with("sd") && name.as_bytes().last().is_some_and(u8::is_ascii_alphabetic))
        || (name.starts_with("nvme") && !name.contains('p'))
        || (name.starts_with("vd") && name.as_bytes().last().is_some_and(u8::is_ascii_alphabetic))
}

fn parse_net_dev(contents: &str) -> Result<IoCounters, MetricsError> {
    let mut result = IoCounters::default();
    for line in contents.lines() {
        let Some((interface, counters)) = line.split_once(':') else {
            continue;
        };
        if interface.trim() == "lo" {
            continue;
        }
        let fields = counters.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 9 {
            return Err(MetricsError::invalid(
                "/proc/net/dev",
                "interface counter line is incomplete",
            ));
        }
        result.first = result
            .first
            .saturating_add(parse_counter("/proc/net/dev", fields[0])?);
        result.second = result
            .second
            .saturating_add(parse_counter("/proc/net/dev", fields[8])?);
    }
    Ok(result)
}

fn parse_counter(context: &'static str, value: &str) -> Result<u64, MetricsError> {
    value
        .parse::<u64>()
        .map_err(|error| MetricsError::invalid(context, format!("invalid counter: {error}")))
}

fn parse_current_gpu_clock(contents: &str) -> Option<u32> {
    contents.lines().find_map(|line| {
        if !line.trim_end().ends_with('*') {
            return None;
        }
        let value = line.split_ascii_whitespace().nth(1)?;
        value
            .strip_suffix("Mhz")
            .or_else(|| value.strip_suffix("MHz"))?
            .parse()
            .ok()
    })
}

fn mounted_volumes(mounts_path: &Path) -> Vec<StorageVolume> {
    let Ok(contents) = read(mounts_path) else {
        return Vec::new();
    };
    let mut volumes = Vec::new();
    let mut seen_filesystems = BTreeSet::new();
    for line in contents.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(source) = fields.next() else {
            continue;
        };
        let Some(mount_point) = fields.next() else {
            continue;
        };
        if !source.starts_with("/dev/") {
            continue;
        }
        let mount_point = unescape_mount_field(mount_point);
        if !is_user_storage_mount(&mount_point) {
            continue;
        }
        let Ok(stats) = rustix::fs::statvfs(Path::new(&mount_point)) else {
            continue;
        };
        let total = stats.f_blocks.saturating_mul(stats.f_frsize);
        let available = stats.f_bavail.saturating_mul(stats.f_frsize);
        if !seen_filesystems.insert((stats.f_fsid, total)) {
            continue;
        }
        volumes.push(StorageVolume {
            label: mount_point,
            used_bytes: total.saturating_sub(available),
            total_bytes: total,
        });
    }
    volumes.sort_by(|left, right| left.label.cmp(&right.label));
    volumes
}

fn is_user_storage_mount(path: &str) -> bool {
    matches!(path, "/" | "/home" | "/var")
        || path.starts_with("/mnt/")
        || path.starts_with("/media/")
        || path.starts_with("/run/media/")
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn motherboard_name(dmi_root: &Path) -> String {
    let vendor = read_trimmed(&dmi_root.join("board_vendor")).unwrap_or_default();
    let model = read_trimmed(&dmi_root.join("board_name")).unwrap_or_default();
    [vendor, model]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn drive_models(block_root: &Path) -> Vec<String> {
    let mut models = Vec::new();
    let Ok(mut devices) = directory_paths(block_root) else {
        return models;
    };
    devices.sort();
    for device in devices {
        let Some(name) = device.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_physical_disk(name) {
            continue;
        }
        let Some(model) = read_trimmed(&device.join("device/model")) else {
            continue;
        };
        if !model.is_empty() {
            models.push(model);
        }
    }
    models
}

fn cpu_name(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_owned())
    })
}

fn os_pretty_name(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        line.strip_prefix("PRETTY_NAME=")
            .map(|value| value.trim_matches('"').replace("\\\"", "\""))
    })
}

fn installed_memory_gib(bytes: u64) -> u64 {
    let rounded_gib = bytes.saturating_add(1 << 29) / (1 << 30);
    rounded_gib.saturating_add(2) / 4 * 4
}

fn rate(delta: u64, elapsed: std::time::Duration) -> u64 {
    let nanos = elapsed.as_nanos();
    if nanos == 0 {
        return 0;
    }
    let value = u128::from(delta).saturating_mul(1_000_000_000) / nanos;
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn read(path: &Path) -> Result<String, MetricsError> {
    fs::read_to_string(path).map_err(|error| MetricsError::io(path, error))
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn read_u32(path: &Path) -> Result<u32, MetricsError> {
    let value = read(path)?;
    value.trim().parse().map_err(|error| {
        MetricsError::invalid("Linux telemetry", format!("{}: {error}", path.display()))
    })
}

fn read_u64(path: &Path) -> Result<u64, MetricsError> {
    let value = read(path)?;
    value.trim().parse().map_err(|error| {
        MetricsError::invalid("Linux telemetry", format!("{}: {error}", path.display()))
    })
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
    use std::time::Duration;

    use super::{
        IoCounters, LinuxTelemetrySampler, installed_memory_gib, is_user_storage_mount,
        parse_current_gpu_clock, parse_diskstats, parse_meminfo, parse_net_dev, pci_device_name,
    };
    use crate::metrics::test_support::FixtureDir;

    #[test]
    fn parses_memory_disk_and_network_counters() {
        assert_eq!(
            parse_meminfo("MemTotal: 1000 kB\nMemAvailable: 250 kB\n").unwrap(),
            (1_024_000, 768_000)
        );
        assert_eq!(
            parse_diskstats(
                "259 0 nvme0n1 1 0 20 0 2 0 30 0 0 0 0 0 0 0 0\n\
                 259 1 nvme0n1p1 1 0 999 0 2 0 999 0 0 0 0 0 0 0 0\n"
            )
            .unwrap(),
            IoCounters {
                first: 20 * 512,
                second: 30 * 512,
            }
        );
        assert_eq!(
            parse_net_dev(
                "Inter-| Receive | Transmit\n lo: 50 0 0 0 0 0 0 0 70\n eth0: 100 0 0 0 0 0 0 0 200\n"
            )
            .unwrap(),
            IoCounters {
                first: 100,
                second: 200,
            }
        );
    }

    #[test]
    fn parses_active_gpu_clock_and_pci_name() {
        assert_eq!(
            parse_current_gpu_clock("0: 500Mhz\n1: 59Mhz *\n2: 2570Mhz\n"),
            Some(59)
        );
        assert_eq!(
            pci_device_name("1002  AMD\n\t7550  Navi 48 [RX 9070 XT]\n", "1002", "7550").as_deref(),
            Some("Navi 48 [RX 9070 XT]")
        );
    }

    #[test]
    fn inventory_formats_installed_memory_and_filters_service_bind_mounts() {
        assert_eq!(installed_memory_gib(65_476_064 * 1024), 64);
        assert!(is_user_storage_mount("/"));
        assert!(is_user_storage_mount("/mnt/DATA"));
        assert!(is_user_storage_mount("/run/media/user/drive"));
        assert!(!is_user_storage_mount("/etc"));
        assert!(!is_user_storage_mount("/boot/efi"));
        assert!(!is_user_storage_mount("/var/lib/ak620-linux"));
    }

    #[test]
    fn sampler_derives_rates_from_isolated_proc_fixtures() {
        let fixture = FixtureDir::new("telemetry");
        fixture.write("proc/diskstats", "8 0 sda 1 0 10 0 1 0 20 0 0 0 0\n");
        fixture.write("proc/net/dev", "eth0: 100 0 0 0 0 0 0 0 200\n");
        fixture.write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 400 kB\n");
        fixture.write("proc/cpuinfo", "model name : Test CPU\n");
        fixture.write("proc/sys/kernel/hostname", "test-host\n");
        fixture.write("proc/self/mounts", "");
        fixture.write("etc/os-release", "PRETTY_NAME=\"Test Linux\"\n");
        let started = std::time::Instant::now();
        let mut sampler = LinuxTelemetrySampler::discover_in(
            &fixture.path().join("proc"),
            &fixture.path().join("sys"),
            &fixture.path().join("etc"),
            started,
        )
        .unwrap();

        fixture.write("proc/diskstats", "8 0 sda 1 0 30 0 1 0 60 0 0 0 0\n");
        fixture.write("proc/net/dev", "eth0: 300 0 0 0 0 0 0 0 500\n");
        let snapshot = sampler.sample_at(started + Duration::from_secs(2)).unwrap();

        assert_eq!(snapshot.cpu_name, "Test CPU");
        assert_eq!(snapshot.host_name, "test-host");
        assert_eq!(snapshot.operating_system, "Test Linux");
        assert_eq!(snapshot.memory_used_bytes, 600 * 1024);
        assert_eq!(snapshot.storage_read_bytes_per_second, 5_120);
        assert_eq!(snapshot.storage_write_bytes_per_second, 10_240);
        assert_eq!(snapshot.network_receive_bytes_per_second, 100);
        assert_eq!(snapshot.network_transmit_bytes_per_second, 150);
    }
}
