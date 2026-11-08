//! CPU drawing on top of tiny-skia: shapes, text (ab_glyph), glass buttons, icons and the genie
//! effect. Sizes are logical px; `Painter` applies the monitor scale and the page offset.

pub mod fonts;
pub mod genie;
pub mod icons;
pub mod painter;
pub mod shapes;

use tiny_skia::Color;

pub fn color(hex: u32, a: f32) -> Color {
    let c = |s: u32| ((hex >> s) & 0xFF) as f32 / 255.0;
    Color::from_rgba(c(16), c(8), c(0), a.clamp(0.0, 1.0)).unwrap()
}

pub const TEXT: u32 = 0xF2F3F7;
pub const TEXT2: u32 = 0x9AA0AE;
pub const TEXT3: u32 = 0x5E6472;
pub const ACCENT1: u32 = 0xFF9244;
pub const ACCENT2: u32 = 0xFF4D6D;
pub const GREEN: u32 = 0x42D99B;
pub const WARNING: u32 = 0xFFC247;
/// Panel background: solid, the glass is only on buttons.
pub const PANEL: u32 = 0x15161B;
pub const SHEET: u32 = 0x1E1F26;
