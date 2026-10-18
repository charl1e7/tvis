//! Windows backend: WDDM performance counters (any vendor) plus DXGI for the
//! exact adapter list, so an integrated GPU shows up next to a discrete one.

use tvis_core::frame::Pid;

use crate::pdh::GpuCounters;
use crate::wddm::{self, AdapterInfo};
use crate::{Backend, Reading};

pub(crate) struct WddmBackend {
    counters: GpuCounters,
    adapters: Vec<AdapterInfo>,
    /// NVIDIA adapters are reported by NVML when it is active, so they are not
    /// listed twice; their per-process numbers still come from the counters.
    nvml_active: bool,
}

impl WddmBackend {
    pub(crate) fn new(nvml_active: bool) -> Option<Self> {
        let counters = GpuCounters::open().or_else(|| {
            tracing::debug!("GPU performance counters unavailable");
            None
        })?;
        Some(Self {
            counters,
            adapters: crate::dxgi::adapters(),
            nvml_active,
        })
    }
}

impl Backend for WddmBackend {
    fn name(&self) -> &'static str {
        "windows-wddm"
    }

    fn read(&mut self, _watched: &[Pid]) -> Reading {
        let s = self.counters.sample();
        let usage = wddm::aggregate(&s.engines, &s.process_memory, &s.adapter_memory);
        Reading {
            devices: wddm::build_devices(&self.adapters, &usage, self.nvml_active),
            processes: usage.processes,
        }
    }
}
