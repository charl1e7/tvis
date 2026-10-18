//! NVIDIA through NVML (`libnvidia-ml`), loaded at run time.

#![cfg_attr(
    not(any(target_os = "linux", target_os = "windows")),
    allow(dead_code, unused_imports)
)]

use std::collections::HashMap;

use tvis_core::frame::Pid;
use tvis_core::gpu::{GpuSample, GpuVendor};

use crate::{Backend, ProcessGpu, Reading};

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use imp::NvmlBackend;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) struct NvmlBackend;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl NvmlBackend {
    pub(crate) fn new() -> Option<Self> {
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl Backend for NvmlBackend {
    fn name(&self) -> &'static str {
        "nvml"
    }

    fn read(&mut self, _watched: &[Pid]) -> Reading {
        Reading::default()
    }
}

/// Folds NVML's per-process lists into one entry per pid (a process can show
/// up as both a compute and a graphics client).
pub(crate) fn merge_process_entries(
    memory: impl IntoIterator<Item = (Pid, Option<u64>)>,
    util: impl IntoIterator<Item = (Pid, f64)>,
) -> Vec<ProcessGpu> {
    let mut map: HashMap<Pid, ProcessGpu> = HashMap::new();
    for (pid, mem) in memory {
        let e = map.entry(pid).or_insert(ProcessGpu {
            pid,
            util: None,
            mem: None,
        });
        if let Some(m) = mem {
            // The same allocation is listed as compute and as graphics: keep the larger view.
            e.mem = Some(e.mem.map_or(m, |old| old.max(m)));
        }
    }
    for (pid, u) in util {
        let e = map.entry(pid).or_insert(ProcessGpu {
            pid,
            util: None,
            mem: None,
        });
        e.util = Some(e.util.map_or(u, |old| old.max(u)));
    }
    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by_key(|p| p.pid);
    v
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod imp {
    use nvml_wrapper::Nvml;
    use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
    use nvml_wrapper::enums::device::UsedGpuMemory;

    use super::*;

    pub(crate) struct NvmlBackend {
        nvml: Nvml,
        count: u32,
        /// Newest utilisation sample seen, so each call only asks for what is new.
        last_seen: Vec<u64>,
    }

    impl NvmlBackend {
        /// `None` if the driver library is missing or reports no device.
        pub(crate) fn new() -> Option<Self> {
            let nvml = match Nvml::init() {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("NVML unavailable: {e}");
                    return None;
                }
            };
            let count = nvml.device_count().ok().filter(|c| *c > 0)?;
            Some(Self {
                nvml,
                count,
                last_seen: vec![0; count as usize],
            })
        }
    }

    impl Backend for NvmlBackend {
        fn name(&self) -> &'static str {
            "nvml"
        }

        fn read(&mut self, _watched: &[Pid]) -> Reading {
            let mut reading = Reading::default();
            for index in 0..self.count {
                let Ok(dev) = self.nvml.device_by_index(index) else {
                    continue;
                };
                let mem = dev.memory_info().ok();
                reading.devices.push(GpuSample {
                    id: format!("nvml:{index}"),
                    name: dev.name().unwrap_or_else(|_| format!("NVIDIA GPU {index}")),
                    vendor: GpuVendor::Nvidia,
                    utilization: dev.utilization_rates().ok().map(|u| f64::from(u.gpu)),
                    mem_used: mem.as_ref().map(|m| m.used),
                    mem_total: mem.as_ref().map(|m| m.total),
                    temperature: dev.temperature(TemperatureSensor::Gpu).ok().map(f64::from),
                    power: dev.power_usage().ok().map(|mw| f64::from(mw) / 1000.0),
                    clock_mhz: dev.clock_info(Clock::Graphics).ok().map(f64::from),
                    fan: dev.fan_speed(0).ok().map(f64::from),
                });

                // Windows (WDDM) does not let NVML see per-process numbers; the
                // performance-counter backend provides them for every vendor there.
                if cfg!(windows) {
                    continue;
                }
                let mut procs = Vec::new();
                for list in [
                    dev.running_compute_processes(),
                    dev.running_graphics_processes(),
                ]
                .into_iter()
                .flatten()
                {
                    procs.extend(list.into_iter().map(|p| {
                        let mem = match p.used_gpu_memory {
                            UsedGpuMemory::Used(b) => Some(b),
                            UsedGpuMemory::Unavailable => None,
                        };
                        (p.pid, mem)
                    }));
                }
                let seen = &mut self.last_seen[index as usize];
                let stats = dev
                    .process_utilization_stats(Some(*seen))
                    .unwrap_or_default();
                if let Some(newest) = stats.iter().map(|s| s.timestamp).max() {
                    *seen = (*seen).max(newest);
                }
                // Several samples per pid may be returned; keep the busiest.
                let util = stats
                    .iter()
                    .map(|s| (s.pid, f64::from(s.sm_util.max(s.enc_util).max(s.dec_util))));
                // Processes holding memory but silent in this window are idle, not unknown.
                let idle: Vec<_> = procs.iter().map(|(pid, _)| (*pid, 0.0)).collect();
                reading
                    .processes
                    .extend(merge_process_entries(procs, idle.into_iter().chain(util)));
            }
            reading
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_and_graphics_listings_merge_per_pid() {
        let v = merge_process_entries(
            [(5, Some(100)), (5, Some(120)), (7, None)],
            [(5, 10.0), (5, 40.0), (9, 3.0)],
        );
        assert_eq!(
            v,
            vec![
                ProcessGpu {
                    pid: 5,
                    util: Some(40.0),
                    mem: Some(120)
                },
                ProcessGpu {
                    pid: 7,
                    util: None,
                    mem: None
                },
                ProcessGpu {
                    pid: 9,
                    util: Some(3.0),
                    mem: None
                },
            ]
        );
    }

    #[test]
    fn missing_driver_is_a_clean_none() {
        // Containers and CI runners have no libnvidia-ml.
        if std::env::var_os("TVIS_TEST_EXPECT_NVML").is_none() {
            let _ = NvmlBackend::new();
        }
    }
}
