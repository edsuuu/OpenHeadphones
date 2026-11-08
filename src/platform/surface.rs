//! Puts the frame (premultiplied RGBA) on screen with per-pixel transparency.
//! Windows: layered window (UpdateLayeredWindow), where alpha 0 also lets the click through.
//! Linux (X11): softbuffer on a 32-bit visual. macOS: a CALayer holding the image.

#[cfg(windows)]
pub use win::Surface;

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::sync::Arc;

    use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
    use windows::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteObject, HBITMAP, HDC, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, ULW_ALPHA, UpdateLayeredWindow, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// Layered + kept out of the taskbar and Alt+Tab.
    const STYLE: isize = (WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0) as isize;

    pub struct Surface {
        hwnd: HWND,
        dc: HDC,
        bitmap: HBITMAP,
        bits: *mut u8,
        size: (u32, u32),
    }

    impl Surface {
        pub fn new(window: Arc<Window>) -> Surface {
            let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else { unreachable!() };
            let hwnd = HWND(h.hwnd.get() as *mut c_void);
            unsafe {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | STYLE);
                Surface { hwnd, dc: CreateCompatibleDC(None), bitmap: HBITMAP::default(), bits: std::ptr::null_mut(), size: (0, 0) }
            }
        }

        pub fn present(&mut self, rgba: &[u8], w: u32, h: u32, x: i32, y: i32) {
            unsafe {
                // winit rewrites the extended style when showing/hiding the window and drops "layered"
                let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                if ex & STYLE != STYLE {
                    SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | STYLE);
                }
                if self.size != (w, h) {
                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: w as i32,
                            biHeight: -(h as i32), // top-down
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut bits = std::ptr::null_mut();
                    let Ok(bitmap) = CreateDIBSection(Some(self.dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                        return;
                    };
                    SelectObject(self.dc, bitmap.into());
                    if !self.bitmap.is_invalid() {
                        let _ = DeleteObject(self.bitmap.into());
                    }
                    (self.bitmap, self.bits, self.size) = (bitmap, bits as *mut u8, (w, h));
                }
                let dib = std::slice::from_raw_parts_mut(self.bits, (w * h * 4) as usize);
                for (d, s) in dib.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], s[3]]); // RGBA → BGRA, already premultiplied
                }
                let blend =
                    BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
                let _ = UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    Some(&POINT { x, y }),
                    Some(&SIZE { cx: w as i32, cy: h as i32 }),
                    Some(self.dc),
                    Some(&POINT { x: 0, y: 0 }),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use x11::Surface;

#[cfg(target_os = "linux")]
mod x11 {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::window::Window;

    pub struct Surface {
        _context: softbuffer::Context<Arc<Window>>,
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
        window: Arc<Window>,
        size: (u32, u32),
        pos: (i32, i32),
    }

    impl Surface {
        pub fn new(window: Arc<Window>) -> Surface {
            let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
            let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer");
            Surface { _context: context, surface, window, size: (0, 0), pos: (i32::MIN, 0) }
        }

        pub fn present(&mut self, rgba: &[u8], w: u32, h: u32, x: i32, y: i32) {
            if self.size != (w, h) {
                let _ = self.window.request_inner_size(PhysicalSize::new(w, h));
                let _ = self.surface.resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap());
                self.size = (w, h);
            }
            if self.pos != (x, y) {
                self.window.set_outer_position(PhysicalPosition::new(x, y));
                self.pos = (x, y);
            }
            let Ok(mut buf) = self.surface.buffer_mut() else { return };
            // X11 32-bit visual: the top byte is alpha (premultiplied)
            for (d, s) in buf.iter_mut().zip(rgba.chunks_exact(4)) {
                *d = (s[3] as u32) << 24 | (s[0] as u32) << 16 | (s[1] as u32) << 8 | s[2] as u32;
            }
            let _ = buf.present();
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::Surface;

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Arc;

    use objc2::rc::Retained;
    use objc2_app_kit::{NSColor, NSView};
    use objc2_core_foundation::{CFData, CFRetained};
    use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo};
    use objc2_quartz_core::CALayer;
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    pub struct Surface {
        window: Arc<Window>,
        layer: Retained<CALayer>,
        colors: CFRetained<CGColorSpace>,
        pos: (i32, i32),
    }

    impl Surface {
        pub fn new(window: Arc<Window>) -> Surface {
            let Ok(RawWindowHandle::AppKit(h)) = window.window_handle().map(|h| h.as_raw()) else { unreachable!() };
            let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
            view.setWantsLayer(true);
            let layer = view.layer().expect("CALayer");
            if let Some(w) = view.window() {
                w.setOpaque(false);
                w.setBackgroundColor(Some(&NSColor::clearColor()));
                w.setHasShadow(false);
            }
            let colors = CGColorSpace::new_device_rgb().expect("CGColorSpace");
            Surface { window, layer, colors, pos: (i32::MIN, 0) }
        }

        pub fn present(&mut self, rgba: &[u8], w: u32, h: u32, x: i32, y: i32) {
            let _ = self.window.request_inner_size(PhysicalSize::new(w, h));
            if self.pos != (x, y) {
                self.window.set_outer_position(PhysicalPosition::new(x, y));
                self.pos = (x, y);
            }
            let data = CFData::from_bytes(rgba);
            let Some(provider) = CGDataProvider::with_cf_data(Some(&data)) else { return };
            let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
            let image = unsafe {
                CGImage::new(
                    w as usize,
                    h as usize,
                    8,
                    32,
                    w as usize * 4,
                    Some(&self.colors),
                    info,
                    Some(&provider),
                    std::ptr::null(),
                    false,
                    CGColorRenderingIntent::RenderingIntentDefault,
                )
            };
            if let Some(image) = image {
                unsafe { self.layer.setContents(Some(image.as_ref())) };
            }
        }
    }
}
