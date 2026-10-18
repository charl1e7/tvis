//! Adapter enumeration through DXGI: exact name, vendor, dedicated memory and
//! the LUID that links an adapter to the performance-counter instances.

#![allow(unsafe_code)]

use tvis_core::gpu::GpuVendor;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};

use crate::wddm::{AdapterInfo, luid_key};

/// Lists the graphics adapters; empty when DXGI is unavailable.
pub(crate) fn adapters() -> Vec<AdapterInfo> {
    let mut out = Vec::new();
    // SAFETY: plain COM calls on interfaces owned by this function; every
    // returned interface is released when it goes out of scope.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            let len = desc
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.Description.len());
            out.push(AdapterInfo {
                luid: luid_key(desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart),
                name: String::from_utf16_lossy(&desc.Description[..len])
                    .trim()
                    .to_owned(),
                vendor: GpuVendor::from_pci(desc.VendorId),
                dedicated: desc.DedicatedVideoMemory as u64,
                software: desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
            });
        }
    }
    out
}
