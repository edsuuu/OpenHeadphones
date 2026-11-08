//! OS glue: putting pixels on screen with per-pixel transparency, and the tray icon.

pub mod surface;
pub mod tray;

/// Point of the bar the panel comes out of, in physical screen px.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    /// Bar at the top (macOS, GNOME): the panel drops down.
    pub on_top: bool,
}
