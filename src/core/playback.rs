//! Pure playback domain helpers: time formatting, speed snapping, and UI scale clamping.

pub const DEFAULT_UI_SCALE: f64 = 1.20;
pub const SPEED_PRESETS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

pub fn clamp_scale(s: f64) -> f64 {
    if !s.is_finite() {
        return DEFAULT_UI_SCALE;
    }
    s.clamp(0.85, 2.00)
}

pub fn snap_speed(rate: f64) -> f64 {
    if !rate.is_finite() {
        return 1.0;
    }
    SPEED_PRESETS
        .iter()
        .copied()
        .min_by(|a, b| (a - rate).abs().total_cmp(&(b - rate).abs()))
        .unwrap_or(1.0)
}

pub fn speed_label(rate: f64) -> String {
    if (rate - rate.round()).abs() < 0.01 {
        format!("{}x", rate as i32)
    } else {
        format!("{rate}x")
    }
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let r = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m:02}:{r:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time_cleanly() {
        assert_eq!(fmt_time(0.0), "00:00");
        assert_eq!(fmt_time(65.9), "01:05");
        assert_eq!(fmt_time(3661.0), "1:01:01");
    }

    #[test]
    fn scale_and_speed_clamping() {
        assert!((clamp_scale(0.20) - 0.85).abs() < 1e-9);
        assert!((clamp_scale(1.20) - 1.20).abs() < 1e-9);
        assert!((clamp_scale(4.50) - 2.00).abs() < 1e-9);
        assert!((clamp_scale(f64::NAN) - DEFAULT_UI_SCALE).abs() < 1e-9);

        assert!((snap_speed(0.5) - 1.0).abs() < 1e-9);
        assert!((snap_speed(1.20) - 1.25).abs() < 1e-9);
        assert!((snap_speed(1.48) - 1.5).abs() < 1e-9);
        assert!((snap_speed(2.8) - 2.0).abs() < 1e-9);
        assert!((snap_speed(f64::NAN) - 1.0).abs() < 1e-9);
        assert_eq!(speed_label(1.0), "1x");
        assert_eq!(speed_label(1.25), "1.25x");
    }
}
