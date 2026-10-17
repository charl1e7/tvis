//! Windows backend: WDDM performance counters through WMI.
//!
//! A WMI connection is bound to the COM apartment of the thread that created
//! it and is therefore not `Send`. It lives on a dedicated worker thread that
//! refreshes a shared snapshot; `read()` only copies the latest one, so the
//! (comparatively slow) WMI queries never delay the engine's sampling loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tvis_core::frame::Pid;
use tvis_core::gpu::{GpuSample, GpuVendor};
use wmi::WMIConnection;

use crate::wddm::{self, Aggregate};
use crate::{Backend, Reading};

#[derive(Deserialize)]
#[serde(
    rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine",
    rename_all = "PascalCase"
)]
struct EngineRow {
    name: String,
    utilization_percentage: u64,
}

#[derive(Deserialize)]
#[serde(
    rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory",
    rename_all = "PascalCase"
)]
struct ProcessMemoryRow {
    name: String,
    dedicated_usage: u64,
}

#[derive(Deserialize)]
#[serde(
    rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory",
    rename_all = "PascalCase"
)]
struct AdapterMemoryRow {
    name: String,
    dedicated_usage: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_VideoController", rename_all = "PascalCase")]
struct VideoController {
    name: String,
}

#[derive(Default)]
struct Snapshot {
    aggregate: Aggregate,
    controllers: Vec<String>,
}

pub(crate) struct WddmBackend {
    shared: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    /// When NVML is active it reports NVIDIA devices (with temperature, power
    /// and total memory); this backend then only contributes per-process data
    /// so no GPU is listed twice.
    devices_from_nvml: bool,
}

fn refresh(wmi: &WMIConnection) -> Aggregate {
    let engines: Vec<(String, f64)> = wmi
        .query::<EngineRow>()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.name, r.utilization_percentage as f64))
        .collect();
    let pmem: Vec<(String, u64)> = wmi
        .query::<ProcessMemoryRow>()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.name, r.dedicated_usage))
        .collect();
    let amem: Vec<(String, u64)> = wmi
        .query::<AdapterMemoryRow>()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.name, r.dedicated_usage))
        .collect();
    wddm::aggregate(&engines, &pmem, &amem)
}

impl WddmBackend {
    pub(crate) fn new(nvml_active: bool) -> Option<Self> {
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();

        let worker = {
            let (shared, stop) = (shared.clone(), stop.clone());
            thread::Builder::new()
                .name("tvis-wddm".into())
                .spawn(move || {
                    let wmi = match WMIConnection::new() {
                        Ok(w) => w,
                        Err(e) => {
                            tracing::debug!("WMI unavailable: {e}");
                            let _ = ready_tx.send(false);
                            return;
                        }
                    };
                    // The counter classes only exist on Windows 10 1709+ with a WDDM 2 driver.
                    if let Err(e) = wmi.query::<EngineRow>() {
                        tracing::debug!("GPU performance counters unavailable: {e}");
                        let _ = ready_tx.send(false);
                        return;
                    }
                    let controllers: Vec<String> = wmi
                        .query::<VideoController>()
                        .map(|v| v.into_iter().map(|c| c.name).collect())
                        .unwrap_or_default();
                    let _ = ready_tx.send(true);
                    while !stop.load(Ordering::Relaxed) {
                        let started = Instant::now();
                        let aggregate = refresh(&wmi);
                        if let Ok(mut s) = shared.lock() {
                            s.aggregate = aggregate;
                            s.controllers = controllers.clone();
                        }
                        // Poll about once a second, and back off when WMI is slow.
                        let took = started.elapsed();
                        thread::sleep(Duration::from_millis(700).max(took * 2));
                    }
                })
                .ok()?
        };

        if ready_rx.recv().unwrap_or(false) {
            Some(Self {
                shared,
                stop,
                worker: Some(worker),
                devices_from_nvml: nvml_active,
            })
        } else {
            let _ = worker.join();
            None
        }
    }
}

impl Drop for WddmBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn vendor_of(name: &str) -> GpuVendor {
    if name.contains("NVIDIA") {
        GpuVendor::Nvidia
    } else if name.contains("AMD") || name.contains("Radeon") {
        GpuVendor::Amd
    } else if name.contains("Intel") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}

impl Backend for WddmBackend {
    fn name(&self) -> &'static str {
        "windows-wddm"
    }

    fn read(&mut self, _watched: &[Pid]) -> Reading {
        let Ok(snap) = self.shared.lock() else {
            return Reading::default();
        };
        let devices = if self.devices_from_nvml {
            Vec::new()
        } else {
            snap.aggregate
                .adapters
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    // WMI does not link a LUID to a controller name; with several
                    // adapters the names are matched by order, which is the common case.
                    let name = snap
                        .controllers
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("GPU {i}"));
                    GpuSample {
                        id: format!("wddm:{}", a.luid),
                        vendor: vendor_of(&name),
                        name,
                        utilization: Some(a.util),
                        mem_used: a.mem_used,
                        ..GpuSample::default()
                    }
                })
                .collect()
        };
        Reading {
            devices,
            processes: snap.aggregate.processes.clone(),
        }
    }
}
