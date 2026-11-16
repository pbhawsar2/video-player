mod config;
mod hw_policy;
mod pipeline;
mod telemetry;
mod theme;
mod ui;
mod win_controls;

use clap::Parser;
use gstreamer as gst;
use gstreamer::prelude::*;
use hw_policy::{GpuRoute, apply_gst_codec_ranks, apply_process_gpu_isolation};
use telemetry::read_battery_info;
use ui::{LaunchOptions, run_gui};

#[derive(Parser, Debug)]
#[command(
    name = "video-player",
    version,
    about = "Ultra-low-power AMD VA-API DMA-BUF + GNOME Wayland native Video Player"
)]
struct Cli {
    /// Local video path, file:// URI, or direct media stream URI
    input: Option<String>,

    /// Print hardware isolation + GStreamer decoder probe and exit
    #[arg(long)]
    probe: bool,

    /// Show the live hardware telemetry HUD on startup
    #[arg(long)]
    hud: bool,

    /// Allow NVIDIA NVDEC (disabled by default on eDP-1 to keep RTD3 suspended)
    #[arg(long)]
    allow_nvidia: bool,

    /// UI and text scale multiplier (default 1.20 = 120%, range 0.85..2.00)
    #[arg(long)]
    scale: Option<f64>,

    /// Initial playback speed preset (1.0, 1.25, 1.5, 2.0)
    #[arg(long)]
    speed: Option<f64>,

    /// Auto-print telemetry and exit after N seconds (for automated verification)
    #[arg(long)]
    auto_quit_secs: Option<u64>,

    /// Run automated local VP9/H.264/HEVC + speed (1x..2x) + seek stress verification
    #[arg(long)]
    test_stress: bool,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let cfg = config::AppConfig::load();
    let force_nvidia = cli.allow_nvidia || cfg.allow_nvidia_nvdec;

    // Step 1: Isolate GPU / VA-API / Vulkan / EGL / GStreamer registry BEFORE toolkit init.
    let topology = apply_process_gpu_isolation(force_nvidia);

    // Step 2: Initialize GStreamer & enforce AMD-first / NVIDIA-RTD3-safe codec ranks.
    if let Err(e) = gst::init() {
        eprintln!("Failed to initialize GStreamer: {e}");
        return std::process::ExitCode::FAILURE;
    }

    let policy = if topology.route == GpuRoute::AllowNvidiaDgpu {
        hw_policy::CodecPolicy::PreferNvidiaAv1
    } else {
        hw_policy::CodecPolicy::AutoBatteryFirst
    };
    apply_gst_codec_ranks(topology.route, policy);

    if cli.probe {
        print_probe(&topology);
        return std::process::ExitCode::SUCCESS;
    }

    let opts = LaunchOptions {
        initial_input: cli.input,
        show_hud: cli.hud || cli.test_stress,
        auto_quit_secs: cli.auto_quit_secs,
        ui_scale: cli.scale,
        initial_speed: cli.speed,
        test_stress: cli.test_stress,
        topology,
    };

    let exit = run_gui(opts);
    if exit == gtk4::glib::ExitCode::SUCCESS {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn print_probe(topo: &hw_policy::DisplayTopology) {
    let reg = gst::Registry::get();
    let (watts, bat_status) = read_battery_info();

    println!("=== Video Player Hardware & Isolation Probe ===");
    println!("Active AMD connectors   : {:?}", topo.active_amd_connectors);
    println!("Active NVIDIA connectors: {:?}", topo.active_nvidia_connectors);
    println!("Selected GPU route      : {:?}", topo.route);
    println!(
        "NVIDIA RTD3 state       : {}",
        telemetry::read_nvidia_rtd3_status()
    );
    println!("AMD iGPU state          : {}", telemetry::read_amd_status());
    if let Some(w) = watts {
        println!("Battery                 : {w:.2} W ({bat_status})");
    } else {
        println!("Battery                 : {bat_status}");
    }
    println!("Process RSS at probe    : {:.1} MiB", telemetry::read_rss_mib());
    println!("Process env isolation   :");
    for var in [
        "VK_DRIVER_FILES",
        "__EGL_VENDOR_LIBRARY_FILENAMES",
        "LIBVA_DRIVER_NAME",
        "CUDA_VISIBLE_DEVICES",
        "GST_VA_ALL_DRIVERS",
        "GST_REGISTRY",
    ] {
        println!(
            "  {var:<28}={}",
            std::env::var(var).unwrap_or_else(|_| "<unset>".into())
        );
    }
    println!("GStreamer decoder ranks (process-local):");
    for name in [
        "vavp9dec",
        "vah264dec",
        "vah265dec",
        "dav1ddec",
        "nvvp9dec",
        "nvh264dec",
        "nvav1dec",
        "scaletempo",
        "gtk4paintablesink",
        "pipewiresink",
    ] {
        let rank_str = reg
            .lookup_feature(name)
            .map(|f| format!("{:?}", f.rank()))
            .unwrap_or_else(|| "<disabled/absent>".into());
        println!("  {name:<20} -> {rank_str}");
    }
}
