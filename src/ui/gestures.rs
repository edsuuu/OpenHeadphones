//! Touch tab: what each touch does, per earbud, and the sheet to pick a new action.

use super::widgets::section_title;
use super::{Action, CONTENT_Y, Out, Ui, WIDTH};
use crate::device::{Command, DeviceState};
use crate::i18n::{t, tf};
use crate::profile::{Profile, Side};
use crate::render::painter::Painter;
use crate::render::{TEXT, TEXT2, color, icons};

fn action_label(profile: &Profile, id: Option<u8>) -> String {
    match id {
        Some(id) => match profile.actions.iter().find(|a| a.id == id) {
            Some(a) => t(&format!("action.{}", a.name)),
            None => tf("action.unknown", &[("id", &format!("{id:#04x}"))]),
        },
        None => "–".to_string(),
    }
}

impl Ui {
    pub(super) fn draw_gestures(&mut self, p: &mut Painter, state: &DeviceState, profile: &Profile) {
        let Some(pairs) = &state.gestures else {
            p.text(WIDTH / 2.0, CONTENT_Y + 80.0, &t("gestures.loading"), 13.0, false, color(TEXT2, 1.0), 0.5);
            return;
        };
        let alpha = p.alpha;
        if !state.connected {
            p.alpha *= 0.4;
        }
        let mut y = CONTENT_Y + 12.0;
        for side in [Side::Left, Side::Right] {
            let gestures: Vec<_> = profile.gestures.iter().filter(|g| g.side == side).collect();
            if gestures.is_empty() {
                continue;
            }
            section_title(p, 16.0, y, &t(side.key()).to_uppercase());
            y += 10.0;
            for g in gestures {
                let a = Action::PickGesture(g.id);
                if state.connected {
                    self.target(p, 16.0, y, WIDTH - 32.0, 40.0, a);
                }
                p.glass(16.0, y, WIDTH - 32.0, 40.0, 14.0, self.glass(a, p));
                let label = t(&format!("gesture.{}", g.kind));
                p.text(30.0, y + 25.0, &label, 12.5, false, color(TEXT2, 1.0), 0.0);
                let current = action_label(profile, pairs.iter().find(|x| x.0 == g.id).map(|x| x.1));
                let room = WIDTH - 32.0 - 14.0 - p.measure(&label, 12.5, false) - 16.0 - 34.0;
                p.text_fit(WIDTH - 50.0, y + 25.0, &current, 13.0, room, true, color(TEXT, 1.0), 1.0);
                p.icon(icons::CHEVRON_DOWN, WIDTH - 34.0, y + 20.0, 14.0, color(TEXT2, 1.0), 1.8);
                y += 44.0;
            }
            y += 22.0;
        }
        p.alpha = alpha;
    }

    pub(super) fn draw_gesture_sheet(&mut self, p: &mut Painter, gesture: u8) {
        let profile = self.current_profile();
        let Some(g) = profile.gestures.iter().find(|g| g.id == gesture) else { return };
        let current = self.state().and_then(|s| s.gestures.as_ref()?.iter().find(|x| x.0 == gesture).map(|x| x.1));
        let top = self.sheet_frame(p, 72.0 + profile.actions.len() as f32 * 44.0);
        let title = format!("{} · {}", t(g.side.key()), t(&format!("gesture.{}", g.kind)));
        p.text(WIDTH / 2.0, top + 42.0, &title, 15.0, true, color(TEXT, 1.0), 0.5);
        for (k, action) in profile.actions.iter().enumerate() {
            let (y, a, selected) = (top + 62.0 + k as f32 * 44.0, Action::SetGesture(gesture, action.id), current == Some(action.id));
            if self.sheet_open {
                self.target(p, 16.0, y, WIDTH - 32.0, 38.0, a);
            }
            let mut gl = self.glass(a, p);
            gl.on = self.active(a, selected);
            p.glass(16.0, y, WIDTH - 32.0, 38.0, 13.0, gl);
            let c = color(if selected { 0xFFFFFF } else { TEXT }, 1.0);
            p.text(32.0, y + 24.0, &t(&format!("action.{}", action.name)), 13.0, selected, c, 0.0);
            if selected {
                p.icon(icons::CHECK, WIDTH - 38.0, y + 19.0, 16.0, c, 2.2);
            }
        }
    }

    /// Changes one gesture and sends the whole list, as the JBL app does.
    pub(super) fn set_gesture(&mut self, gesture: u8, action: u8) -> Out {
        let Some(list) = self.state_mut().and_then(|s| s.gestures.as_mut()) else { return vec![] };
        match list.iter_mut().find(|x| x.0 == gesture) {
            Some(x) => x.1 = action,
            None => list.push((gesture, action)),
        }
        let list = list.clone();
        self.send(Command::Gestures(list))
    }
}
