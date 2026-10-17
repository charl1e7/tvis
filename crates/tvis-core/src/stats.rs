//! Descriptive statistics and linear regression.

/// Summary of a window of samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

impl Summary {
    pub fn of(values: &[f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        Some(Self {
            count: values.len(),
            min: sorted[0],
            max: sorted[sorted.len() - 1],
            mean,
            stddev: var.sqrt(),
            p50: percentile_sorted(&sorted, 0.50),
            p95: percentile_sorted(&sorted, 0.95),
            p99: percentile_sorted(&sorted, 0.99),
        })
    }
}

/// Percentile (`q` in `[0, 1]`) of an ascending slice, linearly interpolated.
pub fn percentile_sorted(sorted: &[f64], q: f64) -> f64 {
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0],
        n => {
            let rank = q.clamp(0.0, 1.0) * (n - 1) as f64;
            let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
            sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
        }
    }
}

/// Result of an ordinary-least-squares fit `v = slope * t + intercept`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearFit {
    pub slope: f64,
    pub intercept: f64,
    /// Coefficient of determination in `[0, 1]`; 1 is a perfect line.
    pub r2: f64,
}

/// Fits a line through `(t, v)` pairs. `None` for fewer than two distinct `t`.
pub fn linear_fit(points: impl IntoIterator<Item = (f64, f64)>) -> Option<LinearFit> {
    let pts: Vec<(f64, f64)> = points.into_iter().collect();
    let n = pts.len() as f64;
    if pts.len() < 2 {
        return None;
    }
    // Centre `t` for numerical stability with large timestamps.
    let t0 = pts[0].0;
    let (mut st, mut sv) = (0.0, 0.0);
    for &(t, v) in &pts {
        st += t - t0;
        sv += v;
    }
    let (mt, mv) = (st / n, sv / n);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for &(t, v) in &pts {
        let (dx, dy) = (t - t0 - mt, v - mv);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if sxx <= f64::EPSILON {
        return None;
    }
    let slope = sxy / sxx;
    let r2 = if syy <= f64::EPSILON {
        0.0
    } else {
        (sxy * sxy / (sxx * syy)).clamp(0.0, 1.0)
    };
    Some(LinearFit {
        slope,
        intercept: mv - slope * (mt + t0),
        r2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_basics() {
        let s = Summary::of(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!((s.min, s.max, s.mean, s.p50), (1.0, 5.0, 3.0, 3.0));
        assert!((s.stddev - 2f64.sqrt()).abs() < 1e-12);
        assert!((s.p95 - 4.8).abs() < 1e-12);
        assert!(Summary::of(&[]).is_none());
    }

    #[test]
    fn fit_recovers_line_with_large_timestamps() {
        let f = linear_fit((0..50).map(|i| (1e9 + i as f64, 3.0 * i as f64 + 7.0))).unwrap();
        assert!((f.slope - 3.0).abs() < 1e-6);
        assert!(f.r2 > 0.999_999);
    }

    #[test]
    fn fit_flat_and_degenerate() {
        let f = linear_fit((0..10).map(|i| (i as f64, 5.0))).unwrap();
        assert_eq!((f.slope, f.r2), (0.0, 0.0));
        assert!(linear_fit([(1.0, 1.0)]).is_none());
        assert!(linear_fit([(1.0, 1.0), (1.0, 2.0)]).is_none());
    }
}
