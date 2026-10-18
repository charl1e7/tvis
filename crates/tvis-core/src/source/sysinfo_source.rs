use std::collections::HashMap;
use std::time::Instant;

use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System};

use super::{Probe, ProbeContext, ProcessSource};
use crate::frame::{Frame, Pid, ProcessKind, ProcessSample, SystemInfo, SystemSample};
use crate::metric::{MetricDescriptor, builtin, builtin_descriptors};

/// Real data from the operating system via the `sysinfo` crate.
pub struct SysinfoSource {
    sys: System,
}

impl SysinfoSource {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, Self::refresh_kind());
        Self { sys }
    }

    /// What is refreshed for every process on every tick.
    ///
    /// Command lines, executable paths and environments are deliberately *not*
    /// requested: with `UpdateKind::OnlyIfNotSet` sysinfo retries every process
    /// whose command line is still empty on each refresh, and protected Windows
    /// processes never yield one, so each tick re-opens and re-reads dozens of
    /// processes for nothing.
    fn refresh_kind() -> ProcessRefreshKind {
        // Disk I/O and open files are per-process syscalls that only matter for
        // watched processes, so `ProcessDetailProbe` reads them instead.
        ProcessRefreshKind::nothing().with_cpu().with_memory()
    }
}

impl Default for SysinfoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSource for SysinfoSource {
    fn info(&self) -> SystemInfo {
        let cpus = self.sys.cpus();
        SystemInfo {
            host_name: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_else(|| System::name().unwrap_or_default()),
            kernel: System::kernel_version().unwrap_or_default(),
            cpu_brand: cpus
                .first()
                .map(|c| c.brand().trim().to_owned())
                .unwrap_or_default(),
            cpu_cores: cpus.len().max(1),
            physical_cores: System::physical_core_count().unwrap_or(cpus.len()).max(1),
        }
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        // Disk I/O and open files are provided by `ProcessDetailProbe`.
        builtin_descriptors()
            .into_iter()
            .filter(|d| !DETAIL_METRICS.contains(&d.id))
            .collect()
    }

    fn sample(&mut self) -> Frame {
        self.sys.refresh_cpu_usage();
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::refresh_kind());

        let load = System::load_average();
        let system = SystemSample {
            cpu_total: f64::from(self.sys.global_cpu_usage()),
            cpu_per_core: self
                .sys
                .cpus()
                .iter()
                .map(|c| f64::from(c.cpu_usage()))
                .collect(),
            mem_total: self.sys.total_memory(),
            mem_used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            load_average: [load.one, load.five, load.fifteen],
            uptime_secs: System::uptime(),
            process_count: 0,
        };

        let mut processes = Vec::with_capacity(self.sys.processes().len());
        for (pid, p) in self.sys.processes() {
            let mut s = ProcessSample::new(
                pid.as_u32(),
                p.parent().map(|p| p.as_u32()),
                p.name().to_string_lossy(),
            );
            s.kind = if p.thread_kind().is_some() {
                ProcessKind::Thread
            } else {
                ProcessKind::Process
            };
            s.start_time = p.start_time();
            s.set(builtin::CPU, f64::from(p.cpu_usage()));
            s.set(builtin::MEM_RSS, p.memory() as f64);
            s.set(builtin::MEM_VIRT, p.virtual_memory() as f64);
            if let Some(tasks) = p.tasks().filter(|_| s.kind == ProcessKind::Process) {
                s.set(builtin::THREADS, tasks.len().max(1) as f64);
            }
            processes.push(s);
        }
        let process_count = processes.iter().filter(|p| !p.is_thread()).count();
        let mut frame = Frame::new(
            SystemSample {
                process_count,
                ..system
            },
            processes,
        );
        frame.wall_ms = 0;
        frame
    }
}

/// Metrics measured by [`ProcessDetailProbe`] rather than by the base source.
const DETAIL_METRICS: [crate::metric::MetricId; 3] =
    [builtin::IO_READ, builtin::IO_WRITE, builtin::OPEN_FILES];

/// Disk throughput and open files/handles of *watched* processes.
///
/// Both need extra syscalls per process, so doing them for the few processes
/// someone watches instead of the whole table keeps ticks cheap on machines
/// with hundreds of processes.
pub struct ProcessDetailProbe {
    sys: System,
    /// Cumulative I/O counters at the previous sample, per pid.
    previous: HashMap<Pid, IoSnapshot>,
}

struct IoSnapshot {
    read: u64,
    written: u64,
    at: Instant,
}

impl ProcessDetailProbe {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            previous: HashMap::new(),
        }
    }
}

impl Default for ProcessDetailProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe for ProcessDetailProbe {
    fn descriptors(&self) -> Vec<MetricDescriptor> {
        builtin_descriptors()
            .into_iter()
            .filter(|d| DETAIL_METRICS.contains(&d.id))
            .collect()
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>, frame: &mut Frame) {
        let pids: Vec<sysinfo::Pid> = ctx
            .watched
            .iter()
            .filter(|p| frame.process(**p).is_some_and(|s| !s.is_thread()))
            .map(|p| sysinfo::Pid::from_u32(*p))
            .collect();
        self.previous
            .retain(|pid, _| pids.iter().any(|p| p.as_u32() == *pid));
        if pids.is_empty() {
            return;
        }
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing()
                .with_disk_usage()
                .without_tasks(),
        );
        let now = Instant::now();
        for pid in pids {
            let Some(process) = self.sys.process(pid) else {
                continue;
            };
            let Some(sample) = frame.process_mut(pid.as_u32()) else {
                continue;
            };
            if let Some(count) = process.open_files() {
                sample.set(builtin::OPEN_FILES, count as f64);
            }
            let io = process.disk_usage();
            let (read, written) = (io.total_read_bytes, io.total_written_bytes);
            // Rates come from the cumulative counters so a freshly watched
            // process does not report its whole history as one second of I/O.
            if let Some(prev) = self.previous.get(&pid.as_u32()) {
                let dt = now.duration_since(prev.at).as_secs_f64().max(0.05);
                sample.set(builtin::IO_READ, read.saturating_sub(prev.read) as f64 / dt);
                sample.set(
                    builtin::IO_WRITE,
                    written.saturating_sub(prev.written) as f64 / dt,
                );
            }
            self.previous.insert(
                pid.as_u32(),
                IoSnapshot {
                    read,
                    written,
                    at: now,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_itself() {
        let mut src = SysinfoSource::new();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let frame = src.sample();
        let me = frame
            .process(std::process::id())
            .expect("own process listed");
        assert!(me.value(builtin::MEM_RSS) > 0.0);
        assert!(frame.system.mem_total > 0);
        assert!(src.info().cpu_cores >= 1);
    }

    #[test]
    fn detail_probe_reports_for_watched_only_and_rates_need_two_samples() {
        let mut src = SysinfoSource::new();
        let me = std::process::id();
        let mut probe = ProcessDetailProbe::new();

        let mut frame = src.sample();
        probe.probe(&ProbeContext { watched: &[me] }, &mut frame);
        let mine = frame.process(me).unwrap();
        // Platforms without support simply report nothing.
        if cfg!(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )) {
            assert!(mine.get(builtin::OPEN_FILES).is_some());
        }
        assert!(
            mine.get(builtin::IO_READ).is_none(),
            "first sight only records a baseline"
        );

        std::thread::sleep(std::time::Duration::from_millis(60));
        let mut frame = src.sample();
        probe.probe(&ProbeContext { watched: &[me] }, &mut frame);
        let mine = frame.process(me).unwrap();
        assert!(mine.get(builtin::IO_READ).is_some());
        assert!(mine.get(builtin::IO_WRITE).is_some());

        let other = frame
            .processes
            .iter()
            .find(|p| p.pid != me && !p.is_thread())
            .unwrap();
        assert!(other.get(builtin::OPEN_FILES).is_none());
        assert!(other.get(builtin::IO_READ).is_none());
    }
}
