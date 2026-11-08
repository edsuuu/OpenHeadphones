//! The device list: a header and one card per device with its photo, batteries and noise modes.

use tiny_skia::Shader;

use super::widgets::{battery_bar, mode_icon};
use super::{Action, CARD_H, LIST_Y, Ui, WIDTH, display_name};
use crate::i18n::{t, tf};
use crate::render::painter::Painter;
use crate::render::{ACCENT1, GREEN, TEXT, TEXT2, TEXT3, color, icons, shapes};

impl Ui {
    pub(super) fn draw_list(&mut self, p: &mut Painter) {
        p.text(16.0, 31.0, &t("list.title"), 15.5, true, color(TEXT, 1.0), 0.0);
        let sub = match self.devices.iter().filter(|d| d.state.connected).count() {
            0 => t("list.none_connected"),
            1 => t("list.one_connected"),
            n => tf("list.many_connected", &[("n", &n)]),
        };
        p.text(WIDTH - 16.0, 31.0, &sub, 11.0, false, color(TEXT2, 1.0), 1.0);

        if self.devices.is_empty() {
            let profile = self.showcase();
            let (cx, cy) = (WIDTH / 2.0, 160.0);
            let time = self.started.elapsed().as_secs_f32();
            for k in 0..3 {
                let f = (time * 0.55 + k as f32 / 3.0).fract();
                p.stroke(&shapes::circle(cx, cy, 30.0 + f * 80.0), Shader::SolidColor(color(ACCENT1, (1.0 - f) * 0.45)), 1.5);
            }
            self.draw_photo(p, &profile, None, cx, cy, 84.0);
            p.text(cx, cy + 112.0, &t("list.searching"), 14.0, true, color(TEXT, 1.0), 0.5);
            p.text(cx, cy + 132.0, &t("list.searching_hint"), 11.0, false, color(TEXT2, 1.0), 0.5);
            return;
        }

        for i in 0..self.devices.len() {
            let (y, open) = (LIST_Y + i as f32 * (CARD_H + 12.0), Action::Open(i));
            self.target(p, 16.0, y, WIDTH - 32.0, CARD_H, open);
            p.glass(16.0, y, WIDTH - 32.0, CARD_H, 16.0, self.glass(open));
            let state = self.devices[i].state.clone();
            let profile = self.profile_for(&state);
            let alpha = p.alpha;
            if !state.connected {
                p.alpha *= 0.55;
            }

            // photo, name, what it is doing
            self.draw_photo(p, &profile, state.color, 62.0, y + 46.0, 64.0);
            p.text_fit(108.0, y + 42.0, &display_name(&state, &profile), 13.5, WIDTH - 160.0, true, color(TEXT, 1.0), 0.0);
            let status = match (state.connected, state.mode) {
                (true, Some(m)) => t(&format!("noise.{}_long", m.key())),
                (true, None) => t("device.connected"),
                (false, _) => t("list.take_out"),
            };
            p.fill(&shapes::circle(111.0, y + 57.5, 3.0), Shader::SolidColor(color(if state.connected { GREEN } else { TEXT3 }, 1.0)));
            p.text_fit(120.0, y + 61.0, &status, 11.0, WIDTH - 170.0, false, color(TEXT2, 1.0), 0.0);
            p.icon(icons::CHEVRON_RIGHT, WIDTH - 36.0, y + 46.0, 14.0, color(TEXT3, 1.0), 2.0);

            // batteries, spaced out: the first against the card's left edge, the last against its right
            if let Some(b) = state.battery.filter(|_| state.connected) {
                let cols = [("battery.left", b.left, b.left_charging), ("battery.right", b.right, b.right_charging), ("battery.case", b.case, b.case_charging)];
                let widths = cols.map(|(label, level, _)| p.measure(&t(label), 10.0, false).max(28.0 + p.measure(&format!("{level}%"), 12.0, true)));
                let gap = ((WIDTH - 60.0 - widths.iter().sum::<f32>()) / 2.0).max(8.0);
                let mut x = 30.0;
                for ((label, level, charging), w) in cols.into_iter().zip(widths) {
                    p.text(x, y + 104.0, &t(label), 10.0, false, color(TEXT2, 1.0), 0.0);
                    battery_bar(p, x, y + 117.0, 22.0, level, charging);
                    p.text(x + 28.0, y + 125.5, &format!("{level}%"), 12.0, true, color(TEXT, 1.0), 0.0);
                    x += w + gap;
                }
            }

            // the noise modes right on the card
            let modes = profile.noise_modes.clone();
            if !modes.is_empty() {
                let items: Vec<_> = modes.iter().map(|&m| (Action::QuickMode(i, m), state.mode == Some(m))).collect();
                self.segmented(p, 30.0, y + CARD_H - 50.0, WIDTH - 60.0, 36.0, &items, state.connected, true, |p, k, cx, cy, c| {
                    p.icon(mode_icon(modes[k]), cx, cy, 16.0, c, 1.7)
                });
            }
            p.alpha = alpha;
        }
    }
}
