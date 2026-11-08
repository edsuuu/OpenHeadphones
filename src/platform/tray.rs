//! Tray icon: tray-icon on Windows/macOS, ksni (StatusNotifierItem, no GTK) on Linux.
//! Left click opens the panel out of the icon; the right-click menu has Open and Quit.

use std::sync::Mutex;

use winit::event_loop::EventLoopProxy;

use super::Anchor;
use crate::app::AppEvent;
use crate::i18n::t;

/// The system's headphones icon, white (black "template" on macOS, which the system tints).
/// Straight RGBA.
#[cfg(not(target_os = "linux"))]
fn pixels(size: u32) -> Option<Vec<u8>> {
    let c: u8 = if cfg!(target_os = "macos") { 0 } else { 255 };
    Some(super::headphones_icon(size)?.into_iter().flat_map(|a| [c, c, c, a]).collect())
}

#[cfg(not(target_os = "linux"))]
pub type Handle = tray_icon::TrayIcon;

#[cfg(not(target_os = "linux"))]
pub fn create(proxy: EventLoopProxy<AppEvent>) -> Option<Handle> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let menu = Menu::new();
    let (open, quit) = (MenuItem::new(t("app.open"), true, None), MenuItem::new(t("app.quit"), true, None));
    menu.append_items(&[&open, &quit]).ok()?;
    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
    let tx = Mutex::new(proxy.clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let ev = if e.id == quit_id {
            AppEvent::Quit
        } else if e.id == open_id {
            AppEvent::Open(None)
        } else {
            return;
        };
        let _ = tx.lock().unwrap().send_event(ev);
    }));
    let tx = Mutex::new(proxy);
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = tx.lock().unwrap().send_event(AppEvent::Open(anchor(Some(rect))));
        }
    }));
    let icon = tray_icon::Icon::from_rgba(pixels(32)?, 32, 32).ok()?;
    let b = TrayIconBuilder::new().with_menu(Box::new(menu)).with_menu_on_left_click(false).with_tooltip(t("app.name"));
    #[cfg(target_os = "macos")]
    let b = b.with_icon_templated(icon);
    #[cfg(not(target_os = "macos"))]
    let b = b.with_icon(icon);
    b.build().ok()
}

/// Where the icon is now (to open without a click: --open, the menu).
#[cfg(not(target_os = "linux"))]
pub fn anchor_now(h: &Handle) -> Option<Anchor> {
    anchor(h.rect())
}

/// The panel comes out of the top edge of the taskbar: above the icon, or above the ^ chevron
/// when the icon is hidden in the overflow (then the click comes from inside the overflow window).
#[cfg(windows)]
fn anchor(r: Option<tray_icon::Rect>) -> Option<Anchor> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowRect};
    use windows::core::w;
    // the icon area of the taskbar; its first button is the chevron
    let mut area = RECT::default();
    unsafe {
        let bar = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let icons = FindWindowExW(Some(bar), None, w!("TrayNotifyWnd"), None).ok()?;
        GetWindowRect(icons, &mut area).ok()?;
    }
    let (l, t, r2, b) = (area.left as f64, area.top as f64, area.right as f64, area.bottom as f64);
    let center = r.map(|r| (r.position.x + r.size.width as f64 / 2.0, r.position.y + r.size.height as f64 / 2.0));
    let x = match center {
        Some((x, y)) if x >= l && x < r2 && y >= t && y < b => x,
        _ => l + (b - t) / 3.0, // chevron: 32×48 at 100%
    };
    Some(Anchor { x, y: t, on_top: false })
}

/// Menu bar on top: the panel drops down from the icon.
#[cfg(target_os = "macos")]
fn anchor(r: Option<tray_icon::Rect>) -> Option<Anchor> {
    r.map(|r| Anchor { x: r.position.x + r.size.width as f64 / 2.0, y: r.position.y + r.size.height as f64, on_top: true })
}

#[cfg(target_os = "linux")]
pub type Handle = ksni::blocking::Handle<Tray>;

#[cfg(target_os = "linux")]
pub fn anchor_now(_: &Handle) -> Option<Anchor> {
    None
}

#[cfg(target_os = "linux")]
pub struct Tray(Mutex<EventLoopProxy<AppEvent>>);

#[cfg(target_os = "linux")]
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        t("app.name")
    }

    /// The icon theme's own headphones.
    fn icon_name(&self) -> String {
        "audio-headphones".into()
    }

    fn activate(&mut self, x: i32, y: i32) {
        // ponytail: the GNOME/KDE panel is at the top or the bottom; the screen half decides.
        let anchor = ((x, y) != (0, 0)).then_some(Anchor { x: x as f64, y: y as f64, on_top: y < 400 });
        let _ = self.0.lock().unwrap().send_event(AppEvent::Open(anchor));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: String, ev: fn() -> AppEvent| {
            StandardItem {
                label,
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.0.lock().unwrap().send_event(ev());
                }),
                ..Default::default()
            }
            .into()
        };
        vec![item(t("app.open"), || AppEvent::Open(None)), item(t("app.quit"), || AppEvent::Quit)]
    }
}

#[cfg(target_os = "linux")]
pub fn create(proxy: EventLoopProxy<AppEvent>) -> Option<Handle> {
    use ksni::blocking::TrayMethods;
    Tray(Mutex::new(proxy)).spawn().ok()
}
