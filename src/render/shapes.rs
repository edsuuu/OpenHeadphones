//! Paths and gradients.

use tiny_skia::{Color, GradientStop, LinearGradient, Path, PathBuilder, Point, RadialGradient, Shader, SpreadMode, Transform};

use super::{ACCENT1, ACCENT2, color};

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = r * 0.4477; // r × (1 − 0.5523): control points of a quarter circle
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - k, y, x + w, y + k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - k, x + w - k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + k, y + h, x, y + h - k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + k, x + k, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

pub fn circle(cx: f32, cy: f32, r: f32) -> Path {
    PathBuilder::from_circle(cx, cy, r).unwrap()
}

/// Arc from `a0` to `a1` degrees (0 = top, clockwise).
pub fn arc(cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let n = ((a1 - a0).abs() / 6.0).ceil().max(1.0) as usize;
    for i in 0..=n {
        let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
        let (x, y) = (cx + r * a.sin(), cy - r * a.cos());
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    pb.finish()
}

/// Mini SVG path: absolute M L H V C Q Z only (enough for the icons).
pub fn svg_path(d: &str) -> Path {
    let mut pb = PathBuilder::new();
    let mut nums = Vec::new();
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut cmd = ' ';
    let flush = |cmd: char, nums: &mut Vec<f32>, pb: &mut PathBuilder, x: &mut f32, y: &mut f32| {
        let n = match cmd {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'Q' => 4,
            _ => 0,
        };
        let mut first = true;
        while n > 0 && nums.len() >= n {
            let a: Vec<f32> = nums.drain(..n).collect();
            match cmd {
                'M' if first => pb.move_to(a[0], a[1]),
                'M' | 'L' => pb.line_to(a[0], a[1]),
                'H' => pb.line_to(a[0], *y),
                'V' => pb.line_to(*x, a[0]),
                'C' => pb.cubic_to(a[0], a[1], a[2], a[3], a[4], a[5]),
                _ => pb.quad_to(a[0], a[1], a[2], a[3]),
            }
            first = false;
            match cmd {
                'H' => *x = a[0],
                'V' => *y = a[0],
                _ => (*x, *y) = (a[n - 2], a[n - 1]),
            }
        }
        if cmd == 'Z' {
            pb.close();
        }
    };
    let mut num = String::new();
    for ch in d.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && num.is_empty()) {
            num.push(ch);
            continue;
        }
        if !num.is_empty() {
            nums.push(num.parse().unwrap_or(0.0));
            num.clear();
        }
        if ch == '-' {
            num.push(ch);
        } else if ch.is_ascii_alphabetic() {
            flush(cmd, &mut nums, &mut pb, &mut x, &mut y);
            nums.clear();
            cmd = ch;
            if cmd == 'Z' {
                flush(cmd, &mut nums, &mut pb, &mut x, &mut y);
                cmd = ' ';
            }
        }
    }
    flush(cmd, &mut nums, &mut pb, &mut x, &mut y);
    pb.finish().unwrap()
}

pub fn linear(x0: f32, y0: f32, x1: f32, y1: f32, stops: &[(f32, Color)]) -> Shader<'static> {
    let s = stops.iter().map(|(p, c)| GradientStop::new(*p, *c)).collect();
    LinearGradient::new(Point::from_xy(x0, y0), Point::from_xy(x1, y1), s, SpreadMode::Pad, Transform::identity())
        .unwrap_or(Shader::SolidColor(stops[0].1))
}

pub fn radial(cx: f32, cy: f32, r: f32, stops: &[(f32, Color)]) -> Shader<'static> {
    let s = stops.iter().map(|(p, c)| GradientStop::new(*p, *c)).collect();
    let c = Point::from_xy(cx, cy);
    RadialGradient::new(c, c, r, s, SpreadMode::Pad, Transform::identity()).unwrap_or(Shader::SolidColor(stops[0].1))
}

/// The accent gradient (orange → coral), horizontal.
pub fn accent(x: f32, w: f32, a: f32) -> Shader<'static> {
    linear(x, 0.0, x + w, 0.0, &[(0.0, color(ACCENT1, a)), (1.0, color(ACCENT2, a))])
}
