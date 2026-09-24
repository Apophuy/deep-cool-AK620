use std::{error::Error, fmt, io, path::PathBuf};

use ak620_core::MetricError;

/// Failure while discovering, reading, or deriving Linux CPU metrics.
#[derive(Debug)]
pub enum MetricsError {
    /// A Linux pseudo-file or directory could not be accessed.
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying operating-system error.
        source: io::Error,
    },
    /// A pseudo-file did not contain the expected Linux ABI value.
    InvalidData {
        /// Human-readable source such as `/proc/stat` or `k10temp`.
        context: &'static str,
        /// Specific parse or consistency problem.
        detail: String,
    },
    /// No supported AMD CPU temperature channel was found.
    TemperatureSensorNotFound,
    /// No supported CPU package energy counter was found.
    EnergySensorNotFound,
    /// A cumulative counter moved backwards without a known wrap range.
    CounterRegressed {
        /// Earlier counter value.
        previous: u64,
        /// Later counter value.
        current: u64,
    },
    /// Two counter snapshots covered no measurable interval or CPU activity.
    NoProgress {
        /// Counter or clock that did not advance.
        context: &'static str,
    },
    /// A derived value could not be represented by the display domain.
    InvalidMetric(MetricError),
}

impl MetricsError {
    pub(super) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(super) fn invalid(context: &'static str, detail: impl Into<String>) -> Self {
        Self::InvalidData {
            context,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for MetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::InvalidData { context, detail } => write!(formatter, "{context}: {detail}"),
            Self::TemperatureSensorNotFound => {
                formatter.write_str("no k10temp Tdie or Tctl CPU temperature sensor was found")
            }
            Self::EnergySensorNotFound => formatter.write_str(
                "no CPU package energy counter was found in powercap or amd_energy hwmon",
            ),
            Self::CounterRegressed { previous, current } => write!(
                formatter,
                "counter regressed from {previous} to {current} without a known wrap range"
            ),
            Self::NoProgress { context } => write!(formatter, "{context} did not advance"),
            Self::InvalidMetric(source) => source.fmt(formatter),
        }
    }
}

impl Error for MetricsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidMetric(source) => Some(source),
            _ => None,
        }
    }
}

impl From<MetricError> for MetricsError {
    fn from(value: MetricError) -> Self {
        Self::InvalidMetric(value)
    }
}
