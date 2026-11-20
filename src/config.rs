pub use crate::core::playback::{DEFAULT_UI_SCALE, SPEED_PRESETS, clamp_scale, snap_speed};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_RECENT_FILES: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// UI & text scale multiplier (default 1.20 = 120%, clamped 0.85..=2.00).
    pub ui_scale: f64,
    /// Saved audio volume (0.0..=1.5).
    pub volume: f64,
    /// Saved playback speed preset (1.0, 1.25, 1.5, 2.0).
    pub playback_speed: f64,
    /// Allow NVIDIA NVDEC when on eDP-1 (false keeps RTX 3050 Ti in RTD3 suspended).
    pub allow_nvidia_nvdec: bool,
    /// Recent local video paths for quick resume on the welcome view.
    pub recent_files: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let env_scale = std::env::var("VIDEO_PLAYER_SCALE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_UI_SCALE);
        Self {
            ui_scale: clamp_scale(env_scale),
            volume: 0.85,
            playback_speed: 1.0,
            allow_nvidia_nvdec: false,
            recent_files: Vec::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("video-player");
    let _ = fs::create_dir_all(&dir);
    dir
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_dir().join("config.json");
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut cfg: Self = serde_json::from_str(&raw).unwrap_or_default();
        cfg.ui_scale = clamp_scale(cfg.ui_scale);
        cfg.volume = if cfg.volume.is_finite() {
            cfg.volume.clamp(0.0, 1.5)
        } else {
            0.85
        };
        cfg.playback_speed = snap_speed(cfg.playback_speed);
        if let Ok(v) = std::env::var("VIDEO_PLAYER_SCALE") {
            if let Ok(parsed) = v.parse::<f64>() {
                cfg.ui_scale = clamp_scale(parsed);
            }
        }
        cfg
    }

    pub fn push_recent(&mut self, path: &str) {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return;
        }
        self.recent_files.retain(|p| p != trimmed);
        self.recent_files.insert(0, trimmed.to_string());
        self.recent_files.truncate(MAX_RECENT_FILES);
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_dir().join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        fs::write(&path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_speed_clamping() {
        assert!((AppConfig::default().ui_scale - DEFAULT_UI_SCALE).abs() < 1e-9);
        assert!((clamp_scale(0.20) - 0.85).abs() < 1e-9);
        assert!((clamp_scale(1.20) - 1.20).abs() < 1e-9);
        assert!((clamp_scale(4.50) - 2.00).abs() < 1e-9);
        assert!((clamp_scale(f64::NAN) - DEFAULT_UI_SCALE).abs() < 1e-9);

        assert!((snap_speed(0.5) - 1.0).abs() < 1e-9);
        assert!((snap_speed(1.20) - 1.25).abs() < 1e-9);
        assert!((snap_speed(1.48) - 1.5).abs() < 1e-9);
        assert!((snap_speed(2.8) - 2.0).abs() < 1e-9);
        assert!((snap_speed(f64::NAN) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn recent_files_dedupes_and_caps() {
        let mut cfg = AppConfig::default();
        for i in 0..8 {
            cfg.push_recent(&format!("/tmp/vid{i}.mp4"));
        }
        assert_eq!(cfg.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(cfg.recent_files[0], "/tmp/vid7.mp4");

        cfg.push_recent("/tmp/vid3.mp4");
        assert_eq!(cfg.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(cfg.recent_files[0], "/tmp/vid3.mp4");
    }
}
