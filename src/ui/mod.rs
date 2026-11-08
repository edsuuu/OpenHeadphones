//! The panel: device list → device page (sound, touch controls, find my buds). Immediate mode:
//! every frame draws everything and records where each clickable thing is. Coordinates are
//! logical px of the panel.

mod detail;
mod eq;
mod find;
mod gestures;
mod list;
mod settings;
mod sound;
mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use tiny_skia::{Color, FillRule, Mask, Pixmap, PremultipliedColorU8, Shader};

use crate::device::{Command, DeviceState, NoiseMode};
use crate::i18n::t;
use crate::platform;
use crate::update;
use crate::profile::{self, Profile, Side};
use crate::render::fonts::Fonts;
use crate::render::painter::{Glass, Painter};
use crate::render::{PANEL, TEXT2, color, shapes};

/// Panel size (logical px) and the room between it and the taskbar.
pub const WIDTH: f32 = 360.0;
pub const HEIGHT: f32 = 650.0;
pub const GAP: f32 = 12.0;
const RADIUS: f32 = 20.0;
/// Where a tab's content starts on the device page.
const CONTENT_Y: f32 = 238.0;
/// A device's card in the list, and where the first one starts.
const CARD_H: f32 = 194.0;
const LIST_Y: f32 = 50.0;

/// Commands for the devices, by device id.
type Out = Vec<(String, Command)>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Sound,
    Gestures,
    Find,
    Settings,
}

impl Tab {
    fn key(self) -> &'static str {
        match self {
            Tab::Sound => "tab.sound",
            Tab::Gestures => "tab.gestures",
            Tab::Find => "tab.find",
            Tab::Settings => "tab.settings",
        }
    }
}

/// Bottom sheet over the device page.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sheet {
    /// Picking the action of this gesture id.
    Gesture(u8),
    /// Confirming the beep on this side.
    Beep(Side),
    /// Picking an option of setting n.
    Options(usize),
    /// Confirming the button of setting n.
    Confirm(usize),
    /// The app's own settings: version and updates.
    App,
}

/// What the app (not a device) is asked to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppRequest {
    Check,
    Install,
}

/// What a click does; also the key of the control's hover/press animation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Action {
    Open(usize),
    Back,
    Tab(Tab),
    Mode(NoiseMode),
    /// Noise mode right from a card in the list.
    QuickMode(usize, NoiseMode),
    Adaptive,
    Level(u8),
    AmbientLevel(u8),
    Eq,
    Preset(usize),
    Custom,
    Curve,
    PickGesture(u8),
    /// (gesture id, action id)
    SetGesture(u8, u8),
    /// Stops it if beeping, otherwise asks first.
    ToggleBeep(Side),
    ConfirmBeep(Side),
    /// Settings tab, by index in the profile's settings.
    SettingSwitch(usize),
    /// (setting, option)
    SettingOption(usize, usize),
    /// Opens the sheet with the options of a setting.
    PickOption(usize),
    SettingLevel(usize, u8),
    /// The slider of a level setting with many steps (dragged).
    Slider(usize),
    SettingButton(usize),
    ConfirmButton(usize),
    CloseSheet,
    /// Click on the dimmed page around a sheet.
    Dismiss,
    AppSettings,
    AutoUpdate,
    CheckUpdate,
    InstallUpdate,
}

struct Target {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    action: Action,
}

impl Target {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

struct Device {
    id: String,
    state: DeviceState,
}

/// [hover, press, on, on target] of a control.
type Anim = [f32; 4];

pub struct Ui {
    profiles: Vec<Rc<Profile>>,
    /// For devices no profile matches.
    no_profile: Rc<Profile>,
    devices: Vec<Device>,
    /// The bar is on top (macOS, GNOME): the panel hangs from the top of the frame.
    pub tip_on_top: bool,
    current: usize,
    detail: bool,
    tab: Tab,
    sheet: Option<Sheet>,
    sheet_open: bool,
    sheet_t: f32,
    /// 0 = list … 1 = device page, animated.
    page: f32,
    /// The list is as tall as its content; the device page uses all of it.
    height: f32,
    pointer: Option<(f32, f32)>,
    pressed: Option<Action>,
    targets: Vec<Target>,
    anim: HashMap<Action, Anim>,
    eq: eq::EqEdit,
    /// Settings tab: how far it scrolled, how far it can, and the slider being dragged.
    scroll: f32,
    scroll_max: f32,
    slider: Option<settings::Drag>,
    /// Clicks only count below this y (what scrolled under the header is hidden).
    clip_top: f32,
    started: Instant,
    /// Decoded photos by file.
    photos: HashMap<PathBuf, Option<Pixmap>>,
    /// Updating the app: shown in its sheet, asked of the app through `app_request`.
    pub update_status: update::Status,
    pub app_request: Option<AppRequest>,
    auto_update: bool,
    /// This copy may replace itself (not a dev build in target/, not a .deb).
    installable: bool,
}

fn approach(v: &mut f32, target: f32, dt: f32, speed: f32) -> bool {
    *v += (target - *v) * (1.0 - (-dt * speed).exp());
    if (target - *v).abs() < 0.003 {
        *v = target;
        return false;
    }
    true
}

fn display_name(state: &DeviceState, profile: &Profile) -> String {
    match (state.name.is_empty(), profile.name.is_empty()) {
        (false, _) => state.name.clone(),
        (true, false) => profile.name.clone(),
        _ => t("device.unknown"),
    }
}

impl Ui {
    pub fn new(profiles: Vec<Rc<Profile>>) -> Ui {
        Ui {
            profiles,
            no_profile: Rc::default(),
            devices: vec![],
            tip_on_top: false,
            current: 0,
            detail: false,
            tab: Tab::Sound,
            sheet: None,
            sheet_open: false,
            sheet_t: 0.0,
            page: 0.0,
            height: 330.0,
            pointer: None,
            pressed: None,
            targets: vec![],
            anim: HashMap::new(),
            eq: eq::EqEdit::new(0),
            scroll: 0.0,
            scroll_max: 0.0,
            slider: None,
            clip_top: 0.0,
            started: Instant::now(),
            photos: HashMap::new(),
            update_status: update::Status::Idle,
            app_request: None,
            auto_update: update::auto(),
            installable: update::can_install(),
        }
    }

    pub fn update(&mut self, id: String, state: DeviceState) {
        let i = match self.devices.iter().position(|d| d.id == id) {
            Some(i) => {
                self.devices[i].state = state;
                i
            }
            None => {
                self.devices.push(Device { id, state });
                self.devices.len() - 1
            }
        };
        if i == self.current {
            self.sync_eq();
        }
    }

    fn profile_for(&self, state: &DeviceState) -> Rc<Profile> {
        profile::find(&self.profiles, state.product_id, &state.name).unwrap_or(&self.no_profile).clone()
    }

    fn current_profile(&self) -> Rc<Profile> {
        self.devices.get(self.current).map_or_else(|| self.no_profile.clone(), |d| self.profile_for(&d.state))
    }

    /// Shown while searching: the first profile with a photo.
    fn showcase(&self) -> Rc<Profile> {
        self.profiles.iter().find(|p| p.images.default.is_some()).unwrap_or(&self.no_profile).clone()
    }

    fn state(&self) -> Option<&DeviceState> {
        self.devices.get(self.current).map(|d| &d.state)
    }

    fn state_mut(&mut self) -> Option<&mut DeviceState> {
        self.devices.get_mut(self.current).map(|d| &mut d.state)
    }

    fn send(&self, c: Command) -> Out {
        self.devices.get(self.current).map(|d| vec![(d.id.clone(), c)]).unwrap_or_default()
    }

    fn target_height(&self) -> f32 {
        match (self.detail, self.devices.len()) {
            (true, _) => HEIGHT,
            (false, 0) => 330.0,
            (false, n) => (LIST_Y + n as f32 * (CARD_H + 12.0) + 4.0).min(HEIGHT),
        }
    }

    /// No animation: puts page and height in place (when the panel opens).
    pub fn settle(&mut self) {
        self.page = if self.detail { 1.0 } else { 0.0 };
        self.height = self.target_height();
    }

    pub fn open_device(&mut self, i: usize) {
        (self.current, self.detail, self.tab, self.sheet_open, self.scroll) = (i, true, Tab::Sound, false, 0.0);
        let bands = self.current_profile().eq.as_ref().map_or(0, |e| e.bands.len());
        self.eq = eq::EqEdit::new(bands);
        self.sync_eq();
    }

    /// For `--snapshot`: shows a tab, optionally with its sheet open.
    pub fn preview(&mut self, tab: Tab, with_sheet: bool) {
        self.tab = tab;
        let first_gesture = self.current_profile().gestures.first().map(|g| g.id);
        let first_button = self.current_profile().settings.iter().position(|s| s.kind == profile::Kind::Button);
        self.sheet = match tab {
            Tab::Gestures => first_gesture.map(Sheet::Gesture),
            Tab::Find => Some(Sheet::Beep(Side::Left)),
            Tab::Settings => first_button.map(Sheet::Confirm),
            Tab::Sound => None,
        }
        .filter(|_| with_sheet);
        self.sheet_open = self.sheet.is_some();
        self.sheet_t = self.sheet_open as u8 as f32;
    }

    /// For `--snapshot`: the app's sheet open.
    pub fn preview_app_sheet(&mut self) {
        (self.sheet, self.sheet_open, self.sheet_t) = (Some(Sheet::App), true, 1.0);
    }

    /// Esc: closes the open sheet; `false` when there is none (then the panel closes).
    pub fn escape(&mut self) -> bool {
        std::mem::replace(&mut self.sheet_open, false)
    }

    /// Moves the animations forward; tells whether anything is still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut moving = approach(&mut self.page, self.detail as u8 as f32, dt, 13.0);
        let target = self.target_height();
        moving |= approach(&mut self.height, target, dt, 13.0);
        moving |= approach(&mut self.sheet_t, self.sheet_open as u8 as f32, dt, 16.0);
        if !self.sheet_open && self.sheet_t == 0.0 {
            self.sheet = None;
        }
        let (hover, pressed) = (self.hovered(), self.pressed);
        if let Some(h) = hover {
            self.anim.entry(h).or_default();
        }
        for (a, v) in self.anim.iter_mut() {
            let h = (hover == Some(*a)) as u8 as f32;
            let p = (pressed == Some(*a) && hover == Some(*a)) as u8 as f32;
            moving |= approach(&mut v[0], h, dt, 16.0);
            moving |= approach(&mut v[1], p, dt, 28.0);
            let on = v[3];
            moving |= approach(&mut v[2], on, dt, 14.0);
        }
        moving || (self.devices.is_empty() && self.page < 1.0) || self.beeping()
    }

    fn hovered(&self) -> Option<Action> {
        if self.eq.dragging.is_some() {
            return Some(Action::Curve);
        }
        if let Some(d) = &self.slider {
            return Some(Action::Slider(d.setting));
        }
        let (x, y) = self.pointer?;
        self.targets.iter().rev().find(|t| t.contains(x, y)).map(|t| t.action)
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Out {
        self.pointer = Some((x, y));
        if self.slider.is_some() {
            self.drag_slider(x);
        }
        match self.eq.dragging {
            Some(band) => self.drag_band(band, y, false),
            None => vec![],
        }
    }

    /// Mouse wheel / touchpad, in logical px (positive = content goes up).
    pub fn wheel(&mut self, dy: f32) {
        if self.detail && self.tab == Tab::Settings && !self.sheet_open {
            self.scroll = (self.scroll + dy).clamp(0.0, self.scroll_max);
        }
    }

    pub fn pointer_left(&mut self) {
        self.pointer = None;
    }

    pub fn press(&mut self) -> Out {
        let Some(a) = self.hovered() else { return vec![] };
        self.pressed = Some(a);
        match (a, self.pointer) {
            (Action::Curve, Some((x, y))) => self.start_drag(x, y),
            (Action::Slider(i), Some((x, _))) => {
                self.start_slider(i, x);
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn release(&mut self) -> Out {
        let pressed = self.pressed.take();
        if self.eq.dragging.is_some() {
            return self.end_drag();
        }
        if self.slider.is_some() {
            return self.end_slider();
        }
        match pressed {
            Some(a) if self.hovered() == Some(a) => self.act(a),
            _ => vec![],
        }
    }

    fn act(&mut self, a: Action) -> Out {
        match a {
            Action::Open(i) => {
                self.open_device(i);
                vec![]
            }
            Action::Back => {
                self.detail = false;
                vec![]
            }
            Action::Tab(tab) => {
                self.tab = tab;
                vec![]
            }
            // the screen changes right away; the device confirms with a notification
            Action::Mode(m) => {
                if let Some(s) = self.state_mut() {
                    s.mode = Some(m);
                }
                self.send(Command::Mode(m))
            }
            Action::QuickMode(i, m) => {
                let Some(d) = self.devices.get_mut(i) else { return vec![] };
                d.state.mode = Some(m);
                vec![(d.id.clone(), Command::Mode(m))]
            }
            Action::Adaptive => {
                let on = !self.state().and_then(|s| s.adaptive).unwrap_or(false);
                if let Some(s) = self.state_mut() {
                    s.adaptive = Some(on);
                }
                self.send(Command::Adaptive(on))
            }
            Action::Level(n) => {
                if let Some(s) = self.state_mut() {
                    s.anc_level = Some(n);
                }
                self.send(Command::AncLevel(n))
            }
            Action::AmbientLevel(n) => {
                if let Some(s) = self.state_mut() {
                    s.ambient_level = Some(n);
                }
                self.send(Command::AmbientLevel(n))
            }
            Action::Eq | Action::Preset(_) | Action::Custom => self.eq_action(a),
            Action::Curve => vec![],
            Action::PickGesture(g) => self.open_sheet(Sheet::Gesture(g)),
            Action::SetGesture(g, action) => {
                self.sheet_open = false;
                self.set_gesture(g, action)
            }
            Action::ToggleBeep(side) if self.beep_on(side) => self.beep(side, false),
            Action::ToggleBeep(side) => self.open_sheet(Sheet::Beep(side)),
            Action::ConfirmBeep(side) => {
                self.sheet_open = false;
                self.beep(side, true)
            }
            Action::PickOption(i) => self.open_sheet(Sheet::Options(i)),
            Action::SettingButton(i) => self.open_sheet(Sheet::Confirm(i)),
            Action::SettingOption(..) | Action::ConfirmButton(_) => {
                self.sheet_open = false;
                self.setting_action(a)
            }
            Action::SettingSwitch(_) | Action::SettingLevel(..) | Action::Slider(_) => self.setting_action(a),
            Action::AppSettings => self.open_sheet(Sheet::App),
            Action::AutoUpdate => {
                self.auto_update = !self.auto_update;
                update::set_auto(self.auto_update);
                vec![]
            }
            Action::CheckUpdate | Action::InstallUpdate => {
                self.app_request = Some(if a == Action::InstallUpdate { AppRequest::Install } else { AppRequest::Check });
                vec![]
            }
            Action::CloseSheet | Action::Dismiss => {
                self.sheet_open = false;
                vec![]
            }
        }
    }

    fn open_sheet(&mut self, s: Sheet) -> Out {
        (self.sheet, self.sheet_open) = (Some(s), true);
        vec![]
    }

    // ---------- drawing ----------

    fn target(&mut self, p: &Painter, x: f32, y: f32, w: f32, h: f32, action: Action) {
        // the part above clip_top is under the header
        let top = y.max(self.clip_top);
        if top < y + h {
            self.targets.push(Target { x: x + p.dx, y: top + p.dy, w, h: y + h - top, action });
        }
    }

    fn glass(&self, a: Action) -> Glass {
        let v = self.anim.get(&a).copied().unwrap_or_default();
        Glass { hover: v[0], press: v[1], on: v[2] }
    }

    /// Animated "on" value of control `a` (the first time it starts in place).
    fn active(&mut self, a: Action, on: bool) -> f32 {
        let target = on as u8 as f32;
        let v = self.anim.entry(a).or_insert([0.0, 0.0, target, target]);
        v[3] = target;
        v[2]
    }

    /// The model's photo in the device's color, in a `size`×`size` box; the system's headphones
    /// icon for a model without one (a photo per JBL model cannot be kept by hand).
    #[allow(clippy::too_many_arguments)]
    fn draw_photo(&mut self, p: &mut Painter, profile: &Profile, color_id: Option<u8>, cx: f32, cy: f32, size: f32) {
        if let Some(path) = profile.image(color_id)
            && let Some(img) = self.photos.entry(path).or_insert_with_key(|path| Pixmap::load_png(path).ok())
        {
            return p.photo(img, cx, cy, size);
        }
        // the icon in the middle of a canvas twice its size, so it sits small on the disc
        let icon = self.photos.entry(PathBuf::from("<system headphones icon>")).or_insert_with(|| {
            let (n, k) = (64, 128);
            let alpha = platform::headphones_icon(n)?;
            let c = color(TEXT2, 1.0);
            let mut pm = Pixmap::new(k, k)?;
            for (i, a) in alpha.into_iter().enumerate() {
                let (x, y) = (i as u32 % n + (k - n) / 2, i as u32 / n + (k - n) / 2);
                let ch = |v: f32| (v * a as f32).round() as u8;
                pm.pixels_mut()[(y * k + x) as usize] = PremultipliedColorU8::from_rgba(ch(c.red()), ch(c.green()), ch(c.blue()), a)?;
            }
            Some(pm)
        });
        match icon {
            Some(img) => p.photo(img, cx, cy, size),
            None => p.fill(&shapes::circle(cx, cy, size * 0.5), Shader::SolidColor(color(0xFFFFFF, 0.045))),
        }
    }

    pub fn draw(&mut self, pm: &mut Pixmap, scale: f32, fonts: &Fonts) {
        pm.fill(Color::TRANSPARENT);
        self.targets.clear();
        let h = self.height;
        let top = if self.tip_on_top { GAP } else { HEIGHT - h };
        let outline = shapes::rounded_rect(0.0, 0.0, WIDTH, h, RADIUS);
        let mut p = Painter { pm, scale, dx: 0.0, dy: top, alpha: 1.0, fonts };

        p.fill(&outline, Shader::SolidColor(color(PANEL, 1.0)));
        let t = self.page;
        if t < 0.999 {
            (p.dx, p.alpha) = (-WIDTH * 0.3 * t, 1.0 - t);
            self.draw_list(&mut p);
        }
        if t > 0.001 {
            (p.dx, p.alpha) = (WIDTH * (1.0 - t), 1.0);
            self.draw_detail(&mut p);
        }
        (p.dx, p.alpha) = (0.0, 1.0);
        match self.sheet.filter(|_| self.sheet_t > 0.001) {
            Some(Sheet::Gesture(g)) => self.draw_gesture_sheet(&mut p, g),
            Some(Sheet::Beep(side)) => self.draw_beep_sheet(&mut p, side),
            Some(Sheet::Options(i)) => self.draw_options_sheet(&mut p, i),
            Some(Sheet::Confirm(i)) => self.draw_confirm_sheet(&mut p, i),
            Some(Sheet::App) => self.draw_app_sheet(&mut p),
            None => {}
        }

        p.stroke(&shapes::rounded_rect(0.5, 0.5, WIDTH - 1.0, h - 1.0, RADIUS - 0.5), Shader::SolidColor(color(0xFFFFFF, 0.09)), 1.0);
        // clips what slid past the outline (pages, sheet)
        if let Some(mut m) = Mask::new(p.pm.width(), p.pm.height()) {
            m.fill_path(&outline, FillRule::Winding, true, p.transform());
            p.pm.apply_mask(&m);
        }
    }
}
