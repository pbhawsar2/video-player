use crate::config::clamp_scale;

pub fn spx(base_px: f64, scale: f64) -> i32 {
    (base_px * clamp_scale(scale)).round().max(1.0) as i32
}

/// Crisp, anti-aliased obsidian & cyan/emerald GTK4 CSS tuned for 1x 1080p Wayland displays:
/// ultra-slim top header bar + comfortable, user-friendly bottom playback & seek controls.
pub fn build_css(scale: f64) -> String {
    let s = clamp_scale(scale);
    let pt = |base: f64| -> String { format!("{:.2}pt", base * s) };
    let px = |base: f64| -> i32 { spx(base, s) };

    let f_brand = pt(10.0);
    let f_body = pt(9.2);
    let f_pill = pt(8.6);
    let f_mono_sm = pt(8.1);
    let f_heading = pt(14.0);
    let f_hud = pt(8.4);

    let pad_hdr_v = px(3.0);
    let pad_hdr_h = px(8.0);
    let pad_card = px(20.0);
    let hdr_btn_h = px(22.0);
    let osd_btn_h = px(28.0);
    let pad_btn_h = px(8.0);

    format!(
        r#"
window, .root-canvas {{
    background-color: #000000;
    color: #F8FAFC;
    font-family: "Adwaita Sans", sans-serif;
    font-size: {f_body};
}}

.welcome-bg {{
    background-color: #080C14;
}}

.top-header {{
    background-color: rgba(12, 18, 30, 0.92);
    border-bottom: 1px solid rgba(148, 163, 184, 0.10);
    padding: {pad_hdr_v}px {pad_hdr_h}px;
}}

.top-header.playing-hdr {{
    background-color: rgba(8, 12, 20, 0.78);
    border-bottom: none;
}}

.brand-title {{
    color: #F8FAFC;
    font-weight: 800;
    font-size: {f_brand};
    letter-spacing: 0.2px;
}}

.now-playing-title {{
    color: #CBD5E1;
    font-size: {f_pill};
    font-weight: 600;
}}

.video-canvas {{
    background-color: #000000;
}}

.welcome-card {{
    background-color: #0E1524;
    border: 1px solid rgba(148, 163, 184, 0.13);
    border-radius: 14px;
    padding: {pad_card}px;
}}

.welcome-title {{
    color: #F8FAFC;
    font-size: {f_heading};
    font-weight: 800;
}}

.welcome-sub {{
    color: #94A3B8;
    font-size: {f_body};
}}

button.vp-btn, menubutton.vp-speed > button {{
    background-image: none;
    box-shadow: none;
    outline: none;
    margin: 0;
    min-height: {hdr_btn_h}px;
    min-width: 0;
    padding: 0 {pad_btn_h}px;
    border-radius: 6px;
    border: none;
    background-color: #131C2C;
    color: #E2E8F0;
    font-size: {f_pill};
    font-weight: 600;
    transition: background-color 120ms ease, color 120ms ease;
}}

button.vp-btn:hover, menubutton.vp-speed > button:hover {{
    background-color: #1C2940;
    color: #FFFFFF;
}}

button.vp-btn.hdr-open {{
    background-color: rgba(56, 189, 248, 0.15);
    color: #7DD3FC;
    border: none;
}}

button.vp-btn.hdr-open:hover {{
    background-color: rgba(56, 189, 248, 0.26);
    color: #E0F2FE;
}}

button.vp-btn.primary-cta {{
    background-color: #0284C7;
    color: #FFFFFF;
    border: none;
    padding: 0 {px_16}px;
    min-height: {px_32}px;
    border-radius: 7px;
}}

button.vp-btn.primary-cta:hover {{
    background-color: #0EA5E9;
}}

button.vp-btn.emerald-chip {{
    background-color: rgba(52, 211, 153, 0.11);
    color: #34D399;
    border: none;
    font-family: "Adwaita Mono", monospace;
    font-size: {f_mono_sm};
    padding: 0 {px_8}px;
}}

button.vp-btn.emerald-chip:hover {{
    background-color: rgba(52, 211, 153, 0.21);
}}

button.vp-btn.amber-chip {{
    background-color: rgba(251, 191, 36, 0.11);
    color: #FBBF24;
    border: none;
    font-family: "Adwaita Mono", monospace;
    font-size: {f_mono_sm};
    padding: 0 {px_8}px;
}}

button.vp-btn.amber-chip:hover {{
    background-color: rgba(251, 191, 36, 0.21);
}}

button.osd-icon-btn {{
    background-image: none;
    box-shadow: none;
    outline: none;
    margin: 0;
    min-height: {osd_btn_h}px;
    min-width: {osd_btn_h}px;
    padding: 0;
    border: none;
    border-radius: 7px;
    background-color: rgba(255, 255, 255, 0.06);
    color: #F1F5F9;
}}

button.osd-icon-btn:hover {{
    background-color: rgba(56, 189, 248, 0.22);
    color: #FFFFFF;
}}

menubutton.vp-speed > button {{
    min-height: {osd_btn_h}px;
    background-color: rgba(255, 255, 255, 0.10);
    color: #F1F5F9;
    font-family: "Adwaita Mono", monospace;
    font-size: {f_pill};
    font-weight: 700;
    padding: 0 {px_9}px;
}}

menubutton.vp-speed > button:hover {{
    background-color: rgba(56, 189, 248, 0.24);
    color: #FFFFFF;
}}

popover.speed-popover > contents {{
    background-color: #0E1524;
    border: 1px solid rgba(148, 163, 184, 0.16);
    border-radius: 9px;
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55);
}}

button.speed-choice {{
    background-image: none;
    box-shadow: none;
    outline: none;
    border: none;
    min-height: {hdr_btn_h}px;
    min-width: 0;
    padding: 0 {px_9}px;
    border-radius: 6px;
    background-color: transparent;
    color: #94A3B8;
    font-family: "Adwaita Mono", monospace;
    font-size: {f_mono_sm};
    font-weight: 700;
}}

button.speed-choice:hover {{
    background-color: rgba(255, 255, 255, 0.08);
    color: #F8FAFC;
}}

button.speed-choice.active {{
    background-color: rgba(56, 189, 248, 0.20);
    color: #38BDF8;
}}

button.recent-btn {{
    background-image: none;
    box-shadow: none;
    outline: none;
    border: none;
    border-radius: 7px;
    padding: 6px 10px;
    min-height: 0;
    background-color: rgba(255, 255, 255, 0.03);
    color: #E2E8F0;
    font-size: {f_pill};
    font-weight: 600;
}}

button.recent-btn:hover {{
    background-color: rgba(56, 189, 248, 0.14);
    color: #FFFFFF;
}}

.osd-bar {{
    background-color: rgba(8, 12, 20, 0.90);
    border-radius: 11px;
    border: 1px solid rgba(148, 163, 184, 0.15);
    padding: 6px 12px;
}}

.hud-pill {{
    background-color: rgba(8, 12, 20, 0.90);
    border-radius: 9px;
    border: 1px solid rgba(52, 211, 153, 0.24);
    padding: 8px 12px;
    font-family: "Adwaita Mono", monospace;
    font-size: {f_hud};
    font-weight: 600;
    color: #F1F5F9;
}}

.mono-stat {{
    font-family: "Adwaita Mono", monospace;
    font-variant-numeric: tabular-nums;
    font-size: {f_pill};
    font-weight: 700;
    color: #F1F5F9;
}}

.win-sep {{
    background-color: rgba(148, 163, 184, 0.16);
    min-width: 1px;
    min-height: 14px;
}}

scale.seek-bar trough {{
    background-color: rgba(148, 163, 184, 0.26);
    border-radius: 999px;
    min-height: 6px;
}}

scale.seek-bar highlight {{
    background-color: #38BDF8;
    border-radius: 999px;
    min-height: 6px;
}}

scale.seek-bar slider {{
    background-color: #F8FAFC;
    border: 2px solid #38BDF8;
    border-radius: 999px;
    min-width: 15px;
    min-height: 15px;
    margin: -5px;
}}

scale.vol-bar trough {{
    background-color: rgba(148, 163, 184, 0.24);
    border-radius: 999px;
    min-height: 5px;
}}

scale.vol-bar highlight {{
    background-color: #38BDF8;
    border-radius: 999px;
    min-height: 5px;
}}

scale.vol-bar slider {{
    background-color: #F8FAFC;
    border: 2px solid #38BDF8;
    border-radius: 999px;
    min-width: 12px;
    min-height: 12px;
    margin: -4px;
}}

scrollbar slider {{
    background-color: #273449;
    border-radius: 6px;
    min-width: 6px;
    min-height: 28px;
}}

scrollbar slider:hover {{
    background-color: #38BDF8;
}}
"#,
        px_16 = px(16.0),
        px_32 = px(32.0),
        px_9 = px(9.0),
        px_8 = px(8.0),
    )
}
