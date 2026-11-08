//! `--snapshot <dir>`: renders every screen with a fake device to PNGs, no window (to check the look).

use std::rc::Rc;

use tiny_skia::{IntSize, Pixmap};

use crate::device::{Battery, DeviceState, NoiseMode};
use crate::i18n::t;
use crate::profile::Profile;
use crate::protocol::jbl;
use crate::render::fonts::Fonts;
use crate::render::genie::genie;
use crate::ui::{self, Tab, Ui};

const SETTINGS: [&str; 5] = [
    "aa 06 3d 00 02 00 05 03 03 05 00 00 06 01 07 00 0b 03 00 1e 00 00 0c 02 00 01 00 0d 02 00 02 00 14 02 00 00 \
     00 16 03 00 00 00 00 1a 02 00 60 00 1b 02 00 61 00 1c 02 01 28 00 20 01 00 00 23 01 01",
    "aa 91 09 22 01 00 04 05 05 01 a1 03",
    "aa 80 09 02 c5 00 2e 00 18 01 ff ff",
    "aa 94 11 02 4d 4d 30 32 34 35 2d 44 50 30 32 34 36 39 36 31",
    "aa a8 05 02 01 01 02 61 aa 93 02 05 01 aa 9c 02 02 00",
];

pub fn run(dir: &str, profiles: Vec<Rc<Profile>>) {
    let fonts = Fonts::load();
    let s = 1.0; // how it looks on a 100% monitor
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

    let state = DeviceState {
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
    let mut state = state;
    // the settings as the real device sent them (Nov 8)
    for packet in SETTINGS {
        let bytes: Vec<u8> = packet.split(' ').filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        jbl::apply(&mut state, &bytes);
    }
    u.update("fake".into(), state.clone());
    u.settle();
    u.draw(&mut pm, s, &fonts);
    save(&pm, "list");

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
