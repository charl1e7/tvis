//! Human readable formatting and axis helpers.

use crate::metric::Unit;

/// How byte quantities are scaled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ByteBase {
    /// 1 KiB = 1024 B
    #[default]
    Binary,
    /// 1 kB = 1000 B
    Decimal,
}

impl ByteBase {
    fn step(self) -> f64 {
        match self {
            Self::Binary => 1024.0,
            Self::Decimal => 1000.0,
        }
    }

    fn suffixes(self) -> [&'static str; 6] {
        match self {
            Self::Binary => ["B", "KiB", "MiB", "GiB", "TiB", "PiB"],
            Self::Decimal => ["B", "kB", "MB", "GB", "TB", "PB"],
        }
    }
}

/// Index of the largest suffix for which `value` is at least 1.
fn scale_index(value: f64, base: ByteBase) -> usize {
    let mut v = value.abs();
    let mut i = 0;
    while v >= base.step() && i < 5 {
        v /= base.step();
        i += 1;
    }
    i
}

pub fn format_bytes(value: f64, base: ByteBase) -> String {
    let i = scale_index(value, base);
    let scaled = value / base.step().powi(i as i32);
    let suffix = base.suffixes()[i];
    if i == 0 || scaled.abs() >= 100.0 {
        format!("{scaled:.0} {suffix}")
    } else if scaled.abs() >= 10.0 {
        format!("{scaled:.1} {suffix}")
    } else {
        format!("{scaled:.2} {suffix}")
    }
}

pub fn format_percent(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}%")
    } else {
        format!("{value:.1}%")
    }
}

pub fn format_count(value: f64) -> String {
    format!("{value:.0}")
}

pub fn format_value(unit: Unit, value: f64, base: ByteBase) -> String {
    match unit {
        Unit::Percent => format_percent(value),
        Unit::Bytes => format_bytes(value, base),
        Unit::BytesPerSec => format!("{}/s", format_bytes(value, base)),
        Unit::Count => format_count(value),
    }
}

/// Signed rate of change, e.g. `+12.4 MiB/min`.
pub fn format_trend(unit: Unit, per_sec: f64, base: ByteBase) -> String {
    let per_min = per_sec * 60.0;
    let sign = if per_min >= 0.0 { "+" } else { "−" };
    let body = match unit {
        Unit::Percent => format!("{:.1}%", per_min.abs()),
        Unit::Bytes | Unit::BytesPerSec => format_bytes(per_min.abs(), base),
        Unit::Count => format!("{:.1}", per_min.abs()),
    };
    format!("{sign}{body}/min")
}

pub fn format_duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    let (d, h, m, sec) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60, s % 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{sec}s"),
        (0, 0, _) => format!("{m}m {sec:02}s"),
        (0, _, _) => format!("{h}h {m:02}m"),
        _ => format!("{d}d {h}h"),
    }
}

/// A chart axis with "nice" tick values.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub max: f64,
    pub ticks: Vec<f64>,
}

/// Builds a `0..=max` axis that comfortably contains `data_max`.
///
/// Bytes use power-of-base scaling so ticks land on `1 MiB`, `2 MiB`, `5 MiB`…
pub fn nice_axis(data_max: f64, unit: Unit, base: ByteBase, target_ticks: usize) -> Axis {
    let data_max = if data_max.is_finite() && data_max > 0.0 {
        data_max
    } else {
        1.0
    };
    let unit_scale = match unit {
        Unit::Bytes | Unit::BytesPerSec => base.step().powi(scale_index(data_max, base) as i32),
        _ => 1.0,
    };
    let scaled_max = data_max / unit_scale;
    let rough = scaled_max / target_ticks.max(1) as f64;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= rough)
        .unwrap_or(10.0 * magnitude);
    let steps = (scaled_max / step).ceil().max(1.0) as usize;
    let ticks: Vec<f64> = (0..=steps).map(|i| i as f64 * step * unit_scale).collect();
    Axis {
        max: *ticks.last().unwrap_or(&data_max),
        ticks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_binary_and_decimal() {
        assert_eq!(format_bytes(0.0, ByteBase::Binary), "0 B");
        assert_eq!(format_bytes(1536.0, ByteBase::Binary), "1.50 KiB");
        assert_eq!(format_bytes(1_500_000.0, ByteBase::Decimal), "1.50 MB");
        assert_eq!(format_bytes(250.0 * 1048576.0, ByteBase::Binary), "250 MiB");
        assert_eq!(
            format_bytes(12.34 * 1073741824.0, ByteBase::Binary),
            "12.3 GiB"
        );
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(5.0), "5s");
        assert_eq!(format_duration(65.0), "1m 05s");
        assert_eq!(format_duration(3700.0), "1h 01m");
        assert_eq!(format_duration(90_000.0), "1d 1h");
    }

    #[test]
    fn trend_sign() {
        assert_eq!(
            format_trend(Unit::Bytes, 1048576.0 / 60.0, ByteBase::Binary),
            "+1.00 MiB/min"
        );
        assert!(format_trend(Unit::Percent, -1.0, ByteBase::Binary).starts_with('−'));
    }

    #[test]
    fn axis_is_nice_and_covers_data() {
        let a = nice_axis(87.0, Unit::Percent, ByteBase::Binary, 4);
        assert!(a.max >= 87.0);
        assert_eq!(a.ticks[0], 0.0);
        assert!(a.ticks.len() <= 7);
        let b = nice_axis(300.0 * 1048576.0, Unit::Bytes, ByteBase::Binary, 4);
        assert!(b.max >= 300.0 * 1048576.0);
        assert!(b.ticks.iter().all(|t| (t / 1048576.0).fract() == 0.0));
        let z = nice_axis(0.0, Unit::Count, ByteBase::Binary, 4);
        assert!(z.max > 0.0);
    }
}
