//! Windows Performance Data Helper: the cheap way to read the WDDM GPU
//! counters. One persistent query is sampled per tick; PDH computes rates from
//! the previous sample itself, so a read costs a few milliseconds even with
//! hundreds of counter instances (WMI's "formatted data" classes re-sample on
//! every query and keep a WMI provider process busy).

#![allow(unsafe_code)]

use windows::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::core::w;

/// One sample of every GPU counter.
#[derive(Debug, Default)]
pub(crate) struct Sample {
    /// `GPU Engine(*)\Utilization Percentage`
    pub engines: Vec<(String, f64)>,
    /// `GPU Process Memory(*)\Dedicated Usage`, bytes
    pub process_memory: Vec<(String, u64)>,
    /// `GPU Adapter Memory(*)\Dedicated Usage`, bytes
    pub adapter_memory: Vec<(String, u64)>,
}

pub(crate) struct GpuCounters {
    query: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    process_memory: PDH_HCOUNTER,
    adapter_memory: PDH_HCOUNTER,
}

// SAFETY: PDH handles are plain tokens that may be used from any thread as long
// as calls are not concurrent; `GpuCounters` is only ever used by one thread at
// a time (the engine thread) and needs `&mut self` for every call.
unsafe impl Send for GpuCounters {}

impl GpuCounters {
    /// `None` when the counters do not exist (before Windows 10 1709 / WDDM 2).
    pub(crate) fn open() -> Option<Self> {
        // SAFETY: out-pointers refer to locals; handles are closed in `Drop`.
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut query) != 0 {
                return None;
            }
            let add = |path: windows::core::PCWSTR| {
                let mut counter = PDH_HCOUNTER::default();
                (PdhAddEnglishCounterW(query, path, 0, &mut counter) == 0).then_some(counter)
            };
            let counters = (|| {
                Some((
                    add(w!("\\GPU Engine(*)\\Utilization Percentage"))?,
                    add(w!("\\GPU Process Memory(*)\\Dedicated Usage"))?,
                    add(w!("\\GPU Adapter Memory(*)\\Dedicated Usage"))?,
                ))
            })();
            let Some((engine, process_memory, adapter_memory)) = counters else {
                PdhCloseQuery(query);
                return None;
            };
            // Prime the rate counters; the first real reading is one tick later.
            PdhCollectQueryData(query);
            Some(Self {
                query,
                engine,
                process_memory,
                adapter_memory,
            })
        }
    }

    /// Takes a new sample. Counters that cannot be formatted yet are left empty.
    pub(crate) fn sample(&mut self) -> Sample {
        // SAFETY: `query` is a valid open query for the lifetime of `self`.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return Sample::default();
        }
        Sample {
            engines: read_array(self.engine),
            process_memory: read_array(self.process_memory)
                .into_iter()
                .map(|(n, v)| (n, v.max(0.0) as u64))
                .collect(),
            adapter_memory: read_array(self.adapter_memory)
                .into_iter()
                .map(|(n, v)| (n, v.max(0.0) as u64))
                .collect(),
        }
    }
}

impl Drop for GpuCounters {
    fn drop(&mut self) {
        // SAFETY: closing the query also releases its counters.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// Reads every instance of a wildcard counter as `(instance name, value)`.
fn read_array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    // The instance set can grow between the size query and the read; retry a few times.
    let mut size = 0u32;
    for _ in 0..4 {
        let mut count = 0u32;
        // SAFETY: `size`/`count` are valid out-pointers; with no buffer PDH only
        // reports the required size.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
        };
        if status != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // u64 elements keep the buffer 8-byte aligned, as the item struct requires.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        // SAFETY: `buffer` is at least `size` bytes, as PDH asked.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(items),
            )
        };
        if status == PDH_MORE_DATA {
            continue;
        }
        if status != 0 {
            return Vec::new();
        }
        // SAFETY: on success PDH wrote `count` valid items at the start of the
        // buffer; their names point into the same buffer, which outlives this loop.
        let slice = unsafe { std::slice::from_raw_parts(items, count as usize) };
        return slice
            .iter()
            .filter(|i| {
                i.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA
                    || i.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA
            })
            .map(|i| {
                // SAFETY: `szName` is a NUL-terminated UTF-16 string inside `buffer`;
                // the double member is the active union field for PDH_FMT_DOUBLE.
                let (name, value) = unsafe {
                    (
                        i.szName.to_string().unwrap_or_default(),
                        i.FmtValue.Anonymous.doubleValue,
                    )
                };
                (name, value)
            })
            .collect();
    }
    Vec::new()
}
