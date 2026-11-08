//! What the app knows about a pair of headphones and what it can ask of them — the same for
//! every brand and transport.

use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseMode {
    Anc,
    Ambient,
    TalkThru,
    Off,
}

impl NoiseMode {
    /// Label key suffix: `noise.<key>` (short) and `noise.<key>_long`.
    pub fn key(self) -> &'static str {
        match self {
            NoiseMode::Anc => "anc",
            NoiseMode::Ambient => "ambient",
            NoiseMode::TalkThru => "talk_thru",
            NoiseMode::Off => "off",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Battery {
    pub left: u8,
    pub right: u8,
    pub case: u8,
    pub left_charging: bool,
    pub right_charging: bool,
    pub case_charging: bool,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct DeviceState {
    pub connected: bool,
    pub name: String,
    /// Model id and color id the device reports about itself.
    pub product_id: Option<u16>,
    pub color: Option<u8>,
    pub mode: Option<NoiseMode>,
    pub adaptive: Option<bool>,
    pub anc_level: Option<u8>,
    pub battery: Option<Battery>,
    /// Gain per band in dB; `Some(None)` = EQ off.
    pub eq: Option<Option<Vec<f32>>>,
    /// (gesture id, action id) pairs.
    pub gestures: Option<Vec<(u8, u8)>>,
    /// Left, right beeping (find my buds).
    pub beep: Option<(bool, bool)>,
}

#[derive(Clone, Debug)]
pub enum Command {
    Mode(NoiseMode),
    Adaptive(bool),
    AncLevel(u8),
    /// (frequency Hz, gain dB) per band; `None` turns the EQ off.
    Eq(Option<Vec<(f32, f32)>>),
    /// The whole (gesture, action) list, as the JBL app sends it.
    Gestures(Vec<(u8, u8)>),
    /// `Some(true)` beeps, `Some(false)` stops, `None` leaves that side alone.
    Beep { left: Option<bool>, right: Option<bool> },
}
