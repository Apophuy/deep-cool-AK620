//! Testable display-update primitives and reconnect policy.

use std::{error::Error, fmt, time::Duration};

use ak620_core::{DisplayMetrics, DisplayReport};

use crate::{
    hid::{Ak620Device, DeviceError},
    metrics::{LinuxMetricSampler, MetricsError},
};

const INITIAL_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Source of validated metric snapshots.
pub trait MetricSource {
    /// Samples metrics for one display update.
    fn sample(&mut self) -> Result<DisplayMetrics, MetricsError>;
}

impl MetricSource for LinuxMetricSampler {
    fn sample(&mut self) -> Result<DisplayMetrics, MetricsError> {
        LinuxMetricSampler::sample(self)
    }
}

/// Sink which accepts only a fully encoded AK620 display report.
pub trait DisplaySink {
    /// Sends one complete report.
    fn write_report(&self, report: &DisplayReport) -> Result<(), DeviceError>;
}

impl DisplaySink for Ak620Device {
    fn write_report(&self, report: &DisplayReport) -> Result<(), DeviceError> {
        Ak620Device::write_report(self, report)
    }
}

/// Couples a metric source and a validated display transport.
pub struct UpdateEngine<M, D> {
    metrics: M,
    display: D,
}

impl<M, D> UpdateEngine<M, D>
where
    M: MetricSource,
    D: DisplaySink,
{
    /// Creates an update engine from independently initialized adapters.
    pub const fn new(metrics: M, display: D) -> Self {
        Self { metrics, display }
    }

    /// Samples, encodes, and writes exactly one display update.
    pub fn update_once(&mut self) -> Result<DisplayMetrics, UpdateError> {
        let metrics = self.metrics.sample()?;
        let report = DisplayReport::encode(metrics);
        self.display.write_report(&report)?;
        Ok(metrics)
    }

    /// Returns the adapters, for example when replacing a failed HID handle.
    pub fn into_parts(self) -> (M, D) {
        (self.metrics, self.display)
    }
}

/// Error from one update attempt.
#[derive(Debug)]
pub enum UpdateError {
    /// Linux metric collection failed.
    Metrics(MetricsError),
    /// HID output failed.
    Device(DeviceError),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metrics(error) => write!(formatter, "metric update failed: {error}"),
            Self::Device(error) => write!(formatter, "display update failed: {error}"),
        }
    }
}

impl Error for UpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metrics(error) => Some(error),
            Self::Device(error) => Some(error),
        }
    }
}

impl From<MetricsError> for UpdateError {
    fn from(error: MetricsError) -> Self {
        Self::Metrics(error)
    }
}

impl From<DeviceError> for UpdateError {
    fn from(error: DeviceError) -> Self {
        Self::Device(error)
    }
}

/// Bounded exponential delay used after discovery or HID failures.
#[derive(Debug, Clone)]
pub struct ReconnectBackoff {
    next: Duration,
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self {
            next: INITIAL_RECONNECT_DELAY,
        }
    }
}

impl ReconnectBackoff {
    /// Returns the current delay and advances it for a subsequent failure.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(MAX_RECONNECT_DELAY);
        delay
    }

    /// Restores the initial delay after a successful connection and write.
    pub fn reset(&mut self) {
        self.next = INITIAL_RECONNECT_DELAY;
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use ak620_core::{
        CpuFrequencyMhz, CpuUtilization, DisplayMetrics, DisplayReport, PowerWatts, Temperature,
        TemperatureUnit,
    };

    use super::{DisplaySink, MAX_RECONNECT_DELAY, MetricSource, ReconnectBackoff, UpdateEngine};
    use crate::{hid::DeviceError, metrics::MetricsError};

    struct FakeMetrics(VecDeque<DisplayMetrics>);

    impl MetricSource for FakeMetrics {
        fn sample(&mut self) -> Result<DisplayMetrics, MetricsError> {
            Ok(self.0.pop_front().unwrap())
        }
    }

    #[derive(Default)]
    struct FakeDisplay(RefCell<Vec<DisplayReport>>);

    impl DisplaySink for FakeDisplay {
        fn write_report(&self, report: &DisplayReport) -> Result<(), DeviceError> {
            self.0.borrow_mut().push(*report);
            Ok(())
        }
    }

    fn metrics() -> DisplayMetrics {
        DisplayMetrics::new(
            PowerWatts::new(88).unwrap(),
            Temperature::new(64.0, TemperatureUnit::Celsius).unwrap(),
            CpuUtilization::new(37).unwrap(),
            CpuFrequencyMhz::new(5_100).unwrap(),
        )
    }

    #[test]
    fn update_engine_encodes_and_writes_one_coherent_snapshot() {
        let source = FakeMetrics(VecDeque::from([metrics()]));
        let display = FakeDisplay::default();
        let mut engine = UpdateEngine::new(source, display);

        assert_eq!(engine.update_once().unwrap(), metrics());
        let (_, display) = engine.into_parts();
        assert_eq!(
            display.0.into_inner(),
            vec![DisplayReport::encode(metrics())]
        );
    }

    #[test]
    fn reconnect_backoff_is_bounded_and_resettable() {
        let mut backoff = ReconnectBackoff::default();
        assert_eq!(backoff.next_delay(), std::time::Duration::from_millis(500));
        assert_eq!(backoff.next_delay(), std::time::Duration::from_secs(1));
        for _ in 0..20 {
            backoff.next_delay();
        }
        assert_eq!(backoff.next_delay(), MAX_RECONNECT_DELAY);

        backoff.reset();
        assert_eq!(backoff.next_delay(), std::time::Duration::from_millis(500));
    }
}
