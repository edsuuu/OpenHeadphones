//! Sound tab: noise control, ANC options and the equalizer.

use tiny_skia::Shader;

use super::widgets::{mode_icon, section_title};
use super::{Action, CONTENT_Y, Ui, WIDTH};
use crate::device::{DeviceState, NoiseMode};
use crate::i18n::{t, tf};
use crate::profile::Profile;
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, color, shapes};

/// A row in the ANC card.
const ROW: f32 = 46.0;

impl Ui {
    pub(super) fn draw_sound(&mut self, p: &mut Painter, state: &DeviceState, profile: &Profile) {
        let mut y = CONTENT_Y;
        let modes = profile.noise_modes.clone();
        if !modes.is_empty() {
            section_title(p, 20.0, y + 8.0, &t("noise.section"));
            let items: Vec<_> = modes.iter().map(|&m| (Action::Mode(m), state.mode == Some(m))).collect();
            self.segmented(p, 16.0, y + 18.0, WIDTH - 32.0, 58.0, &items, true, true, |p, k, cx, cy, c| {
                p.icon(mode_icon(modes[k]), cx, cy - 8.0, 18.0, c, 1.7);
                p.text(cx, cy + 16.0, &t(&format!("noise.{}", modes[k].key())), 10.0, false, c, 0.5);
            });
            y += 88.0;
        }

        // advanced ANC, in one card: adaptive on/off, then one row of levels (the ambient level
        // in ambient mode, the ANC intensity otherwise)
        let anc = state.mode == Some(NoiseMode::Anc);
        let auto = profile.adaptive_anc && state.adaptive == Some(true);
        let ambient = state.mode == Some(NoiseMode::Ambient) && profile.ambient_levels > 0;
        let (max, current, title) = match ambient {
            true => (profile.ambient_levels, state.ambient_level, "anc.ambient_level"),
            false => (profile.anc_levels, state.anc_level, "anc.intensity"),
        };
        let rows = [(ROW, None)].repeat(profile.adaptive_anc as usize + (max > 0) as usize);
        if !rows.is_empty() {
            self.group(p, y, &rows);
        }
        if profile.adaptive_anc {
            p.alpha = if anc { 1.0 } else { 0.35 };
            p.text(30.0, y + 20.0, &t("anc.adaptive"), 12.0, false, color(TEXT, 1.0), 0.0);
            let hint = t(if anc { "anc.adaptive_hint" } else { "anc.only_anc" });
            p.text_fit(30.0, y + 35.0, &hint, 10.0, WIDTH - 130.0, false, color(TEXT2, 1.0), 0.0);
            self.switch(p, WIDTH - 76.0, y + 9.0, auto, anc, Action::Adaptive);
            y += ROW;
        }
        let auto = auto && !ambient;
        if max > 0 {
            p.alpha = if anc || ambient { 1.0 } else { 0.35 };
            let x0 = WIDTH - 30.0 - (max as f32 * 18.0 + (max - 1) as f32 * 5.0);
            p.text_fit(30.0, y + 20.0, &t(title), 12.0, x0 - 40.0, false, color(TEXT, 1.0), 0.0);
            let level = current.map_or("–".to_string(), |n| n.to_string());
            let hint = if auto { t("anc.automatic") } else { tf("anc.level", &[("n", &level), ("max", &max)]) };
            p.text_fit(30.0, y + 35.0, &hint, 10.0, x0 - 40.0, false, color(TEXT2, 1.0), 0.0);
            if auto {
                p.alpha *= 0.45;
            }
            for n in 1..=max {
                let a = if ambient { Action::AmbientLevel(n) } else { Action::Level(n) };
                let (x, yb) = (x0 + (n - 1) as f32 * 23.0, y + 10.0);
                if ambient || (anc && !auto) {
                    self.target(p, x - 2.5, yb - 4.0, 23.0, 34.0, a);
                }
                let mut g = self.glass(a);
                g.on = self.active(a, current.is_some_and(|k| k >= n));
                p.glass(x, yb, 18.0, 26.0, 6.0, g);
                let bar = 3.0 + 12.0 * n as f32 / max as f32;
                let fill = Shader::SolidColor(color(0xFFFFFF, 0.3 + 0.5 * g.on));
                p.fill(&shapes::rounded_rect(x + 6.0, yb + 21.0 - bar, 6.0, bar, 1.5), fill);
            }
            y += ROW;
        }
        p.alpha = 1.0;
        if !rows.is_empty() {
            y += 16.0;
        }

        if let Some(eq) = &profile.eq {
            self.draw_eq(p, eq, y);
        }
    }
}
