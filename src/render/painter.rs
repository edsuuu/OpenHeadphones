//! `Painter`: draws in logical px with the monitor scale and the page offset applied.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, Path, Pixmap, PixmapPaint, Shader, Stroke, StrokeDash, Transform,
};

use super::fonts::Fonts;
use super::shapes::{accent, arc, circle, linear, radial, rounded_rect, svg_path};
use super::{GREEN, TEXT, color, icons};

/// Every letter comes out 10% bigger than the layout asks for.
const TEXT_SCALE: f32 = 1.1;

/// Animation state of a glass button.
#[derive(Clone, Copy, Default)]
pub struct Glass {
    pub hover: f32,
    pub press: f32,
    pub on: f32,
    /// Pointer in the panel's logical px, for the light that follows the mouse.
    pub light: Option<(f32, f32)>,
}

pub struct Painter<'a> {
    pub pm: &'a mut Pixmap,
    pub scale: f32,
    pub dx: f32,
    pub dy: f32,
    /// Opacity of everything drawn (disabled groups).
    pub alpha: f32,
    pub fonts: &'a Fonts,
}

impl Painter<'_> {
    pub fn transform(&self) -> Transform {
        Transform::from_row(self.scale, 0.0, 0.0, self.scale, self.dx * self.scale, self.dy * self.scale)
    }

    fn paint(&self, mut shader: Shader<'static>) -> Paint<'static> {
        shader.apply_opacity(self.alpha);
        Paint { shader, anti_alias: true, ..Default::default() }
    }

    pub fn fill(&mut self, path: &Path, shader: Shader<'static>) {
        self.fill_with(path, shader, Transform::identity());
    }

    pub fn fill_with(&mut self, path: &Path, shader: Shader<'static>, local: Transform) {
        let (paint, t) = (self.paint(shader), self.transform().pre_concat(local));
        self.pm.fill_path(path, &paint, FillRule::Winding, t, None);
    }

    pub fn stroke(&mut self, path: &Path, shader: Shader<'static>, width: f32) {
        self.stroke_with(path, shader, width, None, Transform::identity());
    }

    pub fn stroke_with(&mut self, path: &Path, shader: Shader<'static>, width: f32, dash: Option<(f32, f32)>, local: Transform) {
        let st = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            dash: dash.and_then(|(a, b)| StrokeDash::new(vec![a, b], 0.0)),
            ..Default::default()
        };
        let (paint, t) = (self.paint(shader), self.transform().pre_concat(local));
        self.pm.stroke_path(path, &paint, &st, t, None);
    }

    pub fn dashed(&mut self, path: &Path, c: Color, width: f32) {
        self.stroke_with(path, Shader::SolidColor(c), width, Some((3.0, 4.0)), Transform::identity());
    }

    fn font(&self, bold: bool) -> &FontVec {
        if bold { &self.fonts.bold } else { &self.fonts.regular }
    }

    pub fn measure(&self, txt: &str, size: f32, bold: bool) -> f32 {
        let f = self.font(bold).as_scaled(PxScale::from(size * TEXT_SCALE));
        let mut w = 0.0;
        let mut prev = None;
        for ch in txt.chars() {
            let g = f.glyph_id(ch);
            if let Some(a) = prev {
                w += f.kern(a, g);
            }
            w += f.h_advance(g);
            prev = Some(g);
        }
        w
    }

    /// Writes with the baseline at `y`; `anchor` 0 = left, 0.5 = center, 1 = right.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, x: f32, y: f32, txt: &str, size: f32, bold: bool, c: Color, anchor: f32) {
        let x = x - self.measure(txt, size, bold) * anchor;
        let s = self.scale;
        let fonts = self.fonts; // out of `self`, so the pixmap can be written
        let f = if bold { &fonts.bold } else { &fonts.regular }.as_scaled(PxScale::from(size * TEXT_SCALE * s));
        let (pw, ph) = (self.pm.width() as i32, self.pm.height() as i32);
        let a = c.alpha() * self.alpha;
        let (r, g, b) = (c.red() * a, c.green() * a, c.blue() * a);
        // on whole pixels: without hinting, a baseline in the middle of a pixel blurs everything
        let mut caret = ((x + self.dx) * s).round();
        let base = ((y + self.dy) * s).round();
        let mut prev = None;
        let mut glyphs = vec![];
        for ch in txt.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                caret += f.kern(p, id);
            }
            glyphs.push(id.with_scale_and_position(f.scale(), point(caret, base)));
            caret += f.h_advance(id);
            prev = Some(id);
        }
        let data = self.pm.data_mut();
        for gl in glyphs {
            let Some(o) = f.outline_glyph(gl) else { continue };
            let bb = o.px_bounds();
            o.draw(|gx, gy, cover| {
                let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px >= pw || py >= ph {
                    return;
                }
                let i = (py * pw + px) as usize * 4;
                // blending happens in sRGB: light text on dark comes out thin and faded unless the coverage is corrected
                let k = cover.min(1.0).powf(0.7);
                let src = [r * k, g * k, b * k, a * k];
                for (j, s) in src.iter().enumerate() {
                    let d = data[i + j] as f32 / 255.0;
                    data[i + j] = ((s + d * (1.0 - src[3])) * 255.0 + 0.5).min(255.0) as u8;
                }
            });
        }
    }

    /// `text`, shrunk to fit in `max_w` when it is wider.
    #[allow(clippy::too_many_arguments)]
    pub fn text_fit(&mut self, x: f32, y: f32, txt: &str, size: f32, max_w: f32, bold: bool, c: Color, anchor: f32) {
        let w = self.measure(txt, size, bold);
        let size = if w > max_w && max_w > 0.0 { size * max_w / w } else { size };
        self.text(x, y, txt, size, bold, c, anchor);
    }

    /// Liquid Glass style button: translucent body, shine on top, a rim that catches light at
    /// the top and bottom, and a light that follows the pointer (iOS `isInteractive`).
    pub fn glass(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, g: Glass) {
        let k = g.press * 1.5; // sinks a little when pressed
        let (x, y, w, h) = (x + k, y + k, w - 2.0 * k, h - 2.0 * k);
        let shape = rounded_rect(x, y, w, h, r);
        // short shadow
        self.fill(&rounded_rect(x - 0.5, y + 1.5, w + 1.0, h + 1.0, r + 0.5), Shader::SolidColor(color(0, 0.22)));
        // body
        let lift = 0.04 * g.hover + 0.05 * g.press;
        self.fill(&shape, linear(0.0, y, 0.0, y + h, &[(0.0, color(0xFFFFFF, 0.10 + lift)), (1.0, color(0xFFFFFF, 0.035 + lift))]));
        if g.on > 0.0 {
            self.fill(&shape, accent(x, w, 0.95 * g.on));
        }
        // specular shine
        let top = 0.16 + 0.06 * g.hover + 0.10 * g.on;
        self.fill(&shape, linear(0.0, y, 0.0, y + h * 0.55, &[(0.0, color(0xFFFFFF, top)), (1.0, color(0xFFFFFF, 0.0))]));
        // pointer light
        if let Some((px, py)) = g.light.filter(|_| g.hover > 0.01) {
            let a = (0.12 + 0.10 * g.press) * g.hover;
            self.fill(&shape, radial(px, py, w.max(h) * 0.8, &[(0.0, color(0xFFFFFF, a)), (1.0, color(0xFFFFFF, 0.0))]));
        }
        // rim: bright on top, fading in the middle, faint again at the bottom
        let rim = rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r - 0.5);
        let bright = 0.42 + 0.2 * g.on;
        let stops = [(0.0, color(0xFFFFFF, bright)), (0.45, color(0xFFFFFF, 0.06)), (1.0, color(0xFFFFFF, 0.16))];
        self.stroke(&rim, linear(0.0, y, 0.0, y + h, &stops), 1.0);
    }

    /// 24×24 stroke icon centered at (cx, cy), `size` px.
    pub fn icon(&mut self, d: &str, cx: f32, cy: f32, size: f32, c: Color, width: f32) {
        let k = size / 24.0;
        let local = Transform::from_row(k, 0.0, 0.0, k, cx - 12.0 * k, cy - 12.0 * k);
        self.stroke_with(&svg_path(d), Shader::SolidColor(c), width / k, None, local);
    }

    /// Product photo in a `size`×`size` box centered at (cx, cy), with a glow behind it
    /// (black headphones vanish on a dark panel without it).
    pub fn photo(&mut self, img: &Pixmap, cx: f32, cy: f32, size: f32) {
        let glow = radial(cx, cy, size * 0.5, &[(0.0, color(0xFFFFFF, 0.16)), (1.0, color(0xFFFFFF, 0.0))]);
        self.fill(&circle(cx, cy, size * 0.5), glow);
        let k = size / img.width() as f32;
        let t = self.transform().pre_concat(Transform::from_row(k, 0.0, 0.0, k, cx - size / 2.0, cy - size / 2.0));
        let paint = PixmapPaint { opacity: self.alpha, quality: FilterQuality::Bicubic, ..Default::default() };
        self.pm.draw_pixmap(0, 0, img.as_ref(), &paint, t, None);
    }

    /// Battery ring with the number in the middle.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, level: Option<u8>, charging: bool) {
        if let Some(a) = arc(cx, cy, r, 0.0, 360.0) {
            self.stroke(&a, Shader::SolidColor(color(0xFFFFFF, 0.08)), 4.5);
        }
        let n = level.unwrap_or(0).min(100) as f32;
        if n > 0.0
            && let Some(a) = arc(cx, cy, r, 0.0, 360.0 * n / 100.0)
        {
            let sh = if charging { Shader::SolidColor(color(GREEN, 1.0)) } else { accent(cx - r, 2.0 * r, 1.0) };
            self.stroke(&a, sh, 4.5);
        }
        let txt = level.map_or("–".to_string(), |n| format!("{n}%"));
        self.text(cx, cy + 4.5, &txt, 13.0, true, color(TEXT, 1.0), 0.5);
        if charging {
            let k = 11.0 / 24.0;
            let local = Transform::from_row(k, 0.0, 0.0, k, cx + r * 0.62 - 5.5, cy - r - 4.0);
            self.fill(&circle(cx + r * 0.62, cy - r + 1.5, 8.0), Shader::SolidColor(color(0x0E0F14, 1.0)));
            self.fill_with(&svg_path(icons::BOLT), Shader::SolidColor(color(GREEN, 1.0)), local);
        }
    }
}
