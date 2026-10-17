use std::time::Instant;

use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::{Probe, ProbeContext, ProcessSource};
use crate::frame::{Frame, ProcessKind, ProcessSample, SystemInfo, SystemSample};
use crate::metric::{MetricDescriptor, builtin, builtin_descriptors};

/// Real data from the operating system via the `sysinfo` crate.
pub struct SysinfoSource {
    sys: System,
    last_refresh: Instant,
}

impl SysinfoSource {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, Self::refresh_kind());
        Self {
            sys,
            last_refresh: Instant::now(),
        }
    }

    fn refresh_kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_cmd(UpdateKind::OnlyIfNotSet)
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
        // Open files are provided by `OpenFilesProbe`.
        builtin_descriptors()
            .into_iter()
            .filter(|d| d.id != builtin::OPEN_FILES)
            .collect()
    }

    fn sample(&mut self) -> Frame {
        let dt = self.last_refresh.elapsed().as_secs_f64().max(0.05);
        self.last_refresh = Instant::now();
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
            s.command = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            let io = p.disk_usage();
            s.set(builtin::CPU, f64::from(p.cpu_usage()));
            s.set(builtin::MEM_RSS, p.memory() as f64);
            s.set(builtin::MEM_VIRT, p.virtual_memory() as f64);
            s.set(builtin::IO_READ, io.read_bytes as f64 / dt);
            s.set(builtin::IO_WRITE, io.written_bytes as f64 / dt);
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

/// Counts open file descriptors/handles of watched processes.
///
/// Reading them is comparatively expensive, hence a probe that only looks at
/// processes somebody cares about.
pub struct OpenFilesProbe {
    sys: System,
}

impl OpenFilesProbe {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }
}

impl Default for OpenFilesProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe for OpenFilesProbe {
    fn descriptors(&self) -> Vec<MetricDescriptor> {
        builtin_descriptors()
            .into_iter()
            .filter(|d| d.id == builtin::OPEN_FILES)
            .collect()
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>, frame: &mut Frame) {
        let pids: Vec<sysinfo::Pid> = ctx
            .watched
            .iter()
            .filter(|p| frame.process(**p).is_some_and(|s| !s.is_thread()))
            .map(|p| sysinfo::Pid::from_u32(*p))
            .collect();
        if pids.is_empty() {
            return;
        }
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        for pid in pids {
            let count = self.sys.process(pid).and_then(|p| p.open_files());
            if let (Some(count), Some(sample)) = (count, frame.process_mut(pid.as_u32())) {
                sample.set(builtin::OPEN_FILES, count as f64);
            }
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
    fn open_files_probe_reports_for_watched_only() {
        let mut src = SysinfoSource::new();
        let mut frame = src.sample();
        let me = std::process::id();
        OpenFilesProbe::new().probe(&ProbeContext { watched: &[me] }, &mut frame);
        // Platforms without support simply report nothing.
        if cfg!(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )) {
            assert!(
                frame
                    .process(me)
                    .unwrap()
                    .get(builtin::OPEN_FILES)
                    .is_some()
            );
        }
        let other = frame
            .processes
            .iter()
            .find(|p| p.pid != me && !p.is_thread())
            .unwrap();
        assert!(other.get(builtin::OPEN_FILES).is_none());
    }
}
