//! macOS: device-level GPU statistics from IOKit's `IOAccelerator` via `ioreg`.
//!
//! macOS offers no per-process GPU counters to unprivileged programs, so only
//! devices are reported. `ioreg -a` emits an XML property list that is parsed
//! with the `plist` crate; running the tool keeps this crate free of `unsafe`.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use tvis_core::frame::Pid;

use crate::{Backend, Reading};

pub(crate) struct IoregBackend;

impl IoregBackend {
    pub(crate) fn new() -> Option<Self> {
        if cfg!(target_os = "macos") {
            #[cfg(target_os = "macos")]
            return (!imp::query().is_empty()).then_some(Self);
        }
        None
    }
}

impl Backend for IoregBackend {
    fn name(&self) -> &'static str {
        "macos-ioreg"
    }

    fn read(&mut self, _watched: &[Pid]) -> Reading {
        #[cfg(target_os = "macos")]
        return Reading {
            devices: imp::query(),
            processes: Vec::new(),
        };
        #[cfg(not(target_os = "macos"))]
        Reading::default()
    }
}

#[cfg(any(target_os = "macos", test))]
mod imp {
    use tvis_core::gpu::{GpuSample, GpuVendor};

    #[cfg(target_os = "macos")]
    pub(super) fn query() -> Vec<GpuSample> {
        let out = std::process::Command::new("ioreg")
            .args(["-r", "-d", "1", "-w", "0", "-a", "-c", "IOAccelerator"])
            .output();
        match out {
            Ok(o) if o.status.success() => parse(&o.stdout),
            _ => Vec::new(),
        }
    }

    fn number(d: &plist::Dictionary, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|k| match d.get(k)? {
            plist::Value::Integer(i) => i
                .as_signed()
                .map(|v| v as f64)
                .or_else(|| i.as_unsigned().map(|v| v as f64)),
            plist::Value::Real(f) => Some(*f),
            _ => None,
        })
    }

    fn text(d: &plist::Dictionary, key: &str) -> Option<String> {
        match d.get(key)? {
            plist::Value::String(s) => Some(s.clone()),
            plist::Value::Data(b) => Some(
                String::from_utf8_lossy(b)
                    .trim_matches('\0')
                    .trim()
                    .to_owned(),
            ),
            _ => None,
        }
    }

    pub(crate) fn parse(xml: &[u8]) -> Vec<GpuSample> {
        let Ok(plist::Value::Array(items)) = plist::Value::from_reader(std::io::Cursor::new(xml))
        else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|v| v.as_dictionary())
            .enumerate()
            .filter_map(|(i, d)| {
                let stats = d.get("PerformanceStatistics")?.as_dictionary()?;
                let name = text(d, "model")
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "GPU".to_owned());
                let class = text(d, "IOClass").unwrap_or_default();
                let vendor = if class.starts_with("AGX") || name.starts_with("Apple") {
                    GpuVendor::Apple
                } else if name.contains("AMD") || name.contains("Radeon") {
                    GpuVendor::Amd
                } else if name.contains("Intel") {
                    GpuVendor::Intel
                } else if name.contains("NVIDIA") {
                    GpuVendor::Nvidia
                } else {
                    GpuVendor::Other
                };
                let used = number(stats, &["vramUsedBytes", "In use system memory"]);
                let free = number(stats, &["vramFreeBytes"]);
                Some(GpuSample {
                    id: format!("ioreg:{i}"),
                    name,
                    vendor,
                    utilization: number(stats, &["Device Utilization %", "GPU Activity(%)"])
                        .map(|v| v.clamp(0.0, 100.0)),
                    mem_used: used.map(|v| v as u64),
                    mem_total: used.zip(free).map(|(u, f)| (u + f) as u64),
                    temperature: number(stats, &["Temperature(C)"]),
                    power: number(stats, &["Total Power(W)"]),
                    clock_mhz: number(stats, &["Core Clock(MHz)"]),
                    fan: number(stats, &["Fan Speed(%)"]),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::imp::parse;
    use tvis_core::gpu::GpuVendor;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array>
<dict>
  <key>IOClass</key><string>AGXAcceleratorG14X</string>
  <key>model</key><string>Apple M2 Pro</string>
  <key>PerformanceStatistics</key>
  <dict>
    <key>Device Utilization %</key><integer>42</integer>
    <key>In use system memory</key><integer>1073741824</integer>
    <key>Temperature(C)</key><integer>55</integer>
  </dict>
</dict>
<dict>
  <key>IOClass</key><string>AMDRadeonX6000_AMDNavi21Accelerator</string>
  <key>model</key><data>QU1EIFJhZGVvbiBQcm8gNTcwMA==</data>
  <key>PerformanceStatistics</key>
  <dict>
    <key>Device Utilization %</key><real>12.5</real>
    <key>vramUsedBytes</key><integer>2147483648</integer>
    <key>vramFreeBytes</key><integer>6442450944</integer>
  </dict>
</dict>
<dict><key>IOClass</key><string>NoStats</string></dict>
</array></plist>"#;

    #[test]
    fn parses_apple_and_discrete_accelerators() {
        let g = parse(FIXTURE.as_bytes());
        assert_eq!(g.len(), 2, "entries without statistics are ignored");
        assert_eq!(g[0].vendor, GpuVendor::Apple);
        assert_eq!(g[0].name, "Apple M2 Pro");
        assert_eq!(g[0].utilization, Some(42.0));
        assert_eq!(g[0].mem_used, Some(1 << 30));
        assert_eq!(g[0].mem_total, None);
        assert_eq!(g[0].temperature, Some(55.0));
        assert_eq!(g[1].vendor, GpuVendor::Amd);
        assert_eq!(g[1].name, "AMD Radeon Pro 5700");
        assert_eq!(g[1].utilization, Some(12.5));
        assert_eq!(g[1].mem_total, Some(8 << 30));
    }

    #[test]
    fn garbage_is_an_empty_list() {
        assert!(parse(b"not a plist").is_empty());
    }
}
