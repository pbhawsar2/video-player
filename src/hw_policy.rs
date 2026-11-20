use gstreamer as gst;
use gstreamer::glib::translate::{FromGlib, IntoGlib};
use gstreamer::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuRoute {
    /// Keep NVIDIA RTX 3050 Ti in RTD3 suspended (~0W); decode & scanout strictly on AMD iGPU.
    AmdIgpuOnly,
    /// Active display is wired to NVIDIA dGPU (e.g. HDMI-A-1) or user explicitly forced NVDEC.
    AllowNvidiaDgpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecPolicy {
    /// Prefer VP9 > H.264 on AMD VCN (zero-copy DMA-BUF); fallback AV1 to throttled CPU dav1ddec.
    AutoBatteryFirst,
    /// Allow NVIDIA NVDEC when available (useful on AC power / 4K60+ AV1 / HDMI-A-1).
    PreferNvidiaAv1,
}

#[derive(Debug, Clone)]
pub struct DisplayTopology {
    pub active_amd_connectors: Vec<String>,
    pub active_nvidia_connectors: Vec<String>,
    pub route: GpuRoute,
}

/// Inspect `/sys/class/drm` read-only to find which GPU drives connected, enabled displays.
pub fn detect_display_topology() -> DisplayTopology {
    let mut active_amd = Vec::new();
    let mut active_nv = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.contains('-') || !name.starts_with("card") {
                continue;
            }
            let base = entry.path();
            let status = fs::read_to_string(base.join("status"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let enabled = fs::read_to_string(base.join("enabled"))
                .unwrap_or_default()
                .trim()
                .to_string();
            if status != "connected" && enabled != "enabled" {
                continue;
            }
            let card_name = name.split('-').next().unwrap_or("");
            let vendor_path = Path::new("/sys/class/drm")
                .join(card_name)
                .join("device/vendor");
            let vendor = fs::read_to_string(vendor_path)
                .unwrap_or_default()
                .trim()
                .to_lowercase();
            if vendor == "0x1002" {
                active_amd.push(name);
            } else if vendor == "0x10de" {
                active_nv.push(name);
            }
        }
    }

    let route = if active_nv.is_empty() {
        GpuRoute::AmdIgpuOnly
    } else {
        GpuRoute::AllowNvidiaDgpu
    };

    DisplayTopology {
        active_amd_connectors: active_amd,
        active_nvidia_connectors: active_nv,
        route,
    }
}

fn app_gst_registry_path(route: GpuRoute) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("video-player");
    let _ = fs::create_dir_all(&dir);
    match route {
        GpuRoute::AmdIgpuOnly => dir.join("gstreamer-registry-igpu.bin"),
        GpuRoute::AllowNvidiaDgpu => dir.join("gstreamer-registry-dual.bin"),
    }
}

/// Apply strictly process-scoped GPU isolation BEFORE GTK4 / Vulkan / EGL / GStreamer initialize.
/// Never modifies any Fedora system config or shared GStreamer registry cache.
pub fn apply_process_gpu_isolation(force_nvidia: bool) -> DisplayTopology {
    let mut topo = detect_display_topology();
    if force_nvidia {
        topo.route = GpuRoute::AllowNvidiaDgpu;
    }

    // Isolate this app's GStreamer registry cache per GPU route from the rest of the user session.
    let reg_path = app_gst_registry_path(topo.route);

    // SAFETY: Called on main thread before spawning GTK/GStreamer worker threads.
    unsafe {
        std::env::set_var("GST_REGISTRY", &reg_path);

        if topo.route == GpuRoute::AmdIgpuOnly {
            // Prevent Vulkan loader / GTK GSK from probing nvidia_icd.x86_64.json and waking dGPU.
            if Path::new("/usr/share/vulkan/icd.d/radeon_icd.x86_64.json").exists() {
                std::env::set_var(
                    "VK_DRIVER_FILES",
                    "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json",
                );
                std::env::set_var(
                    "VK_ICD_FILENAMES",
                    "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json",
                );
            }
            // Prevent GLVND EGL from probing 10_nvidia.json.
            if Path::new("/usr/share/glvnd/egl_vendor.d/50_mesa.json").exists() {
                std::env::set_var(
                    "__EGL_VENDOR_LIBRARY_FILENAMES",
                    "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
                );
            }
            // Block libva from dlopening nvidia_drv_video.so + libcuda.so when scanning renderD129.
            std::env::set_var("LIBVA_DRIVER_NAME", "radeonsi");
            // Prevent CUDA initialization inside nvcodec plugin scanner.
            std::env::set_var("CUDA_VISIBLE_DEVICES", "");
            std::env::set_var("__NV_PRIME_RENDER_OFFLOAD", "0");
            std::env::set_var("GST_VA_ALL_DRIVERS", "0");
        }
    }

    topo
}

/// Enforce hardware-aware decoder ranking after `gst::init()`.
/// On Fedora 45, `nvav1dec` (257) outranks `dav1ddec` (256) and `nvh264dec` ties `vah264dec` (257).
/// In `AmdIgpuOnly` mode we zero out NVIDIA decoders so the dGPU remains in RTD3 suspended (~0W).
pub fn apply_gst_codec_ranks(route: GpuRoute, policy: CodecPolicy) {
    let reg = gst::Registry::get();

    let nv_features = [
        "nvav1dec",
        "nvh264dec",
        "nvh265dec",
        "nvvp8dec",
        "nvvp9dec",
        "nvjpegdec",
        "nvmpeg2videodec",
        "nvmpeg4videodec",
        "nvmpegvideodec",
        "vulkanav1device1dec",
        "vulkanh264device1dec",
        "vulkanh265device1dec",
        "vulkanvp9device1dec",
    ];

    let amd_va_features = ["vavp9dec", "vah264dec", "vah265dec"];
    let cpu_fallback_features = ["dav1ddec", "vp9dec", "avdec_h264", "avdec_h265"];

    let allow_nv = route == GpuRoute::AllowNvidiaDgpu || policy == CodecPolicy::PreferNvidiaAv1;

    if !allow_nv {
        for name in nv_features {
            if let Some(f) = reg.lookup_feature(name) {
                f.set_rank(gst::Rank::NONE);
            }
        }
        if let Some(plugin) = reg.find_plugin("nvcodec") {
            reg.remove_plugin(&plugin);
        }
        let high_rank = unsafe { gst::Rank::from_glib(gst::Rank::PRIMARY.into_glib() + 64) };
        for name in amd_va_features {
            if let Some(f) = reg.lookup_feature(name) {
                f.set_rank(high_rank);
            }
        }
        if let Some(f) = reg.lookup_feature("dav1ddec") {
            f.set_rank(gst::Rank::PRIMARY);
        }
        for name in &cpu_fallback_features[1..] {
            if let Some(f) = reg.lookup_feature(name) {
                f.set_rank(gst::Rank::SECONDARY);
            }
        }
    } else {
        let nv_rank = unsafe { gst::Rank::from_glib(gst::Rank::PRIMARY.into_glib() + 64) };
        let amd_rank = unsafe { gst::Rank::from_glib(gst::Rank::PRIMARY.into_glib() + 32) };
        for name in ["nvav1dec", "nvvp9dec", "nvh265dec", "nvh264dec"] {
            if let Some(f) = reg.lookup_feature(name) {
                f.set_rank(nv_rank);
            }
        }
        for name in amd_va_features {
            if let Some(f) = reg.lookup_feature(name) {
                f.set_rank(amd_rank);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_cache_stays_user_scoped() {
        let p_igpu = app_gst_registry_path(GpuRoute::AmdIgpuOnly);
        let p_dual = app_gst_registry_path(GpuRoute::AllowNvidiaDgpu);
        assert!(p_igpu.ends_with("video-player/gstreamer-registry-igpu.bin"));
        assert!(p_dual.ends_with("video-player/gstreamer-registry-dual.bin"));
    }
}
