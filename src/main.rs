#![windows_subsystem = "windows"]
//! Headphones in the tray: lives in the background and, when the tray icon is clicked, the panel
//! comes out of the taskbar with the genie effect. All drawn on the CPU (tiny-skia), no GPU or WebView.
//!
//! - `app`: window, open/close animation, input. `ui`: the panel. `render`: drawing.
//! - `bluetooth` + `protocol`: the link to the devices. `device`: what they are and can do.
//! - `profile` + `devices/`: per-model data. `i18n` + `locales/`: texts, read at runtime.
//!
//! Flags: `--open` (show the panel at start), `--pinned` (does not close on focus loss),
//! `--snapshot <dir>` (render the screens to PNGs), `--lang <code>` (e.g. en, pt-BR),
//! `--debug` (log every Bluetooth packet to stderr).

mod app;
mod bluetooth;
mod data;
mod device;
mod i18n;
mod platform;
mod profile;
mod protocol;
mod render;
mod snapshot;
mod ui;

use std::sync::Mutex;

use winit::event_loop::EventLoop;

use crate::app::{App, AppEvent};
use crate::i18n::t;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
    i18n::init(value("--lang"));
    bluetooth::DEBUG.store(has("--debug"), std::sync::atomic::Ordering::Relaxed);
    let profiles = profile::load_all();
    if has("--snapshot") {
        return snapshot::run(value("--snapshot").unwrap_or("."), profiles);
    }

    #[allow(unused_mut)]
    let mut builder = EventLoop::<AppEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    {
        // X11 (XWayland too): on pure Wayland a window cannot pick its position
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    #[cfg(target_os = "macos")]
    {
        // menu bar app: no Dock icon
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().unwrap_or_else(|e| panic!("{}: {e}", t("error.event_loop")));
    let proxy = event_loop.create_proxy();
    let tx = Mutex::new(proxy.clone());
    let prefixes = profiles.iter().flat_map(|p| p.name_prefixes.clone()).collect();
    let bluetooth = bluetooth::start(prefixes, move |id, state| {
        let _ = tx.lock().unwrap().send_event(AppEvent::Device(id, state));
    });
    let mut app = App::new(proxy, bluetooth, ui::Ui::new(profiles), has("--open") || has("--pinned"), has("--pinned"));
    event_loop.run_app(&mut app).unwrap_or_else(|e| panic!("{}: {e}", t("error.event_loop")));
}
