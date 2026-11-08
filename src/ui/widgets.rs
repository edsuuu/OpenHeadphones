//! Small pieces the pages share: smooth curve, section titles, the on/off switch, the battery
//! bar, the segmented control, the grouped card and the bottom sheet frame.

use tiny_skia::{Color, PathBuilder, Shader};

use super::{Action, HEIGHT, RADIUS, Ui, WIDTH};
use crate::device::NoiseMode;
use crate::render::painter::{Glass, Painter};
use crate::render::{GREEN, SHEET, TEXT2, TEXT3, color, icons, shapes};

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

/// Small caps-like title over a group: `y` is the baseline.
pub(super) fn section_title(p: &mut Painter, x: f32, y: f32, txt: &str) {
    p.text(x, y, &txt.to_uppercase(), 9.5, true, color(TEXT3, 1.0), 0.0);
}

pub(super) fn mode_icon(m: NoiseMode) -> &'static str {
    match m {
        NoiseMode::Anc => icons::ANC,
        NoiseMode::Ambient => icons::AMBIENT,
        NoiseMode::TalkThru => icons::TALK,
        NoiseMode::Off => icons::OFF,
    }
}

/// A capsule 8 px tall from (`x`, `y` = its top), filled to `level` %: green while charging.
pub(super) fn battery_bar(p: &mut Painter, x: f32, y: f32, w: f32, level: u8, charging: bool) {
    p.fill(&shapes::rounded_rect(x, y, w, 8.0, 4.0), Shader::SolidColor(color(0xFFFFFF, 0.10)));
    let f = w * level.min(100) as f32 / 100.0;
    if f > 1.0 {
        let sh = if charging { Shader::SolidColor(color(GREEN, 1.0)) } else { shapes::accent(x, w, 1.0) };
        p.fill(&shapes::rounded_rect(x, y, f.max(8.0), 8.0, 4.0), sh);
    }
}

/// `a` → `b` by `t`.
fn mix(a: u32, b: u32, t: f32) -> Color {
    let (a, b) = (color(a, 1.0), color(b, 1.0));
    let l = |x: f32, y: f32| x + (y - x) * t.clamp(0.0, 1.0);
    Color::from_rgba(l(a.red(), b.red()), l(a.green(), b.green()), l(a.blue(), b.blue()), 1.0).unwrap()
}

impl Ui {
    /// On/off switch; (`x`, `y`) is the top-left of its 54×36 click area.
    pub(super) fn switch(&mut self, p: &mut Painter, x: f32, y: f32, on: bool, enabled: bool, action: Action) {
        if enabled {
            self.target(p, x - 4.0, y - 4.0, 54.0, 36.0, action);
        }
        let mut g = self.glass(action);
        g.on = self.active(action, on);
        // 38×22, against the right edge and centered on the click area's row
        let (x, y) = (x + 8.0, y + 3.0);
        let track = shapes::rounded_rect(x, y, 38.0, 22.0, 11.0);
        p.fill(&track, Shader::SolidColor(color(0xFFFFFF, 0.12 + 0.04 * g.hover)));
        if g.on > 0.0 {
            p.fill(&track, shapes::accent(x, 38.0, g.on));
        }
        let kx = x + 11.0 + g.on * 16.0;
        p.fill(&shapes::circle(kx, y + 11.8, 8.5), Shader::SolidColor(color(0, 0.25)));
        p.fill(&shapes::circle(kx, y + 11.0, 8.5), Shader::SolidColor(color(0xFFFFFF, 1.0)));
    }

    /// Segmented control: a dark track and one pill that slides to the selected segment (it sits
    /// where the segments' animated "on" values put it). `draw(p, k, cx, cy, color)` fills segment k.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn segmented(
        &mut self,
        p: &mut Painter,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        items: &[(Action, bool)],
        live: bool,
        accent: bool,
        mut draw: impl FnMut(&mut Painter, usize, f32, f32, Color),
    ) {
        let r = if h > 44.0 { 14.0 } else { 11.0 };
        p.fill(&shapes::rounded_rect(x, y, w, h, r), Shader::SolidColor(color(0, 0.28)));
        let sw = (w - 6.0) / items.len() as f32;
        let seg = |k: f32| shapes::rounded_rect(x + 3.0 + k * sw, y + 3.0, sw, h - 6.0, r - 3.0);
        let mut ons = vec![];
        for (k, &(a, selected)) in items.iter().enumerate() {
            if live {
                self.target(p, x + 3.0 + k as f32 * sw, y, sw, h, a);
            }
            let hover = self.glass(a).hover;
            if hover > 0.01 {
                p.fill(&seg(k as f32), Shader::SolidColor(color(0xFFFFFF, 0.06 * hover)));
            }
            ons.push(self.active(a, selected));
        }
        let total: f32 = ons.iter().sum();
        if total > 0.01 {
            let k = ons.iter().enumerate().map(|(k, on)| k as f32 * on).sum::<f32>() / total;
            let px = x + 3.0 + k * sw;
            let a = total.min(1.0);
            p.fill(&seg(k), if accent { shapes::accent(px, sw, a) } else { Shader::SolidColor(color(0xFFFFFF, 0.12 * a)) });
        }
        for (k, on) in ons.into_iter().enumerate() {
            draw(p, k, x + 3.0 + (k as f32 + 0.5) * sw, y + h / 2.0, mix(TEXT2, 0xFFFFFF, on));
        }
    }

    /// Card holding rows (a grouped list): the glass, hairlines between the rows and, for rows
    /// that are a button, the click area and a hover pill. `rows` = (height, action).
    pub(super) fn group(&mut self, p: &mut Painter, y: f32, rows: &[(f32, Option<Action>)]) {
        let h: f32 = rows.iter().map(|r| r.0).sum();
        p.glass(16.0, y, WIDTH - 32.0, h, 14.0, Glass::default());
        let mut ry = y;
        for (k, &(rh, a)) in rows.iter().enumerate() {
            if let Some(a) = a {
                self.target(p, 16.0, ry, WIDTH - 32.0, rh, a);
                let hover = self.glass(a).hover;
                if hover > 0.01 {
                    p.fill(&shapes::rounded_rect(19.0, ry + 3.0, WIDTH - 38.0, rh - 6.0, 11.0), Shader::SolidColor(color(0xFFFFFF, 0.05 * hover)));
                }
            }
            if k > 0 {
                p.fill(&shapes::rounded_rect(30.0, ry - 0.5, WIDTH - 46.0, 1.0, 0.0), Shader::SolidColor(color(0xFFFFFF, 0.06)));
            }
            ry += rh;
        }
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
        p.fill(&shapes::rounded_rect(0.0, top, WIDTH, h + 40.0, RADIUS), Shader::SolidColor(color(SHEET, 1.0)));
        p.stroke(&shapes::rounded_rect(0.5, top + 0.5, WIDTH - 1.0, h + 40.0, RADIUS - 0.5), Shader::SolidColor(color(0xFFFFFF, 0.09)), 1.0);
        p.fill(&shapes::rounded_rect(WIDTH / 2.0 - 18.0, top + 8.0, 36.0, 4.0, 2.0), Shader::SolidColor(color(0xFFFFFF, 0.18)));
        top
    }
}
