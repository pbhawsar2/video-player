# Video Player

Ultra-low-power, bezel-less native Linux Video Player built in **Rust** with **GTK 4.24**, **Libadwaita 1.10**, and **GStreamer 1.28**, tuned specifically for **Fedora / GNOME Wayland** on hybrid **AMD iGPU (`VCN` + `DMA-BUF`) + NVIDIA dGPU (`RTD3 suspended`)** laptops.

---

## Key Features & Optimizations

- **Zero-Copy Hardware Decode (`DMA-BUF`):** Decodes H.264 (`vah264dec`), VP9 (`vavp9dec`), and HEVC/H.265 (`vah265dec`) directly on AMD VCN → `NV12` / `P010` `DMA-BUF` → `gtk4paintablesink` → `GtkGraphicsOffload` → Wayland Direct Scanout.
- **Discrete GPU Isolation (`RTD3 suspended`):** Process-local environment + user-scoped GStreamer registry lock decode and rendering onto `/dev/dri/renderD128` (`card1-eDP-1`), keeping the NVIDIA dGPU (`01:00.0`) suspended at `~0 W`.
- **NVMe SSD & Background Wakeup Optimization:**
  - `1 MiB` `filesrc` read blocksize + `8 MiB` ring buffer for local `file://` media so VFS readahead batches drive I/O.
  - `0 Hz` actor polling and zero GTK widget updates whenever playback controls auto-hide.
- **Bezel-Less Playback & Ergonomic Controls:**
  - Top window buttons (`Minimize`, `Maximize`, `Fullscreen`, `Close`) auto-hide during playback and stay hidden while resizing the window, appearing only when moving the cursor to the top strip (`y ≤ 38px`).
  - Window height auto-fits the video's native aspect ratio to avoid black letterboxing bars.
  - Instant `0 ms` leading-edge seek bar with mouse-wheel `±5s` scrubbing and pitch-preserved speed presets (`1x`, `1.25x`, `1.5x`, `2x`).
- **GNOME Files Integration:** Single-instance `HANDLES_OPEN` `.desktop` launcher with full `video/*` MIME associations.

---

## Installation

### Option 1: One-Command Containerized Build & User Install (Recommended, Zero Host Changes)
Uses rootless `podman` and [`Containerfile`](Containerfile) so no `-devel` packages need to be installed on your host system:

```bash
chmod +x build.sh install-desktop.sh install.sh uninstall.sh
./install.sh
```

To also set **Video Player** as your default handler for `.mp4`, `.mkv`, `.webm`, `.mov`, `.avi`, and `.mpeg` in GNOME Files:

```bash
./install.sh --default
```

### Option 2: Native Host Build (If `gtk4-devel`, `libadwaita-devel`, `gstreamer1-devel`, and `cargo` are installed)

```bash
./install.sh --native
```

### Step-by-Step Manual Install
```bash
./build.sh              # Builds target/release/video-player inside Fedora 45 container
./install-desktop.sh    # Symlinks ~/.local/bin/video-player and installs icon + .desktop launcher
```

### Uninstall
```bash
./uninstall.sh
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
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Adjust UI scale (default `120%`) |
| `Ctrl+Q` / `Ctrl+W` | Quit |

---

## CLI Options

```bash
video-player [OPTIONS] [FILE_OR_URI]
  --hud                    Start with hardware telemetry HUD visible
  --speed <RATE>           Initial speed preset (1.0, 1.25, 1.5, 2.0)
  --scale <FACTOR>         UI scale factor (0.8 .. 1.6)
  --auto-quit-secs <N>     Play for N seconds, print telemetry, and exit
  --test-stress            Run automated multi-codec & UI layout QA suite
```

---

## Repository Layout

```text
├── assets/
│   ├── io.github.videoplayer.App.desktop   # GNOME launcher & MIME associations
│   └── io.github.videoplayer.App.svg       # Scalable app icon
├── src/
│   ├── config.rs        # User settings, recent files, speed/scale presets
│   ├── hw_policy.rs     # Display topology detection & NVIDIA RTD3 isolation
│   ├── main.rs          # CLI parsing & startup policy
│   ├── pipeline.rs      # GStreamer actor, 1 MiB SSD blocksize, non-blocking seek
│   ├── telemetry.rs     # AMD APU PPT power, fan RPM, DMA-BUF, CPU & I/O stats
│   ├── theme.rs         # DPI-scaled dark glassmorphic GTK4 CSS
│   ├── ui.rs            # Window, bezel-less controls, gestures & stress QA
│   └── win_controls.rs  # Crisp Cairo-rendered window control buttons
├── build.sh             # Containerized (or --native) release builder
├── Containerfile        # Reproducible Fedora 45 Rust + GTK4 + GStreamer build env
├── install-desktop.sh   # User-local ~/.local installation script
├── install.sh           # One-command build + install (+ optional --default MIME)
├── uninstall.sh         # Clean user-local uninstaller
├── Cargo.toml
└── Cargo.lock
```
