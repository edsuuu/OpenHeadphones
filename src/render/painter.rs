//! `Painter`: draws in logical px with the monitor scale and the page offset applied.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, Path, Pixmap, PixmapPaint, Shader, Stroke, StrokeDash, Transform,
};

use super::fonts::Fonts;
use super::shapes::{accent, circle, rounded_rect, svg_path};
use super::color;

/// Every letter comes out 10% bigger than the layout asks for.
const TEXT_SCALE: f32 = 1.1;

/// Animation state of a glass button.
#[derive(Clone, Copy, Default)]
pub struct Glass {
    pub hover: f32,
    pub press: f32,
    pub on: f32,
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

    /// Flat glass: a faint white body with a hairline rim, the accent gradient when on.
    pub fn glass(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, g: Glass) {
        let k = g.press; // sinks a little when pressed
        let (x, y, w, h) = (x + k, y + k, w - 2.0 * k, h - 2.0 * k);
        let shape = rounded_rect(x, y, w, h, r);
        self.fill(&shape, Shader::SolidColor(color(0xFFFFFF, 0.055 + 0.045 * g.hover + 0.03 * g.press)));
        if g.on > 0.0 {
            self.fill(&shape, accent(x, w, g.on));
        }
        let rim = rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r - 0.5);
        self.stroke(&rim, Shader::SolidColor(color(0xFFFFFF, 0.08 + 0.14 * g.on)), 1.0);
    }

    /// 24×24 stroke icon centered at (cx, cy), `size` px.
    pub fn icon(&mut self, d: &str, cx: f32, cy: f32, size: f32, c: Color, width: f32) {
        let k = size / 24.0;
        let local = Transform::from_row(k, 0.0, 0.0, k, cx - 12.0 * k, cy - 12.0 * k);
        self.stroke_with(&svg_path(d), Shader::SolidColor(c), width / k, None, local);
    }

    /// Product photo in a `size`×`size` box centered at (cx, cy), on a faint disc (black
    /// headphones vanish on a dark panel without it).
    pub fn photo(&mut self, img: &Pixmap, cx: f32, cy: f32, size: f32) {
        self.fill(&circle(cx, cy, size * 0.5), Shader::SolidColor(color(0xFFFFFF, 0.045)));
        let k = size / img.width() as f32;
        let t = self.transform().pre_concat(Transform::from_row(k, 0.0, 0.0, k, cx - size / 2.0, cy - size / 2.0));
        let paint = PixmapPaint { opacity: self.alpha, quality: FilterQuality::Bicubic, ..Default::default() };
        self.pm.draw_pixmap(0, 0, img.as_ref(), &paint, t, None);
    }
}
