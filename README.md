# Video Player

Ultra-low-power, bezel-less native Linux Video Player built in **Rust** with **GTK 4.24**, **Libadwaita 1.10**, and **GStreamer 1.28**, tuned for **Fedora / GNOME Wayland** on hybrid **AMD iGPU (`VCN` + `DMA-BUF`) + NVIDIA dGPU (`RTD3 suspended`)** laptops. Designed as the companion brother application to **Photos**, sharing the same 3-layer SOLID architecture (`core` / `hw` / `ui`), obsidian/cyan-emerald visual design, and turnkey native installation workflow.

---

## Key Features & Optimizations

- **Zero-Copy Hardware Decode (`DMA-BUF`):** Decodes H.264 (`vah264dec`), VP9 8/10-bit (`vavp9dec`), and HEVC 8/10-bit (`vah265dec`) directly on AMD VCN → `NV12` / `P010` `DMA-BUF` → `gtk4paintablesink` (`20 ms` lateness budget) → `GtkGraphicsOffload` → Wayland Direct Scanout.
- **Discrete GPU Isolation (`RTD3 suspended`):** Process-local environment + per-route user-scoped GStreamer registries (`gstreamer-registry-igpu.bin` / `gstreamer-registry-dual.bin`) keep decode and scanout strictly on AMD iGPU (`card1-eDP-1`), keeping the NVIDIA dGPU suspended at `~0 W`.
- **Throttled Software AV1 & Power-Saving PipeWire Audio:**
  - Automatically throttles `dav1ddec` (`n-threads=4` on battery / `6` on AC, `max-frame-delay=2`) to prevent 16-thread CPU boost spikes and fan spin on AV1 videos.
  - Configures `pipewiresink` with `media.role=Movie` stream properties so PipeWire/WirePlumber selects larger power-saving audio quantums.
- **True `0 Hz` Event-Driven Idle & NVMe APST Efficiency:**
  - `1 MiB` `filesrc` sequential read blocksize so NVMe APST sleeps between bursts.
  - Event-driven GStreamer actor wakeup (`0 ms` EOS/Error/Buffering/Resize latency) with an on-demand OSD timer that stops completely (`0 Hz` GTK main-thread wakeups) as soon as overlays auto-hide for Wayland Direct Scanout.
- **Native Zen 3 + Fat LTO + Optional PGO Binary:**
  - Compiled natively with `-C target-cpu=znver3`, `lto = "fat"`, `codegen-units = 1`, and `panic = "abort"` into a lean `~1.2 MiB` standalone executable, with optional two-pass PGO (`--pgo [VIDEO_OR_DIR]`) using `--test-stress` and brief 2-second local video samples.
- **Bezel-Less Playback & Ergonomic Controls:**
  - Top window buttons (`Minimize`, `Maximize`, `Fullscreen`, `Close`) auto-hide during playback and stay hidden while resizing, appearing only when moving the cursor to the top strip (`y ≤ 38px`).
  - Window height auto-fits the video's native aspect ratio to avoid black letterboxing bars.
  - Instant `0 ms` leading-edge seek bar with mouse-wheel `±5s` scrubbing and pitch-preserved speed presets (`1x`, `1.25x`, `1.5x`, `2x`).
- **Accurate Live Telemetry HUD (`I`):**
  - Dynamic `/sys/bus/pci/devices` GPU RTD3 discovery, `BAT*` multi-battery fallback, system-normalized CPU% (`16T`), and dual system + APU wattage readout on battery (`XX.XX W (sys · Y.Y W APU · Discharging)`).

---

## Modular Architecture (`core` / `hw` / `ui`)

Follows the exact same 3-layer SOLID segregation as **Photos** so OS- or GPU-specific logic is strictly isolated inside `src/hw/`:

```text
src/
├── core/                    # 100% pure, hardware-agnostic & toolkit-agnostic logic
│   └── playback.rs          # Time formatting, speed presets/snapping, UI scale clamping
├── hw/                      # Platform, GPU, DRM/VA-API, GSt ranks & sysfs telemetry layer
│   ├── policy.rs            # Display topology detection, RTD3 isolation & GSt codec ranks
│   └── telemetry.rs         # Dynamic PCI RTD3, BAT*/APU power, fan RPM, RSS, CPU & I/O
├── ui/                      # GTK 4.24 + Libadwaita + GraphicsOffload presentation layer
│   ├── mod.rs               # Window session, 0 Hz idle loop, gestures, shortcuts & QA suite
│   ├── theme.rs             # DPI-scaled obsidian/cyan-emerald GTK4 CSS & spx() scaling
│   └── win_controls.rs      # Crisp Cairo-rendered window control buttons
├── pipeline.rs              # Event-driven GStreamer actor (DMA-BUF, PipeWire, dav1d throttle)
├── config.rs                # Persistent JSON settings (~/.config/video-player/config.json)
├── lib.rs                   # Library crate root exposing core, hw, ui, config & pipeline
└── main.rs                  # Thin CLI parser & startup orchestrator
```

---

## Installation on Linux

### 1. Host Build Prerequisites (Native Fast Compilation — Zero Container Required)

Video Player defaults to fast native compilation using your host toolchain (~17s release builds):

```bash
# Fedora Linux:
sudo dnf install rust cargo pkgconf-pkg-config gtk4-devel libadwaita-devel gstreamer1-devel

# Ubuntu / Debian:
sudo apt install cargo rustc pkg-config libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev

# Arch Linux:
sudo pacman -S rust cargo pkgconf gtk4 libadwaita gstreamer
```

*(Prefer not to install host dev packages? Use the container fallback scripts `./build-container.sh` or `./install-container.sh`.)*

### 2. Turnkey Native Installation (Recommended)

```bash
chmod +x build.sh build-container.sh install.sh install-container.sh install-desktop.sh uninstall.sh
./install.sh
```

Running `./install.sh` without arguments performs a complete, container-free installation into `~/.local/`:
- Compiles the optimized native Zen 3 + Fat LTO release binary (`cargo build --release`)
- Installs the standalone executable to `~/.local/bin/video-player` (`install -p -m 755`, zero fragile symlinks)
- Installs the matched scalable SVG app icon to `~/.local/share/icons/hicolor/scalable/apps/`
- Generates, validates (`desktop-file-validate`), and installs the desktop launcher with portable `Exec` path
- Refreshes the system desktop database and GTK icon cache
- Registers **Video Player** as the default handler for common video formats (`.mp4`, `.mkv`, `.webm`, `.mov`, `.avi`, `.flv`, `.ogg`, `.ts`, `.3gp`, `.mpeg`, `.wmv`)
- Runs a post-install `--version` integrity check and notifies if `~/.local/bin` is missing from `PATH`

### 3. Optional Installer Flags

- `--pgo [PATH]`: Run two-pass PGO build using `--test-stress` + brief 2s video samples from `PATH`
- `--rebuild`: Force recompilation even if `target/release/video-player` already exists
- `--container`: Build inside an isolated Fedora container via Podman instead of host
- `--no-mime`: Skip registering Video Player as the default video handler

### 4. Custom Prefix / System-Wide Install & Desktop Refresh

```bash
# System-wide install:
PREFIX=/usr/local ./install.sh

# Re-install binary, icon, and desktop entry only:
./install-desktop.sh
```

### 5. Clean Uninstall

```bash
# Remove binary, icon, desktop launcher, and cleanly strip MIME entries (preserving fallback handlers):
./uninstall.sh

# Also delete user config and GStreamer registry cache:
./uninstall.sh --purge
```

---

## Controls & Shortcuts

| Input | Action |
| :--- | :--- |
| `Space` / `K` | Play / Pause |
| `Single-Tap` (on video) | Hide / Show all overlays (pure bezel-less view) |
| `Double-Tap` / `F` / `F11` | Toggle Fullscreen |
| `Esc` | Exit Fullscreen |
| `Left` / `Right` or `Mouse Wheel` (on seek bar) | Seek `±5s` |
| `J` / `L` | Seek `±10s` |
| `Home` / `0` | Jump to start (`00:00`) |
| `Up` / `Down` | Volume `±5%` |
| `M` | Mute / Unmute |
| `[` / `]` | Cycle speed (`1x`, `1.25x`, `1.5x`, `2x`) |
| `I` | Toggle live Hardware & Power Telemetry HUD |
| `Ctrl+O` | Open video file |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Adjust UI scale (`0.85..2.00`, default `1.20`) |
| `Ctrl+Q` / `Ctrl+W` | Quit |

---

## CLI Options & Diagnostics

```bash
video-player [OPTIONS] [FILE_OR_URI]
  --probe                  Print hardware isolation, GPU connectors & GStreamer codec ranks
  --hud                    Start with hardware telemetry HUD visible
  --allow-nvidia           Allow NVIDIA NVDEC (disabled by default on eDP-1 for RTD3 ~0W)
  --speed <RATE>           Initial speed preset (1.0, 1.25, 1.5, 2.0)
  --scale <FACTOR>         UI scale factor (0.85 .. 2.00, default 1.20)
  --auto-quit-secs <N>     Play for N seconds, print telemetry, and exit
  --test-stress            Run automated multi-codec (VP9/H.264/HEVC) & UI layout QA suite
```
