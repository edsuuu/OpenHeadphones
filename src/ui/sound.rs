//! Sound tab: noise control, ANC options and the equalizer.

use tiny_skia::Shader;

use super::widgets::{mode_icon, section_title};
use super::{Action, CONTENT_Y, Ui, WIDTH};
use crate::device::{DeviceState, NoiseMode};
use crate::i18n::{t, tf};
use crate::profile::Profile;
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, color, shapes};

impl Ui {
    pub(super) fn draw_sound(&mut self, p: &mut Painter, state: &DeviceState, profile: &Profile) {
        let mut y = CONTENT_Y;
        let modes = &profile.noise_modes;
        if !modes.is_empty() {
            section_title(p, 16.0, y, &t("noise.section"));
            let n = modes.len() as f32;
            let w = (WIDTH - 32.0 - (n - 1.0) * 8.0) / n;
            for (i, &m) in modes.iter().enumerate() {
                let (x, a, selected) = (16.0 + i as f32 * (w + 8.0), Action::Mode(m), state.mode == Some(m));
                self.target(p, x, y + 10.0, w, 66.0, a);
                let mut g = self.glass(a, p);
                g.on = self.active(a, selected);
                p.glass(x, y + 10.0, w, 66.0, 16.0, g);
                let c = color(if selected { 0xFFFFFF } else { TEXT2 }, 1.0);
                p.icon(mode_icon(m), x + w / 2.0, y + 36.0, 22.0, c, 1.7);
                p.text(x + w / 2.0, y + 63.0, &t(&format!("noise.{}", m.key())), 11.5, selected, c, 0.5);
            }
            y += 76.0;
        }

        // advanced ANC
        let anc = state.mode == Some(NoiseMode::Anc);
        let auto = profile.adaptive_anc && state.adaptive == Some(true);
        p.alpha = if anc { 1.0 } else { 0.35 };
        if profile.adaptive_anc {
            p.text(16.0, y + 30.0, &t("anc.adaptive"), 13.5, true, color(TEXT, 1.0), 0.0);
            let hint = t(if anc { "anc.adaptive_hint" } else { "anc.only_anc" });
            p.text(16.0, y + 47.0, &hint, 11.5, false, color(TEXT2, 1.0), 0.0);
            self.switch(p, WIDTH - 62.0, y + 18.0, auto, anc, Action::Adaptive);
            y += 48.0;
        }
        let max = profile.anc_levels;
        if max > 0 {
            p.text(16.0, y + 30.0, &t("anc.intensity"), 13.5, true, color(TEXT, 1.0), 0.0);
            let level = state.anc_level.map_or("–".to_string(), |n| n.to_string());
            let hint = if auto { t("anc.automatic") } else { tf("anc.level", &[("n", &level), ("max", &max)]) };
            p.text(16.0, y + 47.0, &hint, 11.5, false, color(TEXT2, 1.0), 0.0);
            let x0 = WIDTH - 16.0 - (max as f32 * 18.0 + (max - 1) as f32 * 5.0);
            if auto {
                p.alpha *= 0.45;
            }
            for n in 1..=max {
                let (x, yb, a) = (x0 + (n - 1) as f32 * 23.0, y + 19.0, Action::Level(n));
                if anc && !auto {
                    self.target(p, x - 2.5, yb - 4.0, 23.0, 34.0, a);
                }
                let mut g = self.glass(a, p);
                g.on = self.active(a, state.anc_level.is_some_and(|k| k >= n));
                p.glass(x, yb, 18.0, 26.0, 6.0, g);
                let bar = 3.0 + 12.0 * n as f32 / max as f32;
                let fill = Shader::SolidColor(color(0xFFFFFF, 0.3 + 0.5 * g.on));
                p.fill(&shapes::rounded_rect(x + 6.0, yb + 21.0 - bar, 6.0, bar, 1.5), fill);
            }
            y += 48.0;
        }
        p.alpha = 1.0;

        if let Some(eq) = &profile.eq {
            self.draw_eq(p, eq, y);
        }
    }
}
