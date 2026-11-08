//! Per-model data in `devices/<model>/profile.toml` (+ photos), read at runtime: a new model that
//! speaks a known protocol is a new folder, not a new build.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde::Deserialize;

use crate::device::NoiseMode;
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
    pub find_buds: bool,
    pub images: Images,
    pub eq: Option<Eq>,
    pub gestures: Vec<Gesture>,
    pub actions: Vec<GestureAction>,
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
        }
    }
}
