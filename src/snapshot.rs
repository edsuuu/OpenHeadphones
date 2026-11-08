//! `--snapshot <dir>`: renders every screen with a fake device to PNGs, no window (to check the look).

use std::rc::Rc;

use tiny_skia::{IntSize, Pixmap};

use crate::device::{Battery, DeviceState, NoiseMode};
use crate::i18n::t;
use crate::profile::Profile;
use crate::render::fonts::Fonts;
use crate::render::genie::genie;
use crate::ui::{self, Tab, Ui};

pub fn run(dir: &str, profiles: Vec<Rc<Profile>>) {
    let fonts = Fonts::load();
    let s = 1.2; // how it looks on a 100% monitor
    let (w, h) = ((ui::WIDTH * s).round() as u32, ((ui::HEIGHT + ui::GAP) * s).round() as u32);
    let mut pm = Pixmap::new(w, h).unwrap();
    let save = |pm: &Pixmap, name: &str| {
        pm.save_png(format!("{dir}/{name}.png")).unwrap_or_else(|e| panic!("{}: {e}", t("error.snapshot")));
    };
    // the first model with an id stands in for a real device
    let model = profiles.iter().find(|p| !p.product_ids.is_empty()).cloned().unwrap_or_default();

    let mut u = Ui::new(profiles);
    u.tip_x = 290.0;
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
    };
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

    u.preview(Tab::Sound, false);
    u.draw(&mut pm, s, &fonts);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (name, p) in [("genie1", 0.3), ("genie2", 0.6)] {
        genie(pm.data(), &mut out, w as usize, h as usize, p, 290.0 * s, 26.0 * s, false);
        save(&Pixmap::from_vec(out.clone(), IntSize::from_wh(w, h).unwrap()).unwrap(), name);
    }
}
