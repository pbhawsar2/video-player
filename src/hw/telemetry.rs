use crate::pipeline::PlayerEngine;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct PlaybackTelemetry {
    pub decoder_name: String,
    pub audio_decoder_name: String,
    pub memory_path: String,
    pub drm_format: String,
    pub resolution: String,
    pub fps_avg: f64,
    pub rendered_frames: u64,
    pub dropped_frames: u64,
    pub nvidia_rtd3: String,
    pub amd_status: String,
    pub battery_watts: Option<f64>,
    pub battery_status: String,
    pub fan_rpm: u32,
    pub rss_mib: f64,
    pub cpu_percent: f64,
    pub cpu_threads: usize,
    pub read_syscalls: u64,
    pub open_ready_ms: u64,
}

pub struct CpuSampler {
    last_ticks: u64,
    last_rendered: u64,
    last_time: Instant,
    session_start: Option<Instant>,
    last_fps: f64,
    last_cpu: f64,
    num_cpus: usize,
}

impl CpuSampler {
    pub fn new() -> Self {
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get().max(1))
            .unwrap_or(1);
        Self {
            last_ticks: read_self_cpu_ticks().unwrap_or(0),
            last_rendered: 0,
            last_time: Instant::now(),
            session_start: None,
            last_fps: 0.0,
            last_cpu: 0.0,
            num_cpus,
        }
    }

    pub fn num_cpus(&self) -> usize {
        self.num_cpus
    }

    pub fn sample(&mut self, rendered_frames: u64) -> (f64, f64) {
        let now = Instant::now();
        if rendered_frames == 0 {
            self.session_start = None;
            self.last_rendered = 0;
            self.last_fps = 0.0;
        } else if self.session_start.is_none() || rendered_frames < self.last_rendered {
            self.session_start = Some(now);
        }

        let elapsed = now.duration_since(self.last_time).as_secs_f64();
        if elapsed < 0.15 {
            let fallback_fps = if self.last_fps > 0.0 {
                self.last_fps
            } else if let Some(start) = self.session_start {
                let s_el = now.duration_since(start).as_secs_f64();
                if s_el > 0.2 {
                    rendered_frames as f64 / s_el
                } else {
                    0.0
                }
            } else {
                0.0
            };
            return (self.last_cpu, fallback_fps);
        }

        let ticks = read_self_cpu_ticks().unwrap_or(self.last_ticks);
        let dticks = ticks.saturating_sub(self.last_ticks) as f64;
        let dframes = rendered_frames.saturating_sub(self.last_rendered) as f64;
        self.last_ticks = ticks;
        self.last_rendered = rendered_frames;
        self.last_time = now;
        let raw_pct = (dticks / 100.0 / elapsed) * 100.0;
        self.last_cpu = (raw_pct / self.num_cpus as f64).clamp(0.0, 100.0);
        let fps = dframes / elapsed;
        if fps > 0.0 {
            self.last_fps = fps;
        } else if rendered_frames == 0 {
            self.last_fps = 0.0;
        } else if let Some(start) = self.session_start {
            let s_el = now.duration_since(start).as_secs_f64();
            if s_el > 0.2 {
                self.last_fps = rendered_frames as f64 / s_el;
            }
        }
        (self.last_cpu, self.last_fps)
    }
}

fn read_self_cpu_ticks() -> Option<u64> {
    let raw = fs::read_to_string("/proc/self/stat").ok()?;
    let rparen = raw.rfind(')')?;
    let mut fields = raw[rparen + 2..].split_whitespace();
    let utime: u64 = fields.nth(11)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    Some(utime + stime)
}

fn find_pci_gpu_runtime_status(vendor_hex: &str) -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/bus/pci/devices").ok()?;
    for entry in entries.flatten() {
        let base = entry.path();
        let vendor = read_trimmed(&base.join("vendor").to_string_lossy())?;
        if !vendor.eq_ignore_ascii_case(vendor_hex) {
            continue;
        }
        // Match Display Controller PCI class (0x03xxxx: VGA / 3D / Display)
        if let Some(cls) = read_trimmed(&base.join("class").to_string_lossy()) {
            if cls.starts_with("0x03") {
                let st = base.join("power/runtime_status");
                if st.exists() {
                    return Some(st);
                }
            }
        }
    }
    None
}

pub fn read_nvidia_rtd3_status() -> String {
    static NV_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    let path = NV_PATH.get_or_init(|| find_pci_gpu_runtime_status("0x10de"));
    path.as_deref()
        .and_then(|p| read_trimmed(&p.to_string_lossy()))
        .unwrap_or_else(|| "unknown".into())
}

pub fn read_amd_status() -> String {
    static AMD_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    let path = AMD_PATH.get_or_init(|| find_pci_gpu_runtime_status("0x1002"));
    path.as_deref()
        .and_then(|p| read_trimmed(&p.to_string_lossy()))
        .unwrap_or_else(|| "unknown".into())
}

fn find_hwmon_file(driver_name: &str, filename: &str) -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in entries.flatten() {
        let base = entry.path();
        if let Some(name) = read_trimmed(&base.join("name").to_string_lossy()) {
            if name == driver_name {
                let target = base.join(filename);
                if target.exists() {
                    return Some(target);
                }
            }
        }
    }
    None
}

/// Read live AMD APU package power (PPT in Watts) from `amdgpu` hwmon sensor.
/// Works accurately on both AC power (`Charging`/`Full`) and Battery (`Discharging`).
pub fn read_apu_watts() -> Option<f64> {
    static AMDGPU_PWR_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    let path = AMDGPU_PWR_PATH
        .get_or_init(|| {
            find_hwmon_file("amdgpu", "power1_input")
                .or_else(|| find_hwmon_file("amdgpu", "power1_average"))
        })
        .as_ref()?;
    read_trimmed(&path.to_string_lossy())
        .and_then(|v| v.parse::<f64>().ok())
        .map(|uw| uw / 1_000_000.0)
}

/// Read maximum hardware cooling fan RPM (`hp` hwmon `fan1_input` / `fan2_input`).
pub fn read_fan_rpm() -> u32 {
    static FAN_PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    let paths = FAN_PATHS.get_or_init(|| {
        let mut out = Vec::new();
        if let Some(p1) = find_hwmon_file("hp", "fan1_input") {
            out.push(p1);
        }
        if let Some(p2) = find_hwmon_file("hp", "fan2_input") {
            out.push(p2);
        }
        out
    });
    paths
        .iter()
        .filter_map(|p| read_trimmed(&p.to_string_lossy())?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

fn battery_dir() -> Option<&'static PathBuf> {
    static BAT_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    BAT_DIR
        .get_or_init(|| {
            let bat0 = PathBuf::from("/sys/class/power_supply/BAT0");
            if bat0.join("status").exists() {
                return Some(bat0);
            }
            fs::read_dir("/sys/class/power_supply")
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("BAT"))
                        && p.join("status").exists()
                })
        })
        .as_ref()
}

/// Returns `(effective_watts, status_label)`.
/// When discharging on battery, uses `BAT*/power_now`; when on AC (`Charging`/`Full`),
/// reports `amdgpu` APU package power instead of battery charging current.
pub fn read_battery_info() -> (Option<f64>, String) {
    let Some(bat) = battery_dir() else {
        return (read_apu_watts(), "APU · AC".into());
    };
    let status = read_trimmed(&bat.join("status").to_string_lossy()).unwrap_or_else(|| "AC".into());
    if status.eq_ignore_ascii_case("Discharging") {
        let bat_w = read_trimmed(&bat.join("power_now").to_string_lossy())
            .and_then(|v| v.parse::<f64>().ok())
            .map(|uw| uw / 1_000_000.0)
            .filter(|&w| w > 0.5);
        let apu_w = read_apu_watts();
        match (bat_w, apu_w) {
            (Some(bw), Some(aw)) => (Some(bw), format!("sys · {aw:.1} W APU · Discharging")),
            (Some(bw), None) => (Some(bw), "sys · Discharging".into()),
            (None, Some(aw)) => (Some(aw), "APU · Discharging".into()),
            (None, None) => (None, "Discharging".into()),
        }
    } else {
        (read_apu_watts(), format!("APU · AC {status}"))
    }
}

pub fn read_rss_mib() -> f64 {
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return 0.0;
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb_str = rest.trim().split_whitespace().next().unwrap_or("0");
            if let Ok(kb) = kb_str.parse::<f64>() {
                return kb / 1024.0;
            }
        }
    }
    0.0
}

pub fn read_io_syscalls() -> u64 {
    let Ok(raw) = fs::read_to_string("/proc/self/io") else {
        return 0;
    };
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("syscr:") {
            if let Ok(v) = rest.trim().parse::<u64>() {
                return v;
            }
        }
    }
    0
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Lock-free / cached telemetry readout safe to run on the GTK main thread.
pub fn collect_telemetry(engine: &PlayerEngine, cpu_sampler: &mut CpuSampler) -> PlaybackTelemetry {
    let (battery_watts, battery_status) = read_battery_info();
    let (vdec, adec) = engine.decoders_snapshot();
    let caps = engine.caps_snapshot();
    let sink = engine.video_sink();

    let mut t = PlaybackTelemetry {
        nvidia_rtd3: read_nvidia_rtd3_status(),
        amd_status: read_amd_status(),
        battery_watts,
        battery_status,
        fan_rpm: read_fan_rpm(),
        rss_mib: read_rss_mib(),
        cpu_percent: 0.0,
        read_syscalls: read_io_syscalls(),
        open_ready_ms: engine.open_ready_ms(),
        decoder_name: vdec,
        audio_decoder_name: adec,
        memory_path: if caps.memory_path.is_empty() {
            "none".into()
        } else {
            caps.memory_path
        },
        drm_format: if caps.drm_format.is_empty() {
            "-".into()
        } else {
            caps.drm_format
        },
        resolution: if caps.resolution.is_empty() {
            "-".into()
        } else {
            caps.resolution
        },
        ..Default::default()
    };

    if sink.has_property("stats") {
        let stats = sink.property::<gst::Structure>("stats");
        t.fps_avg = stats.get::<f64>("average-rate").unwrap_or(0.0);
        t.rendered_frames = stats.get::<u64>("rendered").unwrap_or(0);
        t.dropped_frames = stats.get::<u64>("dropped").unwrap_or(0);
    }

    let (cpu_pct, measured_fps) = cpu_sampler.sample(t.rendered_frames);
    t.cpu_percent = cpu_pct;
    t.cpu_threads = cpu_sampler.num_cpus();
    if t.fps_avg < 1.0 && measured_fps > 0.0 {
        t.fps_avg = measured_fps;
    }

    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_sampler_preserves_fps_across_fast_polls() {
        let mut sampler = CpuSampler::new();
        let (cpu0, fps0) = sampler.sample(0);
        assert_eq!(fps0, 0.0);
        assert!(cpu0 >= 0.0);

        std::thread::sleep(std::time::Duration::from_millis(220));
        let (_cpu1, fps1) = sampler.sample(15);
        assert!(fps1 > 10.0, "expected measured fps > 10, got {fps1}");

        let (_cpu2, fps2) = sampler.sample(16);
        assert!((fps2 - fps1).abs() < 1e-6);

        let (_cpu3, fps3) = sampler.sample(0);
        assert_eq!(fps3, 0.0);
    }
}
