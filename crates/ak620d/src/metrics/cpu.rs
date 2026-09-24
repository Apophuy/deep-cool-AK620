use ak620_core::{CpuFrequencyMhz, CpuUtilization};

use super::MetricsError;

/// Aggregate counters from the leading `cpu` line in `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    total: u64,
    idle: u64,
}

impl CpuTimes {
    /// Calculates rounded aggregate utilization between two monotonic snapshots.
    pub fn utilization_since(self, current: Self) -> Result<CpuUtilization, MetricsError> {
        let total_delta =
            current
                .total
                .checked_sub(self.total)
                .ok_or(MetricsError::CounterRegressed {
                    previous: self.total,
                    current: current.total,
                })?;
        let idle_delta =
            current
                .idle
                .checked_sub(self.idle)
                .ok_or(MetricsError::CounterRegressed {
                    previous: self.idle,
                    current: current.idle,
                })?;
        if total_delta == 0 {
            return Err(MetricsError::NoProgress {
                context: "/proc/stat CPU counters",
            });
        }
        let busy_delta = total_delta.checked_sub(idle_delta).ok_or_else(|| {
            MetricsError::invalid(
                "/proc/stat",
                "idle delta is greater than the total CPU delta",
            )
        })?;

        let rounded_percent =
            (u128::from(busy_delta) * 100 + u128::from(total_delta) / 2) / u128::from(total_delta);
        let percent = u8::try_from(rounded_percent).map_err(|_| {
            MetricsError::invalid("/proc/stat", "derived utilization does not fit in u8")
        })?;
        CpuUtilization::new(percent).map_err(Into::into)
    }
}

/// Parses the aggregate CPU counters from `/proc/stat` text.
pub fn parse_cpu_times(contents: &str) -> Result<CpuTimes, MetricsError> {
    let line = contents
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| MetricsError::invalid("/proc/stat", "aggregate cpu line is missing"))?;
    let fields = line
        .split_ascii_whitespace()
        .skip(1)
        .map(|field| {
            field.parse::<u64>().map_err(|error| {
                MetricsError::invalid(
                    "/proc/stat",
                    format!("invalid CPU counter {field:?}: {error}"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if fields.len() < 4 {
        return Err(MetricsError::invalid(
            "/proc/stat",
            format!("expected at least 4 CPU counters, found {}", fields.len()),
        ));
    }

    // guest and guest_nice are already included in user and nice, so Linux aggregate usage uses
    // only user through steal (fields 0..=7).
    let total = fields
        .iter()
        .take(8)
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))
        .ok_or_else(|| MetricsError::invalid("/proc/stat", "CPU total counter overflowed"))?;
    let idle = fields[3]
        .checked_add(fields.get(4).copied().unwrap_or(0))
        .ok_or_else(|| MetricsError::invalid("/proc/stat", "CPU idle counter overflowed"))?;

    Ok(CpuTimes { total, idle })
}

/// Parses the highest `cpu MHz` entry from `/proc/cpuinfo` and rounds it to whole MHz.
pub fn parse_highest_frequency_mhz(contents: &str) -> Result<CpuFrequencyMhz, MetricsError> {
    let mut highest = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "cpu MHz" {
            continue;
        }
        let mhz = parse_rounded_decimal(value.trim())?;
        highest = Some(highest.map_or(mhz, |current: u32| current.max(mhz)));
    }

    let highest = highest
        .ok_or_else(|| MetricsError::invalid("/proc/cpuinfo", "cpu MHz entries are missing"))?;
    CpuFrequencyMhz::new(highest).map_err(Into::into)
}

fn parse_rounded_decimal(value: &str) -> Result<u32, MetricsError> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.parse::<u32>().map_err(|error| {
        MetricsError::invalid(
            "/proc/cpuinfo",
            format!("invalid cpu MHz value {value:?}: {error}"),
        )
    })?;
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(MetricsError::invalid(
            "/proc/cpuinfo",
            format!("invalid cpu MHz fraction {value:?}"),
        ));
    }
    let round_up = fraction
        .as_bytes()
        .first()
        .is_some_and(|digit| *digit >= b'5');
    whole
        .checked_add(u32::from(round_up))
        .ok_or_else(|| MetricsError::invalid("/proc/cpuinfo", "rounded cpu MHz value overflowed"))
}

#[cfg(test)]
mod tests {
    use ak620_core::MetricError;

    use super::{MetricsError, parse_cpu_times, parse_highest_frequency_mhz};

    #[test]
    fn calculates_rounded_aggregate_utilization() {
        let before = parse_cpu_times("cpu  100 20 30 400 10 5 3 2 0 0\ncpu0 0 0 0 0").unwrap();
        let after = parse_cpu_times("cpu  130 20 50 440 10 5 3 2 0 0").unwrap();

        assert_eq!(before.utilization_since(after).unwrap().get(), 56);
    }

    #[test]
    fn guest_fields_are_not_double_counted() {
        let times = parse_cpu_times("cpu  10 20 30 40 5 6 7 8 900 1000").unwrap();
        let next = parse_cpu_times("cpu  11 20 30 40 5 6 7 8 1900 2000").unwrap();

        assert_eq!(times.utilization_since(next).unwrap().get(), 100);
    }

    #[test]
    fn rejects_stalled_or_regressing_cpu_counters() {
        let first = parse_cpu_times("cpu  1 2 3 4").unwrap();
        let same = parse_cpu_times("cpu  1 2 3 4").unwrap();
        let older = parse_cpu_times("cpu  0 1 2 3").unwrap();

        assert!(matches!(
            first.utilization_since(same),
            Err(MetricsError::NoProgress { .. })
        ));
        assert!(matches!(
            first.utilization_since(older),
            Err(MetricsError::CounterRegressed { .. })
        ));
    }

    #[test]
    fn parses_and_rounds_highest_cpu_frequency() {
        let cpuinfo = "processor : 0\ncpu MHz : 4391.499\nprocessor : 1\ncpu MHz : 5657.501\n";
        assert_eq!(parse_highest_frequency_mhz(cpuinfo).unwrap().get(), 5_658);
    }

    #[test]
    fn rejects_missing_or_unrepresentable_frequency() {
        assert!(matches!(
            parse_highest_frequency_mhz("processor : 0"),
            Err(MetricsError::InvalidData { .. })
        ));
        assert!(matches!(
            parse_highest_frequency_mhz("cpu MHz : 70000.0"),
            Err(MetricsError::InvalidMetric(
                MetricError::FrequencyOutOfRange { .. }
            ))
        ));
    }
}
