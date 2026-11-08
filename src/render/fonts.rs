//! The system UI font, regular and semibold: Segoe UI, SF Pro, Ubuntu or DejaVu, whichever exists.

use ab_glyph::{FontVec, VariableFont};

use crate::i18n::t;

pub struct Fonts {
    pub regular: FontVec,
    pub bold: FontVec,
}

/// (file, weight for variable fonts)
type Candidates = [(&'static str, Option<f32>); 4];

const REGULAR: Candidates = [
    (r"C:\Windows\Fonts\segoeui.ttf", None),
    ("/System/Library/Fonts/SFNS.ttf", Some(400.0)),
    ("/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf", None),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", None),
];
const BOLD: Candidates = [
    (r"C:\Windows\Fonts\seguisb.ttf", None),
    ("/System/Library/Fonts/SFNS.ttf", Some(600.0)),
    // Medium is barely heavier than Regular next to Segoe Semibold / SF 600: Bold reads like them
    ("/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf", None),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", None),
];

fn open(list: &Candidates) -> Option<FontVec> {
    list.iter().find_map(|(file, weight)| {
        let mut f = FontVec::try_from_vec(std::fs::read(file).ok()?).ok()?;
        if let Some(w) = weight {
            f.set_variation(b"wght", *w);
        }
        Some(f)
    })
}

impl Fonts {
    pub fn load() -> Fonts {
        let missing = || panic!("{}", t("error.no_font"));
        Fonts { regular: open(&REGULAR).unwrap_or_else(missing), bold: open(&BOLD).or_else(|| open(&REGULAR)).unwrap_or_else(missing) }
    }
}
