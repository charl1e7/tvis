use super::ProcessSource;
use crate::frame::{Frame, ProcessSample, SystemInfo, SystemSample};
use crate::gpu::{GpuSample, GpuVendor};
use crate::metric::{MetricDescriptor, builtin, builtin_descriptors};

/// Deterministic fake machine: used by `--demo`, screenshots and tests, and a
/// compact example of a custom [`ProcessSource`].
pub struct SyntheticSource {
    tick: u64,
    backfill_ticks: u64,
}

const STEP_SECS: f64 = 1.0;
const MIB: f64 = 1024.0 * 1024.0;

impl SyntheticSource {
    /// `backfill_secs` seconds of history are produced up front.
    pub fn new(backfill_secs: u64) -> Self {
        Self {
            tick: 0,
            backfill_ticks: backfill_secs,
        }
    }

    fn frame_at(tick: u64, t: f64) -> Frame {
        let n = tick as f64;
        let noise = |seed: u64| -> f64 {
            // splitmix64 → [-1, 1]
            let mut z = tick
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z as f64 / u64::MAX as f64) * 2.0 - 1.0
        };

        let mut procs: Vec<ProcessSample> = Vec::new();
        let mut add = |pid: u32,
                       parent: u32,
                       name: &str,
                       cpu: f64,
                       rss_mib: f64,
                       threads: f64,
                       rd: f64,
                       wr: f64,
                       files: f64| {
            let mut p = ProcessSample::new(pid, Some(parent), name);
            p.command = format!("/usr/bin/{name} --serve");
            p.start_time = 1_700_000_000;
            p.set(builtin::CPU, cpu.max(0.0));
            p.set(builtin::MEM_RSS, rss_mib * MIB);
            p.set(builtin::MEM_VIRT, rss_mib * MIB * 3.4);
            p.set(builtin::THREADS, threads.round());
            p.set(builtin::IO_READ, rd.max(0.0));
            p.set(builtin::IO_WRITE, wr.max(0.0));
            p.set(builtin::OPEN_FILES, files.round());
            procs.push(p);
        };

        let wave = |period: f64, phase: f64| (n / period * std::f64::consts::TAU + phase).sin();
        add(1, 0, "init", 0.2, 12.0, 1.0, 0.0, 0.0, 6.0);
        // Healthy service with a daily rhythm.
        add(
            100,
            1,
            "gateway",
            14.0 + 9.0 * wave(90.0, 0.0) + 2.0 * noise(1),
            210.0 + 3.0 * noise(2),
            24.0,
            1.2e5 * (1.0 + noise(3)),
            3.0e4,
            180.0,
        );
        // Slow memory leak, growing worker pool.
        add(
            210,
            1,
            "api-server",
            22.0 + 12.0 * wave(60.0, 1.0) + 4.0 * noise(4),
            320.0 + n * 0.9 + 4.0 * noise(5),
            40.0 + (n / 40.0).floor(),
            4.0e5 * (1.0 + noise(6)),
            9.0e4,
            420.0 + n * 0.1,
        );
        for k in 0..3u32 {
            let ph = f64::from(k);
            add(
                211 + k,
                210,
                "api-worker",
                18.0 + 15.0 * wave(45.0 + 9.0 * ph, ph) + 3.0 * noise(10 + u64::from(k)),
                140.0 + 40.0 * ph + n * 0.35 + 2.0 * noise(20 + u64::from(k)),
                12.0,
                1.0e5,
                5.0e4,
                60.0,
            );
        }
        // Database with bursty IO.
        let burst = if (tick / 25) % 4 == 3 { 1.0 } else { 0.15 };
        add(
            300,
            1,
            "postgres",
            8.0 + 30.0 * burst + 5.0 * noise(30),
            1180.0 + 15.0 * wave(120.0, 0.5),
            18.0,
            6.0e7 * burst * (1.0 + 0.3 * noise(31)),
            2.5e7 * burst,
            96.0,
        );
        for k in 0..4u32 {
            add(
                301 + k,
                300,
                "postgres",
                3.0 + 12.0 * burst * (1.0 + noise(32 + u64::from(k))),
                95.0 + 12.0 * f64::from(k),
                1.0,
                4.0e6 * burst,
                1.0e6,
                22.0,
            );
        }
        add(
            400,
            1,
            "redis-server",
            3.0 + 2.0 * noise(40),
            512.0 + 0.4 * n.min(600.0),
            5.0,
            0.0,
            6.0e4 * (1.0 + noise(41)),
            40.0,
        );
        add(
            500,
            1,
            "nginx",
            2.0 + 1.5 * noise(50),
            38.0,
            2.0,
            2.0e4,
            8.0e4,
            64.0,
        );
        for k in 0..2u32 {
            add(
                501 + k,
                500,
                "nginx",
                4.0 + 3.0 * wave(30.0, f64::from(k)) + noise(51 + u64::from(k)),
                52.0,
                1.0,
                1.0e4,
                5.0e4,
                120.0,
            );
        }
        // Occasional CPU spikes.
        let spike = if tick.is_multiple_of(97) { 340.0 } else { 0.0 };
        add(
            600,
            1,
            "node",
            9.0 + 4.0 * noise(60) + spike,
            260.0 + 6.0 * wave(80.0, 2.0),
            11.0,
            3.0e4,
            2.0e4,
            75.0,
        );
        // Pegged core.
        add(
            700,
            1,
            "ffmpeg",
            280.0 + 40.0 * noise(70),
            840.0 + 25.0 * wave(20.0, 0.0),
            16.0,
            9.0e7,
            7.0e7,
            14.0,
        );

        // GPU load of a few processes (renderer + an encoder + an ML worker with growing VRAM).
        let mut gpu_load = |pid: u32, util: f64, mem_mib: f64| {
            if let Some(p) = procs.iter_mut().find(|p| p.pid == pid) {
                p.set(builtin::GPU_UTIL, util.clamp(0.0, 100.0));
                p.set(builtin::GPU_MEM, mem_mib * MIB);
            }
        };
        gpu_load(
            700,
            62.0 + 18.0 * wave(25.0, 0.3) + 4.0 * noise(80),
            1850.0 + 20.0 * wave(40.0, 0.0),
        );
        gpu_load(600, 8.0 + 5.0 * wave(50.0, 1.2) + 2.0 * noise(81), 420.0);
        gpu_load(
            210,
            14.0 + 10.0 * wave(70.0, 0.7) + 3.0 * noise(82),
            900.0 + n * 1.4,
        );
        gpu_load(1, 1.0, 60.0);
        let gpu_util = |pid: u32| {
            procs
                .iter()
                .find(|p| p.pid == pid)
                .map_or(0.0, |p| p.value(builtin::GPU_UTIL))
        };
        let gpu_used: f64 = procs.iter().map(|p| p.value(builtin::GPU_MEM)).sum();
        let discrete_util =
            (gpu_util(700) + gpu_util(210) + 4.0 + 2.0 * noise(83)).clamp(0.0, 100.0);
        let gpus = vec![
            GpuSample {
                id: "demo:0".into(),
                name: "Demo RTX 4080".into(),
                vendor: GpuVendor::Nvidia,
                utilization: Some(discrete_util),
                mem_used: Some((gpu_used + 900.0 * MIB) as u64),
                mem_total: Some((16.0 * 1024.0 * MIB) as u64),
                temperature: Some(52.0 + discrete_util * 0.32 + noise(84)),
                power: Some(38.0 + discrete_util * 2.1 + 2.0 * noise(85)),
                clock_mhz: Some(1400.0 + discrete_util * 14.0),
                fan: Some(30.0 + discrete_util * 0.4),
            },
            GpuSample {
                id: "demo:1".into(),
                name: "Demo Radeon Graphics".into(),
                vendor: GpuVendor::Amd,
                utilization: Some(
                    (7.0 + 5.0 * wave(33.0, 2.0) + 2.0 * noise(86)).clamp(0.0, 100.0),
                ),
                mem_used: Some((620.0 * MIB) as u64),
                mem_total: Some((2048.0 * MIB) as u64),
                temperature: Some(41.0 + 2.0 * noise(87)),
                power: Some(6.5 + noise(88)),
                clock_mhz: Some(800.0),
                fan: None,
            },
        ];

        let cores = 16usize;
        let per_core: Vec<f64> = (0..cores)
            .map(|c| {
                (35.0 + 30.0 * (n / 40.0 + c as f64).sin() + 10.0 * noise(100 + c as u64))
                    .clamp(1.0, 100.0)
            })
            .collect();
        let total = per_core.iter().sum::<f64>() / cores as f64;
        let mem_total = 32.0 * 1024.0 * MIB;
        let system = SystemSample {
            cpu_total: total,
            cpu_per_core: per_core,
            mem_total: mem_total as u64,
            mem_used: (14.5 * 1024.0 * MIB + n * 0.9 * MIB) as u64,
            swap_total: (8.0 * 1024.0 * MIB) as u64,
            swap_used: (0.4 * 1024.0 * MIB) as u64,
            load_average: [total / 100.0 * 6.0, 4.1, 3.6],
            uptime_secs: 86_400 * 3 + tick,
            process_count: procs.len(),
        };
        let mut frame = Frame::new(system, procs);
        frame.gpus = gpus;
        frame.t = t;
        frame
    }
}

impl ProcessSource for SyntheticSource {
    fn info(&self) -> SystemInfo {
        SystemInfo {
            host_name: "demo-workstation".into(),
            os: "TVIS Synthetic OS".into(),
            kernel: "demo".into(),
            cpu_brand: "Virtual 16-core CPU".into(),
            cpu_cores: 16,
            physical_cores: 8,
        }
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        let mut d = builtin_descriptors();
        d.extend(crate::gpu::descriptors());
        d
    }

    fn sample(&mut self) -> Frame {
        self.tick += 1;
        Self::frame_at(self.backfill_ticks + self.tick, 0.0)
    }

    fn backfill(&mut self) -> Vec<Frame> {
        let n = self.backfill_ticks;
        (0..n)
            .map(|i| Self::frame_at(i, -((n - i) as f64) * STEP_SECS))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic_and_backfills_oldest_first() {
        let mut a = SyntheticSource::new(30);
        let mut b = SyntheticSource::new(30);
        let (fa, fb) = (a.backfill(), b.backfill());
        assert_eq!(fa.len(), 30);
        assert!(fa.windows(2).all(|w| w[0].t < w[1].t));
        assert!(fa[29].t < 0.0);
        assert_eq!(
            fa[5].process(210).unwrap().value(builtin::MEM_RSS),
            fb[5].process(210).unwrap().value(builtin::MEM_RSS)
        );
        let f = a.sample();
        assert!(f.process(700).is_some());
        assert_eq!(f.gpus.len(), 2);
        assert!(f.process(700).unwrap().value(builtin::GPU_MEM) > 0.0);
        assert!(f.gpus[0].utilization.unwrap() <= 100.0);
    }
}
