//! The device list: one card per device with its photo, batteries and noise modes.

use tiny_skia::Shader;

use super::widgets::{mini_battery, mode_icon};
use super::{Action, Ui, WIDTH, display_name};
use crate::device::DeviceState;
use crate::i18n::{t, tf};
use crate::render::painter::Painter;
use crate::render::{ACCENT1, TEXT, TEXT2, color, icons, shapes};

impl Ui {
    pub(super) fn draw_list(&mut self, p: &mut Painter) {
        let (first, profile) = match self.devices.first() {
            Some(d) => (d.state.clone(), self.profile_for(&d.state)),
            None => (DeviceState::default(), self.showcase()),
        };
        self.draw_photo(p, &profile, first.color, 34.0, 34.0, 40.0);
        p.text(64.0, 32.0, &t("list.title"), 17.0, true, color(TEXT, 1.0), 0.0);
        let sub = match self.devices.iter().filter(|d| d.state.connected).count() {
            0 => t("list.none_connected"),
            1 => t("list.one_connected"),
            n => tf("list.many_connected", &[("n", &n)]),
        };
        p.text(64.0, 49.0, &sub, 12.0, false, color(TEXT2, 1.0), 0.0);

        if self.devices.is_empty() {
            let (cx, cy) = (WIDTH / 2.0, 160.0);
            let time = self.started.elapsed().as_secs_f32();
            for k in 0..3 {
                let f = (time * 0.55 + k as f32 / 3.0).fract();
                p.stroke(&shapes::circle(cx, cy, 30.0 + f * 80.0), Shader::SolidColor(color(ACCENT1, (1.0 - f) * 0.45)), 1.5);
            }
            self.draw_photo(p, &profile, None, cx, cy, 84.0);
            p.text(cx, cy + 112.0, &t("list.searching"), 15.0, true, color(TEXT, 1.0), 0.5);
            p.text(cx, cy + 133.0, &t("list.searching_hint"), 12.0, false, color(TEXT2, 1.0), 0.5);
            return;
        }

        for i in 0..self.devices.len() {
            let (y, open) = (72.0 + i as f32 * 162.0, Action::Open(i));
            self.target(p, 16.0, y, WIDTH - 32.0, 150.0, open);
            p.glass(16.0, y, WIDTH - 32.0, 150.0, 20.0, self.glass(open, p));
            let state = self.devices[i].state.clone();
            let profile = self.profile_for(&state);
            let alpha = p.alpha;
            if !state.connected {
                p.alpha *= 0.55;
            }
            // the noise modes right on the card
            let modes = &profile.noise_modes;
            let n = modes.len() as f32;
            let w = (WIDTH - 56.0 - (n - 1.0) * 8.0) / n;
            for (k, &m) in modes.iter().enumerate() {
                let (x, yb, a) = (28.0 + k as f32 * (w + 8.0), y + 100.0, Action::QuickMode(i, m));
                let selected = state.mode == Some(m);
                if state.connected {
                    self.target(p, x, yb, w, 38.0, a);
                }
                let mut g = self.glass(a, p);
                g.on = self.active(a, selected);
                p.glass(x, yb, w, 38.0, 13.0, g);
                p.icon(mode_icon(m), x + w / 2.0, yb + 19.0, 19.0, color(if selected { 0xFFFFFF } else { TEXT2 }, 1.0), 1.7);
            }
            self.draw_photo(p, &profile, state.color, 58.0, y + 48.0, 72.0);
            p.text(104.0, y + 30.0, &display_name(&state, &profile), 15.0, true, color(TEXT, 1.0), 0.0);
            let status = match (state.connected, state.mode) {
                (true, Some(m)) => t(&format!("noise.{}_long", m.key())),
                (true, None) => t("device.connected"),
                (false, _) => t("list.take_out"),
            };
            p.text(104.0, y + 48.0, &status, 12.0, false, color(TEXT2, 1.0), 0.0);
            if let Some(b) = state.battery.filter(|_| state.connected) {
                let mut x = 104.0;
                let parts =
                    [("battery.left_short", b.left, b.left_charging), ("battery.right_short", b.right, b.right_charging), ("battery.case", b.case, b.case_charging)];
                for (label, level, charging) in parts {
                    x = mini_battery(p, x, y + 72.0, &t(label), level, charging) + 12.0;
                }
            }
            p.icon(icons::CHEVRON_RIGHT, WIDTH - 40.0, y + 47.0, 18.0, color(TEXT2, 1.0), 1.8);
            p.alpha = alpha;
        }
    }
}
