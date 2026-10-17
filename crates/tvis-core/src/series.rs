//! Time-series storage and decimation.

use std::collections::VecDeque;

/// One observation: `t` in seconds since engine start.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Point {
    pub t: f64,
    pub v: f64,
}

/// Append-only series bounded by age and by point count.
#[derive(Clone, Debug)]
pub struct TimeSeries {
    points: VecDeque<Point>,
    max_age: f64,
    max_points: usize,
}

impl TimeSeries {
    pub fn new(max_age_secs: f64, max_points: usize) -> Self {
        Self {
            points: VecDeque::new(),
            max_age: max_age_secs,
            max_points: max_points.max(2),
        }
    }

    pub fn push(&mut self, t: f64, v: f64) {
        self.points.push_back(Point { t, v });
        self.evict(t);
    }

    pub fn set_limits(&mut self, max_age_secs: f64, max_points: usize) {
        self.max_age = max_age_secs;
        self.max_points = max_points.max(2);
        if let Some(last) = self.points.back().map(|p| p.t) {
            self.evict(last);
        }
    }

    fn evict(&mut self, now: f64) {
        while self.points.len() > self.max_points
            || self.points.front().is_some_and(|p| now - p.t > self.max_age)
        {
            self.points.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn last(&self) -> Option<Point> {
        self.points.back().copied()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Point> + '_ {
        self.points.iter().copied()
    }

    /// Points with `t >= from`.
    pub fn since(&self, from: f64) -> impl Iterator<Item = Point> + '_ {
        let start = self.points.partition_point(|p| p.t < from);
        self.points.range(start..).copied()
    }

    /// The values of the last `window_secs` seconds measured from the newest point.
    pub fn window_values(&self, window_secs: f64) -> Vec<f64> {
        match self.last() {
            Some(last) => self
                .since(last.t - window_secs)
                .map(|p| p.v)
                .collect(),
            None => Vec::new(),
        }
    }
}

/// Reduces `points` to at most `2 * buckets` points while keeping every local
/// extreme (min/max per bucket), so short spikes survive decimation.
pub fn decimate_min_max(points: &[Point], buckets: usize) -> Vec<Point> {
    let buckets = buckets.max(1);
    if points.len() <= buckets * 2 {
        return points.to_vec();
    }
    let (t0, t1) = (points[0].t, points[points.len() - 1].t);
    let span = (t1 - t0).max(f64::EPSILON);
    let mut out = Vec::with_capacity(buckets * 2 + 2);
    let mut i = 0;
    for b in 0..buckets {
        let end_t = t0 + span * (b + 1) as f64 / buckets as f64;
        let start = i;
        while i < points.len() && (points[i].t < end_t || b == buckets - 1) {
            i += 1;
        }
        let slice = &points[start..i];
        if slice.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = (slice[0], slice[0]);
        for p in slice {
            if p.v < lo.v {
                lo = *p;
            }
            if p.v > hi.v {
                hi = *p;
            }
        }
        if lo.t <= hi.t {
            out.push(lo);
            if hi != lo {
                out.push(hi);
            }
        } else {
            out.push(hi);
            out.push(lo);
        }
    }
    out
}

/// Exponential moving average; `alpha` in `[0, 1)` is the weight of history.
pub fn smooth(points: &mut [Point], alpha: f64) {
    if alpha <= 0.0 {
        return;
    }
    let alpha = alpha.min(0.98);
    let mut acc = match points.first() {
        Some(p) => p.v,
        None => return,
    };
    for p in points.iter_mut() {
        acc = alpha * acc + (1.0 - alpha) * p.v;
        p.v = acc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(n: usize) -> Vec<Point> {
        (0..n)
            .map(|i| Point {
                t: i as f64,
                v: if i == n / 2 { 1000.0 } else { (i % 7) as f64 },
            })
            .collect()
    }

    #[test]
    fn evicts_by_age_and_count() {
        let mut s = TimeSeries::new(10.0, 1000);
        for i in 0..100 {
            s.push(i as f64, 1.0);
        }
        assert_eq!(s.len(), 11);
        let mut s = TimeSeries::new(1e9, 5);
        for i in 0..100 {
            s.push(i as f64, 1.0);
        }
        assert_eq!(s.len(), 5);
        assert_eq!(s.last().unwrap().t, 99.0);
    }

    #[test]
    fn since_uses_time_not_index() {
        let mut s = TimeSeries::new(1e9, 1000);
        for i in 0..10 {
            s.push(i as f64, i as f64);
        }
        let v: Vec<_> = s.since(7.0).map(|p| p.v).collect();
        assert_eq!(v, vec![7.0, 8.0, 9.0]);
        assert_eq!(s.window_values(2.0), vec![7.0, 8.0, 9.0]);
    }

    #[test]
    fn decimation_keeps_spikes_and_bounds_size() {
        let p = pts(10_000);
        let d = decimate_min_max(&p, 100);
        assert!(d.len() <= 202);
        assert!(d.iter().any(|p| p.v == 1000.0));
        assert!(d.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn small_inputs_pass_through() {
        let p = pts(10);
        assert_eq!(decimate_min_max(&p, 100), p);
    }

    #[test]
    fn smoothing_dampens_but_preserves_constant() {
        let mut c: Vec<Point> = (0..10).map(|i| Point { t: i as f64, v: 5.0 }).collect();
        smooth(&mut c, 0.8);
        assert!(c.iter().all(|p| (p.v - 5.0).abs() < 1e-9));
        let mut s = pts(21);
        smooth(&mut s, 0.8);
        assert!(s[10].v < 1000.0);
    }
}
