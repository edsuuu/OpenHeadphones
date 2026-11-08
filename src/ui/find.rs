//! Find tab: makes an earbud beep, after a warning to take them out of the ears (it is loud).

use tiny_skia::Shader;

use super::{Action, CONTENT_Y, Out, Tab, Ui, WIDTH};
use crate::device::{Command, DeviceState};
use crate::i18n::t;
use crate::profile::Side;
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, WARNING, color, icons, shapes};

impl Ui {
    pub(super) fn beep_on(&self, side: Side) -> bool {
        let (left, right) = self.state().and_then(|s| s.beep).unwrap_or_default();
        if side == Side::Left { left } else { right }
    }

    /// Keeps frames coming while the find tab shows a beeping earbud (pulsing rings).
    pub(super) fn beeping(&self) -> bool {
        self.detail && self.tab == Tab::Find && (self.beep_on(Side::Left) || self.beep_on(Side::Right))
    }

    pub(super) fn beep(&mut self, side: Side, on: bool) -> Out {
        if let Some(s) = self.state_mut() {
            let b = s.beep.get_or_insert((false, false));
            match side {
                Side::Left => b.0 = on,
                Side::Right => b.1 = on,
            }
        }
        let this = |s: Side| (s == side).then_some(on);
        self.send(Command::Beep { left: this(Side::Left), right: this(Side::Right) })
    }

    pub(super) fn draw_find(&mut self, p: &mut Painter, state: &DeviceState) {
        let y = CONTENT_Y;
        // the warning is not a button: solid, no glass
        p.fill(&shapes::rounded_rect(16.0, y, WIDTH - 32.0, 62.0, 16.0), Shader::SolidColor(color(WARNING, 0.10)));
        p.stroke(&shapes::rounded_rect(16.5, y + 0.5, WIDTH - 33.0, 61.0, 15.5), Shader::SolidColor(color(WARNING, 0.35)), 1.0);
        p.icon(icons::WARNING, 42.0, y + 30.0, 22.0, color(WARNING, 1.0), 1.8);
        p.text(64.0, y + 27.0, &t("find.warning"), 13.5, true, color(TEXT, 1.0), 0.0);
        p.text(64.0, y + 45.0, &t("find.warning_hint"), 11.5, false, color(TEXT2, 1.0), 0.0);

        let alpha = p.alpha;
        if !state.connected {
            p.alpha *= 0.4;
        }
        let (w, h, yb) = ((WIDTH - 42.0) / 2.0, 172.0, y + 80.0);
        let time = self.started.elapsed().as_secs_f32();
        for (k, side) in [Side::Left, Side::Right].into_iter().enumerate() {
            let (x, a, on) = (16.0 + k as f32 * (w + 10.0), Action::ToggleBeep(side), self.beep_on(side));
            if state.connected {
                self.target(p, x, yb, w, h, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, on);
            p.glass(x, yb, w, h, 20.0, g);
            let (cx, cy) = (x + w / 2.0, yb + 58.0);
            if on {
                for ring in 0..2 {
                    let f = (time * 0.9 + ring as f32 / 2.0).fract();
                    p.stroke(&shapes::circle(cx, cy, 22.0 + f * 26.0), Shader::SolidColor(color(0xFFFFFF, (1.0 - f) * 0.55)), 1.5);
                }
            }
            let c = color(if on { 0xFFFFFF } else { TEXT2 }, 1.0);
            p.icon(icons::SOUND, cx, cy, 30.0, c, 1.8);
            p.text(cx, yb + 114.0, &t(side.key()), 14.0, true, color(TEXT, 1.0), 0.5);
            p.text(cx, yb + 133.0, &t(if on { "find.playing" } else { "find.idle" }), 11.5, false, c, 0.5);
            p.text(cx, yb + 157.0, &t(if on { "find.stop" } else { "find.play" }), 12.5, true, color(TEXT, 1.0), 0.5);
        }
        p.alpha = alpha;
    }

    pub(super) fn draw_beep_sheet(&mut self, p: &mut Painter, side: Side) {
        let top = self.sheet_frame(p, 236.0);
        p.icon(icons::WARNING, WIDTH / 2.0, top + 50.0, 34.0, color(WARNING, 1.0), 2.0);
        p.text(WIDTH / 2.0, top + 98.0, &t("find.confirm_title"), 16.0, true, color(TEXT, 1.0), 0.5);
        p.text(WIDTH / 2.0, top + 121.0, &t("find.confirm_line1"), 12.5, false, color(TEXT2, 1.0), 0.5);
        p.text(WIDTH / 2.0, top + 139.0, &t("find.confirm_line2"), 12.5, false, color(TEXT2, 1.0), 0.5);
        let w = (WIDTH - 42.0) / 2.0;
        let buttons = [(Action::CloseSheet, "find.cancel", false), (Action::ConfirmBeep(side), "find.confirm", true)];
        for (k, (a, label, primary)) in buttons.into_iter().enumerate() {
            let (x, y) = (16.0 + k as f32 * (w + 10.0), top + 160.0);
            if self.sheet_open {
                self.target(p, x, y, w, 44.0, a);
            }
            let mut g = self.glass(a);
            g.on = self.active(a, primary);
            p.glass(x, y, w, 44.0, 16.0, g);
            p.text(x + w / 2.0, y + 27.0, &t(label), 13.5, true, color(0xFFFFFF, 1.0), 0.5);
        }
    }
}
