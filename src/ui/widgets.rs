//! Small pieces the pages share: panel outline, smooth curve, section titles, the on/off switch,
//! the mini battery and the bottom sheet frame.

use tiny_skia::{Path, PathBuilder, Shader};

use super::{Action, GAP, HEIGHT, RADIUS, Ui, WIDTH};
use crate::device::NoiseMode;
use crate::render::painter::Painter;
use crate::render::{GREEN, SHEET, TEXT, TEXT2, color, icons, shapes};

/// Panel outline with the tail pointing at the tray icon (below, or above with the bar on top).
pub(super) fn panel_outline(tip_x: f32, on_top: bool, h: f32) -> Path {
    let (w, r, k, bx) = (WIDTH, RADIUS, RADIUS * 0.4477, tip_x);
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    if on_top {
        pb.line_to(bx - 20.0, 0.0);
        pb.cubic_to(bx - 9.0, 0.0, bx - 5.0, -GAP + 1.0, bx, -GAP);
        pb.cubic_to(bx + 5.0, -GAP + 1.0, bx + 9.0, 0.0, bx + 20.0, 0.0);
    }
    pb.line_to(w - r, 0.0);
    pb.cubic_to(w - k, 0.0, w, k, w, r);
    pb.line_to(w, h - r);
    pb.cubic_to(w, h - k, w - k, h, w - r, h);
    if !on_top {
        pb.line_to(bx + 20.0, h);
        pb.cubic_to(bx + 9.0, h, bx + 5.0, h + GAP - 1.0, bx, h + GAP);
        pb.cubic_to(bx - 5.0, h + GAP - 1.0, bx - 9.0, h, bx - 20.0, h);
    }
    pb.line_to(r, h);
    pb.cubic_to(k, h, 0.0, h - k, 0.0, h - r);
    pb.line_to(0.0, r);
    pb.cubic_to(0.0, k, k, 0.0, r, 0.0);
    pb.close();
    pb.finish().unwrap()
}

/// Smooth curve (Catmull-Rom) through every point.
pub(super) fn smooth_curve(pts: &[(f32, f32)]) -> PathBuilder {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    let n = pts.len();
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (pts[i.saturating_sub(1)], pts[i], pts[i + 1], pts[(i + 2).min(n - 1)]);
        pb.cubic_to(
            p1.0 + (p2.0 - p0.0) / 6.0,
            p1.1 + (p2.1 - p0.1) / 6.0,
            p2.0 - (p3.0 - p1.0) / 6.0,
            p2.1 - (p3.1 - p1.1) / 6.0,
            p2.0,
            p2.1,
        );
    }
    pb
}

pub(super) fn section_title(p: &mut Painter, x: f32, y: f32, txt: &str) {
    p.text(x, y, txt, 11.0, true, color(TEXT2, 0.85), 0.0);
}

pub(super) fn mode_icon(m: NoiseMode) -> &'static str {
    match m {
        NoiseMode::Anc => icons::ANC,
        NoiseMode::Ambient => icons::AMBIENT,
        NoiseMode::TalkThru => icons::TALK,
        NoiseMode::Off => icons::OFF,
    }
}

/// Small battery + "L 96%"; returns where it ended.
pub(super) fn mini_battery(p: &mut Painter, x: f32, y: f32, label: &str, level: u8, charging: bool) -> f32 {
    let x = x + p.measure(label, 11.5, false) + 5.0;
    p.text(x - 5.0, y + 4.0, label, 11.5, false, color(TEXT2, 1.0), 1.0);
    p.stroke(&shapes::rounded_rect(x + 0.5, y - 4.5, 17.0, 9.0, 2.5), Shader::SolidColor(color(0xFFFFFF, 0.35)), 1.0);
    p.fill(&shapes::rounded_rect(x + 18.5, y - 1.5, 1.5, 3.0, 0.7), Shader::SolidColor(color(0xFFFFFF, 0.35)));
    let w = 14.0 * level.min(100) as f32 / 100.0;
    if w > 0.5 {
        let sh = if charging { Shader::SolidColor(color(GREEN, 1.0)) } else { shapes::accent(x + 2.0, 14.0, 1.0) };
        p.fill(&shapes::rounded_rect(x + 2.0, y - 3.0, w, 6.0, 1.5), sh);
    }
    let txt = format!("{level}%");
    p.text(x + 24.0, y + 4.0, &txt, 11.5, true, color(TEXT, 1.0), 0.0);
    x + 24.0 + p.measure(&txt, 11.5, true)
}

impl Ui {
    /// Glass on/off switch; (`x`, `y`) is the top-left of its 54×36 click area.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn switch(&mut self, p: &mut Painter, x: f32, y: f32, on: bool, enabled: bool, action: Action) {
        if enabled {
            self.target(p, x - 4.0, y - 4.0, 54.0, 36.0, action);
        }
        let mut g = self.glass(action, p);
        g.on = self.active(action, on);
        // 38×22, against the right edge and centered on the click area's row
        let (x, y) = (x + 8.0, y + 3.0);
        p.glass(x, y, 38.0, 22.0, 11.0, g);
        let kx = x + 11.0 + g.on * 16.0;
        p.fill(&shapes::circle(kx, y + 11.8, 8.5), Shader::SolidColor(color(0, 0.28)));
        let knob = [(0.0, color(0xFFFFFF, 1.0)), (1.0, color(0xDCDFE6, 1.0))];
        p.fill(&shapes::circle(kx, y + 11.0, 8.5), shapes::linear(0.0, y + 2.5, 0.0, y + 19.5, &knob));
    }

    /// Dims the page and slides a sheet `h` tall up from the bottom; returns its top.
    pub(super) fn sheet_frame(&mut self, p: &mut Painter, h: f32) -> f32 {
        let t = self.sheet_t;
        p.fill(&shapes::rounded_rect(0.0, 0.0, WIDTH, HEIGHT, 0.0), Shader::SolidColor(color(0, 0.55 * t)));
        if self.sheet_open {
            self.target(p, 0.0, 0.0, WIDTH, HEIGHT, Action::Dismiss);
        }
        let top = HEIGHT - h * t;
        // taller than needed: the panel's rounded bottom clips it
        p.fill(&shapes::rounded_rect(0.0, top, WIDTH, h + 40.0, 22.0), Shader::SolidColor(color(SHEET, 1.0)));
        let rim = [(0.0, color(0xFFFFFF, 0.16)), (1.0, color(0xFFFFFF, 0.0))];
        p.stroke(&shapes::rounded_rect(0.5, top + 0.5, WIDTH - 1.0, h + 40.0, 21.5), shapes::linear(0.0, top, 0.0, top + 40.0, &rim), 1.0);
        p.fill(&shapes::rounded_rect(WIDTH / 2.0 - 18.0, top + 8.0, 36.0, 4.0, 2.0), Shader::SolidColor(color(0xFFFFFF, 0.18)));
        top
    }
}
