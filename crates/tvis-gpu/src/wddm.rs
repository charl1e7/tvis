//! Windows WDDM performance counters (`GPU Engine`, `GPU Process Memory`,
//! `GPU Adapter Memory`): the only per-process GPU source that works for every
//! vendor on Windows. Parsing and aggregation are pure so they are tested on
//! all platforms; the WMI access itself lives in `windows.rs`.

#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};

use tvis_core::frame::Pid;
use tvis_core::gpu::{GpuSample, GpuVendor};

use crate::ProcessGpu;

/// `pid_4_luid_0x00000000_0x000074A2_phys_0_eng_0_engtype_3D`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EngineInstance {
    pub pid: Pid,
    pub luid: String,
    /// `eng_0_engtype_3D`: one physical engine.
    pub engine: String,
    pub engine_type: String,
}

/// Canonical text form of a LUID, shared by the counters and DXGI:
/// `HHHHHHHH_LLLLLLLL` in upper-case hex.
pub(crate) fn luid_key(high: u32, low: u32) -> String {
    format!("{high:08X}_{low:08X}")
}

fn luid_from(parts: &[&str], at: usize) -> Option<String> {
    // `luid` `0x…` `0x…`
    if parts.get(at) != Some(&"luid") {
        return None;
    }
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok();
    Some(luid_key(hex(parts.get(at + 1)?)?, hex(parts.get(at + 2)?)?))
}

pub(crate) fn parse_engine_name(name: &str) -> Option<EngineInstance> {
    let p: Vec<&str> = name.split('_').collect();
    if p.first() != Some(&"pid") {
        return None;
    }
    let pid = p.get(1)?.parse().ok()?;
    let luid = luid_from(&p, 2)?;
    let eng_at = p.iter().position(|s| *s == "eng")?;
    let type_at = p.iter().position(|s| *s == "engtype")?;
    let engine_type = p.get(type_at + 1..)?.join("_");
    Some(EngineInstance {
        pid,
        luid,
        engine: format!("eng_{}_{}", p.get(eng_at + 1)?, engine_type),
        engine_type,
    })
}

/// `pid_7696_luid_0x00000000_0x00008453_phys_0`
pub(crate) fn parse_process_memory_name(name: &str) -> Option<(Pid, String)> {
    let p: Vec<&str> = name.split('_').collect();
    (p.first() == Some(&"pid")).then_some(())?;
    Some((p.get(1)?.parse().ok()?, luid_from(&p, 2)?))
}

/// `luid_0x00000000_0x000074A2_phys_0`
pub(crate) fn parse_adapter_name(name: &str) -> Option<String> {
    luid_from(&name.split('_').collect::<Vec<_>>(), 0)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdapterUsage {
    pub luid: String,
    pub util: f64,
    pub mem_used: Option<u64>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Aggregate {
    pub adapters: Vec<AdapterUsage>,
    pub processes: Vec<ProcessGpu>,
}

/// Combines the raw counter rows the way Task Manager does:
/// * a process's GPU % is its busiest engine type (instances of a type add up);
/// * an adapter's GPU % is its busiest single engine across all processes.
pub(crate) fn aggregate(
    engines: &[(String, f64)],
    process_memory: &[(String, u64)],
    adapter_memory: &[(String, u64)],
) -> Aggregate {
    // (pid, luid, engine type) → Σ util
    let mut per_process: HashMap<(Pid, String, String), f64> = HashMap::new();
    // (luid, engine) → Σ util
    let mut per_engine: BTreeMap<(String, String), f64> = BTreeMap::new();
    for (name, util) in engines {
        let Some(e) = parse_engine_name(name) else {
            continue;
        };
        *per_process
            .entry((e.pid, e.luid.clone(), e.engine_type))
            .or_default() += util;
        *per_engine.entry((e.luid, e.engine)).or_default() += util;
    }

    let mut procs: BTreeMap<Pid, ProcessGpu> = BTreeMap::new();
    for ((pid, _, _), util) in per_process {
        let e = procs.entry(pid).or_insert(ProcessGpu {
            pid,
            util: None,
            mem: None,
        });
        e.util = Some(e.util.map_or(util, |old| old.max(util)).min(100.0));
    }
    for (name, bytes) in process_memory {
        let Some((pid, _)) = parse_process_memory_name(name) else {
            continue;
        };
        let e = procs.entry(pid).or_insert(ProcessGpu {
            pid,
            util: None,
            mem: None,
        });
        *e.mem.get_or_insert(0) += bytes;
    }

    let mut adapters: BTreeMap<String, AdapterUsage> = BTreeMap::new();
    for ((luid, _), util) in per_engine {
        let a = adapters.entry(luid.clone()).or_insert(AdapterUsage {
            luid,
            util: 0.0,
            mem_used: None,
        });
        a.util = a.util.max(util.min(100.0));
    }
    for (name, bytes) in adapter_memory {
        let Some(luid) = parse_adapter_name(name) else {
            continue;
        };
        let a = adapters.entry(luid.clone()).or_insert(AdapterUsage {
            luid,
            util: 0.0,
            mem_used: None,
        });
        *a.mem_used.get_or_insert(0) += bytes;
    }

    Aggregate {
        adapters: adapters.into_values().collect(),
        processes: procs.into_values().collect(),
    }
}

/// One graphics adapter as enumerated by DXGI.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdapterInfo {
    pub luid: String,
    pub name: String,
    pub vendor: GpuVendor,
    /// Dedicated video memory (a small carve-out for integrated GPUs).
    pub dedicated: u64,
    pub software: bool,
}

/// Merges the adapter list with the counters into device samples.
///
/// NVIDIA adapters are skipped when `skip_nvidia` is set because NVML already
/// reports them with richer data (temperature, power, total memory); every other
/// adapter, including an integrated GPU next to a discrete NVIDIA one, is listed.
pub(crate) fn build_devices(
    adapters: &[AdapterInfo],
    usage: &Aggregate,
    skip_nvidia: bool,
) -> Vec<GpuSample> {
    if adapters.is_empty() {
        // DXGI unavailable: fall back to what the counters alone can tell.
        return if skip_nvidia {
            Vec::new()
        } else {
            usage
                .adapters
                .iter()
                .enumerate()
                .map(|(i, a)| GpuSample {
                    id: format!("wddm:{}", a.luid),
                    name: format!("GPU {i}"),
                    utilization: Some(a.util),
                    mem_used: a.mem_used,
                    ..GpuSample::default()
                })
                .collect()
        };
    }
    adapters
        .iter()
        .filter(|a| !a.software && !(skip_nvidia && a.vendor == GpuVendor::Nvidia))
        .map(|a| {
            let u = usage.adapters.iter().find(|u| u.luid == a.luid);
            GpuSample {
                id: format!("wddm:{}", a.luid),
                name: a.name.clone(),
                vendor: a.vendor,
                utilization: Some(u.map_or(0.0, |u| u.util)),
                mem_used: u.and_then(|u| u.mem_used),
                mem_total: (a.dedicated > 0).then_some(a.dedicated),
                ..GpuSample::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: &str = "luid_0x00000000_0x000074A2_phys_0";

    #[test]
    fn parses_real_instance_names() {
        let e =
            parse_engine_name("pid_4_luid_0x00000000_0x000074A2_phys_0_eng_0_engtype_3D").unwrap();
        assert_eq!(e.pid, 4);
        assert_eq!(e.luid, "00000000_000074A2");
        assert_eq!(e.engine, "eng_0_3D");
        assert_eq!(e.engine_type, "3D");
        let v = parse_engine_name("pid_100_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode").unwrap();
        assert_eq!(
            (v.engine_type.as_str(), v.engine.as_str()),
            ("VideoDecode", "eng_3_VideoDecode")
        );
        let multi =
            parse_engine_name("pid_9_luid_0x0_0x1_phys_0_eng_1_engtype_Video_Codec").unwrap();
        assert_eq!(multi.engine_type, "Video_Codec");
        assert!(parse_engine_name("garbage").is_none());
        assert!(parse_engine_name("pid_x_luid_0x0_0x1_phys_0_eng_0_engtype_3D").is_none());

        assert_eq!(
            parse_process_memory_name("pid_7696_luid_0x00000000_0x00008453_phys_0"),
            Some((7696, "00000000_00008453".into()))
        );
        assert_eq!(parse_adapter_name(L), Some("00000000_000074A2".into()));
        assert_eq!(
            parse_adapter_name("luid_0x0_0xabc_phys_0"),
            Some(luid_key(0, 0xABC)),
            "hex case and padding do not matter"
        );
        assert_eq!(parse_adapter_name("pid_1_luid_0x0_0x1_phys_0"), None);
    }

    #[test]
    fn aggregates_like_task_manager() {
        let n = |pid: u32, eng: u32, ty: &str| {
            format!("pid_{pid}_luid_0x0_0x1_phys_0_eng_{eng}_engtype_{ty}")
        };
        let engines = vec![
            (n(10, 0, "3D"), 30.0),
            (n(10, 1, "3D"), 20.0), // same type: adds up to 50 for pid 10
            (n(10, 0, "Copy"), 5.0),
            (n(11, 0, "3D"), 25.0), // shares engine 0 with pid 10 → engine total 55
            (n(12, 0, "VideoDecode"), 70.0),
        ];
        let pm = vec![
            ("pid_10_luid_0x0_0x1_phys_0".to_owned(), 1000),
            ("pid_10_luid_0x0_0x2_phys_0".to_owned(), 500),
            ("pid_11_luid_0x0_0x1_phys_0".to_owned(), 7),
        ];
        let am = vec![("luid_0x0_0x1_phys_0".to_owned(), 4000)];
        let a = aggregate(&engines, &pm, &am);

        let p10 = a.processes.iter().find(|p| p.pid == 10).unwrap();
        assert_eq!(p10.util, Some(50.0));
        assert_eq!(p10.mem, Some(1500), "memory adds up across adapters");
        let p11 = a.processes.iter().find(|p| p.pid == 11).unwrap();
        assert_eq!(p11.util, Some(25.0));
        assert_eq!(a.adapters.len(), 1);
        assert_eq!(
            a.adapters[0].util, 70.0,
            "busiest engine: the video decoder"
        );
        assert_eq!(a.adapters[0].mem_used, Some(4000));
    }

    #[test]
    fn utilisation_is_capped_and_garbage_rows_ignored() {
        let engines = vec![
            (
                "pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_owned(),
                80.0,
            ),
            (
                "pid_1_luid_0x0_0x1_phys_0_eng_1_engtype_3D".to_owned(),
                80.0,
            ),
            ("???".to_owned(), 99.0),
        ];
        let a = aggregate(&engines, &[("nope".to_owned(), 5)], &[]);
        assert_eq!(
            a.processes,
            vec![ProcessGpu {
                pid: 1,
                util: Some(100.0),
                mem: None
            }]
        );
    }

    #[test]
    fn integrated_gpu_is_listed_next_to_nvml_covered_nvidia() {
        let adapters = vec![
            AdapterInfo {
                luid: luid_key(0, 0x1111),
                name: "NVIDIA GeForce RTX 2050".into(),
                vendor: GpuVendor::Nvidia,
                dedicated: 4 << 30,
                software: false,
            },
            AdapterInfo {
                luid: luid_key(0, 0x2222),
                name: "AMD Radeon(TM) Graphics".into(),
                vendor: GpuVendor::Amd,
                dedicated: 512 << 20,
                software: false,
            },
            AdapterInfo {
                luid: luid_key(0, 0x3333),
                name: "Microsoft Basic Render Driver".into(),
                vendor: GpuVendor::Other,
                dedicated: 0,
                software: true,
            },
        ];
        let usage = aggregate(
            &[(
                "pid_5_luid_0x0_0x2222_phys_0_eng_0_engtype_3D".to_owned(),
                35.0,
            )],
            &[],
            &[("luid_0x0_0x2222_phys_0".to_owned(), 100 << 20)],
        );
        let with_nvml = build_devices(&adapters, &usage, true);
        assert_eq!(
            with_nvml.len(),
            1,
            "NVIDIA comes from NVML, software adapter hidden"
        );
        assert_eq!(with_nvml[0].name, "AMD Radeon(TM) Graphics");
        assert_eq!(with_nvml[0].utilization, Some(35.0));
        assert_eq!(with_nvml[0].mem_used, Some(100 << 20));
        assert_eq!(with_nvml[0].mem_total, Some(512 << 20));

        let without = build_devices(&adapters, &usage, false);
        assert_eq!(without.len(), 2);
        assert_eq!(
            without[0].utilization,
            Some(0.0),
            "idle adapters report 0, not nothing"
        );
        assert_eq!(without[0].mem_total, Some(4 << 30));
    }

    #[test]
    fn counters_only_fallback_when_dxgi_is_unavailable() {
        let usage = aggregate(
            &[(
                "pid_5_luid_0x0_0x2222_phys_0_eng_0_engtype_3D".to_owned(),
                10.0,
            )],
            &[],
            &[],
        );
        assert_eq!(build_devices(&[], &usage, false).len(), 1);
        assert!(build_devices(&[], &usage, true).is_empty());
    }
}
