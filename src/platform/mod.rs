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

/// The system's own headphones icon as an alpha mask, `size`×`size` (row by row): macOS's SF Symbol
/// "headphones" at the menu bar's size for symbols, Windows' "Headphone" glyph from its icon font.
/// Linux has none here: its tray asks the icon theme for "audio-headphones" by name.
#[cfg(target_os = "macos")]
pub fn headphones_icon(size: u32) -> Option<Vec<u8>> {
    use std::ffi::CStr;

    use objc2::msg_send;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    let class = |name: &CStr| AnyClass::get(name);
    let (strings, images, config, reps, contexts) = (
        class(c"NSString")?,
        class(c"NSImage")?,
        class(c"NSImageSymbolConfiguration")?,
        class(c"NSBitmapImageRep")?,
        class(c"NSGraphicsContext")?,
    );
    autoreleasepool(|_| unsafe {
        let text = |t: &CStr| -> *mut AnyObject { msg_send![strings, stringWithUTF8String: t.as_ptr()] };
        let none = std::ptr::null_mut::<AnyObject>();
        let image: *mut AnyObject = msg_send![images, imageWithSystemSymbolName: text(c"headphones"), accessibilityDescription: none];
        if image.is_null() {
            return None;
        }
        // 16 pt in the 22 pt of the menu bar, regular weight (0)
        let config: *mut AnyObject = msg_send![config, configurationWithPointSize: size as f64 * 16.0 / 22.0, weight: 0.0f64];
        let image: *mut AnyObject = msg_send![image, imageWithSymbolConfiguration: config];
        let fits: CGSize = msg_send![image, size];
        let rep: *mut AnyObject = msg_send![reps, alloc];
        let rep: *mut AnyObject = msg_send![rep, initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(), pixelsWide: size as isize, pixelsHigh: size as isize, bitsPerSample: 8isize, samplesPerPixel: 4isize, hasAlpha: true, isPlanar: false, colorSpaceName: text(c"NSDeviceRGBColorSpace"), bytesPerRow: size as isize * 4, bitsPerPixel: 32isize];
        if rep.is_null() {
            return None;
        }
        let context: *mut AnyObject = msg_send![contexts, graphicsContextWithBitmapImageRep: rep];
        let _: () = msg_send![contexts, saveGraphicsState];
        let _: () = msg_send![contexts, setCurrentContext: context];
        let n = size as f64;
        let rect = CGRect { origin: CGPoint { x: (n - fits.width) / 2.0, y: (n - fits.height) / 2.0 }, size: fits };
        let _: () = msg_send![image, drawInRect: rect];
        let _: () = msg_send![contexts, restoreGraphicsState];
        let data: *const u8 = msg_send![rep, bitmapData];
        let alpha = (!data.is_null()).then(|| std::slice::from_raw_parts(data, (size * size * 4) as usize).chunks_exact(4).map(|p| p[3]).collect());
        let _: () = msg_send![rep, release];
        alpha
    })
}

#[cfg(windows)]
pub fn headphones_icon(size: u32) -> Option<Vec<u8>> {
    use ab_glyph::{Font, FontVec, PxScale};

    // Segoe Fluent Icons (Windows 11), else Segoe MDL2 Assets (Windows 10)
    let data = [r"C:\Windows\Fonts\SegoeIcons.ttf", r"C:\Windows\Fonts\segmdl2.ttf"].into_iter().find_map(|f| std::fs::read(f).ok())?;
    let font = FontVec::try_from_vec(data).ok()?;
    let outline = font.outline_glyph(font.glyph_id('\u{E7F6}').with_scale(PxScale::from(size as f32)))?;
    let b = outline.px_bounds();
    let (dx, dy) = (((size as f32 - b.width()) / 2.0).round() as i32, ((size as f32 - b.height()) / 2.0).round() as i32);
    let mut alpha = vec![0u8; (size * size) as usize];
    outline.draw(|x, y, c| {
        let (x, y) = (x as i32 + dx, y as i32 + dy);
        if (0..size as i32).contains(&x) && (0..size as i32).contains(&y) {
            alpha[(y as u32 * size + x as u32) as usize] = (c * 255.0).round() as u8;
        }
    });
    Some(alpha)
}

#[cfg(target_os = "linux")]
pub fn headphones_icon(_: u32) -> Option<Vec<u8>> {
    None
}
