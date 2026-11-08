//! Device page: header, photo and batteries, the tabs, and the open tab's content.

use tiny_skia::Shader;

use super::widgets::battery_bar;
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

fn tab_icon(tab: Tab) -> &'static str {
    match tab {
        Tab::Sound => icons::SOUND,
        Tab::Gestures => icons::TOUCH,
        Tab::Find => icons::LOCATE,
        Tab::Settings => icons::SLIDERS,
    }
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

        // header: back on the left, the name and whether it is connected in the middle
        self.target(p, 10.0, 10.0, 40.0, 40.0, Action::Back);
        p.glass(16.0, 16.0, 28.0, 28.0, 14.0, self.glass(Action::Back));
        p.icon(icons::BACK, 29.5, 30.0, 14.0, color(TEXT, 1.0), 2.2);
        p.text_fit(WIDTH / 2.0, 28.0, &display_name(&state, &profile), 13.5, WIDTH - 120.0, true, color(TEXT, 1.0), 0.5);
        let (dot, status) =
            if state.connected { (color(GREEN, 1.0), t("device.connected")) } else { (color(TEXT3, 1.0), t("device.disconnected")) };
        let w = p.measure(&status, 10.0, false);
        p.fill(&shapes::circle(WIDTH / 2.0 - w / 2.0 - 4.0, 39.5, 2.5), Shader::SolidColor(dot));
        p.text(WIDTH / 2.0 + 4.0, 43.0, &status, 10.0, false, color(TEXT2, 1.0), 0.5);

        // photo + one bar per battery
        self.draw_photo(p, &profile, state.color, 72.0, 114.0, 104.0);
        // disconnected, the last levels would read as current: dashes and empty bars instead
        let b = state.battery.filter(|_| state.connected);
        let rows = [
            ("battery.left", b.map(|b| b.left), b.is_some_and(|b| b.left_charging)),
            ("battery.right", b.map(|b| b.right), b.is_some_and(|b| b.right_charging)),
            ("battery.case", b.map(|b| b.case), b.is_some_and(|b| b.case_charging)),
        ];
        let (x0, x1) = (146.0, WIDTH - 20.0);
        for (k, (label, level, charging)) in rows.into_iter().enumerate() {
            let cy = 114.0 + (k as f32 - 1.0) * 27.0;
            p.text(x0, cy + 4.0, &t(label), 11.0, false, color(TEXT2, 1.0), 0.0);
            battery_bar(p, x0 + 60.0, cy - 4.0, x1 - 44.0 - (x0 + 60.0), level.unwrap_or(0), charging);
            let txt = level.map_or("–".to_string(), |n| format!("{n}%"));
            p.text(x1, cy + 4.5, &txt, 11.5, true, color(TEXT, 1.0), 1.0);
        }

        // tabs
        if tabs.len() > 1 {
            let items: Vec<_> = tabs.iter().map(|&tab| (Action::Tab(tab), self.tab == tab)).collect();
            self.segmented(p, 16.0, 184.0, WIDTH - 32.0, 36.0, &items, true, false, |p, k, cx, cy, c| {
                let label = t(tabs[k].key());
                let x = cx - (19.0 + p.measure(&label, 11.0, false)) / 2.0;
                p.icon(tab_icon(tabs[k]), x + 7.0, cy, 14.0, c, 1.7);
                p.text(x + 19.0, cy + 4.0, &label, 11.0, false, c, 0.0);
            });
        }

        match self.tab {
            Tab::Sound => self.draw_sound(p, &state, &profile),
            Tab::Gestures => self.draw_gestures(p, &state, &profile),
            Tab::Find => self.draw_find(p, &state),
            Tab::Settings => {}
        }
    }
}
