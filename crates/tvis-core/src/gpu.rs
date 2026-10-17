//! GPU data model shared by every backend.
//!
//! Backends live in the `tvis-gpu` crate; the core only defines what a GPU
//! sample looks like and which per-process metrics exist, so the store,
//! alerts, analysis and export handle GPUs like any other metric.

use crate::metric::{Aggregation, MetricDescriptor, Unit, builtin};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    #[default]
    Other,
}

impl GpuVendor {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Apple => "Apple",
            Self::Other => "GPU",
        }
    }

    /// Maps a PCI vendor id.
    pub fn from_pci(vendor_id: u32) -> Self {
        match vendor_id {
            0x10de => Self::Nvidia,
            0x1002 | 0x1022 => Self::Amd,
            0x8086 => Self::Intel,
            _ => Self::Other,
        }
    }
}

/// One GPU at one instant. Optional readings are `None` when the backend or
/// driver cannot provide them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuSample {
    /// Stable identifier, unique across backends (e.g. `nvml:0`, `drm:card1`).
    pub id: String,
    pub name: String,
    pub vendor: GpuVendor,
    /// Busiest engine, 0–100.
    pub utilization: Option<f64>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// Degrees Celsius.
    pub temperature: Option<f64>,
    /// Watts.
    pub power: Option<f64>,
    /// Core clock in MHz.
    pub clock_mhz: Option<f64>,
    /// Fan speed, 0–100.
    pub fan: Option<f64>,
}

/// Descriptors of the per-process GPU metrics that GPU backends fill in.
pub fn descriptors() -> Vec<MetricDescriptor> {
    vec![
        MetricDescriptor {
            id: builtin::GPU_UTIL,
            name: "GPU",
            unit: Unit::Percent,
            aggregation: Aggregation::Sum,
            per_process_history: true,
            leak_candidate: false,
        },
        MetricDescriptor {
            id: builtin::GPU_MEM,
            name: "GPU memory",
            unit: Unit::Bytes,
            aggregation: Aggregation::Sum,
            per_process_history: true,
            leak_candidate: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_from_pci_ids() {
        assert_eq!(GpuVendor::from_pci(0x10de), GpuVendor::Nvidia);
        assert_eq!(GpuVendor::from_pci(0x1002), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci(0x8086), GpuVendor::Intel);
        assert_eq!(GpuVendor::from_pci(0x1234), GpuVendor::Other);
    }

    #[test]
    fn descriptors_cover_both_metrics() {
        let ids: Vec<_> = descriptors().iter().map(|d| d.id).collect();
        assert_eq!(ids, [builtin::GPU_UTIL, builtin::GPU_MEM]);
    }
}
