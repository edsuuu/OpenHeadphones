//! Settings tab: everything else the model has, row by row from the profile's `[[settings]]`.
//! Taller than the panel: it scrolls under the header.

use tiny_skia::Shader;

use super::widgets::section_title;
use super::{Action, CONTENT_Y, HEIGHT, Out, Ui, WIDTH};
use crate::device::{Command, DeviceState};
use crate::i18n::{maybe, t, tf};
use crate::profile::{ButtonCommand, Format, Kind, Profile, Setting};
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, WARNING, color, icons, shapes};

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

    pub(super) fn draw_settings(&mut self, p: &mut Painter, state: &DeviceState, profile: &Profile) {
        let alpha = p.alpha;
        self.clip_top = CONTENT_Y - 8.0;
        let mut y = CONTENT_Y - self.scroll;
        let mut section = "";
        for (i, s) in profile.settings.iter().enumerate() {
            if s.section != section {
                section = &s.section;
                y += if i > 0 { 14.0 } else { 4.0 };
                section_title(p, 16.0, y, &t(&format!("settings.{section}")));
                y += 8.0;
            }
            let has_value = s.kind == Kind::Button || self.setting_value(s).is_some();
            let live = state.connected && has_value && self.setting_live(profile, s);
            p.alpha = alpha * if live { 1.0 } else { 0.4 };
            y += match s.kind {
                Kind::Switch => self.switch_row(p, i, s, y, live),
                Kind::Choice if s.options.len() <= MAX_INLINE => self.choice_row(p, i, s, y, live),
                Kind::Choice => self.picker_row(p, i, s, y, live),
                Kind::Level if s.max.saturating_sub(s.min) < MAX_BUTTONS => self.level_row(p, i, s, y, live),
                Kind::Level => self.slider_row(p, i, s, y, live),
                Kind::Info => self.info_row(p, s, y),
                Kind::Button => self.button_row(p, i, s, y, live),
            };
        }
        p.alpha = alpha;
        self.scroll_max = (y + self.scroll - (HEIGHT - 14.0)).max(0.0);
        self.scroll = self.scroll.min(self.scroll_max);
        self.clip_top = 0.0;
    }

    /// Title (+ hint under it) on the left; returns the text's bottom.
    fn title(&self, p: &mut Painter, s: &Setting, y: f32, room: f32) -> f32 {
        p.text_fit(16.0, y + 22.0, &label(s), 13.5, room, true, color(TEXT, 1.0), 0.0);
        match maybe(&format!("setting.{}_hint", s.name)) {
            Some(hint) => {
                p.text_fit(16.0, y + 39.0, &hint, 11.5, room, false, color(TEXT2, 1.0), 0.0);
                y + 48.0
            }
            None => y + 32.0,
        }
    }

    fn switch_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let bottom = self.title(p, s, y, WIDTH - 32.0 - 60.0);
        let h = bottom - y + 4.0;
        let on = self.byte_of(s) == Some(s.on);
        self.switch(p, WIDTH - 62.0, y + (h - 28.0) / 2.0, on, live, Action::SettingSwitch(i));
        h
    }

    fn choice_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let top = self.title(p, s, y, WIDTH - 32.0);
        let current = self.setting_value(s).and_then(|v| s.current_option(v));
        let n = s.options.len() as f32;
        let w = (WIDTH - 32.0 - (n - 1.0) * 8.0) / n;
        for k in 0..s.options.len() {
            let (x, a, selected) = (16.0 + k as f32 * (w + 8.0), Action::SettingOption(i, k), current == Some(k));
            if live {
                self.target(p, x, top, w, 34.0, a);
            }
            let mut g = self.glass(a, p);
            g.on = self.active(a, selected);
            p.glass(x, top, w, 34.0, 13.0, g);
            let c = color(if selected { 0xFFFFFF } else { TEXT2 }, 1.0);
            p.text_fit(x + w / 2.0, top + 21.5, &option_label(s, k), 12.5, w - 12.0, selected, c, 0.5);
        }
        top + 44.0 - y
    }

    fn picker_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let a = Action::PickOption(i);
        if live {
            self.target(p, 16.0, y + 4.0, WIDTH - 32.0, 40.0, a);
        }
        p.glass(16.0, y + 4.0, WIDTH - 32.0, 40.0, 14.0, self.glass(a, p));
        let title = label(s);
        p.text(30.0, y + 29.0, &title, 13.0, true, color(TEXT, 1.0), 0.0);
        let current = self.setting_value(s).and_then(|v| s.current_option(v));
        let value = current.map_or("–".to_string(), |k| option_label(s, k));
        let room = WIDTH - 32.0 - 14.0 - p.measure(&title, 13.0, true) - 16.0 - 34.0;
        p.text_fit(WIDTH - 50.0, y + 29.0, &value, 12.5, room, false, color(TEXT2, 1.0), 1.0);
        p.icon(icons::CHEVRON_DOWN, WIDTH - 34.0, y + 24.0, 14.0, color(TEXT2, 1.0), 1.8);
        50.0
    }

    /// The same row as the ANC intensity: a button per step, the bar growing with it.
    fn level_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let current = self.byte_of(s);
        let steps = s.max - s.min + 1;
        let x0 = WIDTH - 16.0 - (steps as f32 * 18.0 + (steps - 1) as f32 * 5.0);
        p.text_fit(16.0, y + 22.0, &label(s), 13.5, x0 - 24.0, true, color(TEXT, 1.0), 0.0);
        let value = current.map_or("–".to_string(), |n| level_text(s, n));
        p.text_fit(16.0, y + 39.0, &value, 11.5, x0 - 24.0, false, color(TEXT2, 1.0), 0.0);
        for n in s.min..=s.max {
            let (x, yb, a) = (x0 + (n - s.min) as f32 * 23.0, y + 12.0, Action::SettingLevel(i, n));
            if live {
                self.target(p, x - 2.5, yb - 4.0, 23.0, 34.0, a);
            }
            let mut g = self.glass(a, p);
            g.on = self.active(a, current.is_some_and(|k| k >= n));
            p.glass(x, yb, 18.0, 26.0, 6.0, g);
            let bar = 3.0 + 12.0 * (n - s.min + 1) as f32 / steps as f32;
            let fill = Shader::SolidColor(color(0xFFFFFF, 0.3 + 0.5 * g.on));
            p.fill(&shapes::rounded_rect(x + 6.0, yb + 21.0 - bar, 6.0, bar, 1.5), fill);
        }
        52.0
    }

    fn slider_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let current = self.byte_of(s);
        p.text(16.0, y + 22.0, &label(s), 13.5, true, color(TEXT, 1.0), 0.0);
        let value = current.map_or("–".to_string(), |n| level_text(s, n));
        p.text(WIDTH - 16.0, y + 22.0, &value, 12.0, false, color(TEXT2, 1.0), 1.0);
        let (x0, x1, ty) = (TRACK.0, TRACK.1, y + 46.0);
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
        p.glass(kx - 11.0, ty - 11.0, 22.0, 22.0, 11.0, self.glass(a, p));
        66.0
    }

    fn info_row(&mut self, p: &mut Painter, s: &Setting, y: f32) -> f32 {
        let value = self.setting_value(s).map_or("–".to_string(), |v| info_text(s, v));
        p.text(16.0, y + 22.0, &label(s), 13.0, false, color(TEXT2, 1.0), 0.0);
        p.text(WIDTH - 16.0, y + 22.0, &value, 13.0, true, color(TEXT, 1.0), 1.0);
        32.0
    }

    fn button_row(&mut self, p: &mut Painter, i: usize, s: &Setting, y: f32, live: bool) -> f32 {
        let a = Action::SettingButton(i);
        if live {
            self.target(p, 16.0, y + 6.0, WIDTH - 32.0, 40.0, a);
        }
        p.glass(16.0, y + 6.0, WIDTH - 32.0, 40.0, 14.0, self.glass(a, p));
        let danger = s.command == Some(ButtonCommand::FactoryReset);
        p.text(WIDTH / 2.0, y + 31.0, &label(s), 13.0, true, color(if danger { WARNING } else { TEXT }, 1.0), 0.5);
        52.0
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
            let mut g = self.glass(a, p);
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
            let mut g = self.glass(a, p);
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
