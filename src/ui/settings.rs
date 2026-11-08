//! Settings tab: everything else the model has, row by row from the profile's `[[settings]]`.
//! Taller than the panel: it scrolls under the header.

use tiny_skia::Shader;

use super::widgets::section_title;
use super::{Action, CONTENT_Y, HEIGHT, Out, Ui, WIDTH};
use crate::device::{Command, DeviceState};
use crate::i18n::{maybe, t, tf};
use crate::profile::{ButtonCommand, Format, Kind, Profile, Setting};
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, TEXT3, WARNING, color, icons, shapes};

/// A slider being dragged: the setting and the value under the pointer.
pub(super) struct Drag {
    pub setting: usize,
    value: u8,
}

/// Where a slider's track starts and ends.
const TRACK: (f32, f32) = (30.0, WIDTH - 30.0);
/// Up to this many steps a level is a row of buttons; more, a slider.
const MAX_BUTTONS: u8 = 10;
/// Up to this many options they sit in the row; more, they open a sheet.
const MAX_INLINE: usize = 3;

fn label(s: &Setting) -> String {
    t(&format!("setting.{}", s.name))
}

fn hint(s: &Setting) -> Option<String> {
    maybe(&format!("setting.{}_hint", s.name))
}

/// Where an inline choice's options start in its row (under the title and hint).
fn choice_top(s: &Setting) -> f32 {
    if hint(s).is_some() { 46.0 } else { 34.0 }
}

fn option_label(s: &Setting, k: usize) -> String {
    s.options.get(k).map_or("–".to_string(), |c| t(&format!("option.{}", c.name)))
}

/// What an info setting shows.
fn info_text(s: &Setting, v: &[u8]) -> String {
    match s.format {
        Some(Format::Version) => v.iter().map(u8::to_string).collect::<Vec<_>>().join("."),
        _ => String::from_utf8_lossy(v).trim_matches(char::from(0)).trim().to_string(),
    }
}

/// "Level n of max"; with a center (balance), how far to which side of it.
fn level_text(s: &Setting, n: u8) -> String {
    let key = |side: &str| format!("setting.{}_{side}", s.name);
    match s.center {
        Some(c) if n < c => tf(&key("low"), &[("n", &(c - n))]),
        Some(c) if n > c => tf(&key("high"), &[("n", &(n - c))]),
        Some(_) => t(&key("center")),
        None => tf("anc.level", &[("n", &(n - s.min + 1)), ("max", &(s.max - s.min + 1))]),
    }
}

impl Ui {
    fn setting_value(&self, s: &Setting) -> Option<&Vec<u8>> {
        self.state()?.registers.get(&s.register?)
    }

    fn byte_of(&self, s: &Setting) -> Option<u8> {
        self.setting_value(s)?.get(s.byte).copied()
    }

    /// Its `when` switch is on (or it has none).
    fn setting_live(&self, profile: &Profile, s: &Setting) -> bool {
        let Some(name) = &s.when else { return true };
        profile.settings.iter().find(|x| &x.name == name).is_none_or(|w| self.byte_of(w) == Some(w.on))
    }

    /// How tall the row of `s` is: its card is drawn before the rows.
    fn row_height(&self, s: &Setting) -> f32 {
        let hint = hint(s).is_some();
        match s.kind {
            Kind::Switch => 44.0 + 6.0 * hint as u8 as f32,
            Kind::Choice if s.options.len() <= MAX_INLINE => choice_top(s) + 46.0,
            Kind::Choice | Kind::Button => 44.0,
            Kind::Level if s.max.saturating_sub(s.min) < MAX_BUTTONS => 50.0,
            Kind::Level => 68.0,
            Kind::Info => 40.0,
        }
    }

    pub(super) fn draw_settings(&mut self, p: &mut Painter, state: &DeviceState, profile: &Profile) {
        let alpha = p.alpha;
        self.clip_top = CONTENT_Y - 8.0;
        let mut y = CONTENT_Y - self.scroll;
        let all = &profile.settings;
        let mut i = 0;
        while i < all.len() {
            // a section: its title, then its rows in one card
            let section = &all[i].section;
            let end = all[i..].iter().position(|s| &s.section != section).map_or(all.len(), |n| i + n);
            section_title(p, 20.0, y + 8.0, &t(&format!("settings.{section}")));
            y += 18.0;
            let live: Vec<bool> = all[i..end]
                .iter()
                .map(|s| {
                    let has_value = s.kind == Kind::Button || self.setting_value(s).is_some();
                    state.connected && has_value && self.setting_live(profile, s)
                })
                .collect();
            // a row that opens a sheet is a button itself
            let rows: Vec<_> = (i..end)
                .map(|k| {
                    let s = &all[k];
                    let a = match s.kind {
                        Kind::Choice if s.options.len() > MAX_INLINE => Some(Action::PickOption(k)),
                        Kind::Button => Some(Action::SettingButton(k)),
                        _ => None,
                    };
                    (self.row_height(s), a.filter(|_| live[k - i]))
                })
                .collect();
            self.group(p, y, &rows);
            for k in i..end {
                let (s, h, live) = (&all[k], rows[k - i].0, live[k - i]);
                p.alpha = alpha * if live { 1.0 } else { 0.4 };
                match s.kind {
                    Kind::Switch => self.switch_row(p, k, s, y, h, live),
                    Kind::Choice if s.options.len() <= MAX_INLINE => self.choice_row(p, k, s, y, live),
                    Kind::Choice => self.picker_row(p, s, y),
                    Kind::Level if s.max.saturating_sub(s.min) < MAX_BUTTONS => self.level_row(p, k, s, y, live),
                    Kind::Level => self.slider_row(p, k, s, y, live),
                    Kind::Info => self.info_row(p, s, y),
                    Kind::Button => self.button_row(p, s, y),
                }
                y += h;
            }
            p.alpha = alpha;
            y += 16.0;
            i = end;
        }
        self.scroll_max = (y + self.scroll - (HEIGHT - 14.0)).max(0.0);
        self.scroll = self.scroll.min(self.scroll_max);
        self.clip_top = 0.0;
    }

    /// Title (+ hint under it) on the left, centered in the `h` from `y`.
    fn title(&self, p: &mut Painter, s: &Setting, y: f32, h: f32, room: f32) {
        match hint(s) {
            Some(hint) => {
                p.text_fit(30.0, y + h / 2.0 - 3.0, &label(s), 12.0, room, false, color(TEXT, 1.0), 0.0);
                p.text_fit(30.0, y + h / 2.0 + 12.0, &hint, 10.0, room, false, color(TEXT2, 1.0), 0.0);
            }
            None => p.text_fit(30.0, y + h / 2.0 + 4.5, &label(s), 12.0, room, false, color(TEXT, 1.0), 0.0),
        }
    }

    fn switch_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, h: f32, live: bool) {
        self.title(p, s, y, h, WIDTH - 60.0 - 56.0);
        let on = self.byte_of(s) == Some(s.on);
        self.switch(p, WIDTH - 76.0, y + h / 2.0 - 14.0, on, live, Action::SettingSwitch(i));
    }

    fn choice_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) {
        let top = choice_top(s);
        self.title(p, s, y, top, WIDTH - 60.0);
        let current = self.setting_value(s).and_then(|v| s.current_option(v));
        let items: Vec<_> = (0..s.options.len()).map(|k| (Action::SettingOption(i, k), current == Some(k))).collect();
        let room = (WIDTH - 66.0) / items.len() as f32 - 12.0;
        self.segmented(p, 30.0, y + top, WIDTH - 60.0, 34.0, &items, live, true, |p, k, cx, cy, c| {
            p.text_fit(cx, cy + 4.0, &option_label(s, k), 11.0, room, false, c, 0.5)
        });
    }

    fn picker_row(&mut self, p: &mut Painter, s: &Setting, y: f32) {
        let title = label(s);
        p.text(30.0, y + 26.5, &title, 12.0, false, color(TEXT, 1.0), 0.0);
        let current = self.setting_value(s).and_then(|v| s.current_option(v));
        let value = current.map_or("–".to_string(), |k| option_label(s, k));
        let room = WIDTH - 60.0 - p.measure(&title, 12.0, false) - 30.0;
        p.text_fit(WIDTH - 48.0, y + 26.5, &value, 11.5, room, false, color(TEXT2, 1.0), 1.0);
        p.icon(icons::CHEVRON_RIGHT, WIDTH - 36.0, y + 22.0, 12.0, color(TEXT3, 1.0), 2.0);
    }

    /// The same row as the ANC intensity: a button per step, the bar growing with it.
    fn level_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) {
        let current = self.byte_of(s);
        let steps = s.max - s.min + 1;
        let x0 = WIDTH - 30.0 - (steps as f32 * 18.0 + (steps - 1) as f32 * 5.0);
        p.text_fit(30.0, y + 22.0, &label(s), 12.0, x0 - 40.0, false, color(TEXT, 1.0), 0.0);
        let value = current.map_or("–".to_string(), |n| level_text(s, n));
        p.text_fit(30.0, y + 37.0, &value, 10.0, x0 - 40.0, false, color(TEXT2, 1.0), 0.0);
        for n in s.min..=s.max {
            let (x, yb, a) = (x0 + (n - s.min) as f32 * 23.0, y + 12.0, Action::SettingLevel(i, n));
            if live {
                self.target(p, x - 2.5, yb - 4.0, 23.0, 34.0, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, current.is_some_and(|k| k >= n));
            p.glass(x, yb, 18.0, 26.0, 6.0, g);
            let bar = 3.0 + 12.0 * (n - s.min + 1) as f32 / steps as f32;
            let fill = Shader::SolidColor(color(0xFFFFFF, 0.3 + 0.5 * g.on));
            p.fill(&shapes::rounded_rect(x + 6.0, yb + 21.0 - bar, 6.0, bar, 1.5), fill);
        }
    }

    fn slider_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) {
        let current = self.byte_of(s);
        p.text(30.0, y + 24.0, &label(s), 12.0, false, color(TEXT, 1.0), 0.0);
        let value = current.map_or("–".to_string(), |n| level_text(s, n));
        p.text(WIDTH - 30.0, y + 24.0, &value, 11.0, false, color(TEXT2, 1.0), 1.0);
        let (x0, x1, ty) = (TRACK.0, TRACK.1, y + 48.0);
        let a = Action::Slider(i);
        if live {
            self.target(p, x0 - 14.0, ty - 16.0, x1 - x0 + 28.0, 32.0, a);
        }
        p.fill(&shapes::rounded_rect(x0, ty - 2.0, x1 - x0, 4.0, 2.0), Shader::SolidColor(color(0xFFFFFF, 0.12)));
        let at = |n: u8| x0 + (n.clamp(s.min, s.max) - s.min) as f32 / (s.max - s.min).max(1) as f32 * (x1 - x0);
        // the bar grows from the center (balance) or from the start
        let origin = s.center.map_or(x0, at);
        if s.center.is_some() {
            p.fill(&shapes::rounded_rect(origin - 0.75, ty - 6.0, 1.5, 12.0, 0.75), Shader::SolidColor(color(0xFFFFFF, 0.25)));
        }
        let kx = current.map_or(origin, at);
        let (from, to) = if kx < origin { (kx, origin) } else { (origin, kx) };
        p.fill(&shapes::rounded_rect(from, ty - 2.0, to - from, 4.0, 2.0), shapes::accent(x0, x1 - x0, 1.0));
        let r = 9.0 + 1.5 * self.glass(a).hover;
        p.fill(&shapes::circle(kx, ty + 1.0, r), Shader::SolidColor(color(0, 0.25)));
        p.fill(&shapes::circle(kx, ty, r), Shader::SolidColor(color(0xFFFFFF, 1.0)));
    }

    fn info_row(&mut self, p: &mut Painter, s: &Setting, y: f32) {
        let value = self.setting_value(s).map_or("–".to_string(), |v| info_text(s, v));
        p.text(30.0, y + 24.5, &label(s), 12.0, false, color(TEXT2, 1.0), 0.0);
        p.text(WIDTH - 30.0, y + 24.5, &value, 12.0, true, color(TEXT, 1.0), 1.0);
    }

    fn button_row(&mut self, p: &mut Painter, s: &Setting, y: f32) {
        let danger = s.command == Some(ButtonCommand::FactoryReset);
        p.text(WIDTH / 2.0, y + 26.5, &label(s), 12.0, true, color(if danger { WARNING } else { TEXT }, 1.0), 0.5);
    }

    pub(super) fn draw_options_sheet(&mut self, p: &mut Painter, i: usize) {
        let profile = self.current_profile();
        let Some(s) = profile.settings.get(i) else { return };
        let current = self.setting_value(s).and_then(|v| s.current_option(v));
        let top = self.sheet_frame(p, 72.0 + s.options.len() as f32 * 44.0);
        p.text(WIDTH / 2.0, top + 42.0, &label(s), 15.0, true, color(TEXT, 1.0), 0.5);
        for k in 0..s.options.len() {
            let (y, a, selected) = (top + 62.0 + k as f32 * 44.0, Action::SettingOption(i, k), current == Some(k));
            if self.sheet_open {
                self.target(p, 16.0, y, WIDTH - 32.0, 38.0, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, selected);
            p.glass(16.0, y, WIDTH - 32.0, 38.0, 13.0, g);
            let c = color(if selected { 0xFFFFFF } else { TEXT }, 1.0);
            p.text(32.0, y + 24.0, &option_label(s, k), 13.0, selected, c, 0.0);
            if selected {
                p.icon(icons::CHECK, WIDTH - 38.0, y + 19.0, 16.0, c, 2.2);
            }
        }
    }

    pub(super) fn draw_confirm_sheet(&mut self, p: &mut Painter, i: usize) {
        let profile = self.current_profile();
        let Some(s) = profile.settings.get(i) else { return };
        let top = self.sheet_frame(p, 218.0);
        p.icon(icons::WARNING, WIDTH / 2.0, top + 50.0, 34.0, color(WARNING, 1.0), 2.0);
        p.text(WIDTH / 2.0, top + 98.0, &label(s), 16.0, true, color(TEXT, 1.0), 0.5);
        let line = t(&format!("setting.{}_confirm", s.name));
        p.text_fit(WIDTH / 2.0, top + 121.0, &line, 12.5, WIDTH - 40.0, false, color(TEXT2, 1.0), 0.5);
        let w = (WIDTH - 42.0) / 2.0;
        let buttons = [(Action::CloseSheet, "settings.cancel", false), (Action::ConfirmButton(i), "settings.confirm", true)];
        for (k, (a, key, primary)) in buttons.into_iter().enumerate() {
            let (x, y) = (16.0 + k as f32 * (w + 10.0), top + 142.0);
            if self.sheet_open {
                self.target(p, x, y, w, 44.0, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, primary);
            p.glass(x, y, w, 44.0, 16.0, g);
            p.text(x + w / 2.0, y + 27.0, &t(key), 13.5, true, color(0xFFFFFF, 1.0), 0.5);
        }
    }

    // ---------- changes ----------

    pub(super) fn setting_action(&mut self, a: Action) -> Out {
        let profile = self.current_profile();
        let (Action::SettingSwitch(i) | Action::SettingOption(i, _) | Action::SettingLevel(i, _) | Action::ConfirmButton(i)) = a
        else {
            return vec![];
        };
        let Some(s) = profile.settings.get(i) else { return vec![] };
        match a {
            Action::SettingSwitch(_) => {
                let next = if self.byte_of(s) == Some(s.on) { s.off } else { s.on };
                self.write(s, |v| v[s.byte] = next)
            }
            Action::SettingOption(_, k) => {
                let Some(c) = s.options.get(k) else { return vec![] };
                let b = s.option_bytes(c);
                self.write(s, |v| v[s.byte..s.byte + b.len()].copy_from_slice(&b))
            }
            Action::SettingLevel(_, n) => self.write(s, |v| v[s.byte] = n),
            _ => match s.command {
                Some(ButtonCommand::PowerOff) => self.send(Command::PowerOff),
                Some(ButtonCommand::FactoryReset) => self.send(Command::FactoryReset),
                None => vec![],
            },
        }
    }

    /// Changes the register as last read (padded when shorter) and sends it whole; the screen
    /// shows the change right away, the device's answer confirms it.
    fn write(&mut self, s: &Setting, edit: impl FnOnce(&mut Vec<u8>)) -> Out {
        let Some(reg) = s.register else { return vec![] };
        let width = s.options.iter().map(|c| c.values.len() * s.width).max().unwrap_or(1);
        let need = s.set.iter().map(|&(b, _)| b + 1).fold(s.byte + width, usize::max);
        let Some(state) = self.state_mut() else { return vec![] };
        let v = state.registers.entry(reg).or_default();
        if v.len() < need {
            v.resize(need, 0);
        }
        edit(v);
        for &(b, x) in &s.set {
            v[b] = x;
        }
        let v = v.clone();
        self.send(Command::Write(reg, v))
    }

    pub(super) fn start_slider(&mut self, i: usize, x: f32) {
        let value = self.current_profile().settings.get(i).and_then(|s| self.byte_of(s)).unwrap_or_default();
        self.slider = Some(Drag { setting: i, value });
        self.drag_slider(x);
    }

    pub(super) fn drag_slider(&mut self, x: f32) {
        let profile = self.current_profile();
        let Some(d) = &mut self.slider else { return };
        let Some(s) = profile.settings.get(d.setting) else { return };
        let f = ((x - TRACK.0) / (TRACK.1 - TRACK.0)).clamp(0.0, 1.0);
        d.value = s.min + (f * (s.max - s.min) as f32).round() as u8;
        let value = d.value;
        if let (Some(reg), Some(state)) = (s.register, self.state_mut()) {
            let v = state.registers.entry(reg).or_default();
            if v.len() <= s.byte {
                v.resize(s.byte + 1, 0);
            }
            v[s.byte] = value;
        }
    }

    /// Sends once, on release: every step of the drag would flood the link.
    pub(super) fn end_slider(&mut self) -> Out {
        let Some(d) = self.slider.take() else { return vec![] };
        let profile = self.current_profile();
        let Some(s) = profile.settings.get(d.setting) else { return vec![] };
        self.write(s, |v| v[s.byte] = d.value)
    }
}
