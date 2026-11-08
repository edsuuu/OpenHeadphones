//! Device page: header, photo and batteries, the tabs, and the open tab's content.

use tiny_skia::Shader;

use super::{Action, CONTENT_Y, Tab, Ui, WIDTH, display_name};
use crate::i18n::t;
use crate::profile::Profile;
use crate::render::painter::Painter;
use crate::render::{GREEN, PANEL, TEXT, TEXT2, TEXT3, color, icons, shapes};

/// Tabs the model has: sound always, touch and find when the profile has them.
fn tabs_of(profile: &Profile) -> Vec<Tab> {
    let mut v = vec![Tab::Sound];
    if !profile.gestures.is_empty() {
        v.push(Tab::Gestures);
    }
    if profile.find_buds {
        v.push(Tab::Find);
    }
    if !profile.settings.is_empty() {
        v.push(Tab::Settings);
    }
    v
}

impl Ui {
    pub(super) fn draw_detail(&mut self, p: &mut Painter) {
        let Some(state) = self.state().cloned() else { return };
        let profile = self.profile_for(&state);
        let tabs = tabs_of(&profile);
        if !tabs.contains(&self.tab) {
            self.tab = Tab::Sound;
        }

        // the settings scroll under the header: drawn first, then covered (only when scrolled,
        // or the cover would hide the list sliding by)
        if self.tab == Tab::Settings {
            self.draw_settings(p, &state, &profile);
        }
        if self.tab == Tab::Settings && self.scroll > 0.5 {
            let edge = CONTENT_Y - 8.0;
            p.fill(&shapes::rounded_rect(0.0, -40.0, WIDTH, edge + 40.0, 0.0), Shader::SolidColor(color(PANEL, 1.0)));
            let fade = [(0.0, color(PANEL, 1.0)), (1.0, color(PANEL, 0.0))];
            p.fill(&shapes::rounded_rect(0.0, edge, WIDTH, 12.0, 0.0), shapes::linear(0.0, edge, 0.0, edge + 12.0, &fade));
        }

        // header
        self.target(p, 12.0, 12.0, 42.0, 40.0, Action::Back);
        p.glass(16.0, 15.0, 34.0, 34.0, 17.0, self.glass(Action::Back, p));
        p.icon(icons::BACK, 32.5, 32.0, 18.0, color(TEXT, 1.0), 2.0);
        p.text(62.0, 30.0, &display_name(&state, &profile), 16.0, true, color(TEXT, 1.0), 0.0);
        let (dot, status) =
            if state.connected { (color(GREEN, 1.0), t("device.connected")) } else { (color(TEXT3, 1.0), t("device.disconnected")) };
        p.fill(&shapes::circle(66.0, 43.0, 3.5), Shader::SolidColor(dot));
        p.text(75.0, 47.0, &status, 11.5, false, color(TEXT2, 1.0), 0.0);

        // photo + batteries
        self.draw_photo(p, &profile, state.color, 78.0, 112.0, 100.0);
        let b = state.battery;
        let rings = [
            ("battery.left", b.map(|b| b.left), b.is_some_and(|b| b.left_charging)),
            ("battery.right", b.map(|b| b.right), b.is_some_and(|b| b.right_charging)),
            ("battery.case", b.map(|b| b.case), b.is_some_and(|b| b.case_charging)),
        ];
        for (k, (label, level, charging)) in rings.into_iter().enumerate() {
            let cx = 188.0 + k as f32 * 64.0;
            p.ring(cx, 104.0, 23.0, level, charging);
            p.text(cx, 147.0, &t(label), 11.0, false, color(TEXT2, 1.0), 0.5);
        }

        // tabs
        if tabs.len() > 1 {
            let n = tabs.len() as f32;
            let w = (WIDTH - 32.0 - 6.0 * (n - 1.0)) / n;
            for (i, &tab) in tabs.iter().enumerate() {
                let (x, a, selected) = (16.0 + i as f32 * (w + 6.0), Action::Tab(tab), self.tab == tab);
                self.target(p, x, 166.0, w, 34.0, a);
                let mut g = self.glass(a, p);
                g.on = self.active(a, selected);
                p.glass(x, 166.0, w, 34.0, 17.0, g);
                let c = color(if selected { 0xFFFFFF } else { TEXT2 }, 1.0);
                p.text(x + w / 2.0, 188.0, &t(tab.key()), 13.0, selected, c, 0.5);
            }
        }

        match self.tab {
            Tab::Sound => self.draw_sound(p, &state, &profile),
            Tab::Gestures => self.draw_gestures(p, &state, &profile),
            Tab::Find => self.draw_find(p, &state),
            Tab::Settings => {}
        }
    }
}
