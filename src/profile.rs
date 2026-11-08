//! Per-model data in `devices/<model>/profile.toml` (+ photos), read at runtime: a new model that
//! speaks a known protocol is a new folder, not a new build.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde::Deserialize;

use crate::device::{NoiseMode, Register};
use crate::i18n::{t, tf};

#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// Model ids the device reports about itself.
    pub product_ids: Vec<u16>,
    /// Bluetooth name prefixes: used to find the device, and to match it before it reports its id.
    pub name_prefixes: Vec<String>,
    pub noise_modes: Vec<NoiseMode>,
    pub adaptive_anc: bool,
    /// Manual ANC levels 1…n (0 = none).
    pub anc_levels: u8,
    /// Ambient levels 1…n (0 = none).
    pub ambient_levels: u8,
    pub find_buds: bool,
    pub images: Images,
    pub eq: Option<Eq>,
    pub gestures: Vec<Gesture>,
    pub actions: Vec<GestureAction>,
    /// The settings tab, in order.
    pub settings: Vec<Setting>,
    /// Folder the profile came from; photos are relative to it.
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Images {
    pub default: Option<String>,
    /// Color id the device reports → photo file.
    pub colors: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Eq {
    pub max_db: f32,
    /// Band frequencies in Hz.
    pub bands: Vec<f32>,
    #[serde(default)]
    pub presets: Vec<Preset>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Label key `eq.<name>`.
    pub name: String,
    pub gains: Vec<f32>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn key(self) -> &'static str {
        match self {
            Side::Left => "side.left",
            Side::Right => "side.right",
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Gesture {
    pub id: u8,
    pub side: Side,
    /// Label key `gesture.<kind>`.
    pub kind: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GestureAction {
    pub id: u8,
    /// Label key `action.<name>`.
    pub name: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// On/off: byte `byte` is `on` or `off`.
    Switch,
    /// One of `options`: `values` (each `width` bytes, little-endian) from byte `byte` on.
    Choice,
    /// `min`…`max` in byte `byte`.
    Level,
    /// Read only: the register shown as `format`.
    Info,
    /// Sends `command` after a confirmation.
    Button,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Bytes joined by dots (firmware 3.5.0).
    Version,
    Text,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ButtonCommand {
    PowerOff,
    FactoryReset,
}

/// One row of the settings tab. Labels: `setting.<name>` and, if present, `setting.<name>_hint`.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub name: String,
    /// Section title key `settings.<section>`; a new title where it changes.
    pub section: String,
    pub kind: Kind,
    pub register: Option<Register>,
    #[serde(default)]
    pub byte: usize,
    #[serde(default = "one")]
    pub on: u8,
    #[serde(default)]
    pub off: u8,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default = "one_usize")]
    pub width: usize,
    #[serde(default)]
    pub min: u8,
    #[serde(default)]
    pub max: u8,
    /// Level: the neutral value (balance), shown as left/right of it.
    pub center: Option<u8>,
    /// [byte, value] pairs written along with every change (an "on" flag the JBL app sets too).
    #[serde(default)]
    pub set: Vec<(usize, u8)>,
    pub format: Option<Format>,
    pub command: Option<ButtonCommand>,
    /// Name of a switch setting: this row only counts while that one is on.
    pub when: Option<String>,
}

fn one() -> u8 {
    1
}

fn one_usize() -> usize {
    1
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// Label key `option.<name>`.
    pub name: String,
    pub values: Vec<u32>,
}

impl Setting {
    /// The option's bytes, as they go in the register.
    pub fn option_bytes(&self, c: &Choice) -> Vec<u8> {
        c.values.iter().flat_map(|v| v.to_le_bytes()[..self.width].to_vec()).collect()
    }

    /// Which option the register holds now.
    pub fn current_option(&self, value: &[u8]) -> Option<usize> {
        self.options.iter().position(|c| {
            let b = self.option_bytes(c);
            value.get(self.byte..self.byte + b.len()) == Some(&b[..])
        })
    }
}

impl Profile {
    /// Photo for the color the device reports, or the default one.
    pub fn image(&self, color: Option<u8>) -> Option<PathBuf> {
        let by_color = color.and_then(|c| self.images.colors.get(&c.to_string()));
        Some(self.dir.join(by_color.or(self.images.default.as_ref())?))
    }
}

pub fn load_all() -> Vec<Rc<Profile>> {
    let Some(root) = crate::data::root() else {
        eprintln!("{}", t("error.no_data"));
        return vec![];
    };
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(root.join("devices")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| {
            let path = dir.join("profile.toml");
            let text = std::fs::read_to_string(&path).ok()?;
            match toml::from_str::<Profile>(&text) {
                Ok(p) => Some(Rc::new(Profile { dir, ..p })),
                Err(e) => {
                    eprintln!("{}", tf("error.bad_profile", &[("path", &path.display()), ("error", &e)]));
                    None
                }
            }
        })
        .collect()
}

/// By the model id; before the device reports it, by the longest matching name prefix.
pub fn find<'a>(profiles: &'a [Rc<Profile>], product_id: Option<u16>, name: &str) -> Option<&'a Rc<Profile>> {
    let by_id = product_id.and_then(|id| profiles.iter().find(|p| p.product_ids.contains(&id)));
    by_id.or_else(|| {
        profiles
            .iter()
            .filter_map(|p| Some((p.name_prefixes.iter().filter(|n| name.starts_with(n.as_str())).map(|n| n.len()).max()?, p)))
            .max_by_key(|(len, _)| *len)
            .map(|(_, p)| p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_right_profile() {
        let all = load_all();
        let flex = find(&all, Some(0x210e), "").unwrap();
        assert_eq!((flex.name.as_str(), flex.gestures.len(), flex.actions.len()), ("JBL Tune Flex 2", 8, 10));
        assert!(flex.image(Some(14)).unwrap().ends_with("ghost-black.png"));
        assert!(flex.image(Some(99)).unwrap().ends_with("black.png"));
        assert_eq!(find(&all, None, "JBL Tune Flex 2").unwrap().name, "JBL Tune Flex 2");
        assert_eq!(find(&all, Some(0x1234), "JBL Live 770NC").unwrap().name, "JBL");
        assert!(find(&all, None, "WH-1000XM5").is_none());
    }

    #[test]
    fn profiles_are_consistent() {
        let dirs = std::fs::read_dir(crate::data::root().unwrap().join("devices")).unwrap().count();
        assert_eq!(load_all().len(), dirs, "a profile failed to load (see the error above)");
        for p in load_all() {
            for file in p.images.colors.values().chain(&p.images.default) {
                assert!(p.dir.join(file).is_file(), "{}: missing {file}", p.name);
            }
            if let Some(eq) = &p.eq {
                assert!(eq.presets.iter().all(|x| x.gains.len() == eq.bands.len()), "{}: preset size", p.name);
            }
            for s in &p.settings {
                let ok = match s.kind {
                    Kind::Choice => !s.options.is_empty() && s.register.is_some(),
                    Kind::Level => s.min < s.max && s.center.is_none_or(|c| (s.min..=s.max).contains(&c)),
                    Kind::Button => s.command.is_some(),
                    Kind::Switch | Kind::Info => s.register.is_some(),
                };
                let when_ok = s.when.as_ref().is_none_or(|w| p.settings.iter().any(|x| &x.name == w && x.kind == Kind::Switch));
                assert!(ok && when_ok, "{}: setting {}", p.name, s.name);
            }
        }
    }
}
