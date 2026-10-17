//! Turns time series into ready-to-draw SVG path data.
//!
//! Charts are drawn by Slint `Path` elements with a 1000x1000 view box; this
//! module is therefore the only place that knows about plot geometry.

use std::fmt::Write;

use tvis_core::metric::Unit;
use tvis_core::series::{Point, decimate_min_max, smooth};
use tvis_core::units::{ByteBase, format_duration, format_value, nice_axis};

use super::view::{ChartV, LineV, Tick};

const BOX: f64 = 1000.0;

#[derive(Clone, Copy, Debug)]
pub struct Plot {
    /// Right edge of the plot in series time.
    pub now: f64,
    pub window: f64,
    /// Plot width in pixels; a line never needs more than two points per pixel.
    pub width_px: usize,
    pub smoothing: f64,
    /// Samples further apart than this are not joined by a line.
    pub gap: f64,
}

/// Raw input for one line.
pub struct Input {
    pub name: String,
    pub color: u32,
    pub points: Vec<Point>,
}

/// How the y axis is scaled.
#[derive(Clone, Copy, Debug)]
pub enum YScale {
    /// Fit the visible data.
    Auto,
    /// Always `0..=max` (e.g. 100 % or installed memory).
    Fixed(f64),
}

pub fn build(inputs: Vec<Input>, unit: Unit, base: ByteBase, frame: Plot, y: YScale) -> ChartV {
    let start = frame.now - frame.window;
    let mut prepared: Vec<(Input, Vec<Point>)> = inputs
        .into_iter()
        .map(|mut input| {
            input.points.retain(|p| p.t >= start - frame.gap);
            smooth(&mut input.points, frame.smoothing);
            let visible = decimate_min_max(&input.points, frame.width_px.max(50));
            (input, visible)
        })
        .collect();

    let data_max = prepared
        .iter()
        .flat_map(|(_, pts)| pts.iter())
        .filter(|p| p.t >= start)
        .map(|p| p.v)
        .fold(0.0_f64, f64::max);
    let axis_max = match y {
        YScale::Fixed(max) => max.max(f64::EPSILON),
        YScale::Auto => nice_axis(data_max * 1.08, unit, base, 4).max,
    };
    let axis = nice_axis(axis_max, unit, base, 4);
    let axis_max = match y {
        YScale::Fixed(_) => axis_max,
        YScale::Auto => axis.max,
    };

    let lines = prepared
        .drain(..)
        .map(|(input, pts)| {
            let (stroke, fill) = paths(&pts, start, frame, axis_max);
            let last = pts
                .last()
                .map(|p| format_value(unit, p.v, base))
                .unwrap_or_default();
            LineV {
                name: input.name,
                color: input.color,
                stroke,
                fill,
                last,
                points: pts
                    .iter()
                    .filter(|p| p.t >= start)
                    .map(|p| (((p.t - start) / frame.window) as f32, p.v))
                    .collect(),
            }
        })
        .collect();

    let y_ticks = if let YScale::Fixed(max) = y {
        nice_axis(max, unit, base, 4)
            .ticks
            .into_iter()
            .filter(|t| *t <= max * 1.0001)
            .map(|t| (t, max))
            .collect::<Vec<_>>()
    } else {
        axis.ticks.iter().map(|t| (*t, axis_max)).collect()
    };
    let y_ticks = y_ticks
        .into_iter()
        .map(|(t, max)| Tick {
            frac: (t / max) as f32,
            label: format_value(unit, t, base),
        })
        .collect();

    ChartV {
        lines,
        y_ticks,
        x_ticks: x_ticks(frame.window),
        unit,
        base,
        window_secs: frame.window,
    }
}

pub fn x_ticks(window: f64) -> Vec<Tick> {
    (0..=4)
        .map(|i| {
            let frac = i as f32 / 4.0;
            let label = if i == 4 {
                "now".to_owned()
            } else {
                format!("−{}", format_duration(window * f64::from(1.0 - frac)))
            };
            Tick { frac, label }
        })
        .collect()
}

/// `(stroke, fill)` path data for already decimated points.
fn paths(points: &[Point], start: f64, frame: Plot, axis_max: f64) -> (String, String) {
    let x = |t: f64| ((t - start) / frame.window).clamp(0.0, 1.0) * BOX;
    let y = |v: f64| BOX - (v / axis_max).clamp(0.0, 1.0) * BOX;

    let mut stroke = String::with_capacity(points.len() * 14);
    let mut fill = String::with_capacity(points.len() * 14);
    let mut segment: Vec<(f64, f64)> = Vec::new();
    let flush = |segment: &mut Vec<(f64, f64)>, stroke: &mut String, fill: &mut String| {
        if segment.is_empty() {
            return;
        }
        // A lone sample would draw nothing; give it a hairline so it is visible.
        if segment.len() == 1 {
            let (px, py) = segment[0];
            segment.push((px + 1.0, py));
        }
        for (i, (px, py)) in segment.iter().enumerate() {
            let cmd = if i == 0 { 'M' } else { 'L' };
            let _ = write!(stroke, "{cmd} {px:.1} {py:.1} ");
            let _ = write!(fill, "{cmd} {px:.1} {py:.1} ");
        }
        let (last_x, _) = segment[segment.len() - 1];
        let (first_x, _) = segment[0];
        let _ = write!(fill, "L {last_x:.1} {BOX} L {first_x:.1} {BOX} Z ");
        segment.clear();
    };

    let mut prev_t: Option<f64> = None;
    for p in points {
        if p.t < start - frame.gap {
            continue;
        }
        if prev_t.is_some_and(|pt| p.t - pt > frame.gap) {
            flush(&mut segment, &mut stroke, &mut fill);
        }
        // Points before the left edge are clamped onto it, preserving the slope origin.
        segment.push((x(p.t), y(p.v)));
        prev_t = Some(p.t);
    }
    flush(&mut segment, &mut stroke, &mut fill);
    (stroke.trim_end().to_owned(), fill.trim_end().to_owned())
}

/// Auto-scaled stroke for tiny inline charts (no axis, no fill).
pub fn sparkline(points: &[Point], window: f64, now: f64, max_points: usize) -> String {
    let start = now - window;
    let mut pts: Vec<Point> = points.iter().copied().filter(|p| p.t >= start).collect();
    if pts.len() < 2 {
        return String::new();
    }
    pts = decimate_min_max(&pts, max_points);
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.v), hi.max(p.v))
    });
    // Keep flat lines in the middle instead of stretching noise to full height.
    let span = (hi - lo).max(hi.abs() * 0.05).max(1e-9);
    let base = if hi - lo < hi.abs() * 0.05 {
        lo - span / 2.0
    } else {
        lo
    };
    let t0 = pts[0].t;
    let t1 = pts[pts.len() - 1].t;
    let dt = (t1 - t0).max(1e-9);
    let mut s = String::with_capacity(pts.len() * 14);
    for (i, p) in pts.iter().enumerate() {
        let x = (p.t - t0) / dt * BOX;
        let y = BOX - ((p.v - base) / span).clamp(0.0, 1.0) * (BOX - 80.0) - 40.0;
        let _ = write!(s, "{} {x:.1} {y:.1} ", if i == 0 { 'M' } else { 'L' });
    }
    s.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(now: f64, window: f64) -> Plot {
        Plot {
            now,
            window,
            width_px: 800,
            smoothing: 0.0,
            gap: 3.0,
        }
    }

    fn line(n: usize, f: impl Fn(usize) -> f64) -> Vec<Point> {
        (0..n)
            .map(|i| Point {
                t: i as f64,
                v: f(i),
            })
            .collect()
    }

    #[test]
    fn builds_closed_fill_and_stroke_inside_the_box() {
        let c = build(
            vec![Input {
                name: "a".into(),
                color: 0,
                points: line(60, |i| i as f64),
            }],
            Unit::Count,
            ByteBase::Binary,
            frame(59.0, 60.0),
            YScale::Auto,
        );
        let l = &c.lines[0];
        assert!(l.stroke.starts_with("M "));
        assert!(l.fill.ends_with('Z'));
        let nums: Vec<f64> = l
            .stroke
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        assert!(nums.iter().all(|n| (0.0..=1000.0).contains(n)), "{nums:?}");
        assert_eq!(c.x_ticks.len(), 5);
        assert_eq!(c.x_ticks.last().unwrap().label, "now");
        assert!(c.y_ticks.first().unwrap().frac == 0.0 && c.y_ticks.last().unwrap().frac <= 1.0001);
    }

    #[test]
    fn gaps_split_the_line() {
        let mut pts = line(10, |_| 5.0);
        pts.extend((30..40).map(|i| Point {
            t: i as f64,
            v: 5.0,
        }));
        let c = build(
            vec![Input {
                name: "a".into(),
                color: 0,
                points: pts,
            }],
            Unit::Count,
            ByteBase::Binary,
            frame(39.0, 60.0),
            YScale::Auto,
        );
        assert_eq!(c.lines[0].stroke.matches('M').count(), 2);
    }

    #[test]
    fn fixed_scale_keeps_ticks_in_range() {
        let c = build(
            vec![Input {
                name: "cpu".into(),
                color: 0,
                points: line(10, |_| 50.0),
            }],
            Unit::Percent,
            ByteBase::Binary,
            frame(9.0, 30.0),
            YScale::Fixed(100.0),
        );
        assert!(c.y_ticks.iter().all(|t| (0.0..=1.0001).contains(&t.frac)));
        assert_eq!(c.y_ticks.last().unwrap().label, "100%");
    }

    #[test]
    fn empty_and_single_point_inputs_do_not_panic() {
        let c = build(
            vec![
                Input {
                    name: "e".into(),
                    color: 0,
                    points: vec![],
                },
                Input {
                    name: "one".into(),
                    color: 1,
                    points: vec![Point { t: 1.0, v: 1.0 }],
                },
            ],
            Unit::Bytes,
            ByteBase::Decimal,
            frame(1.0, 30.0),
            YScale::Auto,
        );
        assert!(c.lines[0].stroke.is_empty());
        assert!(!c.lines[1].stroke.is_empty());
    }

    #[test]
    fn sparkline_needs_two_points_and_stays_in_box() {
        assert!(sparkline(&line(1, |_| 1.0), 60.0, 0.0, 30).is_empty());
        let s = sparkline(&line(100, |i| (i % 10) as f64), 200.0, 99.0, 30);
        assert!(s.starts_with('M'));
        let nums: Vec<f64> = s
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        assert!(nums.iter().all(|n| (0.0..=1000.0).contains(n)));
    }
}
