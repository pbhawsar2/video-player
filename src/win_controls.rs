use crate::theme::spx;
use gtk4::cairo;
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinCtrlKind {
    Minimize,
    Maximize,
    Fullscreen,
    Close,
}

/// Custom vector-drawn window control button matching the dark obsidian `#111927` / `#26334A`
/// and `#38BDF8` Cyan (or `#F87171` Coral for Close) aesthetic of `system-monitor`.
pub fn make_win_ctrl_button<W, F>(
    kind: WinCtrlKind,
    window: &W,
    scale: f64,
    on_click: F,
) -> gtk4::DrawingArea
where
    W: IsA<gtk4::Window>,
    F: Fn() + 'static,
{
    let da = gtk4::DrawingArea::builder()
        .content_width(spx(24.0, scale))
        .content_height(spx(22.0, scale))
        .focusable(false)
        .can_focus(false)
        .build();

    let tooltip = match kind {
        WinCtrlKind::Minimize => "Minimize window",
        WinCtrlKind::Maximize => "Maximize / Restore window",
        WinCtrlKind::Fullscreen => "Toggle Fullscreen (F)",
        WinCtrlKind::Close => "Close window",
    };
    da.set_tooltip_text(Some(tooltip));
    da.set_cursor_from_name(Some("pointer"));

    let hovered = Rc::new(Cell::new(false));
    let pressed = Rc::new(Cell::new(false));
    let active_state = Rc::new(Cell::new(false));

    // Sync Maximize / Fullscreen icon shape dynamically with GTK window state
    match kind {
        WinCtrlKind::Maximize => {
            active_state.set(window.is_maximized());
            let active_clone = Rc::clone(&active_state);
            let da_weak = da.downgrade();
            window.connect_maximized_notify(move |w| {
                active_clone.set(w.is_maximized());
                if let Some(d) = da_weak.upgrade() {
                    d.queue_draw();
                }
            });
        }
        WinCtrlKind::Fullscreen => {
            active_state.set(window.is_fullscreen());
            let active_clone = Rc::clone(&active_state);
            let da_weak = da.downgrade();
            window.connect_fullscreened_notify(move |w| {
                active_clone.set(w.is_fullscreen());
                if let Some(d) = da_weak.upgrade() {
                    d.queue_draw();
                }
            });
        }
        _ => {}
    }

    let motion = gtk4::EventControllerMotion::new();
    {
        let h = Rc::clone(&hovered);
        let da_weak = da.downgrade();
        motion.connect_enter(move |_, _, _| {
            h.set(true);
            if let Some(d) = da_weak.upgrade() {
                d.queue_draw();
            }
        });
    }
    {
        let h = Rc::clone(&hovered);
        let p = Rc::clone(&pressed);
        let da_weak = da.downgrade();
        motion.connect_leave(move |_| {
            h.set(false);
            p.set(false);
            if let Some(d) = da_weak.upgrade() {
                d.queue_draw();
            }
        });
    }
    da.add_controller(motion);

    let click = gtk4::GestureClick::new();
    {
        let p = Rc::clone(&pressed);
        let da_weak = da.downgrade();
        click.connect_pressed(move |_, _, _, _| {
            p.set(true);
            if let Some(d) = da_weak.upgrade() {
                d.queue_draw();
            }
        });
    }
    {
        let p = Rc::clone(&pressed);
        let h = Rc::clone(&hovered);
        let da_weak = da.downgrade();
        click.connect_released(move |_, _, _, _| {
            let was_pressed = p.replace(false);
            if let Some(d) = da_weak.upgrade() {
                d.queue_draw();
            }
            if was_pressed && h.get() {
                on_click();
            }
        });
    }
    da.add_controller(click);

    {
        let hovered = Rc::clone(&hovered);
        let pressed = Rc::clone(&pressed);
        let active_state = Rc::clone(&active_state);
        da.set_draw_func(move |_da, cr, w, h| {
            draw_button(
                cr,
                w as f64,
                h as f64,
                kind,
                hovered.get(),
                pressed.get(),
                active_state.get(),
            );
        });
    }

    da
}

fn draw_button(
    cr: &cairo::Context,
    w: f64,
    h: f64,
    kind: WinCtrlKind,
    hovered: bool,
    pressed: bool,
    active_state: bool,
) {
    cr.set_antialias(cairo::Antialias::Best);

    let x = 0.5;
    let y = 0.5;
    let rw = (w - 1.0).max(1.0);
    let rh = (h - 1.0).max(1.0);
    let radius = (h * 0.22).clamp(4.5, 6.5);

    // Subtle obsidian & muted slate palette with clean anti-aliased edge contrast
    let (bg, border, fg) = if kind == WinCtrlKind::Close {
        if pressed {
            ((0.24, 0.08, 0.11), (0.973, 0.443, 0.443), (1.0, 1.0, 1.0))
        } else if hovered {
            ((0.18, 0.07, 0.10), (0.85, 0.35, 0.38), (0.973, 0.443, 0.443))
        } else {
            ((0.075, 0.110, 0.173), (0.133, 0.184, 0.271), (0.706, 0.765, 0.839))
        }
    } else if pressed {
        ((0.043, 0.067, 0.118), (0.220, 0.741, 0.973), (0.973, 0.980, 0.988))
    } else if hovered || active_state {
        ((0.095, 0.153, 0.247), (0.220, 0.65, 0.90), (0.973, 0.980, 0.988))
    } else {
        ((0.075, 0.110, 0.173), (0.133, 0.184, 0.271), (0.706, 0.765, 0.839))
    };

    draw_rounded_rect(cr, x, y, rw, rh, radius);
    cr.set_source_rgb(bg.0, bg.1, bg.2);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(border.0, border.1, border.2);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    cr.set_source_rgb(fg.0, fg.1, fg.2);
    cr.set_line_width((h * 0.06).clamp(1.55, 2.2));
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    let cx = w * 0.5;
    let cy = h * 0.5;
    let unit = (h * 0.18).clamp(4.0, 7.0);

    match kind {
        WinCtrlKind::Minimize => {
            let line_y = (cy + unit * 0.45).round() + 0.5;
            cr.move_to(cx - unit, line_y);
            cr.line_to(cx + unit, line_y);
            let _ = cr.stroke();
        }
        WinCtrlKind::Maximize => {
            if !active_state {
                let s = unit * 1.85;
                draw_rounded_rect(cr, cx - s * 0.5, cy - s * 0.5, s, s, 1.5);
                let _ = cr.stroke();
            } else {
                let s = unit * 1.65;
                let off = unit * 0.34;
                let bx = cx - s * 0.5 + off;
                let by = cy - s * 0.5 - off;
                cr.move_to(bx + 1.5, by);
                cr.line_to(bx + s, by);
                cr.line_to(bx + s, by + s - 1.5);
                let _ = cr.stroke();

                let fx = cx - s * 0.5 - off;
                let fy = cy - s * 0.5 + off;
                draw_rounded_rect(cr, fx, fy, s, s, 1.4);
                cr.set_source_rgb(bg.0, bg.1, bg.2);
                let _ = cr.fill_preserve();
                cr.set_source_rgb(fg.0, fg.1, fg.2);
                let _ = cr.stroke();
            }
        }
        WinCtrlKind::Fullscreen => {
            let r = unit * 1.02;
            let arm = unit * 0.66;
            if !active_state {
                cr.move_to(cx - r, cy - r + arm);
                cr.line_to(cx - r, cy - r);
                cr.line_to(cx - r + arm, cy - r);
                cr.move_to(cx + r - arm, cy - r);
                cr.line_to(cx + r, cy - r);
                cr.line_to(cx + r, cy - r + arm);
                cr.move_to(cx + r, cy + r - arm);
                cr.line_to(cx + r, cy + r);
                cr.line_to(cx + r - arm, cy + r);
                cr.move_to(cx - r + arm, cy + r);
                cr.line_to(cx - r, cy + r);
                cr.line_to(cx - r, cy + r - arm);
            } else {
                let ir = unit * 0.36;
                cr.move_to(cx - r, cy - ir);
                cr.line_to(cx - ir, cy - ir);
                cr.line_to(cx - ir, cy - r);
                cr.move_to(cx + r, cy - ir);
                cr.line_to(cx + ir, cy - ir);
                cr.line_to(cx + ir, cy - r);
                cr.move_to(cx + r, cy + ir);
                cr.line_to(cx + ir, cy + ir);
                cr.line_to(cx + ir, cy + r);
                cr.move_to(cx - r, cy + ir);
                cr.line_to(cx - ir, cy + ir);
                cr.line_to(cx - ir, cy + r);
            }
            let _ = cr.stroke();
        }
        WinCtrlKind::Close => {
            let r = unit * 0.92;
            cr.move_to(cx - r, cy - r);
            cr.line_to(cx + r, cy + r);
            cr.move_to(cx + r, cy - r);
            cr.line_to(cx - r, cy + r);
            let _ = cr.stroke();
        }
    }
}

fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let deg = std::f64::consts::PI / 180.0;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -90.0 * deg, 0.0 * deg);
    cr.arc(x + w - r, y + h - r, r, 0.0 * deg, 90.0 * deg);
    cr.arc(x + r, y + h - r, r, 90.0 * deg, 180.0 * deg);
    cr.arc(x + r, y + r, r, 180.0 * deg, 270.0 * deg);
    cr.close_path();
}
