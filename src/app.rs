//! The tray app: a hidden window that comes out of the taskbar with the genie effect when the
//! tray icon is clicked, and goes back in when it loses focus.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowId, WindowLevel};

use crate::bluetooth::Bluetooth;
use crate::device::{Command, DeviceState};
use crate::i18n::t;
use crate::platform::surface::Surface;
use crate::platform::{Anchor, tray};
use crate::render::fonts::Fonts;
use crate::render::genie::{genie, smooth};
use crate::ui::{self, Ui};
use crate::update;

pub enum AppEvent {
    Open(Option<Anchor>),
    Quit,
    Device(String, DeviceState),
    Update(update::Status),
    /// A new version is in place: start it.
    Restart,
}

/// In the background: looks for a newer release and, with `install`, puts it in place (then the
/// app restarts). macOS releases are the Swift app: the Rust one there never updates itself.
fn check_update(proxy: EventLoopProxy<AppEvent>, install: bool) {
    if cfg!(target_os = "macos") {
        return;
    }
    std::thread::spawn(move || {
        let send = |s| {
            let _ = proxy.send_event(AppEvent::Update(s));
        };
        send(update::Status::Checking);
        match update::latest() {
            None => send(update::Status::Failed),
            Some(v) if !update::newer(&v) => send(update::Status::UpToDate),
            Some(v) if !install => send(update::Status::Available(v)),
            Some(v) => {
                send(update::Status::Installing(v));
                match update::install() {
                    Ok(()) => {
                        let _ = proxy.send_event(AppEvent::Restart);
                    }
                    Err(e) => {
                        eprintln!("[update] {e}");
                        send(update::Status::Failed);
                    }
                }
            }
        }
    });
}

#[derive(Clone, Copy)]
enum Phase {
    Closed,
    Opening(Instant),
    Open,
    Closing(Instant),
}

const OPEN_SECS: f32 = 0.8;
const CLOSE_SECS: f32 = 0.55;

pub struct App {
    proxy: EventLoopProxy<AppEvent>,
    bluetooth: Bluetooth,
    fonts: Fonts,
    ui: Ui,
    window: Option<Arc<Window>>,
    surface: Option<Surface>,
    tray: Option<tray::Handle>,
    phase: Phase,
    closed_at: Instant,
    scale: f32,
    pos: (i32, i32),
    /// x of the tray icon inside the window, physical px.
    tip_x: f32,
    frame: Option<Pixmap>,
    warped: Vec<u8>,
    dirty: bool,
    last_step: Instant,
    /// One frame per `period`, a whole number of screen refreshes.
    period: Duration,
    next_frame: Instant,
    open_at_start: bool,
    /// `--pinned`: does not close when it loses focus (for testing).
    pinned: bool,
}

impl App {
    pub fn new(proxy: EventLoopProxy<AppEvent>, bluetooth: Bluetooth, ui: Ui, open_at_start: bool, pinned: bool) -> App {
        App {
            proxy,
            bluetooth,
            fonts: Fonts::load(),
            ui,
            window: None,
            surface: None,
            tray: None,
            phase: Phase::Closed,
            closed_at: Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now),
            scale: 1.0,
            pos: (0, 0),
            tip_x: 0.0,
            frame: None,
            warped: vec![],
            dirty: false,
            last_step: Instant::now(),
            period: Duration::from_secs_f64(1.0 / 60.0),
            next_frame: Instant::now(),
            open_at_start,
            pinned,
        }
    }

    /// 0 = open … 1 = inside the icon.
    fn progress(&self) -> f32 {
        let x = |t: Instant, secs: f32| (t.elapsed().as_secs_f32() / secs).min(1.0);
        match self.phase {
            // eases in and out: (1 - x)³ did half the motion in the first 85 ms, a jolt
            Phase::Opening(t) => 1.0 - smooth(x(t, OPEN_SECS)),
            Phase::Closing(t) => x(t, CLOSE_SECS).powi(2),
            Phase::Open => 0.0,
            Phase::Closed => 1.0,
        }
    }

    fn open(&mut self, el: &ActiveEventLoop, anchor: Option<Anchor>) {
        let Some(window) = self.window.clone() else { return };
        let contains = |m: &MonitorHandle, a: &Anchor| {
            let (p, s) = (m.position(), m.size());
            a.x >= p.x as f64 && a.x < p.x as f64 + s.width as f64 && a.y >= p.y as f64 - 2.0 && a.y <= p.y as f64 + s.height as f64 + 2.0
        };
        let monitor = anchor
            .and_then(|a| el.available_monitors().find(|m| contains(m, &a)))
            .or_else(|| el.primary_monitor())
            .or_else(|| el.available_monitors().next());
        let Some(monitor) = monitor else { return };
        let (mp, ms, sf) = (monitor.position(), monitor.size(), monitor.scale_factor());
        // at most ~60 frames per second (one takes up to ~18 ms to draw), but a whole number of
        // refreshes: 2 per frame on 120 Hz, 3 on 144 Hz
        let hz = monitor.refresh_rate_millihertz().map_or(60.0, |m| m as f64 / 1000.0);
        self.period = Duration::from_secs_f64((hz / 61.0).ceil() / hz);
        // the system scale (the size of the macOS panel in Swift), but fitting the screen height
        let s = (sf as f32).min(ms.height as f32 * 0.85 / (ui::HEIGHT + ui::GAP));
        // no click (--open, the menu): where the icon is; failing that, the corner of the system bar
        let anchor = anchor.or_else(|| self.tray.as_ref().and_then(tray::anchor_now));
        let on_top = !cfg!(windows); // GNOME and macOS: bar at the top
        let mut a = anchor.unwrap_or(Anchor {
            x: mp.x as f64 + ms.width as f64 - 160.0 * sf,
            y: mp.y as f64 + if on_top { 0.0 } else { ms.height as f64 - 48.0 * sf },
            on_top,
        });
        if a.on_top {
            // the click on Linux comes from inside the bar: go below it
            a.y = a.y.max(mp.y as f64 + 32.0 * sf);
        }
        let (w, h) = ((ui::WIDTH * s).round() as u32, ((ui::HEIGHT + ui::GAP) * s).round() as u32);
        let margin = 8.0 * s as f64;
        let x = (a.x - w as f64 / 2.0).clamp(mp.x as f64 + margin, mp.x as f64 + ms.width as f64 - w as f64 - margin);
        let y = if a.on_top { a.y } else { a.y - h as f64 };
        self.pos = (x.round() as i32, y.round() as i32);
        self.tip_x = (a.x - x) as f32;
        self.scale = s;
        self.ui.tip_on_top = a.on_top;
        self.ui.settle();
        if self.frame.as_ref().is_none_or(|p| (p.width(), p.height()) != (w, h)) {
            self.frame = Pixmap::new(w, h);
            self.warped = vec![0; (w * h * 4) as usize];
        }
        self.phase = Phase::Opening(Instant::now());
        self.last_step = Instant::now();
        self.draw(true); // first (empty) frame before showing up
        window.set_visible(true);
        window.focus_window();
    }

    fn close(&mut self) {
        let p = self.progress();
        if matches!(self.phase, Phase::Opening(_) | Phase::Open) {
            // carry on from wherever the opening was
            let t0 = Instant::now().checked_sub(Duration::from_secs_f32(p.sqrt() * CLOSE_SECS)).unwrap_or_else(Instant::now);
            self.phase = Phase::Closing(t0);
            self.closed_at = Instant::now();
            self.last_step = Instant::now(); // idle while open is not a hitch
        }
    }

    /// `ui`: draws the panel again; without it only the genie moves the last drawing (the panel
    /// with a device takes 12–18 ms to draw, more than a 60 Hz frame; the genie 3).
    fn draw(&mut self, ui: bool) {
        let p = self.progress();
        let (Some(frame), Some(surface)) = (self.frame.as_mut(), self.surface.as_mut()) else { return };
        if ui {
            self.ui.draw(frame, self.scale, &self.fonts);
        }
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        genie(frame.data(), &mut self.warped, w, h, p, self.tip_x, 26.0 * self.scale, self.ui.tip_on_top);
        surface.present(&self.warped, w as u32, h as u32, self.pos.0, self.pos.1);
    }

    fn send(&self, commands: Vec<(String, Command)>) {
        for (id, c) in commands {
            self.bluetooth.send(&id, c);
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(t("app.name"))
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        #[cfg(not(windows))]
        let attrs = attrs.with_transparent(true);
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attrs.with_x11_window_type(vec![WindowType::Utility])
        };
        let window = Arc::new(el.create_window(attrs).unwrap_or_else(|e| panic!("{}: {e}", t("error.window"))));
        self.surface = Some(Surface::new(window.clone()));
        self.window = Some(window);
        self.tray = tray::create(self.proxy.clone());
        // updates: looked for at every start; installed right away when automatic
        check_update(self.proxy.clone(), update::auto());
        if self.open_at_start {
            self.open(el, None);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: AppEvent) {
        match ev {
            AppEvent::Device(id, state) => {
                self.ui.update(id, state);
                self.dirty = true;
            }
            AppEvent::Quit => el.exit(),
            AppEvent::Update(s) => {
                self.ui.update_status = s;
                self.dirty = true;
            }
            AppEvent::Restart => update::restart(),
            AppEvent::Open(a) => match self.phase {
                Phase::Opening(_) | Phase::Open => self.close(),
                // clicking the icon took the focus away and already closed it: closing was the point
                _ if self.closed_at.elapsed() < Duration::from_millis(400) => {}
                _ => self.open(el, a),
            },
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::Focused(false) if !self.pinned => self.close(),
            WindowEvent::CloseRequested => self.close(),
            WindowEvent::CursorMoved { position, .. } => {
                let c = self.ui.pointer_moved(position.x as f32 / self.scale, position.y as f32 / self.scale);
                self.send(c);
                self.dirty = true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.ui.pointer_left();
                self.dirty = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 48.0,
                    MouseScrollDelta::PixelDelta(d) => -d.y as f32 / self.scale,
                };
                self.ui.wheel(dy);
                self.dirty = true;
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let c = if state == ElementState::Pressed { self.ui.press() } else { self.ui.release() };
                self.send(c);
                if let Some(r) = self.ui.app_request.take() {
                    check_update(self.proxy.clone(), r == ui::AppRequest::Install);
                }
                self.dirty = true;
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key: Key::Named(NamedKey::Escape), state: ElementState::Pressed, .. },
                ..
            } => {
                if !self.ui.escape() {
                    self.close();
                }
                self.dirty = true;
            }
            WindowEvent::RedrawRequested => self.dirty = true,
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if matches!(self.phase, Phase::Closed) {
            el.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        // at the screen's beat: a 14 ms timer on a 60 Hz screen showed some frames twice and skipped others
        if now < self.next_frame {
            el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            return;
        }
        let on_beat = now - self.next_frame < self.period;
        self.next_frame = if on_beat { self.next_frame + self.period } else { now + self.period };
        let gap = now - self.last_step;
        // a hitch (macOS takes ~50 ms to show the window) pauses the genie instead of skipping a third of it
        if let Phase::Opening(t) | Phase::Closing(t) = &mut self.phase
            && gap > Duration::from_millis(34)
        {
            *t += gap - Duration::from_millis(16);
        }
        let dt = gap.as_secs_f32().min(0.05);
        self.last_step = now;
        let moving = self.ui.step(dt);
        let animating = matches!(self.phase, Phase::Opening(_) | Phase::Closing(_));
        if animating || moving || self.dirty {
            let ui = moving || self.dirty;
            self.dirty = false;
            self.draw(ui);
        }
        match self.phase {
            Phase::Opening(t) if t.elapsed().as_secs_f32() >= OPEN_SECS => {
                self.phase = Phase::Open;
                self.dirty = true;
            }
            Phase::Closing(t) if t.elapsed().as_secs_f32() >= CLOSE_SECS => {
                self.phase = Phase::Closed;
                if let Some(w) = &self.window {
                    w.set_visible(false);
                }
            }
            _ => {}
        }
        let busy = animating || moving || self.dirty;
        el.set_control_flow(if busy { ControlFlow::WaitUntil(self.next_frame) } else { ControlFlow::Wait });
    }
}
