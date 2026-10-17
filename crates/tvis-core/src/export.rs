//! CSV / JSON serialisation of recorded history.

use std::collections::BTreeMap;
use std::io::{self, Write};

use serde_json::json;

use crate::metric::MetricRegistry;
use crate::store::TargetData;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// Writes the aggregated history of `data`.
///
/// `origin_wall_ms` is the unix time in milliseconds of `t = 0`.
pub fn write_target(
    out: &mut impl Write,
    data: &TargetData,
    registry: &MetricRegistry,
    format: ExportFormat,
    origin_wall_ms: u64,
) -> io::Result<()> {
    let metrics: Vec<_> = registry.iter().filter(|d| data.has_metric(d.id)).collect();
    let wall = |ms: i64| origin_wall_ms as i64 + ms;

    // Align all series on millisecond timestamps.
    let mut rows: BTreeMap<i64, Vec<Option<f64>>> = BTreeMap::new();
    for (col, d) in metrics.iter().enumerate() {
        for p in data.series(d.id).into_iter().flat_map(|s| s.iter()) {
            let ms = (p.t * 1000.0).round() as i64;
            rows.entry(ms).or_insert_with(|| vec![None; metrics.len()])[col] = Some(p.v);
        }
    }

    match format {
        ExportFormat::Csv => {
            write!(out, "time_unix_ms,t_seconds")?;
            for d in &metrics {
                write!(out, ",{}", csv_field(d.id.as_str()))?;
            }
            writeln!(out)?;
            for (ms, vals) in &rows {
                write!(out, "{},{:.3}", wall(*ms), *ms as f64 / 1000.0)?;
                for v in vals {
                    match v {
                        Some(v) => write!(out, ",{v}")?,
                        None => write!(out, ",")?,
                    }
                }
                writeln!(out)?;
            }
        }
        ExportFormat::Json => {
            let series: Vec<_> = metrics
                .iter()
                .enumerate()
                .map(|(col, d)| {
                    let points: Vec<_> = rows
                        .iter()
                        .filter_map(|(ms, v)| v[col].map(|v| json!([wall(*ms), v])))
                        .collect();
                    json!({ "id": d.id.as_str(), "name": d.name, "unit": format!("{:?}", d.unit), "points": points })
                })
                .collect();
            let processes: Vec<_> = data
                .members
                .iter()
                .map(|p| json!({ "pid": p.pid, "parent": p.parent, "name": p.name, "command": p.command, "thread": p.is_thread() }))
                .collect();
            let doc = json!({
                "format": "tvis-export/1",
                "tvis_version": env!("CARGO_PKG_VERSION"),
                "target": data.id().as_str(),
                "title": data.title(),
                "origin_unix_ms": origin_wall_ms,
                "series": series,
                "processes": processes,
            });
            serde_json::to_writer_pretty(&mut *out, &doc)?;
            writeln!(out)?;
        }
    }
    Ok(())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, ProcessSample, SystemSample};
    use crate::metric::{builtin, builtin_descriptors};
    use crate::store::{Limits, Store};
    use crate::target::Target;

    fn data() -> (TargetData, MetricRegistry) {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let mut store = Store::new(Limits { retention_secs: 60.0, interval_secs: 1.0 });
        let t = Target::Name("app".into());
        store.watch(t.clone());
        for i in 0..3 {
            let mut p = ProcessSample::new(1, None, "app");
            p.set(builtin::CPU, 10.0 + i as f64);
            if i > 0 {
                p.set(builtin::MEM_RSS, 100.0);
            }
            let mut f = Frame::new(SystemSample::default(), vec![p]);
            f.t = i as f64;
            store.ingest(&f, &reg);
        }
        (store.target(&t.id()).unwrap().clone(), reg)
    }

    #[test]
    fn csv_has_header_blank_cells_and_wall_clock() {
        let (d, r) = data();
        let mut buf = Vec::new();
        write_target(&mut buf, &d, &r, ExportFormat::Csv, 1_000_000).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], "time_unix_ms,t_seconds,cpu,mem.rss");
        assert_eq!(lines[1], "1000000,0.000,10,");
        assert_eq!(lines[2], "1001000,1.000,11,100");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn json_is_valid_and_complete() {
        let (d, r) = data();
        let mut buf = Vec::new();
        write_target(&mut buf, &d, &r, ExportFormat::Json, 5).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["target"], "name:app");
        assert_eq!(v["series"][0]["points"].as_array().unwrap().len(), 3);
        assert_eq!(v["processes"][0]["pid"], 1);
    }

    #[test]
    fn csv_quotes_when_needed() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
