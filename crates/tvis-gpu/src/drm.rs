//! Linux: AMD / Intel / other DRM drivers through sysfs, hwmon and `fdinfo`.
//!
//! * Device figures come from `/sys/class/drm/card*/device` (AMD publishes
//!   busy percent and VRAM use; hwmon adds temperature, power and fan).
//! * Per-process figures come from `/proc/<pid>/fdinfo/*` entries of DRM file
//!   descriptors, documented in the kernel's `drm-usage-stats.rst`:
//!   `drm-engine-<name>: <ns>` is accumulated busy time (utilisation is its
//!   delta over wall time) and `drm-resident-*` / `drm-memory-*` report memory.
//!
//! The parsing is pure and works on a configurable root, so it is tested with
//! fixture trees and compiled on every platform; it only runs on Linux.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tvis_core::frame::Pid;
use tvis_core::gpu::{GpuSample, GpuVendor};

use crate::{Backend, ProcessGpu, Reading};

#[derive(Clone, Debug)]
pub(crate) struct Roots {
    pub sys: PathBuf,
    pub proc: PathBuf,
}

impl Roots {
    fn real() -> Self {
        Self {
            sys: "/sys".into(),
            proc: "/proc".into(),
        }
    }
}

#[derive(Clone, Debug)]
struct Device {
    id: String,
    /// `/sys/class/drm/cardN`
    card: PathBuf,
    vendor: GpuVendor,
    #[allow(dead_code)]
    driver: String,
    name: String,
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn parse_hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

/// Finds `cardN` devices (not connectors such as `card0-HDMI-A-1`).
fn discover(roots: &Roots, skip_nvidia: bool) -> Vec<Device> {
    let Ok(entries) = fs::read_dir(roots.sys.join("class/drm")) else {
        return Vec::new();
    };
    let mut devices: Vec<Device> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let is_card = file
                .strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if !is_card {
                return None;
            }
            let card = e.path();
            let dev = card.join("device");
            let vendor = GpuVendor::from_pci(parse_hex(&read_trimmed(dev.join("vendor"))?)?);
            if skip_nvidia && vendor == GpuVendor::Nvidia {
                return None;
            }
            let uevent = read_trimmed(dev.join("uevent")).unwrap_or_default();
            let field = |key: &str| {
                uevent
                    .lines()
                    .find_map(|l| l.strip_prefix(key))
                    .map(str::to_owned)
                    .unwrap_or_default()
            };
            let driver = field("DRIVER=");
            let pci = field("PCI_SLOT_NAME=");
            let name = read_trimmed(dev.join("product_name"))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| {
                    let base = format!("{} GPU", vendor.name());
                    if pci.is_empty() {
                        base
                    } else {
                        format!("{base} ({pci})")
                    }
                });
            Some(Device {
                id: format!("drm:{file}"),
                card,
                vendor,
                driver,
                name,
            })
        })
        .collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices
}

fn first_hwmon(dev: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<_> = fs::read_dir(dev.join("hwmon"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

fn read_device(d: &Device) -> GpuSample {
    let dev = d.card.join("device");
    let hwmon = first_hwmon(&dev);
    let hw = |name: &str| hwmon.as_ref().and_then(|h| read_u64(h.join(name)));
    let clock = hw("freq1_input")
        .map(|hz| hz as f64 / 1e6)
        .or_else(|| read_u64(d.card.join("gt_cur_freq_mhz")).map(|v| v as f64))
        .or_else(|| read_u64(dev.join("tile0/gt0/freq0/cur_freq")).map(|v| v as f64));
    GpuSample {
        id: d.id.clone(),
        name: d.name.clone(),
        vendor: d.vendor,
        utilization: read_u64(dev.join("gpu_busy_percent")).map(|v| v.min(100) as f64),
        mem_used: read_u64(dev.join("mem_info_vram_used")),
        mem_total: read_u64(dev.join("mem_info_vram_total")),
        temperature: hw("temp1_input").map(|m| m as f64 / 1000.0),
        power: hw("power1_average")
            .or_else(|| hw("power1_input"))
            .map(|uw| uw as f64 / 1e6),
        clock_mhz: clock,
        fan: hw("pwm1").map(|v| (v as f64 / 255.0 * 100.0).min(100.0)),
    }
}

/// Parses `"123"`, `"123 KiB"`, `"5 MiB"`… into bytes.
pub(crate) fn parse_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    let mul = match parts.next() {
        None => 1,
        Some("KiB") => 1 << 10,
        Some("MiB") => 1 << 20,
        Some("GiB") => 1 << 30,
        Some(_) => return None,
    };
    Some(n.saturating_mul(mul))
}

/// One DRM client (several descriptors may refer to the same client).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Client {
    pub key: String,
    /// Accumulated busy time per engine, nanoseconds.
    pub engines: Vec<(String, u64)>,
    pub mem: u64,
}

/// Parses the text of one `fdinfo` file; `None` if it is not a DRM descriptor.
pub(crate) fn parse_fdinfo(text: &str) -> Option<Client> {
    let mut driver = None;
    let mut client_id = String::new();
    let mut pdev = String::new();
    let mut engines = Vec::new();
    let mut mem: HashMap<String, u64> = HashMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "drm-driver" => driver = Some(v.to_owned()),
            "drm-client-id" => client_id = v.to_owned(),
            "drm-pdev" => pdev = v.to_owned(),
            _ if k.starts_with("drm-engine-") && !k.starts_with("drm-engine-capacity-") => {
                let ns = v.trim_end_matches("ns").trim().parse().ok();
                if let Some(ns) = ns {
                    engines.push((k["drm-engine-".len()..].to_owned(), ns));
                }
            }
            _ if k.starts_with("drm-resident-")
                || k.starts_with("drm-memory-")
                || k.starts_with("drm-total-") =>
            {
                if let Some(b) = parse_size(v) {
                    mem.insert(k.to_owned(), b);
                }
            }
            _ => {}
        }
    }
    let driver = driver?;
    Some(Client {
        key: format!("{driver}/{pdev}/{client_id}"),
        engines,
        mem: pick_memory(&mem),
    })
}

/// Picks one memory figure per client. Drivers print several overlapping
/// keys (new kernels alias `drm-memory-*` to `drm-resident-*`), so summing all
/// of them would double count: take the first group that is present.
fn pick_memory(mem: &HashMap<String, u64>) -> u64 {
    for exact in ["drm-resident-vram", "drm-memory-vram"] {
        if let Some(v) = mem.get(exact) {
            return *v;
        }
    }
    for prefix in [
        "drm-resident-vram",
        "drm-memory-vram",
        "drm-resident-local",
        "drm-memory-local",
        "drm-total-local",
    ] {
        let sum: u64 = mem
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(_, v)| *v)
            .sum();
        if sum > 0 {
            return sum;
        }
    }
    mem.get("drm-resident-memory")
        .or_else(|| mem.get("drm-memory-gtt"))
        .copied()
        .unwrap_or(0)
}

/// All distinct DRM clients of `pid`.
fn scan_pid(roots: &Roots, pid: Pid) -> Vec<Client> {
    let Ok(dir) = fs::read_dir(roots.proc.join(pid.to_string()).join("fdinfo")) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if !text.contains("drm-driver:") {
            continue;
        }
        if let Some(c) = parse_fdinfo(&text)
            && seen.insert(c.key.clone())
        {
            out.push(c);
        }
    }
    out
}

pub(crate) struct DrmBackend {
    roots: Roots,
    devices: Vec<Device>,
    /// Previous accumulated engine time per `(pid, client, engine)`.
    prev: HashMap<(Pid, String, String), u64>,
    prev_at: Option<Instant>,
}

impl DrmBackend {
    /// `None` outside Linux or when no DRM device is present.
    pub(crate) fn new(skip_nvidia: bool) -> Option<Self> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        Self::with_roots(Roots::real(), skip_nvidia)
    }

    pub(crate) fn with_roots(roots: Roots, skip_nvidia: bool) -> Option<Self> {
        let devices = discover(&roots, skip_nvidia);
        // Only report on drivers we know how to read.
        (!devices.is_empty()).then_some(Self {
            roots,
            devices,
            prev: HashMap::new(),
            prev_at: None,
        })
    }

    /// Core of [`Backend::read`] with an explicit time step so tests are deterministic.
    pub(crate) fn read_with_dt(&mut self, watched: &[Pid], dt_secs: Option<f64>) -> Reading {
        let devices = self.devices.iter().map(read_device).collect();

        let mut next_prev = HashMap::new();
        let mut processes = Vec::new();
        for &pid in watched {
            let clients = scan_pid(&self.roots, pid);
            if clients.is_empty() {
                continue;
            }
            let mem: u64 = clients.iter().map(|c| c.mem).sum();
            // Busy time per engine name, summed over the pid's clients.
            let mut busy: HashMap<&str, u64> = HashMap::new();
            for c in &clients {
                for (engine, ns) in &c.engines {
                    let key = (pid, c.key.clone(), engine.clone());
                    let delta = self.prev.get(&key).map_or(0, |old| ns.saturating_sub(*old));
                    *busy.entry(engine.as_str()).or_default() += delta;
                    next_prev.insert(key, *ns);
                }
            }
            let util = dt_secs.filter(|dt| *dt > 0.0).map(|dt| {
                let max_ns = busy.values().copied().max().unwrap_or(0) as f64;
                (max_ns / (dt * 1e9) * 100.0).min(100.0)
            });
            processes.push(ProcessGpu {
                pid,
                util,
                mem: Some(mem),
            });
        }
        self.prev = next_prev;
        Reading { devices, processes }
    }
}

impl Backend for DrmBackend {
    fn name(&self) -> &'static str {
        "linux-drm"
    }

    fn read(&mut self, watched: &[Pid]) -> Reading {
        let now = Instant::now();
        let dt = self.prev_at.map(|t| now.duration_since(t).as_secs_f64());
        self.prev_at = Some(now);
        self.read_with_dt(watched, dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn amd_tree(root: &Path) {
        let c = "sys/class/drm/card0";
        write(root, &format!("{c}/device/vendor"), "0x1002\n");
        write(
            root,
            &format!("{c}/device/uevent"),
            "DRIVER=amdgpu\nPCI_SLOT_NAME=0000:03:00.0\n",
        );
        write(root, &format!("{c}/device/gpu_busy_percent"), "37\n");
        write(
            root,
            &format!("{c}/device/mem_info_vram_used"),
            "1073741824\n",
        );
        write(
            root,
            &format!("{c}/device/mem_info_vram_total"),
            "8589934592\n",
        );
        write(
            root,
            &format!("{c}/device/hwmon/hwmon3/temp1_input"),
            "61000\n",
        );
        write(
            root,
            &format!("{c}/device/hwmon/hwmon3/power1_average"),
            "95000000\n",
        );
        write(root, &format!("{c}/device/hwmon/hwmon3/pwm1"), "128\n");
        write(
            root,
            &format!("{c}/device/hwmon/hwmon3/freq1_input"),
            "1800000000\n",
        );
        // connectors must be ignored
        write(root, "sys/class/drm/card0-HDMI-A-1/status", "connected\n");
        write(root, "sys/class/drm/renderD128/x", "");
    }

    const FDINFO: &str = "pos:\t0\nflags:\t02\ndrm-driver:\tamdgpu\ndrm-client-id:\t42\ndrm-pdev:\t0000:03:00.0\n\
        drm-memory-vram:\t200 MiB\ndrm-resident-vram:\t200 MiB\ndrm-memory-gtt:\t50 MiB\n\
        drm-engine-gfx:\t1000000000 ns\ndrm-engine-compute:\t0 ns\ndrm-engine-capacity-gfx:\t1\n";

    #[test]
    fn size_units() {
        assert_eq!(parse_size("12"), Some(12));
        assert_eq!(parse_size("3 KiB"), Some(3072));
        assert_eq!(parse_size("2 MiB"), Some(2 << 20));
        assert_eq!(parse_size("1 GiB"), Some(1 << 30));
        assert_eq!(parse_size("x MiB"), None);
        assert_eq!(parse_size("5 furlongs"), None);
    }

    #[test]
    fn fdinfo_picks_one_memory_figure_and_skips_capacity() {
        let c = parse_fdinfo(FDINFO).unwrap();
        assert_eq!(c.mem, 200 << 20, "aliased keys must not be double counted");
        assert_eq!(c.engines.len(), 2);
        assert_eq!(c.key, "amdgpu/0000:03:00.0/42");
        assert!(parse_fdinfo("pos:\t0\nflags:\t02\n").is_none());
    }

    #[test]
    fn xe_style_region_keys_are_summed() {
        let c = parse_fdinfo(
            "drm-driver: xe\ndrm-client-id: 1\ndrm-resident-vram0: 10 MiB\ndrm-resident-vram1: 5 MiB\ndrm-engine-rcs: 5 ns\n",
        )
        .unwrap();
        assert_eq!(c.mem, 15 << 20);
    }

    #[test]
    fn discovers_amd_device_with_hwmon_and_ignores_connectors() {
        let dir = tempfile::tempdir().unwrap();
        amd_tree(dir.path());
        let roots = Roots {
            sys: dir.path().join("sys"),
            proc: dir.path().join("proc"),
        };
        let mut b = DrmBackend::with_roots(roots, false).expect("device");
        let r = b.read_with_dt(&[], None);
        assert_eq!(r.devices.len(), 1);
        let g = &r.devices[0];
        assert_eq!(g.id, "drm:card0");
        assert_eq!(g.vendor, GpuVendor::Amd);
        assert_eq!(g.utilization, Some(37.0));
        assert_eq!(g.mem_used, Some(1 << 30));
        assert_eq!(g.mem_total, Some(8 << 30));
        assert_eq!(g.temperature, Some(61.0));
        assert_eq!(g.power, Some(95.0));
        assert_eq!(g.clock_mhz, Some(1800.0));
        assert!((g.fan.unwrap() - 50.2).abs() < 0.1);
        assert!(g.name.contains("0000:03:00.0"));
    }

    #[test]
    fn nvidia_cards_are_skipped_when_nvml_covers_them() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "sys/class/drm/card0/device/vendor", "0x10de\n");
        let roots = Roots {
            sys: dir.path().join("sys"),
            proc: dir.path().join("proc"),
        };
        assert!(DrmBackend::with_roots(roots.clone(), true).is_none());
        assert!(DrmBackend::with_roots(roots, false).is_some());
    }

    #[test]
    fn per_process_utilisation_comes_from_engine_time_deltas() {
        let dir = tempfile::tempdir().unwrap();
        amd_tree(dir.path());
        let fd = |ns_gfx: u64| {
            FDINFO.replace(
                "drm-engine-gfx:\t1000000000 ns",
                &format!("drm-engine-gfx:\t{ns_gfx} ns"),
            )
        };
        // Two descriptors of the same client must count once.
        write(dir.path(), "proc/77/fdinfo/5", &fd(1_000_000_000));
        write(dir.path(), "proc/77/fdinfo/6", &fd(1_000_000_000));
        write(dir.path(), "proc/77/fdinfo/7", "pos: 0\nflags: 0\n");
        let roots = Roots {
            sys: dir.path().join("sys"),
            proc: dir.path().join("proc"),
        };
        let mut b = DrmBackend::with_roots(roots, false).unwrap();

        let first = b.read_with_dt(&[77, 78], None);
        assert_eq!(first.processes.len(), 1, "pid 78 has no DRM clients");
        assert_eq!(first.processes[0].util, None, "no previous sample yet");
        assert_eq!(first.processes[0].mem, Some(200 << 20));

        // 0.5 s of engine time over 1 s → 50 %.
        write(dir.path(), "proc/77/fdinfo/5", &fd(1_500_000_000));
        write(dir.path(), "proc/77/fdinfo/6", &fd(1_500_000_000));
        let second = b.read_with_dt(&[77], Some(1.0));
        let u = second.processes[0].util.unwrap();
        assert!((u - 50.0).abs() < 1e-6, "{u}");

        // Counter reset (process restarted its client) must not underflow.
        write(dir.path(), "proc/77/fdinfo/5", &fd(10));
        write(dir.path(), "proc/77/fdinfo/6", &fd(10));
        let third = b.read_with_dt(&[77], Some(1.0));
        assert_eq!(third.processes[0].util, Some(0.0));
    }
}
