pub mod theme;
pub mod win_controls;

use crate::config::{AppConfig, SPEED_PRESETS, clamp_scale, snap_speed};
use crate::core::playback::{fmt_time, speed_label};
use crate::hw_policy::{DisplayTopology, GpuRoute};
use crate::pipeline::{PlayerEngine, PlayerEvent};
use crate::telemetry::{
    CpuSampler, PlaybackTelemetry, collect_telemetry, read_battery_info, read_nvidia_rtd3_status,
};
use self::theme::{build_css, spx};
use self::win_controls::{WinCtrlKind, make_win_ctrl_button};
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

const APP_ID: &str = "io.github.videoplayer.App";

#[derive(Clone)]
pub struct LaunchOptions {
    pub initial_input: Option<String>,
    pub show_hud: bool,
    pub auto_quit_secs: Option<u64>,
    pub ui_scale: Option<f64>,
    pub initial_speed: Option<f64>,
    pub test_stress: bool,
    pub topology: DisplayTopology,
}

#[derive(Clone)]
struct WindowSession {
    window: adw::ApplicationWindow,
    open_uri: Rc<dyn Fn(String)>,
}

pub fn run_gui(opts: LaunchOptions) -> glib::ExitCode {
    let isolated_cli = opts.auto_quit_secs.is_some()
        || opts.test_stress
        || opts.ui_scale.is_some()
        || opts.initial_speed.is_some();

    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if isolated_cli {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();

    let session_slot: Rc<RefCell<Option<WindowSession>>> = Rc::new(RefCell::new(None));

    {
        let opts = opts.clone();
        let slot = Rc::clone(&session_slot);
        app.connect_activate(move |app| {
            if let Some(ref sess) = *slot.borrow() {
                sess.window.present();
                return;
            }
            let sess = build_ui(app, &opts);
            sess.window.present();
            *slot.borrow_mut() = Some(sess);
        });
    }

    {
        let opts = opts.clone();
        let slot = Rc::clone(&session_slot);
        app.connect_open(move |app, files, _hint| {
            let need_init = slot.borrow().is_none();
            if need_init {
                let sess = build_ui(app, &opts);
                sess.window.present();
                *slot.borrow_mut() = Some(sess);
            }
            if let Some(ref sess) = *slot.borrow() {
                sess.window.present();
                if let Some(first) = files.first() {
                    (sess.open_uri)(first.uri().to_string());
                }
            }
        });
    }

    let mut gtk_args: Vec<String> = vec!["video-player".into()];
    if let Some(ref raw) = opts.initial_input {
        if let Ok(uri) = normalize_media_uri(raw) {
            gtk_args.push(uri);
        } else {
            gtk_args.push(raw.clone());
        }
    }

    app.run_with_args(&gtk_args)
}

fn build_ui(app: &adw::Application, opts: &LaunchOptions) -> WindowSession {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let mut init_cfg = AppConfig::load();
    if let Some(s) = opts.ui_scale {
        init_cfg.ui_scale = clamp_scale(s);
    }
    if let Some(spd) = opts.initial_speed {
        init_cfg.playback_speed = snap_speed(spd);
    }

    let current_scale = Rc::new(Cell::new(init_cfg.ui_scale));
    let config = Rc::new(RefCell::new(init_cfg));

    let css = gtk4::CssProvider::new();
    css.load_from_string(&build_css(current_scale.get()));
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let init_vol = config.borrow().volume;
    let init_spd = config.borrow().playback_speed;
    let engine = Rc::new(
        PlayerEngine::new(init_vol, init_spd).expect("GStreamer gtk4paintablesink unavailable"),
    );

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Video Player")
        .default_width(spx(1080.0, current_scale.get()))
        .default_height(spx(608.0, current_scale.get()))
        .build();
    window.set_size_request(560, 340);
    window.add_css_class("root-canvas");

    let is_fullscreen = Rc::new(Cell::new(false));
    let has_media = Rc::new(Cell::new(false));

    // =========================================================================
    // Slim Floating Top Header Bar
    // - Always visible on Welcome screen.
    // - During video playback, stays hidden by default (so Minimize/Maximize/Close
    //   never sit over the playing video when resizing or watching) and only reveals
    //   when the user moves the pointer to the top strip (y <= 38px).
    // =========================================================================
    let app_icon = gtk4::Image::from_icon_name(APP_ID);
    app_icon.set_pixel_size(spx(20.0, current_scale.get()));
    app_icon.set_valign(gtk4::Align::Center);

    let brand_lbl = gtk4::Label::new(Some("Video Player"));
    brand_lbl.add_css_class("brand-title");
    brand_lbl.set_hexpand(false);
    brand_lbl.set_valign(gtk4::Align::BaselineCenter);

    // Always visible + hexpand=true so right-hand controls stay pinned top-right even on Welcome.
    let now_playing_lbl = gtk4::Label::new(Some(""));
    now_playing_lbl.add_css_class("now-playing-title");
    now_playing_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    now_playing_lbl.set_width_chars(0);
    now_playing_lbl.set_hexpand(true);
    now_playing_lbl.set_xalign(0.0);
    now_playing_lbl.set_valign(gtk4::Align::BaselineCenter);
    now_playing_lbl.set_margin_start(6);
    now_playing_lbl.set_margin_end(8);

    let open_btn = gtk4::Button::with_label("Open");
    open_btn.add_css_class("vp-btn");
    open_btn.add_css_class("hdr-open");
    open_btn.set_valign(gtk4::Align::Center);
    open_btn.set_focus_on_click(false);
    open_btn.set_tooltip_text(Some("Open video file (Ctrl+O)"));

    let initial_rtd3 = read_nvidia_rtd3_status();
    let initial_chip = if opts.topology.route == GpuRoute::AmdIgpuOnly {
        format!(
            "VCN · {}W",
            read_battery_info()
                .0
                .map(|w| format!("{w:.1}"))
                .unwrap_or_else(|| "AC".into())
        )
    } else {
        "NVDEC".into()
    };
    let stats_btn = gtk4::Button::with_label(&initial_chip);
    stats_btn.add_css_class("vp-btn");
    stats_btn.add_css_class(if initial_rtd3 == "suspended" {
        "emerald-chip"
    } else {
        "amber-chip"
    });
    stats_btn.set_valign(gtk4::Align::Center);
    stats_btn.set_focus_on_click(false);
    stats_btn.set_tooltip_text(Some("Toggle hardware telemetry HUD (I)"));

    let win_sep = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    win_sep.add_css_class("win-sep");
    win_sep.set_valign(gtk4::Align::Center);
    win_sep.set_margin_start(2);
    win_sep.set_margin_end(2);

    let ctrl_scale = current_scale.get();
    let min_ctrl = {
        let w = window.clone();
        let c = make_win_ctrl_button(WinCtrlKind::Minimize, &window, ctrl_scale, move || {
            w.minimize();
        });
        c.set_valign(gtk4::Align::Center);
        c
    };
    let max_ctrl = {
        let w = window.clone();
        let c = make_win_ctrl_button(WinCtrlKind::Maximize, &window, ctrl_scale, move || {
            if w.is_maximized() {
                w.unmaximize();
            } else {
                w.maximize();
            }
        });
        c.set_valign(gtk4::Align::Center);
        c
    };
    let fs_ctrl = {
        let w = window.clone();
        let fs = Rc::clone(&is_fullscreen);
        let c = make_win_ctrl_button(WinCtrlKind::Fullscreen, &window, ctrl_scale, move || {
            if fs.get() {
                w.unfullscreen();
            } else {
                w.fullscreen();
            }
        });
        c.set_valign(gtk4::Align::Center);
        c
    };
    let close_ctrl = {
        let w = window.clone();
        let c = make_win_ctrl_button(WinCtrlKind::Close, &window, ctrl_scale, move || {
            w.close();
        });
        c.set_valign(gtk4::Align::Center);
        c
    };

    let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    header_box.add_css_class("top-header");
    header_box.set_hexpand(true);
    header_box.append(&app_icon);
    header_box.append(&brand_lbl);
    header_box.append(&now_playing_lbl);
    header_box.append(&open_btn);
    header_box.append(&stats_btn);
    header_box.append(&win_sep);
    header_box.append(&min_ctrl);
    header_box.append(&max_ctrl);
    header_box.append(&fs_ctrl);
    header_box.append(&close_ctrl);

    let header_handle = gtk4::WindowHandle::new();
    header_handle.set_hexpand(true);
    header_handle.set_child(Some(&header_box));

    let header_rev = gtk4::Revealer::builder()
        .transition_type(gtk4::RevealerTransitionType::Crossfade)
        .transition_duration(140)
        .reveal_child(true)
        .visible(true)
        .halign(gtk4::Align::Fill)
        .valign(gtk4::Align::Start)
        .child(&header_handle)
        .build();

    // =========================================================================
    // Welcome View (Responsive Drop Zone + Recent Files)
    // =========================================================================
    let welcome_icon = gtk4::Image::from_icon_name(APP_ID);
    welcome_icon.set_pixel_size(spx(54.0, current_scale.get()));

    let welcome_title = gtk4::Label::new(Some("Video Player"));
    welcome_title.add_css_class("welcome-title");

    let welcome_sub = gtk4::Label::new(Some(
        "AMD VA-API zero-copy playback · Drop a video or click Open",
    ));
    welcome_sub.add_css_class("welcome-sub");
    welcome_sub.set_wrap(true);
    welcome_sub.set_justify(gtk4::Justification::Center);

    let welcome_open_btn = gtk4::Button::with_label("Open Video");
    welcome_open_btn.add_css_class("vp-btn");
    welcome_open_btn.add_css_class("primary-cta");
    welcome_open_btn.set_halign(gtk4::Align::Center);
    welcome_open_btn.set_focus_on_click(false);

    let recent_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    recent_box.set_margin_top(6);

    let welcome_card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    welcome_card.add_css_class("welcome-card");
    welcome_card.set_halign(gtk4::Align::Center);
    welcome_card.set_valign(gtk4::Align::Center);
    welcome_card.set_width_request(spx(340.0, current_scale.get()));
    welcome_card.set_margin_top(spx(46.0, current_scale.get()));
    welcome_card.set_margin_bottom(16);
    welcome_card.set_margin_start(16);
    welcome_card.set_margin_end(16);
    welcome_card.append(&welcome_icon);
    welcome_card.append(&welcome_title);
    welcome_card.append(&welcome_sub);
    welcome_card.append(&welcome_open_btn);
    welcome_card.append(&recent_box);

    let welcome_center = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    welcome_center.set_valign(gtk4::Align::Center);
    welcome_center.set_halign(gtk4::Align::Center);
    welcome_center.set_vexpand(true);
    welcome_center.set_hexpand(true);
    welcome_center.append(&welcome_card);

    let welcome_scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&welcome_center)
        .build();
    welcome_scroll.add_css_class("welcome-bg");

    // =========================================================================
    // Player Viewport (Edge-to-edge Bezel-less Canvas + Ergonomic Bottom Bar)
    // =========================================================================
    let picture = gtk4::Picture::for_paintable(engine.paintable());
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_can_target(false);
    picture.set_hexpand(true);
    picture.set_vexpand(true);

    let offload = gtk4::GraphicsOffload::new(Some(&picture));
    offload.set_enabled(gtk4::GraphicsOffloadEnabled::Enabled);
    offload.set_hexpand(true);
    offload.set_vexpand(true);

    let hud_label = gtk4::Label::new(None);
    hud_label.add_css_class("hud-pill");
    hud_label.set_halign(gtk4::Align::End);
    hud_label.set_valign(gtk4::Align::Start);
    hud_label.set_margin_top(spx(42.0, current_scale.get()));
    hud_label.set_margin_end(12);
    hud_label.set_visible(opts.show_hud);

    let status_banner = gtk4::Label::new(None);
    status_banner.add_css_class("osd-bar");
    status_banner.set_halign(gtk4::Align::Center);
    status_banner.set_valign(gtk4::Align::Center);
    status_banner.set_wrap(true);
    status_banner.set_max_width_chars(56);
    status_banner.set_visible(false);

    // Ergonomic Bottom Playback Bar (Generous 28px scaled Play/Pause + 6px/15px Seek Scroll Bar)
    let play_btn = gtk4::Button::from_icon_name("media-playback-pause-symbolic");
    play_btn.add_css_class("osd-icon-btn");
    play_btn.set_valign(gtk4::Align::Center);
    play_btn.set_focus_on_click(false);
    play_btn.set_tooltip_text(Some("Play / Pause (Space)"));

    let seek_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 0.5);
    seek_scale.add_css_class("seek-bar");
    seek_scale.set_draw_value(false);
    seek_scale.set_hexpand(true);
    seek_scale.set_focusable(false);
    seek_scale.set_valign(gtk4::Align::Center);
    seek_scale.set_tooltip_text(Some("Click, drag, or scroll wheel to seek"));

    let time_label = gtk4::Label::new(Some("00:00 / 00:00"));
    time_label.add_css_class("mono-stat");
    time_label.set_hexpand(false);
    time_label.set_margin_start(2);
    time_label.set_margin_end(2);
    time_label.set_valign(gtk4::Align::Center);

    // Compact Speed MenuButton (1x / 1.25x / 1.5x / 2x)
    let speed_popover_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let mut speed_btns: Vec<(f64, gtk4::Button)> = Vec::new();
    for spd in SPEED_PRESETS {
        let lbl = speed_label(spd);
        let b = gtk4::Button::with_label(&lbl);
        b.add_css_class("speed-choice");
        b.set_focus_on_click(false);
        if (spd - init_spd).abs() < 0.02 {
            b.add_css_class("active");
        }
        speed_popover_row.append(&b);
        speed_btns.push((spd, b));
    }
    let speed_popover = gtk4::Popover::builder()
        .child(&speed_popover_row)
        .position(gtk4::PositionType::Top)
        .has_arrow(false)
        .build();
    speed_popover.add_css_class("speed-popover");

    let speed_menu_btn = gtk4::MenuButton::builder()
        .label(speed_label(init_spd))
        .popover(&speed_popover)
        .direction(gtk4::ArrowType::Up)
        .valign(gtk4::Align::Center)
        .focus_on_click(false)
        .tooltip_text("Playback speed (1x / 1.25x / 1.5x / 2x)")
        .build();
    speed_menu_btn.add_css_class("vp-speed");

    let mute_btn = gtk4::Button::from_icon_name("audio-volume-high-symbolic");
    mute_btn.add_css_class("osd-icon-btn");
    mute_btn.set_valign(gtk4::Align::Center);
    mute_btn.set_focus_on_click(false);
    mute_btn.set_tooltip_text(Some("Mute / Unmute (M)"));

    let vol_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.5, 0.05);
    vol_scale.add_css_class("vol-bar");
    vol_scale.set_draw_value(false);
    vol_scale.set_width_request(spx(54.0, current_scale.get()));
    vol_scale.set_focusable(false);
    vol_scale.set_valign(gtk4::Align::Center);
    vol_scale.set_value(init_vol);
    vol_scale.set_tooltip_text(Some("Volume (Up / Down)"));

    let fs_bot_btn = gtk4::Button::from_icon_name("view-fullscreen-symbolic");
    fs_bot_btn.add_css_class("osd-icon-btn");
    fs_bot_btn.set_valign(gtk4::Align::Center);
    fs_bot_btn.set_focus_on_click(false);
    fs_bot_btn.set_tooltip_text(Some("Fullscreen (F / Double-Click)"));

    let bottom_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    bottom_bar.add_css_class("osd-bar");
    bottom_bar.set_margin_bottom(10);
    bottom_bar.set_margin_start(12);
    bottom_bar.set_margin_end(12);
    bottom_bar.append(&play_btn);
    bottom_bar.append(&seek_scale);
    bottom_bar.append(&time_label);
    bottom_bar.append(&speed_menu_btn);
    bottom_bar.append(&mute_btn);
    bottom_bar.append(&vol_scale);
    bottom_bar.append(&fs_bot_btn);

    let bot_rev = gtk4::Revealer::builder()
        .transition_type(gtk4::RevealerTransitionType::SlideUp)
        .transition_duration(150)
        .reveal_child(true)
        .visible(true)
        .halign(gtk4::Align::Fill)
        .valign(gtk4::Align::End)
        .child(&bottom_bar)
        .build();

    // Collapse cleanly to visible=false so zero GTK layers sit over GraphicsOffload.
    header_rev.connect_child_revealed_notify(|r| {
        if !r.is_child_revealed() && !r.reveals_child() {
            r.set_visible(false);
        }
    });
    bot_rev.connect_child_revealed_notify(|r| {
        if !r.is_child_revealed() && !r.reveals_child() {
            r.set_visible(false);
        }
    });

    let video_overlay = gtk4::Overlay::new();
    video_overlay.add_css_class("video-canvas");
    video_overlay.set_child(Some(&offload));
    video_overlay.add_overlay(&status_banner);
    video_overlay.add_overlay(&hud_label);
    video_overlay.add_overlay(&bot_rev);

    // =========================================================================
    // Root Overlay (Stack fills 100% height; Header floats seamlessly over top)
    // =========================================================================
    let main_stack = gtk4::Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .transition_duration(120)
        .vexpand(true)
        .hexpand(true)
        .build();
    main_stack.add_named(&welcome_scroll, Some("welcome"));
    main_stack.add_named(&video_overlay, Some("player"));
    main_stack.set_visible_child_name("welcome");

    let root_overlay = gtk4::Overlay::new();
    root_overlay.set_child(Some(&main_stack));
    root_overlay.add_overlay(&header_rev);
    window.set_content(Some(&root_overlay));

    // Shared state
    let seek_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let last_seek_sent: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let tap_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let seeking_until: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let is_muted = Rc::new(Cell::new(false));
    let is_eos = Rc::new(Cell::new(false));
    let explicit_hidden = Rc::new(Cell::new(false));
    let hidden_at: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let last_pointer = Rc::new(Cell::new((0.0_f64, 120.0_f64)));
    let inhibit_cookie = Rc::new(Cell::new(0u32));
    let last_activity_tick = Rc::new(Cell::new(0u32));
    let header_tick_div = Rc::new(Cell::new(0u32));
    let cpu_sampler = Rc::new(RefCell::new(CpuSampler::new()));

    let update_inhibit = {
        let app = app.clone();
        let win = window.clone();
        let cookie = Rc::clone(&inhibit_cookie);
        Rc::new(move |should_inhibit: bool| {
            let cur = cookie.get();
            if should_inhibit && cur == 0 {
                let c = app.inhibit(
                    Some(&win),
                    gtk4::ApplicationInhibitFlags::IDLE,
                    Some("Playing video"),
                );
                cookie.set(c);
            } else if !should_inhibit && cur != 0 {
                app.uninhibit(cur);
                cookie.set(0);
            }
        })
    };

    // Event-driven wakeup channel & 0 Hz idle OSD/HUD timer handle
    let ensure_ui_timer_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let wake_ui_timer = {
        let slot = Rc::clone(&ensure_ui_timer_slot);
        Rc::new(move || {
            if let Some(ref f) = *slot.borrow() {
                f();
            }
        })
    };

    // Show ONLY the bottom playback bar during video playback (keeps top bezel/window
    // buttons completely off the playing video unless pointer enters top strip).
    let show_playback_bar: Rc<dyn Fn()> = {
        let bot_rev = bot_rev.clone();
        let overlay = video_overlay.clone();
        let act = Rc::clone(&last_activity_tick);
        let explicit_hidden = Rc::clone(&explicit_hidden);
        let hidden_at = Rc::clone(&hidden_at);
        let eng = Rc::clone(&engine);
        let wake = Rc::clone(&wake_ui_timer);
        Rc::new(move || {
            act.set(0);
            explicit_hidden.set(false);
            hidden_at.set(None);
            overlay.set_cursor_from_name(None);
            bot_rev.set_visible(true);
            bot_rev.set_reveal_child(true);
            eng.set_ui_visible(true);
            wake();
        })
    };

    // Show both top header and bottom playback bar (used on Welcome, top-edge hover, or QA).
    let show_all_controls: Rc<dyn Fn()> = {
        let bot_rev = bot_rev.clone();
        let header_rev = header_rev.clone();
        let overlay = video_overlay.clone();
        let act = Rc::clone(&last_activity_tick);
        let explicit_hidden = Rc::clone(&explicit_hidden);
        let hidden_at = Rc::clone(&hidden_at);
        let eng = Rc::clone(&engine);
        let wake = Rc::clone(&wake_ui_timer);
        Rc::new(move || {
            act.set(0);
            explicit_hidden.set(false);
            hidden_at.set(None);
            overlay.set_cursor_from_name(None);
            header_rev.set_visible(true);
            header_rev.set_reveal_child(true);
            bot_rev.set_visible(true);
            bot_rev.set_reveal_child(true);
            eng.set_ui_visible(true);
            wake();
        })
    };

    let hide_all_controls: Rc<dyn Fn(bool)> = {
        let bot_rev = bot_rev.clone();
        let header_rev = header_rev.clone();
        let hud_label = hud_label.clone();
        let speed_popover = speed_popover.clone();
        let overlay = video_overlay.clone();
        let explicit_hidden = Rc::clone(&explicit_hidden);
        let hidden_at = Rc::clone(&hidden_at);
        let eng = Rc::clone(&engine);
        Rc::new(move |from_single_tap: bool| {
            speed_popover.popdown();
            header_rev.set_reveal_child(false);
            bot_rev.set_reveal_child(false);
            eng.set_ui_visible(false);
            if from_single_tap {
                hud_label.set_visible(false);
                explicit_hidden.set(true);
                hidden_at.set(Some(Instant::now()));
            }
            overlay.set_cursor_from_name(Some("none"));
        })
    };

    let apply_speed: Rc<dyn Fn(f64)> = {
        let eng = Rc::clone(&engine);
        let cfg_rc = Rc::clone(&config);
        let btns = speed_btns.clone();
        let menu_btn = speed_menu_btn.clone();
        let pop = speed_popover.clone();
        Rc::new(move |rate: f64| {
            let snapped = snap_speed(rate);
            eng.set_rate(snapped);
            menu_btn.set_label(&speed_label(snapped));
            for (s, btn) in &btns {
                if (*s - snapped).abs() < 0.02 {
                    btn.add_css_class("active");
                } else {
                    btn.remove_css_class("active");
                }
            }
            pop.popdown();
            let mut cfg = cfg_rc.borrow_mut();
            if (cfg.playback_speed - snapped).abs() > 0.01 {
                cfg.playback_speed = snapped;
                let _ = cfg.save();
            }
        })
    };

    for (spd, btn) in &speed_btns {
        let f = Rc::clone(&apply_speed);
        let target = *spd;
        btn.connect_clicked(move |_| f(target));
    }

    // Open media URI closure — dispatches `eng.open(&uri)` FIRST so AMD VCN preroll starts
    // immediately in parallel with GTK UI updates, and defers config disk write to idle.
    let refresh_recents_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let is_test_session = opts.test_stress || opts.auto_quit_secs.is_some();

    let open_media_uri: Rc<dyn Fn(String)> = {
        let eng = Rc::clone(&engine);
        let win = window.clone();
        let main_stack = main_stack.clone();
        let header_box = header_box.clone();
        let header_rev = header_rev.clone();
        let now_playing_lbl = now_playing_lbl.clone();
        let status_banner = status_banner.clone();
        let play_btn = play_btn.clone();
        let seek_scale = seek_scale.clone();
        let time_label = time_label.clone();
        let has_media = Rc::clone(&has_media);
        let is_eos = Rc::clone(&is_eos);
        let update_inhibit = Rc::clone(&update_inhibit);
        let cfg_rc = Rc::clone(&config);
        let show_bar = Rc::clone(&show_playback_bar);
        let recents_cb = Rc::clone(&refresh_recents_slot);

        Rc::new(move |raw_input: String| {
            let uri = match normalize_media_uri(&raw_input) {
                Ok(u) => u,
                Err(err) => {
                    status_banner.set_text(&err);
                    status_banner.set_visible(true);
                    main_stack.set_visible_child_name("player");
                    return;
                }
            };

            // 1. Kick off GStreamer + AMD VCN hardware decode immediately on actor thread
            let open_res = eng.open(&uri);

            // 2. Update UI labels while hardware prerolls in parallel
            let label_name = display_name_for_media(&uri);
            status_banner.set_visible(false);
            now_playing_lbl.set_text(&label_name);
            header_box.add_css_class("playing-hdr");
            win.set_title(Some(&format!("{label_name} — Video Player")));
            main_stack.set_visible_child_name("player");
            seek_scale.set_value(0.0);
            time_label.set_text("00:00 / 00:00");
            play_btn.set_icon_name("media-playback-pause-symbolic");

            match open_res {
                Ok(()) => {
                    has_media.set(true);
                    is_eos.set(false);
                    update_inhibit(true);
                    // Keep top window buttons hidden when playback starts for a pure
                    // bezel-less top edge; show only the bottom playback bar.
                    header_rev.set_reveal_child(false);
                    show_bar();

                    if !is_test_session {
                        let cfg_rc = Rc::clone(&cfg_rc);
                        let recents_cb = Rc::clone(&recents_cb);
                        let uri_clone = uri.clone();
                        glib::idle_add_local_once(move || {
                            if let Some(local_path) =
                                gio::File::for_uri(&uri_clone).path().filter(|p| p.is_file())
                            {
                                let mut cfg = cfg_rc.borrow_mut();
                                cfg.push_recent(&local_path.display().to_string());
                                let _ = cfg.save();
                                drop(cfg);
                                if let Some(ref refresh) = *recents_cb.borrow() {
                                    refresh();
                                }
                            }
                        });
                    }
                }
                Err(err) => {
                    has_media.set(false);
                    update_inhibit(false);
                    header_rev.set_visible(true);
                    header_rev.set_reveal_child(true);
                    status_banner.set_text(&err);
                    status_banner.set_visible(true);
                }
            }
        })
    };

    let populate_recents: Rc<dyn Fn()> = {
        let recent_box = recent_box.clone();
        let cfg_rc = Rc::clone(&config);
        let open_cb = Rc::clone(&open_media_uri);
        Rc::new(move || {
            while let Some(child) = recent_box.first_child() {
                recent_box.remove(&child);
            }
            let files: Vec<String> = cfg_rc
                .borrow()
                .recent_files
                .iter()
                .filter(|p| Path::new(p.as_str()).is_file())
                .cloned()
                .collect();
            if files.is_empty() {
                return;
            }
            let hdr = gtk4::Label::new(Some("Recent"));
            hdr.add_css_class("now-playing-title");
            hdr.set_xalign(0.0);
            recent_box.append(&hdr);

            for path in files {
                let name = Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                let btn = gtk4::Button::with_label(&name);
                btn.add_css_class("recent-btn");
                btn.set_focus_on_click(false);
                btn.set_tooltip_text(Some(&path));
                if let Some(lbl) = btn.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
                    lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
                    lbl.set_xalign(0.0);
                }
                let cb = Rc::clone(&open_cb);
                btn.connect_clicked(move |_| cb(path.clone()));
                recent_box.append(&btn);
            }
        })
    };
    *refresh_recents_slot.borrow_mut() = Some(Rc::clone(&populate_recents));
    populate_recents();

    // Native FileDialog picker (Ctrl+O / Open)
    let trigger_open_dialog = {
        let win = window.clone();
        let open_cb = Rc::clone(&open_media_uri);
        Rc::new(move || {
            let vid_filter = gtk4::FileFilter::new();
            vid_filter.set_name(Some("Video Files"));
            vid_filter.add_mime_type("video/*");
            vid_filter.add_mime_type("application/x-matroska");

            let all_filter = gtk4::FileFilter::new();
            all_filter.set_name(Some("All Files"));
            all_filter.add_pattern("*");

            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&vid_filter);
            filters.append(&all_filter);

            let dialog = gtk4::FileDialog::builder()
                .title("Open Video")
                .filters(&filters)
                .default_filter(&vid_filter)
                .build();

            let cb = Rc::clone(&open_cb);
            dialog.open(Some(&win), None::<&gio::Cancellable>, move |res| {
                if let Ok(file) = res {
                    cb(file.uri().to_string());
                }
            });
        })
    };

    {
        let f = Rc::clone(&trigger_open_dialog);
        open_btn.connect_clicked(move |_| f());
    }
    {
        let f = Rc::clone(&trigger_open_dialog);
        welcome_open_btn.connect_clicked(move |_| f());
    }

    // Telemetry HUD Toggle
    let toggle_hud = {
        let hud = hud_label.clone();
        let eng = Rc::clone(&engine);
        let cpu = Rc::clone(&cpu_sampler);
        let wake = Rc::clone(&wake_ui_timer);
        Rc::new(move || {
            let next = !hud.is_visible();
            hud.set_visible(next);
            if next {
                let t = collect_telemetry(&eng, &mut cpu.borrow_mut());
                hud.set_text(&format_hud(&t));
                wake();
            }
        })
    };
    {
        let f = Rc::clone(&toggle_hud);
        stats_btn.connect_clicked(move |_| f());
    }

    // Play / Pause Toggle
    let toggle_play = {
        let eng = Rc::clone(&engine);
        let btn = play_btn.clone();
        let has_media = Rc::clone(&has_media);
        let is_eos = Rc::clone(&is_eos);
        let inhibit = Rc::clone(&update_inhibit);
        let show_bar = Rc::clone(&show_playback_bar);
        Rc::new(move || {
            if !has_media.get() {
                return;
            }
            if is_eos.get() {
                is_eos.set(false);
                eng.seek_secs(0.0);
                eng.set_playing(true);
                btn.set_icon_name("media-playback-pause-symbolic");
                inhibit(true);
                show_bar();
                return;
            }
            let next = !eng.is_playing();
            eng.set_playing(next);
            btn.set_icon_name(if next {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            inhibit(next);
            show_bar();
        })
    };
    {
        let f = Rc::clone(&toggle_play);
        play_btn.connect_clicked(move |_| f());
    }

    // Instant Leading-Edge (0ms) + 40ms Throttled Seek on Scroll Bar
    {
        let eng = Rc::clone(&engine);
        let seek_debounce = Rc::clone(&seek_debounce);
        let last_seek_sent = Rc::clone(&last_seek_sent);
        let seeking_until = Rc::clone(&seeking_until);
        let time_label = time_label.clone();
        let is_eos = Rc::clone(&is_eos);
        let show_bar = Rc::clone(&show_playback_bar);
        seek_scale.connect_change_value(move |scale, _, val| {
            let max_v = scale.adjustment().upper();
            let target = val.clamp(0.0, max_v);
            is_eos.set(false);
            show_bar();
            let now = Instant::now();
            seeking_until.set(Some(now + Duration::from_millis(320)));
            time_label.set_text(&format!("{} / {}", fmt_time(target), fmt_time(max_v)));

            let can_fire_now = last_seek_sent
                .get()
                .is_none_or(|prev| now.duration_since(prev) >= Duration::from_millis(40));

            if can_fire_now {
                if let Some(prev) = seek_debounce.borrow_mut().take() {
                    prev.remove();
                }
                last_seek_sent.set(Some(now));
                eng.seek_secs(target);
            } else {
                if let Some(prev) = seek_debounce.borrow_mut().take() {
                    prev.remove();
                }
                let eng = Rc::clone(&eng);
                let slot = Rc::clone(&seek_debounce);
                let last_sent = Rc::clone(&last_seek_sent);
                let id = glib::timeout_add_local_once(Duration::from_millis(40), move || {
                    *slot.borrow_mut() = None;
                    last_sent.set(Some(Instant::now()));
                    eng.seek_secs(target);
                });
                *seek_debounce.borrow_mut() = Some(id);
            }
            glib::Propagation::Proceed
        });
    }

    // Mouse-Wheel / Touchpad Scroll on Seek Bar -> Quick ±5s Seek
    {
        let eng = Rc::clone(&engine);
        let has_media = Rc::clone(&has_media);
        let show_bar = Rc::clone(&show_playback_bar);
        let seek_scroll = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL
                | gtk4::EventControllerScrollFlags::HORIZONTAL,
        );
        seek_scroll.connect_scroll(move |_, dx, dy| {
            if !has_media.get() {
                return glib::Propagation::Proceed;
            }
            let step = if dy < -0.05 || dx > 0.05 {
                5.0
            } else if dy > 0.05 || dx < -0.05 {
                -5.0
            } else {
                return glib::Propagation::Proceed;
            };
            let (pos, dur) = eng.position_and_duration_secs();
            let next = (pos + step).clamp(0.0, dur.max(pos + step).max(0.0));
            eng.seek_secs(next);
            show_bar();
            glib::Propagation::Stop
        });
        seek_scale.add_controller(seek_scroll);
    }

    // Mute & Volume
    let toggle_mute = {
        let eng = Rc::clone(&engine);
        let is_muted = Rc::clone(&is_muted);
        let mute_btn = mute_btn.clone();
        Rc::new(move || {
            let next = !is_muted.get();
            is_muted.set(next);
            eng.set_muted(next);
            mute_btn.set_icon_name(if next {
                "audio-volume-muted-symbolic"
            } else {
                "audio-volume-high-symbolic"
            });
        })
    };
    {
        let f = Rc::clone(&toggle_mute);
        mute_btn.connect_clicked(move |_| f());
    }
    {
        let eng = Rc::clone(&engine);
        let cfg_rc = Rc::clone(&config);
        vol_scale.connect_value_changed(move |s| {
            let v = s.value();
            eng.set_volume(v);
            cfg_rc.borrow_mut().volume = v;
        });
    }

    // Fullscreen Toggle & State Sync
    let toggle_fullscreen = {
        let w = window.clone();
        let fs = Rc::clone(&is_fullscreen);
        Rc::new(move || {
            if fs.get() {
                w.unfullscreen();
            } else {
                w.fullscreen();
            }
        })
    };
    {
        let f = Rc::clone(&toggle_fullscreen);
        fs_bot_btn.connect_clicked(move |_| f());
    }
    {
        let is_fs = Rc::clone(&is_fullscreen);
        let fs_bot_btn = fs_bot_btn.clone();
        let show_bar = Rc::clone(&show_playback_bar);
        window.connect_fullscreened_notify(move |win| {
            let fs = win.is_fullscreen();
            is_fs.set(fs);
            fs_bot_btn.set_icon_name(if fs {
                "view-restore-symbolic"
            } else {
                "view-fullscreen-symbolic"
            });
            if !fs {
                show_bar();
            }
        });
    }

    // Video Surface Interaction:
    // - Double tap: immediately toggles Fullscreen <-> restored windowed size (cancels single tap).
    // - Single tap: hides ALL overlays (header, bottom controls, HUD, cursor) for pure
    //   bezel-less playback, or restores playback bar if hidden.
    let video_click = gtk4::GestureClick::new();
    video_click.set_button(1);
    let double_tap_handled = Rc::new(Cell::new(false));
    {
        let toggle_fs = Rc::clone(&toggle_fullscreen);
        let tap_timer = Rc::clone(&tap_timer);
        let has_media = Rc::clone(&has_media);
        let handled = Rc::clone(&double_tap_handled);

        video_click.connect_pressed(move |_, n_press, _, _| {
            if !has_media.get() {
                return;
            }
            if n_press == 1 {
                handled.set(false);
            } else if n_press >= 2 {
                handled.set(true);
                if let Some(prev) = tap_timer.borrow_mut().take() {
                    prev.remove();
                }
                toggle_fs();
            }
        });
    }
    {
        let hide_all = Rc::clone(&hide_all_controls);
        let show_bar = Rc::clone(&show_playback_bar);
        let bot_rev = bot_rev.clone();
        let header_rev = header_rev.clone();
        let hud_label = hud_label.clone();
        let has_media = Rc::clone(&has_media);
        let tap_timer = Rc::clone(&tap_timer);
        let handled = Rc::clone(&double_tap_handled);

        video_click.connect_released(move |_, n_press, _, y| {
            if !has_media.get() || n_press != 1 || handled.get() {
                return;
            }
            if let Some(prev) = tap_timer.borrow_mut().take() {
                prev.remove();
            }
            let hide_all = Rc::clone(&hide_all);
            let show_bar = Rc::clone(&show_bar);
            let bot_rev = bot_rev.clone();
            let header_rev = header_rev.clone();
            let hud_label = hud_label.clone();
            let slot = Rc::clone(&tap_timer);
            let id = glib::timeout_add_local_once(Duration::from_millis(220), move || {
                *slot.borrow_mut() = None;
                let any_visible = bot_rev.reveals_child()
                    || header_rev.reveals_child()
                    || hud_label.is_visible();
                if any_visible {
                    hide_all(true);
                } else {
                    show_bar();
                    if y <= 42.0 {
                        header_rev.set_visible(true);
                        header_rev.set_reveal_child(true);
                    }
                }
            });
            *tap_timer.borrow_mut() = Some(id);
        });
    }
    offload.add_controller(video_click);

    // Zone-Aware Mouse Motion:
    // - Prevents the top window bar (Minimize/Maximize/Fullscreen/Close) from popping up over
    //   the video when resizing the window or moving the mouse in the video area.
    // - Top bar ONLY reveals when pointer moves into the top strip (y <= 38px and away from
    //   the 14px left/right window resize borders), and auto-hides as soon as y > 54px!
    let motion = gtk4::EventControllerMotion::new();
    {
        let has_media = Rc::clone(&has_media);
        let show_bar = Rc::clone(&show_playback_bar);
        let explicit_hidden = Rc::clone(&explicit_hidden);
        let hidden_at = Rc::clone(&hidden_at);
        let last_pointer = Rc::clone(&last_pointer);
        let act = Rc::clone(&last_activity_tick);
        let bot_rev = bot_rev.clone();
        let header_rev = header_rev.clone();
        let win = window.clone();

        motion.connect_motion(move |_, x, y| {
            let (px, py) = last_pointer.get();
            last_pointer.set((x, y));
            if !has_media.get() {
                return;
            }

            let win_w = win.width() as f64;
            let win_h = win.height() as f64;
            // Ignore motion right on the outer 12px window resize edges so resizing the window
            // never pops up window frame chrome over the playing video.
            let on_resize_border =
                x <= 12.0 || x >= (win_w - 12.0) || y <= 4.0 || y >= (win_h - 8.0);
            if on_resize_border {
                return;
            }

            let in_top_zone = y <= 38.0;
            let left_top_zone = y > 54.0;

            if explicit_hidden.get() {
                let cooldown_ok = hidden_at
                    .get()
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(450));
                let moved_far = (x - px).hypot(y - py) > 18.0;
                if cooldown_ok && moved_far {
                    show_bar();
                    if in_top_zone {
                        header_rev.set_visible(true);
                        header_rev.set_reveal_child(true);
                    }
                }
            } else {
                act.set(0);
                if !bot_rev.reveals_child() {
                    show_bar();
                }
                if in_top_zone && !header_rev.reveals_child() {
                    header_rev.set_visible(true);
                    header_rev.set_reveal_child(true);
                } else if left_top_zone && header_rev.reveals_child() {
                    header_rev.set_reveal_child(false);
                }
            }
        });
    }
    root_overlay.add_controller(motion);

    // Drag-and-Drop anywhere onto Window
    let drop_target = gtk4::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    {
        let open_cb = Rc::clone(&open_media_uri);
        drop_target.connect_drop(move |_, val, _, _| {
            if let Ok(file) = val.get::<gio::File>() {
                open_cb(file.uri().to_string());
                return true;
            }
            false
        });
    }
    window.add_controller(drop_target);

    // Keyboard Shortcuts
    let apply_live_scale = {
        let current_scale = Rc::clone(&current_scale);
        let cfg_rc = Rc::clone(&config);
        let css = css.clone();
        let ic = app_icon.clone();
        let vs = vol_scale.clone();
        Rc::new(move |next_scale: f64| {
            let clamped = clamp_scale(next_scale);
            current_scale.set(clamped);
            css.load_from_string(&build_css(clamped));
            ic.set_pixel_size(spx(20.0, clamped));
            vs.set_width_request(spx(54.0, clamped));
            let mut cfg = cfg_rc.borrow_mut();
            cfg.ui_scale = clamped;
            let _ = cfg.save();
        })
    };

    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let toggle_play = Rc::clone(&toggle_play);
        let toggle_fs = Rc::clone(&toggle_fullscreen);
        let toggle_mute = Rc::clone(&toggle_mute);
        let toggle_hud = Rc::clone(&toggle_hud);
        let trigger_open = Rc::clone(&trigger_open_dialog);
        let apply_speed = Rc::clone(&apply_speed);
        let apply_scale = Rc::clone(&apply_live_scale);
        let current_scale = Rc::clone(&current_scale);
        let eng = Rc::clone(&engine);
        let vol_scale = vol_scale.clone();
        let show_bar = Rc::clone(&show_playback_bar);
        let has_media = Rc::clone(&has_media);
        let is_fs = Rc::clone(&is_fullscreen);
        let win = window.clone();

        key_ctrl.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            if ctrl {
                match key {
                    gdk::Key::o | gdk::Key::O => {
                        trigger_open();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::q | gdk::Key::Q | gdk::Key::w | gdk::Key::W => {
                        win.close();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                        apply_scale(current_scale.get() + 0.10);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::minus | gdk::Key::KP_Subtract => {
                        apply_scale(current_scale.get() - 0.10);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::_0 | gdk::Key::KP_0 => {
                        apply_scale(1.20);
                        return glib::Propagation::Stop;
                    }
                    _ => return glib::Propagation::Proceed,
                }
            }

            match key {
                gdk::Key::f | gdk::Key::F | gdk::Key::F11 => {
                    toggle_fs();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Escape => {
                    if is_fs.get() {
                        win.unfullscreen();
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Proceed;
                }
                gdk::Key::i | gdk::Key::I => {
                    toggle_hud();
                    return glib::Propagation::Stop;
                }
                _ => {}
            }

            if !has_media.get() {
                return glib::Propagation::Proceed;
            }

            match key {
                gdk::Key::space | gdk::Key::k | gdk::Key::K => {
                    toggle_play();
                    glib::Propagation::Stop
                }
                gdk::Key::m | gdk::Key::M => {
                    toggle_mute();
                    glib::Propagation::Stop
                }
                gdk::Key::Home | gdk::Key::_0 => {
                    eng.seek_secs(0.0);
                    show_bar();
                    glib::Propagation::Stop
                }
                gdk::Key::Left | gdk::Key::Right => {
                    let delta = if key == gdk::Key::Left { -5.0 } else { 5.0 };
                    let (pos, dur) = eng.position_and_duration_secs();
                    let next = (pos + delta).clamp(0.0, dur.max(pos + delta).max(0.0));
                    eng.seek_secs(next);
                    show_bar();
                    glib::Propagation::Stop
                }
                gdk::Key::j | gdk::Key::J | gdk::Key::l | gdk::Key::L => {
                    let delta = if matches!(key, gdk::Key::j | gdk::Key::J) {
                        -10.0
                    } else {
                        10.0
                    };
                    let (pos, dur) = eng.position_and_duration_secs();
                    let next = (pos + delta).clamp(0.0, dur.max(pos + delta).max(0.0));
                    eng.seek_secs(next);
                    show_bar();
                    glib::Propagation::Stop
                }
                gdk::Key::Up | gdk::Key::Down => {
                    let dv = if key == gdk::Key::Up { 0.05 } else { -0.05 };
                    vol_scale.set_value((vol_scale.value() + dv).clamp(0.0, 1.5));
                    show_bar();
                    glib::Propagation::Stop
                }
                gdk::Key::bracketleft | gdk::Key::bracketright => {
                    let cur = eng.rate();
                    let idx = SPEED_PRESETS
                        .iter()
                        .position(|s| (*s - cur).abs() < 0.02)
                        .unwrap_or(0);
                    let next_idx = if key == gdk::Key::bracketright {
                        (idx + 1).min(SPEED_PRESETS.len() - 1)
                    } else {
                        idx.saturating_sub(1)
                    };
                    apply_speed(SPEED_PRESETS[next_idx]);
                    show_bar();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    window.add_controller(key_ctrl.clone());

    // =========================================================================
    // Event-Driven Pipeline Drain (0 ms latency) + 0 Hz Idle OSD/HUD Timer
    // =========================================================================
    let drain_pipeline_events: Rc<dyn Fn()> = {
        let eng = Rc::clone(&engine);
        let win = window.clone();
        let status_banner = status_banner.clone();
        let play_btn = play_btn.clone();
        let is_eos = Rc::clone(&is_eos);
        let inhibit = Rc::clone(&update_inhibit);
        Rc::new(move || {
            for ev in eng.drain_events() {
                match ev {
                    PlayerEvent::Error(e) => {
                        status_banner.set_text(&format!("Playback error: {e}"));
                        status_banner.set_visible(true);
                        play_btn.set_icon_name("media-playback-start-symbolic");
                        inhibit(false);
                    }
                    PlayerEvent::Eos => {
                        is_eos.set(true);
                        eng.set_playing(false);
                        play_btn.set_icon_name("media-playback-start-symbolic");
                        inhibit(false);
                    }
                    PlayerEvent::Buffering(pct) => {
                        if pct < 100 {
                            status_banner.set_text(&format!("Buffering {pct}%..."));
                            status_banner.set_visible(true);
                        } else if status_banner.text().starts_with("Buffering") {
                            status_banner.set_visible(false);
                        }
                    }
                    PlayerEvent::VideoDimensions(vw, vh) => {
                        // Auto-fit window height to the video's native aspect ratio so
                        // there are zero black bars/bezels around the video frame!
                        if !is_test_session
                            && !win.is_fullscreen()
                            && !win.is_maximized()
                            && vw > 0
                            && vh > 0
                        {
                            let cur_w = win.width().max(560);
                            let fit_h =
                                ((cur_w as f64 * vh as f64) / vw as f64).round() as i32;
                            win.set_default_size(cur_w, fit_h.clamp(340, 1020));
                        }
                    }
                }
            }
        })
    };

    // Wake GTK main loop immediately on GStreamer actor/bus events (0 Hz when idle)
    {
        let main_ctx = glib::MainContext::default();
        let drain_weak = Rc::downgrade(&drain_pipeline_events);
        // Send-safe wakeup via MainContext::invoke on the GTK main thread
        let drain_ptr = Box::into_raw(Box::new(drain_weak)) as usize;
        // Keep a strong ref alive in the window lifecycle via closure below
        engine.set_event_callback(move || {
            main_ctx.invoke(move || {
                // SAFETY: Always executes on GTK main thread; `drain_weak` only upgrades while window lives.
                let weak = unsafe { &*(drain_ptr as *const std::rc::Weak<dyn Fn()>) };
                if let Some(f) = weak.upgrade() {
                    f();
                }
            });
        });
    }

    // On-demand 250ms OSD/HUD timer: runs ONLY while overlays or HUD are visible,
    // returning ControlFlow::Break (true 0 Hz idle) as soon as OSD hides for Direct Scanout!
    let ui_timer_running = Rc::new(Cell::new(false));
    let ensure_ui_timer: Rc<dyn Fn()> = {
        let running = Rc::clone(&ui_timer_running);
        let drain_ev = Rc::clone(&drain_pipeline_events);
        let eng = Rc::clone(&engine);
        let seek_scale = seek_scale.clone();
        let seeking_until = Rc::clone(&seeking_until);
        let time_label = time_label.clone();
        let hud_label = hud_label.clone();
        let bot_rev = bot_rev.clone();
        let header_rev = header_rev.clone();
        let hide_all = Rc::clone(&hide_all_controls);
        let act = Rc::clone(&last_activity_tick);
        let hdiv = Rc::clone(&header_tick_div);
        let cpu_sampler = Rc::clone(&cpu_sampler);
        let stats_btn = stats_btn.clone();
        let has_media = Rc::clone(&has_media);
        let speed_popover = speed_popover.clone();
        let route = opts.topology.route;

        Rc::new(move || {
            if running.replace(true) {
                return;
            }
            let running = Rc::clone(&running);
            let drain_ev = Rc::clone(&drain_ev);
            let eng = Rc::clone(&eng);
            let seek_scale = seek_scale.clone();
            let seeking_until = Rc::clone(&seeking_until);
            let time_label = time_label.clone();
            let hud_label = hud_label.clone();
            let bot_rev = bot_rev.clone();
            let header_rev = header_rev.clone();
            let hide_all = Rc::clone(&hide_all);
            let act = Rc::clone(&act);
            let hdiv = Rc::clone(&hdiv);
            let cpu_sampler = Rc::clone(&cpu_sampler);
            let stats_btn = stats_btn.clone();
            let has_media = Rc::clone(&has_media);
            let speed_popover = speed_popover.clone();

            glib::timeout_add_local(Duration::from_millis(250), move || {
                drain_ev();

                let osd_active = has_media.get()
                    && (bot_rev.reveals_child() || header_rev.reveals_child());
                let need_hud = hud_label.is_visible();

                // Auto-hide controls after 2.0s idle while playing (keep open if speed popover open)
                if has_media.get() && eng.is_playing() && !speed_popover.is_visible() {
                    let t = act.get().saturating_add(1);
                    act.set(t);
                    if t >= 8 && (bot_rev.reveals_child() || header_rev.reveals_child()) {
                        hide_all(false);
                    }
                }

                // ZERO GTK widget updates when bottom OSD is hidden -> Mutter Direct Scanout!
                let is_seeking = seeking_until
                    .get()
                    .is_some_and(|until| Instant::now() < until);
                if has_media.get() && bot_rev.is_visible() && bot_rev.reveals_child() && !is_seeking {
                    let (pos, dur) = eng.position_and_duration_secs();
                    if dur > 0.1 {
                        seek_scale.set_range(0.0, dur);
                        seek_scale.set_value(pos);
                    }
                    time_label.set_text(&format!("{} / {}", fmt_time(pos), fmt_time(dur)));
                }

                let div = hdiv.get().wrapping_add(1);
                hdiv.set(div);
                let need_chip =
                    header_rev.is_visible() && header_rev.reveals_child() && (div % 10 == 0);
                if need_hud || need_chip {
                    let t = collect_telemetry(&eng, &mut cpu_sampler.borrow_mut());
                    if need_hud {
                        hud_label.set_text(&format_hud(&t));
                    }
                    if need_chip {
                        let w_str = t
                            .battery_watts
                            .map(|w| format!("{w:.1}W"))
                            .unwrap_or_else(|| "AC".into());
                        let dec = if t.decoder_name == "idle" {
                            if route == GpuRoute::AmdIgpuOnly {
                                "VCN".into()
                            } else {
                                "NVDEC".into()
                            }
                        } else {
                            short_codec_label(&t.decoder_name).to_string()
                        };
                        stats_btn.set_label(&format!("{dec} · {w_str}"));
                        stats_btn.remove_css_class("emerald-chip");
                        stats_btn.remove_css_class("amber-chip");
                        stats_btn.add_css_class(if t.nvidia_rtd3 == "suspended" {
                            "emerald-chip"
                        } else {
                            "amber-chip"
                        });
                    }
                }

                // True 0 Hz idle: stop timer completely when no OSD or HUD is active!
                if !osd_active && !need_hud {
                    running.set(false);
                    return glib::ControlFlow::Break;
                }

                glib::ControlFlow::Continue
            });
        })
    };
    *ensure_ui_timer_slot.borrow_mut() = Some(Rc::clone(&ensure_ui_timer));
    if opts.show_hud {
        ensure_ui_timer();
    }

    // Save state cleanly on window close (skip overwriting user speed/recent during test_stress)
    {
        let eng = Rc::clone(&engine);
        let cfg_rc = Rc::clone(&config);
        let inhibit = Rc::clone(&update_inhibit);
        let is_test = opts.test_stress || opts.auto_quit_secs.is_some();
        window.connect_close_request(move |_| {
            inhibit(false);
            if !is_test {
                let _ = cfg_rc.borrow().save();
            }
            eng.stop();
            glib::Propagation::Proceed
        });
    }

    if let Some(secs) = opts.auto_quit_secs {
        let eng = Rc::clone(&engine);
        let win = window.clone();
        let cpu = Rc::clone(&cpu_sampler);
        let close_ctrl_qa = close_ctrl.clone();
        let open_btn_qa = open_btn.clone();
        let stats_btn_qa = stats_btn.clone();
        let header_rev_qa = header_rev.clone();
        glib::timeout_add_local_once(Duration::from_secs(secs), move || {
            header_rev_qa.set_visible(true);
            header_rev_qa.set_reveal_child(true);
            let win_w = win.width();
            let close_x = close_ctrl_qa
                .compute_bounds(&win)
                .map(|b| (b.x() + b.width()).round() as i32)
                .unwrap_or(0);
            let open_w = open_btn_qa.width();
            let stats_w = stats_btn_qa.width();
            let t = collect_telemetry(&eng, &mut cpu.borrow_mut());
            println!("=== VIDEO PLAYER LIVE TELEMETRY ({secs}s) ===");
            println!("{}", format_hud(&t));
            println!("Speed         : {:.2}x", eng.rate());
            println!(
                "UI Alignment  : win_w={win_w} close_right={close_x} open_w={open_w} stats_w={stats_w}"
            );
            eng.stop();
            win.close();
        });
    }

    if opts.test_stress {
        let qa_widgets = UiQaWidgets {
            window: window.clone(),
            header_box: header_box.clone(),
            brand_lbl: brand_lbl.clone(),
            open_btn: open_btn.clone(),
            welcome_open_btn: welcome_open_btn.clone(),
            welcome_card: welcome_card.clone(),
            stats_btn: stats_btn.clone(),
            close_ctrl: close_ctrl.clone(),
            bottom_bar: bottom_bar.clone(),
            time_label: time_label.clone(),
            speed_menu_btn: speed_menu_btn.clone(),
            bot_rev: bot_rev.clone(),
            header_rev: header_rev.clone(),
            hud_label: hud_label.clone(),
            key_ctrl: key_ctrl.clone(),
            hide_all: Rc::clone(&hide_all_controls),
            show_all: Rc::clone(&show_all_controls),
        };
        run_stress_suite(
            &window,
            &engine,
            &cpu_sampler,
            &open_media_uri,
            &apply_speed,
            qa_widgets,
        );
    }

    WindowSession {
        window,
        open_uri: open_media_uri,
    }
}

#[derive(Clone)]
struct UiQaWidgets {
    window: adw::ApplicationWindow,
    header_box: gtk4::Box,
    brand_lbl: gtk4::Label,
    open_btn: gtk4::Button,
    welcome_open_btn: gtk4::Button,
    welcome_card: gtk4::Box,
    stats_btn: gtk4::Button,
    close_ctrl: gtk4::Button,
    bottom_bar: gtk4::Box,
    time_label: gtk4::Label,
    speed_menu_btn: gtk4::MenuButton,
    bot_rev: gtk4::Revealer,
    header_rev: gtk4::Revealer,
    hud_label: gtk4::Label,
    key_ctrl: gtk4::EventControllerKey,
    hide_all: Rc<dyn Fn(bool)>,
    show_all: Rc<dyn Fn()>,
}

fn short_codec_label(dec: &str) -> &str {
    match dec {
        "vavp9dec" | "vp9dec" => "VP9",
        "vah264dec" | "avdec_h264" => "H264",
        "vah265dec" | "avdec_h265" => "HEVC",
        "dav1ddec" => "AV1",
        _ => dec,
    }
}

fn check_row_no_overlap(
    tag: &str,
    row_name: &str,
    win: &adw::ApplicationWindow,
    row: &gtk4::Box,
    min_gap: f32,
) -> Result<(), String> {
    let mut prev_right: Option<f32> = None;
    let mut child = row.first_child();
    let mut idx = 0usize;
    while let Some(cur) = child {
        if cur.is_visible() {
            if let Some(b) = cur.compute_bounds(win) {
                if b.width() > 0.5 {
                    if let Some(pr) = prev_right {
                        if b.x() < pr + min_gap {
                            return Err(format!(
                                "{tag}: {row_name} child[{idx}] overlaps previous (prev_right={pr:.1}, next_x={:.1}, min_gap={min_gap:.1})",
                                b.x()
                            ));
                        }
                    }
                    prev_right = Some(b.x() + b.width());
                }
            }
        }
        child = cur.next_sibling();
        idx += 1;
    }
    Ok(())
}

fn verify_ui_layout(tag: &str, w: &UiQaWidgets, check_player: bool) -> Result<(), String> {
    let win_w = w.window.width();
    let close_right = w
        .close_ctrl
        .compute_bounds(&w.window)
        .map(|b| b.x() + b.width())
        .ok_or_else(|| format!("{tag}: close_ctrl bounds unavailable"))?;
    let right_gap = (win_w as f32 - close_right).abs();
    if right_gap > 24.0 {
        return Err(format!(
            "{tag}: close_ctrl not pinned right (win_w={win_w}, close_right={close_right:.1})"
        ));
    }

    check_row_no_overlap(tag, "header_box", &w.window, &w.header_box, 0.5)?;
    if !check_player {
        if let (Some(hb), Some(wb)) = (
            w.header_box.compute_bounds(&w.window),
            w.welcome_card.compute_bounds(&w.window),
        ) {
            let h_bot = hb.y() + hb.height();
            if wb.y() < h_bot {
                return Err(format!(
                    "{tag}: welcome_card overlaps header (header_bottom={h_bot:.1}, card_y={:.1})",
                    wb.y()
                ));
            }
        }
    }

    let mut targets: Vec<(&str, &gtk4::Widget)> = vec![
        ("brand_lbl", w.brand_lbl.upcast_ref::<gtk4::Widget>()),
        ("open_btn", w.open_btn.upcast_ref::<gtk4::Widget>()),
        ("stats_btn", w.stats_btn.upcast_ref::<gtk4::Widget>()),
    ];
    if check_player {
        targets.push(("time_label", w.time_label.upcast_ref::<gtk4::Widget>()));
        targets.push(("speed_menu", w.speed_menu_btn.upcast_ref::<gtk4::Widget>()));
    }

    for (name, widget) in targets {
        let (min_w, nat_w, _, _) = widget.measure(gtk4::Orientation::Horizontal, -1);
        let alloc_with_margins = widget
            .compute_bounds(&w.window)
            .map(|b| b.width().round() as i32)
            .unwrap_or_else(|| widget.width())
            + widget.margin_start()
            + widget.margin_end();
        let child_clip = if let Ok(btn) = widget.clone().downcast::<gtk4::Button>() {
            btn.child().is_some_and(|ch| {
                let (_, cn, _, _) = ch.measure(gtk4::Orientation::Horizontal, -1);
                ch.width() < cn
            })
        } else {
            false
        };
        if alloc_with_margins < min_w || child_clip {
            return Err(format!(
                "{tag}: {name} clipped/truncated (alloc={alloc_with_margins} < min={min_w}, nat={nat_w}, child_clip={child_clip})"
            ));
        }
    }

    let bot_w = if check_player {
        check_row_no_overlap(tag, "bottom_bar", &w.window, &w.bottom_bar, 4.0)?;
        let bw = w
            .bottom_bar
            .compute_bounds(&w.window)
            .map(|b| b.width().round() as i32)
            .unwrap_or_else(|| w.bottom_bar.width());
        if bw > win_w {
            return Err(format!(
                "{tag}: bottom_bar overflows window ({bw} > {win_w})"
            ));
        }
        bw
    } else {
        0
    };

    println!(
        "UI_QA [{tag}]: win_w={win_w} close_right={close_right:.1} right_gap={right_gap:.1} bottom_w={bot_w} no-overlap OK"
    );
    Ok(())
}

fn run_stress_suite(
    window: &adw::ApplicationWindow,
    engine: &Rc<PlayerEngine>,
    cpu_sampler: &Rc<RefCell<CpuSampler>>,
    open_media: &Rc<dyn Fn(String)>,
    apply_speed: &Rc<dyn Fn(f64)>,
    qa: UiQaWidgets,
) {
    let clips = match ensure_stress_clips() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Stress clip prep failed: {e}");
            window.close();
            return;
        }
    };

    let monitor_done = Rc::new(Cell::new(false));
    let stress_max_stall_ms = Rc::new(Cell::new(0u128));
    let stress_armed = Rc::new(Cell::new(false));
    let ui_qa_errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let last_tick = Rc::new(RefCell::new(Instant::now()));

    {
        let monitor_done = Rc::clone(&monitor_done);
        let stress_max_stall_ms = Rc::clone(&stress_max_stall_ms);
        let stress_armed = Rc::clone(&stress_armed);
        let last_tick = Rc::clone(&last_tick);
        glib::timeout_add_local(Duration::from_millis(25), move || {
            if monitor_done.get() {
                return glib::ControlFlow::Break;
            }
            let elapsed = last_tick.borrow().elapsed().as_millis();
            *last_tick.borrow_mut() = Instant::now();
            let excess = elapsed.saturating_sub(25);
            if stress_armed.get() && excess > stress_max_stall_ms.get() {
                stress_max_stall_ms.set(excess);
            }
            glib::ControlFlow::Continue
        });
    }

    // Check 1 (0.35s): Welcome screen right-aligned header pinning + clean CTA text + key capture
    {
        let qa = qa.clone();
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(350), move || {
            if let Err(e) = verify_ui_layout("welcome-default", &qa, false) {
                eprintln!("UI_QA_ERR: {e}");
                errs.borrow_mut().push(e);
            }
            let welcome_lbl = qa.welcome_open_btn.label().unwrap_or_default();
            if welcome_lbl.contains("...") || welcome_lbl != "Open Video" {
                let msg = format!("welcome button unexpected label: '{welcome_lbl}'");
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [welcome-open-label]: '{welcome_lbl}' OK");
            }
            if qa.key_ctrl.propagation_phase() != gtk4::PropagationPhase::Capture {
                let msg = "key_ctrl must use PropagationPhase::Capture for Spacebar".to_string();
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [key-capture-phase]: Capture OK");
            }
        });
    }

    // 0.45s: Open 1080p60 VP9 + verify top bezel auto-hides on playback start
    {
        let open_cb = Rc::clone(open_media);
        let vp9 = clips[0].clone();
        let qa = qa.clone();
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            open_cb(vp9);
            if qa.header_rev.reveals_child() {
                let msg = "top header should auto-hide when playback starts".to_string();
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [bezel-less-top-on-play]: header_hidden=true OK");
            }
        });
    }

    for (idx, (ms, spd, seek_t)) in [
        (700u64, 1.25_f64, 1.5_f64),
        (1150, 1.5, 4.0),
        (1600, 2.0, 2.0),
        (2050, 1.0, 0.5),
    ]
    .iter()
    .enumerate()
    {
        let eng = Rc::clone(engine);
        let set_spd = Rc::clone(apply_speed);
        let armed = Rc::clone(&stress_armed);
        let s = *spd;
        let t = *seek_t;
        glib::timeout_add_local_once(Duration::from_millis(*ms), move || {
            if idx == 0 {
                armed.set(true);
            }
            set_spd(s);
            eng.seek_secs(t);
        });
    }

    // Check 2 (2.4s -> 2.75s -> 2.95s): Small window resize (580x360) + verify top bezel stays
    // hidden during resize + reveal header to verify non-overlap QA + hour-long timestamp
    {
        let win = qa.window.clone();
        let header_rev = qa.header_rev.clone();
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(2400), move || {
            win.set_default_size(580, 360);
            if header_rev.reveals_child() {
                let msg = "top header popped up during window resize".to_string();
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [no-top-bezel-on-resize]: header_hidden=true OK");
            }
        });
    }
    {
        let qa = qa.clone();
        glib::timeout_add_local_once(Duration::from_millis(2750), move || {
            // Temporarily reveal top header so verify_ui_layout can measure both header & bottom bar at 580x360
            (qa.show_all)();
            qa.time_label.set_text("1:00:10 / 1:19:15");
        });
    }
    {
        let qa = qa.clone();
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(2950), move || {
            if let Err(e) = verify_ui_layout("small-580x360-hour-ts", &qa, true) {
                eprintln!("UI_QA_ERR: {e}");
                errs.borrow_mut().push(e);
            }
            // Test Single-Tap Hide-All -> Verify all controls & HUD hide -> Restore cleanly
            (qa.hide_all)(true);
            if qa.bot_rev.reveals_child()
                || qa.header_rev.reveals_child()
                || qa.hud_label.is_visible()
            {
                let msg = "single-tap hide_all left visible controls".to_string();
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [single-tap-hide-all]: hidden=true OK");
            }
            (qa.show_all)();
            if !qa.bot_rev.reveals_child() || !qa.header_rev.reveals_child() {
                let msg = "single-tap show_all failed to restore controls".to_string();
                eprintln!("UI_QA_ERR: {msg}");
                errs.borrow_mut().push(msg);
            } else {
                println!("UI_QA [single-tap-restore]: restored=true OK");
            }
        });
    }

    // Check 3 (3.2s -> 3.6s -> 4.0s): Fullscreen <-> Windowed roundtrip + Play/Pause toggle
    {
        let win = qa.window.clone();
        let eng = Rc::clone(engine);
        glib::timeout_add_local_once(Duration::from_millis(3200), move || {
            eng.set_playing(false);
            win.fullscreen();
        });
    }
    {
        let win = qa.window.clone();
        let eng = Rc::clone(engine);
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(3600), move || {
            if eng.is_playing() {
                errs.borrow_mut().push("pause state did not stick".into());
            }
            if !win.is_fullscreen() {
                errs.borrow_mut().push("fullscreen() did not enter fullscreen".into());
            } else {
                println!("UI_QA [fullscreen-enter]: is_fullscreen=true OK");
            }
            eng.set_playing(true);
            win.unfullscreen();
        });
    }
    {
        let win = qa.window.clone();
        let eng = Rc::clone(engine);
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(4050), move || {
            if !eng.is_playing() {
                errs.borrow_mut().push("resume state did not stick".into());
            }
            if win.is_fullscreen() {
                errs.borrow_mut().push("unfullscreen() did not restore windowed mode".into());
            } else {
                println!("UI_QA [fullscreen-exit]: is_fullscreen=false OK");
            }
        });
    }

    // 4.5s: Print VP9 snapshot, switch to H.264 + 1.5x speed
    {
        let eng = Rc::clone(engine);
        let cpu = Rc::clone(cpu_sampler);
        let open_cb = Rc::clone(open_media);
        let set_spd = Rc::clone(apply_speed);
        let h264_path = clips[1].clone();
        glib::timeout_add_local_once(Duration::from_millis(4500), move || {
            let t = collect_telemetry(&eng, &mut cpu.borrow_mut());
            println!("=== STRESS SNAPSHOT 1 (4.5s VP9 @ {:.2}x) ===", eng.rate());
            println!("{}", format_hud(&t));
            open_cb(h264_path);
            set_spd(1.5);
        });
    }

    // 8.0s: Print H.264 snapshot, switch to HEVC/H.265 + 1.25x speed
    {
        let eng = Rc::clone(engine);
        let cpu = Rc::clone(cpu_sampler);
        let open_cb = Rc::clone(open_media);
        let set_spd = Rc::clone(apply_speed);
        let hevc_path = clips[2].clone();
        glib::timeout_add_local_once(Duration::from_millis(8000), move || {
            let t = collect_telemetry(&eng, &mut cpu.borrow_mut());
            println!("=== STRESS SNAPSHOT 2 (8.0s H.264 @ {:.2}x) ===", eng.rate());
            println!("{}", format_hud(&t));
            open_cb(hevc_path);
            set_spd(1.25);
        });
    }

    // 11.5s: Print HEVC snapshot + UI QA + stall summary and close cleanly
    {
        let eng = Rc::clone(engine);
        let cpu = Rc::clone(cpu_sampler);
        let win = window.clone();
        let monitor_done = Rc::clone(&monitor_done);
        let stress_max_stall_ms = Rc::clone(&stress_max_stall_ms);
        let errs = Rc::clone(&ui_qa_errors);
        glib::timeout_add_local_once(Duration::from_millis(11500), move || {
            monitor_done.set(true);
            let t = collect_telemetry(&eng, &mut cpu.borrow_mut());
            println!("=== STRESS SNAPSHOT 3 (11.5s HEVC @ {:.2}x) ===", eng.rate());
            println!("{}", format_hud(&t));
            println!("=== STRESS SUMMARY ===");
            println!("max_ui_stall_ms={}", stress_max_stall_ms.get());
            let err_list = errs.borrow();
            if err_list.is_empty() {
                println!("ui_layout_qa=PASS");
            } else {
                eprintln!("ui_layout_qa=FAIL: {}", err_list.join(" | "));
            }
            eng.stop();
            win.close();
        });
    }
}

fn ensure_stress_clips() -> Result<[String; 3], String> {
    let dir = PathBuf::from("/tmp/vp-bench");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create /tmp/vp-bench: {e}"))?;

    let specs = [
        (
            "vp9_1080p60.webm",
            vec![
                "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=8",
                "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=8",
                "-c:v", "libvpx-vp9", "-b:v", "4M", "-deadline", "realtime", "-cpu-used", "8",
                "-row-mt", "1", "-c:a", "libopus", "-b:a", "96k",
            ],
        ),
        (
            "h264_1080p60.mp4",
            vec![
                "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=8",
                "-f", "lavfi", "-i", "sine=frequency=523:sample_rate=48000:duration=8",
                "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart",
            ],
        ),
        (
            "hevc_1080p60.mkv",
            vec![
                "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=8",
                "-f", "lavfi", "-i", "sine=frequency=659:sample_rate=48000:duration=8",
                "-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error",
                "-pix_fmt", "yuv420p", "-c:a", "libopus", "-b:a", "96k",
            ],
        ),
    ];

    let mut out = Vec::with_capacity(3);
    for (file, args) in specs {
        let p = dir.join(file);
        if !p.exists() {
            let status = Command::new("ffmpeg")
                .args(&args)
                .arg(&p)
                .status()
                .map_err(|e| format!("ffmpeg failed: {e}"))?;
            if !status.success() {
                return Err(format!("ffmpeg exited {status} creating {file}"));
            }
        }
        out.push(p.display().to_string());
    }
    Ok([out[0].clone(), out[1].clone(), out[2].clone()])
}

pub fn normalize_media_uri(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("No video file selected.".into());
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        if trimmed.starts_with("file://")
            && gio::File::for_uri(trimmed)
                .path()
                .is_some_and(|p| p.is_dir())
        {
            return Err("Selected item is a folder — please choose a video file.".into());
        }
        return Ok(trimmed.to_string());
    }
    let path = Path::new(trimmed);
    if !path.exists() {
        return Err(format!("File not found: {trimmed}"));
    }
    if path.is_dir() {
        return Err(format!("'{trimmed}' is a folder — please choose a video file."));
    }
    let abs = path
        .canonicalize()
        .map_err(|e| format!("Cannot open '{trimmed}': {e}"))?;
    Ok(gio::File::for_path(&abs).uri().to_string())
}

pub fn display_name_for_media(uri: &str) -> String {
    if uri.starts_with("file://") {
        if let Some(base) = gio::File::for_uri(uri).basename() {
            return base.to_string_lossy().to_string();
        }
    }
    uri.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Video")
        .to_string()
}

pub fn format_hud(t: &PlaybackTelemetry) -> String {
    let pwr = match t.battery_watts {
        Some(w) => format!("{w:.2} W ({})", t.battery_status),
        None => t.battery_status.clone(),
    };
    format!(
        "Video Decoder : {} (open ready: {} ms)\n\
         Audio Decoder : {}\n\
         Memory Path   : {} ({})\n\
         Resolution    : {} @ {:.1} fps (rendered: {}, dropped: {})\n\
         NVIDIA RTD3   : {}  |  AMD iGPU: {}\n\
         App Footprint : {:.1} MiB RSS  |  CPU: {:.1}% ({}T)\n\
         Drive I/O Ops : {} syscalls (1 MiB block)\n\
         Power & Fans  : {}  |  Fan: {} RPM",
        t.decoder_name,
        t.open_ready_ms,
        t.audio_decoder_name,
        t.memory_path,
        t.drm_format,
        t.resolution,
        t.fps_avg,
        t.rendered_frames,
        t.dropped_frames,
        t.nvidia_rtd3,
        t.amd_status,
        t.rss_mib,
        t.cpu_percent,
        t.cpu_threads,
        t.read_syscalls,
        pwr,
        t.fan_rpm
    )
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
    fn normalizes_paths_with_special_chars() {
        let p = PathBuf::from("/tmp/vp_uri_test #1.mp4");
        let _ = std::fs::write(&p, b"");
        let uri = normalize_media_uri(p.to_str().unwrap()).expect("should normalize existing file");
        let _ = std::fs::remove_file(&p);
        assert!(uri.starts_with("file:///tmp/"));
        assert!(uri.contains("%231.mp4"));
        assert_eq!(display_name_for_media(&uri), "vp_uri_test #1.mp4");
    }
}
