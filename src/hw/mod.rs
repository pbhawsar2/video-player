//! Hardware, GPU, DRM/VA-API, GStreamer rank, and Linux sysfs/procfs telemetry isolation layer.
//!
//! All platform- and GPU-specific code lives strictly inside `src/hw/` so porting to another OS
//! or GPU architecture never requires modifying `src/core/` or `src/ui/`.

pub mod policy;
pub mod telemetry;
