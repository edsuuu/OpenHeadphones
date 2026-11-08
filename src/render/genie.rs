//! The macOS genie effect: the panel pours through a funnel into the tray icon.
//! Phase 1 bends the sides toward the tip; phase 2 slides the content down the funnel (they overlap).

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Warps `src` into `dst` (premultiplied RGBA, `w`×`h`). `p`: 0 = open … 1 = gone into the icon.
/// `tip_x` = x of the icon, `tip_w` = funnel width at the icon, `on_top` = bar at the top (macOS/GNOME).
#[allow(clippy::too_many_arguments)]
pub fn genie(src: &[u8], dst: &mut [u8], w: usize, h: usize, p: f32, tip_x: f32, tip_w: f32, on_top: bool) {
    if p <= 0.0 {
        dst.copy_from_slice(src);
        return;
    }
    dst.fill(0);
    let bend = smooth(p / 0.45);
    let slide = smooth((p - 0.2) / 0.8) * h as f32;
    let hf = (h - 1).max(1) as f32;
    let row = |r: usize| if on_top { h - 1 - r } else { r };
    for r in 0..h {
        // r grows toward the icon
        let f = bend * (1.0 - (std::f32::consts::PI * r as f32 / hf).cos()) / 2.0;
        let left = f * (tip_x - tip_w / 2.0);
        let right = w as f32 + f * (tip_x + tip_w / 2.0 - w as f32);
        let sr = r as f32 - slide;
        if sr < 0.0 || sr > hf || right - left < 0.5 {
            continue;
        }
        let (r0, fr) = (sr.floor() as usize, sr.fract());
        let r1 = (r0 + 1).min(h - 1);
        let (a, b) = (row(r0) * w * 4, row(r1) * w * 4);
        let o = row(r) * w * 4;
        let scale = w as f32 / (right - left);
        let x0 = left.floor().max(0.0) as usize;
        let x1 = (right.ceil() as usize).min(w);
        for x in x0..x1 {
            let cover = ((x as f32 + 1.0).min(right) - (x as f32).max(left)).clamp(0.0, 1.0);
            let sx = ((x as f32 + 0.5 - left) * scale - 0.5).clamp(0.0, (w - 1) as f32);
            let (c0, fc) = (sx.floor() as usize, sx.fract());
            let c1 = (c0 + 1).min(w - 1);
            for k in 0..4 {
                let px = |base: usize, c: usize| src[base + c * 4 + k] as f32;
                let top = px(a, c0) * (1.0 - fc) + px(a, c1) * fc;
                let bottom = px(b, c0) * (1.0 - fc) + px(b, c1) * fc;
                dst[o + x * 4 + k] = ((top * (1.0 - fr) + bottom * fr) * cover + 0.5) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_closes_and_narrows() {
        let (w, h) = (40, 30);
        let src = vec![255u8; w * h * 4];
        let mut dst = vec![0u8; w * h * 4];
        genie(&src, &mut dst, w, h, 0.0, 30.0, 4.0, false);
        assert_eq!(dst, src);
        genie(&src, &mut dst, w, h, 1.0, 30.0, 4.0, false);
        assert!(dst.iter().all(|&b| b == 0));
        // halfway, the last row (next to the icon) only has color around the icon's x
        genie(&src, &mut dst, w, h, 0.5, 30.0, 4.0, false);
        let last = &dst[(h - 1) * w * 4..];
        let filled: Vec<usize> = (0..w).filter(|x| last[x * 4 + 3] > 0).collect();
        assert!(!filled.is_empty() && filled.iter().all(|&x| (27..=32).contains(&x)), "{filled:?}");
        // with the icon on top, the first row is the tip
        genie(&src, &mut dst, w, h, 0.5, 10.0, 4.0, true);
        assert!((0..w).filter(|x| dst[x * 4 + 3] > 0).all(|x| (7..=12).contains(&x)));
    }
}
