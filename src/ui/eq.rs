//! Equalizer: presets, the draggable curve, and keeping the screen in step with the device.

use std::time::{Duration, Instant};

use tiny_skia::{PathBuilder, Shader};

use super::widgets::{section_title, smooth_curve};
use super::{Action, HEIGHT, Out, Ui, WIDTH};
use crate::device::Command;
use crate::i18n::{t, tf};
use crate::profile;
use crate::render::painter::{Glass, Painter};
use crate::render::{TEXT2, TEXT3, color, shapes};

pub(super) struct EqEdit {
    pub(super) gains: Vec<f32>,
    pub(super) on: bool,
    /// The user's own curve, kept while trying presets.
    pub(super) custom: Vec<f32>,
    pub(super) dragging: Option<usize>,
    pub(super) sent: Instant,
    /// Plot area: x0, x1, y of 0 dB, px per `max_db`.
    pub(super) graph: (f32, f32, f32, f32),
}

impl EqEdit {
    pub(super) fn new(bands: usize) -> EqEdit {
        EqEdit {
            gains: vec![0.0; bands],
            on: false,
            custom: vec![0.0; bands],
            dragging: None,
            sent: Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now),
            graph: (0.0, 1.0, 0.0, 1.0),
        }
    }
}

fn preset_index(eq: &profile::Eq, gains: &[f32]) -> Option<usize> {
    eq.presets.iter().position(|p| p.gains.len() == gains.len() && p.gains.iter().zip(gains).all(|(a, b)| (a - b).abs() < 0.05))
}

/// 32 → "32", 1000 → "1k", 16000 → "16k".
fn band_label(f: f32) -> String {
    if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }
}

impl Ui {
    /// Brings the device's EQ to the screen without fighting what the user is doing.
    pub(super) fn sync_eq(&mut self) {
        if self.eq.dragging.is_some() || self.eq.sent.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let profile = self.current_profile();
        let Some(eq) = &profile.eq else { return };
        match self.state().and_then(|s| s.eq.clone()) {
            Some(None) => self.eq.on = false,
            Some(Some(g)) if g.len() == eq.bands.len() => {
                self.eq.on = true;
                self.eq.gains = g.iter().map(|x| (x * 2.0).round() / 2.0).collect();
                if preset_index(eq, &self.eq.gains).is_none() {
                    self.eq.custom = self.eq.gains.clone();
                }
            }
            _ => {}
        }
    }

    pub(super) fn eq_action(&mut self, a: Action) -> Out {
        let profile = self.current_profile();
        let Some(eq) = &profile.eq else { return vec![] };
        match a {
            Action::Eq => self.eq.on = !self.eq.on,
            Action::Preset(i) => (self.eq.gains, self.eq.on) = (eq.presets[i].gains.clone(), true),
            _ => (self.eq.gains, self.eq.on) = (self.eq.custom.clone(), true),
        }
        self.eq.gains.resize(eq.bands.len(), 0.0);
        self.send_eq()
    }

    fn send_eq(&mut self) -> Out {
        self.eq.sent = Instant::now();
        let profile = self.current_profile();
        let Some(eq) = &profile.eq else { return vec![] };
        let gains = self.eq.on.then(|| self.eq.gains.clone());
        if let Some(s) = self.state_mut() {
            s.eq = Some(gains.clone());
        }
        let category = preset_index(eq, &self.eq.gains).and_then(|i| eq.presets[i].category).unwrap_or(230);
        self.send(Command::Eq(gains.map(|g| (category, eq.bands.iter().copied().zip(g).collect()))))
    }

    pub(super) fn start_drag(&mut self, x: f32, y: f32) -> Out {
        let n = self.eq.gains.len();
        if n < 2 {
            return vec![];
        }
        let (x0, x1, _, _) = self.eq.graph;
        let band = ((x - x0) / ((x1 - x0) / (n - 1) as f32)).round().clamp(0.0, (n - 1) as f32) as usize;
        self.eq.dragging = Some(band);
        self.drag_band(band, y, true)
    }

    pub(super) fn drag_band(&mut self, band: usize, y: f32, first: bool) -> Out {
        let max = self.current_profile().eq.as_ref().map_or(0.0, |e| e.max_db);
        let (_, _, mid, half) = self.eq.graph;
        let Some(gain) = self.eq.gains.get_mut(band) else { return vec![] };
        // half-dB steps, never past the model's limit
        let g = ((mid - y) / half * max * 2.0).round().clamp(-2.0 * max, 2.0 * max) / 2.0;
        let changed = g != *gain;
        *gain = g;
        // at most every 120 ms while dragging; releasing sends the final value
        if first || (changed && self.eq.sent.elapsed() > Duration::from_millis(120)) { self.send_eq() } else { vec![] }
    }

    pub(super) fn end_drag(&mut self) -> Out {
        self.eq.dragging = None;
        let profile = self.current_profile();
        if profile.eq.as_ref().is_some_and(|eq| preset_index(eq, &self.eq.gains).is_none()) {
            self.eq.custom = self.eq.gains.clone();
        }
        self.send_eq()
    }

    /// Title, on/off switch, preset chips and the curve, from `y` down to the bottom.
    pub(super) fn draw_eq(&mut self, p: &mut Painter, eq: &profile::Eq, y: f32) {
        if self.eq.gains.len() != eq.bands.len() {
            self.eq = EqEdit::new(eq.bands.len());
            self.sync_eq();
        }
        section_title(p, 20.0, y + 8.0, &t("eq.section"));
        self.switch(p, WIDTH - 62.0, y - 9.0, self.eq.on, true, Action::Eq);
        p.alpha = if self.eq.on { 1.0 } else { 0.35 };
        let selected = preset_index(eq, &self.eq.gains).unwrap_or(eq.presets.len());
        let (mut x, mut y) = (16.0, y + 20.0);
        let names = eq.presets.iter().map(|preset| t(&format!("eq.{}", preset.name))).chain([t("eq.custom")]);
        for (i, name) in names.enumerate() {
            let w = p.measure(&name, 11.0, i == selected) + 24.0;
            if x + w > WIDTH - 16.0 {
                (x, y) = (16.0, y + 34.0);
            }
            let a = if i < eq.presets.len() { Action::Preset(i) } else { Action::Custom };
            if self.eq.on {
                self.target(p, x, y, w, 28.0, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, i == selected && self.eq.on);
            p.glass(x, y, w, 28.0, 14.0, g);
            let c = color(if i == selected { 0xFFFFFF } else { TEXT2 }, 1.0);
            p.text(x + w / 2.0, y + 18.0, &name, 11.0, i == selected, c, 0.5);
            x += w + 6.0;
        }
        self.draw_curve(p, eq, y + 38.0);
        p.alpha = 1.0;
    }

    fn draw_curve(&mut self, p: &mut Painter, eq: &profile::Eq, gy: f32) {
        let gh = HEIGHT - 16.0 - gy;
        let n = eq.bands.len().min(self.eq.gains.len());
        if n < 2 || gh < 60.0 {
            return;
        }
        p.glass(16.0, gy, WIDTH - 32.0, gh, 14.0, Glass::default());
        if self.eq.on {
            self.target(p, 16.0, gy, WIDTH - 32.0, gh, Action::Curve);
        }
        let (x0, x1) = (36.0, WIDTH - 36.0);
        let (top, bottom) = (gy + 16.0, gy + gh - 28.0);
        let (mid, half) = ((top + bottom) / 2.0, (bottom - top) / 2.0);
        self.eq.graph = (x0 + p.dx, x1 + p.dx, mid + p.dy, half);
        let line = |pb: &mut PathBuilder, y: f32| {
            pb.move_to(x0 - 10.0, y);
            pb.line_to(x1 + 10.0, y);
        };
        let mut grid = PathBuilder::new();
        for g in [1.0, 0.5, -0.5, -1.0] {
            line(&mut grid, mid - g * half);
        }
        p.stroke(&grid.finish().unwrap(), Shader::SolidColor(color(0xFFFFFF, 0.05)), 1.0);
        let mut zero = PathBuilder::new();
        line(&mut zero, mid);
        p.dashed(&zero.finish().unwrap(), color(0xFFFFFF, 0.16), 1.0);

        let step = (x1 - x0) / (n - 1) as f32;
        let pts: Vec<(f32, f32)> = (0..n).map(|i| (x0 + i as f32 * step, mid - self.eq.gains[i] / eq.max_db * half)).collect();
        let mut area = smooth_curve(&pts);
        area.line_to(x1, mid);
        area.line_to(x0, mid);
        area.close();
        let glow = [(0.0, color(0xFF6A5A, 0.34)), (1.0, color(0xFF6A5A, 0.04))];
        p.fill(&area.finish().unwrap(), shapes::linear(0.0, top, 0.0, mid, &glow));
        p.stroke(&smooth_curve(&pts).finish().unwrap(), shapes::accent(x0, x1 - x0, 1.0), 2.4);
        for (i, &(px, py)) in pts.iter().enumerate() {
            let idle = self.eq.dragging != Some(i);
            let r = if idle { 4.5 } else { 6.5 };
            p.fill(&shapes::circle(px, py, r), Shader::SolidColor(color(if idle { 0x16171D } else { 0xFFFFFF }, 1.0)));
            p.stroke(&shapes::circle(px, py, r), Shader::SolidColor(color(0xFFFFFF, 1.0)), 1.8);
            p.text(px, gy + gh - 10.0, &band_label(eq.bands[i]), 9.0, false, color(TEXT3, 1.0), 0.5);
        }
        if let Some(b) = self.eq.dragging.filter(|&b| b < n) {
            let (px, py) = pts[b];
            let g = self.eq.gains[b];
            let gain = if g > 0.0 { format!("+{g}") } else { format!("{g}") };
            let txt = tf("eq.point", &[("freq", &band_label(eq.bands[b])), ("gain", &gain)]);
            let w = p.measure(&txt, 11.5, true) + 18.0;
            let bx = (px - w / 2.0).clamp(20.0, WIDTH - 20.0 - w);
            let by = if py - 36.0 < gy + 2.0 { py + 14.0 } else { py - 36.0 };
            p.fill(&shapes::rounded_rect(bx, by, w, 24.0, 9.0), Shader::SolidColor(color(0xFFFFFF, 1.0)));
            p.text(bx + w / 2.0, by + 16.5, &txt, 11.5, true, color(0x111216, 1.0), 0.5);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_the_bands() {
        assert_eq!([32.0, 1000.0, 16000.0].map(band_label), ["32", "1k", "16k"]);
    }
}
