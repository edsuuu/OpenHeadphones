//! `--snapshot <dir>`: renders every screen with a fake device to PNGs, no window (to check the look).
//! `--demo <dir>`: the same device in motion, as numbered frames (the README's animation).

use std::rc::Rc;

use tiny_skia::{Color, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::device::{Battery, DeviceState, NoiseMode};
use crate::i18n::t;
use crate::profile::Profile;
use crate::protocol::jbl;
use crate::render::fonts::Fonts;
use crate::render::genie::{genie, smooth};
use crate::ui::{self, Tab, Ui};

const SETTINGS: [&str; 5] = [
    "aa 06 3d 00 02 00 05 03 03 05 00 00 06 01 07 00 0b 03 00 1e 00 00 0c 02 00 01 00 0d 02 00 02 00 14 02 00 00 \
     00 16 03 00 00 00 00 1a 02 00 60 00 1b 02 00 61 00 1c 02 01 28 00 20 01 00 00 23 01 01",
    "aa 91 09 22 01 00 04 05 05 01 a1 03",
    "aa 80 09 02 c5 00 2e 00 18 01 ff ff",
    "aa 94 11 02 4d 4d 30 32 34 35 2d 44 50 30 32 34 36 39 36 31",
    "aa a8 05 02 01 01 02 61 aa 93 02 05 01 aa 9c 02 02 00",
];

/// The Tune Flex 2 as the real one reported itself (Nov 8): every tab has something to show.
fn fake_device(model: &Profile) -> DeviceState {
    let mut state = DeviceState {
        connected: true,
        name: model.name.clone(),
        product_id: model.product_ids.first().copied(),
        color: Some(0),
        mode: Some(NoiseMode::Anc),
        adaptive: Some(false),
        anc_level: Some(5),
        ambient_level: Some(3),
        battery: Some(Battery { left: 96, right: 97, case: 40, case_charging: true, ..Default::default() }),
        eq: model.eq.as_ref().and_then(|e| e.presets.get(2)).map(|p| Some(p.gains.clone())),
        gestures: Some(model.gestures.iter().zip(model.actions.iter().cycle().skip(6)).map(|(g, a)| (g.id, a.id)).collect()),
        beep: Some((false, true)),
        registers: Default::default(),
    };
    // the settings as the real device sent them (Nov 8)
    for packet in SETTINGS {
        let bytes: Vec<u8> = packet.split(' ').filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        jbl::apply(&mut state, &bytes);
    }
    state
}

pub fn run(dir: &str, profiles: Vec<Rc<Profile>>, s: f32) {
    let fonts = Fonts::load();
    let (w, h) = ((ui::WIDTH * s).round() as u32, ((ui::HEIGHT + ui::GAP) * s).round() as u32);
    let mut pm = Pixmap::new(w, h).unwrap();
    let save = |pm: &Pixmap, name: &str| {
        pm.save_png(format!("{dir}/{name}.png")).unwrap_or_else(|e| panic!("{}: {e}", t("error.snapshot")));
    };
    // the Tune Flex 2 stands in for a real device: the settings below are its own packets
    let model = profiles.iter().find(|p| p.product_ids.contains(&0x210e)).cloned().unwrap_or_default();

    let mut u = Ui::new(profiles);
    u.settle();
    u.draw(&mut pm, s, &fonts);
    save(&pm, "empty");

    let state = fake_device(&model);
    u.update("fake".into(), state.clone());
    u.settle();
    u.draw(&mut pm, s, &fonts);
    save(&pm, "list");
    u.preview_app_sheet();
    u.draw(&mut pm, s, &fonts);
    save(&pm, "app");
    u.escape();
    u.step(1.0);

    u.open_device(0);
    let screens = [
        ("sound", Tab::Sound, false, NoiseMode::Anc),
        ("sound-ambient", Tab::Sound, false, NoiseMode::Ambient),
        ("gestures", Tab::Gestures, false, NoiseMode::Anc),
        ("gestures-sheet", Tab::Gestures, true, NoiseMode::Anc),
        ("find", Tab::Find, false, NoiseMode::Anc),
        ("find-sheet", Tab::Find, true, NoiseMode::Anc),
        ("settings", Tab::Settings, false, NoiseMode::Anc),
        ("settings-sheet", Tab::Settings, true, NoiseMode::Anc),
    ];
    for (name, tab, sheet, mode) in screens {
        u.update("fake".into(), DeviceState { mode: Some(mode), ..state.clone() });
        u.preview(tab, sheet);
        // the controls learn their new "on" state while drawing
        for _ in 0..60 {
            u.draw(&mut pm, s, &fonts);
            u.step(1.0 / 60.0);
        }
        u.draw(&mut pm, s, &fonts);
        save(&pm, name);
    }

    // the settings scrolled halfway and to the end, under the header
    u.preview(Tab::Settings, false);
    u.draw(&mut pm, s, &fonts);
    for (name, dy) in [("settings-mid", 460.0), ("settings-end", 10_000.0)] {
        u.wheel(dy);
        u.draw(&mut pm, s, &fonts);
        save(&pm, name);
    }

    u.preview(Tab::Sound, false);
    u.draw(&mut pm, s, &fonts);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (name, p) in [("genie1", 0.3), ("genie2", 0.6)] {
        genie(pm.data(), &mut out, w as usize, h as usize, p, 290.0 * s, 26.0 * s, false);
        save(&Pixmap::from_vec(out.clone(), IntSize::from_wh(w, h).unwrap()).unwrap(), name);
    }
}

/// `--demo <dir>`: the panel coming out of the menu bar, a device opened, a noise mode picked, its
/// tabs, back and closing, at 30 frames a second and 2× (ffmpeg makes the README's GIF of them).
/// The pointer goes through the panel's own events, so hover and press look as they do for real.
pub fn demo(dir: &str, profiles: Vec<Rc<Profile>>) {
    let fonts = Fonts::load();
    let s = 2.0;
    let (w, h) = ((ui::WIDTH * s).round() as u32, ((ui::HEIGHT + ui::GAP) * s).round() as u32);
    let mut pm = Pixmap::new(w, h).unwrap();
    let mut out = vec![0u8; (w * h * 4) as usize];
    let model = profiles.iter().find(|p| p.product_ids.contains(&0x210e)).cloned().unwrap_or_default();
    let mut u = Ui::new(profiles);
    u.tip_on_top = true;
    u.update("fake".into(), fake_device(&model));
    u.settle();
    let mut n = 0;
    let mut pointer: Option<(f32, f32)> = None;
    // one frame: the panel (genie at `p`, 0 = open) on the desktop color, the pointer as a dot
    let mut shot = |u: &mut Ui, p: f32, pointer: Option<(f32, f32)>, pressed: bool| {
        u.step(1.0 / 30.0);
        u.draw(&mut pm, s, &fonts);
        genie(pm.data(), &mut out, w as usize, h as usize, p, 290.0 * s, 26.0 * s, true);
        let mut frame = Pixmap::new(w, h).unwrap();
        frame.fill(Color::from_rgba8(14, 15, 19, 255));
        let panel = Pixmap::from_vec(out.clone(), IntSize::from_wh(w, h).unwrap()).unwrap();
        frame.draw_pixmap(0, 0, panel.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        if let Some((x, y)) = pointer {
            let r = if pressed { 13.0 } else { 9.0 } * s;
            let mut paint = Paint { anti_alias: true, ..Default::default() };
            paint.set_color_rgba8(255, 255, 255, if pressed { 150 } else { 210 });
            frame.fill_path(&PathBuilder::from_circle(x * s, y * s, r).unwrap(), &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
        frame.save_png(format!("{dir}/{n:04}.png")).unwrap_or_else(|e| panic!("{}: {e}", t("error.snapshot")));
        n += 1;
    };
    // where things are (logical px of the panel, which hangs GAP below the bar)
    let row = |y: f32| y + ui::GAP;
    let column = |k: f32| 16.0 + 3.0 + (k + 0.5) * (ui::WIDTH - 38.0) / 4.0;

    // the panel comes out of the icon
    for i in 0..18 {
        shot(&mut u, 1.0 - smooth(i as f32 / 17.0), None, false);
    }
    for _ in 0..10 {
        shot(&mut u, 0.0, None, false);
    }
    go(&mut u, &mut shot, &mut pointer, (180.0, row(92.0)), 16, 20); // the device's card: its page slides in
    go(&mut u, &mut shot, &mut pointer, (column(1.0), row(285.0)), 12, 16); // Ambient
    go(&mut u, &mut shot, &mut pointer, (column(1.0), row(202.0)), 10, 22); // the touch tab
    go(&mut u, &mut shot, &mut pointer, (column(3.0), row(202.0)), 10, 22); // the settings tab
    go(&mut u, &mut shot, &mut pointer, (column(0.0), row(202.0)), 10, 12); // back to sound
    go(&mut u, &mut shot, &mut pointer, (30.0, row(30.0)), 12, 18); // back to the list
    u.pointer_left();
    for _ in 0..8 {
        shot(&mut u, 0.0, None, false);
    }
    // and back into the icon
    for i in 0..15 {
        let x = i as f32 / 14.0;
        shot(&mut u, x * x, None, false);
    }
}

/// The pointer glides to `to` in `frames`, clicks, and the panel is left `then_hold` frames to react.
fn go(
    u: &mut Ui,
    shot: &mut impl FnMut(&mut Ui, f32, Option<(f32, f32)>, bool),
    pointer: &mut Option<(f32, f32)>,
    to: (f32, f32),
    frames: u32,
    then_hold: u32,
) {
    let from = pointer.unwrap_or((330.0, 600.0));
    for i in 1..=frames {
        let k = smooth(i as f32 / frames as f32);
        let at = (from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
        *pointer = Some(at);
        u.pointer_moved(at.0, at.1);
        shot(u, 0.0, *pointer, false);
    }
    u.press();
    shot(u, 0.0, *pointer, true);
    u.release();
    for _ in 0..then_hold {
        shot(u, 0.0, *pointer, false);
    }
}
