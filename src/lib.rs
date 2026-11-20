//! Video Player — Native Hardware-Centric Ultra-Low-Power Video Player Library Crate
//!
//! ### Modular 3-Layer Architecture (Brother Architecture to Photos)
//! - [`core`]: Pure, hardware-agnostic & toolkit-agnostic playback domain helpers.
//! - [`hw`]: Isolated device & hardware acceleration layer (AMD VCN / NVIDIA RTD3 isolation,
//!   GStreamer hardware codec ranking, and dynamic sysfs/procfs/hwmon telemetry).
//! - [`ui`]: GTK 4.24 + Libadwaita + `GraphicsOffload` zero-copy Wayland presentation layer.

pub mod config;
pub mod core;
pub mod hw;
pub mod pipeline;
pub mod ui;

pub use hw::{policy as hw_policy, telemetry};
