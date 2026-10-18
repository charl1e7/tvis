//! GPU metric probes for TVIS.
//!
//! One [`GpuProbe`] aggregates several native backends and plugs into the
//! engine like any other [`Probe`]:
//!
//! | platform | devices                                   | per process                          |
//! |----------|-------------------------------------------|--------------------------------------|
//! | NVIDIA (Linux, Windows) | NVML, loaded at run time   | memory + utilisation (Linux)         |
//! | Linux AMD / Intel       | sysfs + hwmon              | DRM `fdinfo` engine time and memory  |
//! | Windows (any vendor)    | WDDM performance counters  | per-process engine time and VRAM     |
//! | macOS                   | IOKit `IOAccelerator`      | not exposed by the OS without root   |
//!
//! Nothing here links against a vendor library: NVML is `dlopen`ed, so the
//! same binary runs on machines without an NVIDIA driver, and a machine with
//! no supported GPU simply yields no probe ([`GpuProbe::detect`] returns `None`).

#![forbid(unsafe_code)]

mod drm;
mod mac;
mod nvml;
mod wddm;
#[cfg(windows)]
mod windows;

use tvis_core::frame::{Frame, Pid};
use tvis_core::gpu::{self, GpuSample};
use tvis_core::metric::{MetricDescriptor, builtin};
use tvis_core::source::{Probe, ProbeContext};

/// GPU usage of one process on one device.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ProcessGpu {
    pub pid: Pid,
    /// Busiest engine, 0–100.
    pub util: Option<f64>,
    /// Device memory in bytes.
    pub mem: Option<u64>,
}

/// Everything one backend observed in one pass.
#[derive(Debug, Default)]
pub(crate) struct Reading {
    pub devices: Vec<GpuSample>,
    pub processes: Vec<ProcessGpu>,
}

pub(crate) trait Backend: Send {
    fn name(&self) -> &'static str;

    /// Reads devices and per-process usage. `watched` lists the pids the user
    /// cares about; backends that must scan per process (Linux fdinfo) only
    /// look at those, while others return every process they know.
    fn read(&mut self, watched: &[Pid]) -> Reading;
}

/// Probe that merges all available GPU backends.
pub struct GpuProbe {
    backends: Vec<Box<dyn Backend>>,
}

impl GpuProbe {
    /// Initialises every backend that works on this machine. Returns `None`
    /// when no GPU could be found, so callers can skip the probe entirely.
    pub fn detect() -> Option<Self> {
        let mut backends: Vec<Box<dyn Backend>> = Vec::new();

        let nvml = nvml::NvmlBackend::new();
        let nvml_active = nvml.is_some();
        if let Some(n) = nvml {
            backends.push(Box::new(n));
        }
        if let Some(b) = drm::DrmBackend::new(nvml_active) {
            backends.push(Box::new(b));
        }
        #[cfg(windows)]
        if let Some(b) = windows::WddmBackend::new(nvml_active) {
            backends.push(Box::new(b));
        }
        if let Some(b) = mac::IoregBackend::new() {
            backends.push(Box::new(b));
        }

        for b in &backends {
            tracing::info!("GPU backend active: {}", b.name());
        }
        (!backends.is_empty()).then_some(Self { backends })
    }

    /// Names of the active backends, for diagnostics.
    pub fn backend_names(&self) -> Vec<&'static str> {
        self.backends.iter().map(|b| b.name()).collect()
    }
}

impl Probe for GpuProbe {
    fn descriptors(&self) -> Vec<MetricDescriptor> {
        gpu::descriptors()
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>, frame: &mut Frame) {
        let mut per_pid: Vec<ProcessGpu> = Vec::new();
        for backend in &mut self.backends {
            let reading = backend.read(ctx.watched);
            frame.gpus.extend(reading.devices);
            per_pid.extend(reading.processes);
        }
        apply_process_usage(frame, &per_pid);
    }
}

/// Sums the usage of every process across devices and stores it on the frame.
pub(crate) fn apply_process_usage(frame: &mut Frame, usage: &[ProcessGpu]) {
    use std::collections::HashMap;
    let mut sum: HashMap<Pid, (Option<f64>, Option<u64>)> = HashMap::new();
    for u in usage {
        let e = sum.entry(u.pid).or_default();
        if let Some(v) = u.util {
            *e.0.get_or_insert(0.0) += v;
        }
        if let Some(m) = u.mem {
            *e.1.get_or_insert(0) += m;
        }
    }
    for (pid, (util, mem)) in sum {
        let Some(p) = frame.process_mut(pid) else {
            continue;
        };
        if let Some(v) = util {
            p.set(builtin::GPU_UTIL, v.clamp(0.0, 100.0 * 8.0));
        }
        if let Some(m) = mem {
            p.set(builtin::GPU_MEM, m as f64);
        }
    }
}

#[cfg(test)]
mod tests {
    use tvis_core::frame::{ProcessSample, SystemSample};

    use super::*;

    #[test]
    fn usage_is_summed_across_devices_and_unknown_pids_are_ignored() {
        let mut frame = Frame::new(
            SystemSample::default(),
            vec![
                ProcessSample::new(10, None, "a"),
                ProcessSample::new(11, None, "b"),
            ],
        );
        apply_process_usage(
            &mut frame,
            &[
                ProcessGpu {
                    pid: 10,
                    util: Some(30.0),
                    mem: Some(100),
                },
                ProcessGpu {
                    pid: 10,
                    util: Some(15.0),
                    mem: Some(50),
                },
                ProcessGpu {
                    pid: 11,
                    util: None,
                    mem: Some(7),
                },
                ProcessGpu {
                    pid: 999,
                    util: Some(1.0),
                    mem: Some(1),
                },
            ],
        );
        let a = frame.process(10).unwrap();
        assert_eq!(a.get(builtin::GPU_UTIL), Some(45.0));
        assert_eq!(a.get(builtin::GPU_MEM), Some(150.0));
        let b = frame.process(11).unwrap();
        assert_eq!(
            b.get(builtin::GPU_UTIL),
            None,
            "no utilisation reported, none recorded"
        );
        assert_eq!(b.get(builtin::GPU_MEM), Some(7.0));
    }

    #[test]
    fn detect_never_panics_on_machines_without_gpus() {
        // CI runners and containers have no GPU; this must be a clean `None`/`Some`.
        let _ = GpuProbe::detect();
    }
}
