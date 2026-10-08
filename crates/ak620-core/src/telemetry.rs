//! USB-independent models for host telemetry shown by desktop clients.

/// One mounted filesystem shown in the storage overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolume {
    /// Human-readable mount label, normally the mount point.
    pub label: String,
    /// Bytes currently occupied on the filesystem.
    pub used_bytes: u64,
    /// Total filesystem capacity in bytes.
    pub total_bytes: u64,
}

/// Optional AMD GPU values read from DRM and hwmon.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTelemetry {
    /// Aggregate GPU engine utilization.
    pub utilization_percent: u8,
    /// Current graphics clock.
    pub frequency_mhz: u32,
    /// Edge temperature in degrees Celsius.
    pub temperature_celsius: f64,
    /// Allocated dedicated video memory.
    pub memory_used_bytes: u64,
    /// Total dedicated video memory.
    pub memory_total_bytes: u64,
}

/// Best-effort telemetry and inventory which do not participate in HID reports.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HostTelemetry {
    /// Host name reported by the kernel.
    pub host_name: String,
    /// Distribution name from os-release.
    pub operating_system: String,
    /// Processor model from procfs.
    pub cpu_name: String,
    /// Graphics adapter name resolved from the PCI database.
    pub gpu_name: String,
    /// Motherboard vendor and model from DMI.
    pub motherboard_name: String,
    /// Human-readable memory description. Module details may be unavailable unprivileged.
    pub memory_description: String,
    /// Physical block-device models.
    pub drive_models: Vec<String>,
    /// GPU readings, absent when no supported AMD DRM device is readable.
    pub gpu: Option<GpuTelemetry>,
    /// Used system memory.
    pub memory_used_bytes: u64,
    /// Total system memory.
    pub memory_total_bytes: u64,
    /// Mounted local filesystems.
    pub storage_volumes: Vec<StorageVolume>,
    /// Aggregate physical-disk reads per second.
    pub storage_read_bytes_per_second: u64,
    /// Aggregate physical-disk writes per second.
    pub storage_write_bytes_per_second: u64,
    /// Aggregate received bytes per second across non-loopback interfaces.
    pub network_receive_bytes_per_second: u64,
    /// Aggregate transmitted bytes per second across non-loopback interfaces.
    pub network_transmit_bytes_per_second: u64,
    /// CPU/cooler fan speed when a readable motherboard hwmon channel exists.
    pub fan_rpm: Option<u32>,
}
