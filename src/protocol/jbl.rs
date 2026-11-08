//! JBL headphones over BLE, as the JBL Headphones app 5.38.2 speaks it (TuneFlex2Control,
//! HmProtocol3DeviceStatusControl, CmdGen, EQCmd, AdvancedAncCmd, GestureCmd, BeepingCmd).
//!
//! Service 65786365-…-2e636f6d0000: write to …0002, the device notifies on …0001.
//! Packet: `AA <cmd> <len> <payload>`; the EQ (cmd A2) and the device info (cmd 06) have a
//! 2-byte little-endian length.

use uuid::Uuid;

use crate::device::{Battery, Command, DeviceState, NoiseMode, Register};

pub const SERVICE: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0000);
pub const NOTIFY: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0001);
pub const WRITE: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0002);

const INFO_REQUEST: &[u8] = &[0xAA, 0x11, 0x00]; // answers 0x12: name, product id, color, MAC…
const MODE_REQUEST: &[u8] = &[0xAA, 0x91, 0x01, 0x11]; // answers 0x12 (modes) and 0x22 (advanced ANC)
pub const BATTERY_REQUEST: &[u8] = &[0xAA, 0x25, 0x01, 0x00];
const EQ_REQUEST: &[u8] = &[0xAA, 0xA2, 0x02, 0x01, 0xFF]; // 255 = the EQ that is playing
const GESTURES_REQUEST: &[u8] = &[0xAA, 0x77, 0x02, 0x01, 0xFF]; // answers sub 2 + (gesture, action) pairs
const BEEP_REQUEST: &[u8] = &[0xAA, 0x23, 0x01, 0x01]; // answers sub 2 + left, right (1 = beeping)
const BALANCE_REQUEST: &[u8] = &[0xAA, 0xA8, 0x01, 0x01]; // answers sub 2 + 01 on 02 level
const SMART_SWITCH_REQUEST: &[u8] = &[0xAA, 0x80, 0x01, 0x01]; // answers sub 2 + 4 × u16
const VOICE_PROMPT_REQUEST: &[u8] = &[0xAA, 0x93, 0x01, 0x04]; // answers sub 5 + (0 tones, 1 voice)
const TIPS_REQUEST: &[u8] = &[0xAA, 0x9C, 0x01, 0x01]; // answers sub 2 + type
const SERIAL_REQUEST: &[u8] = &[0xAA, 0x94, 0x01, 0x01]; // answers sub 2 + text
const PERSONIFI_REQUEST: &[u8] = &[0xAA, 0xA1, 0x01, 0x01]; // answers sub 2 + (key, value)…, key 1 = on
const STANDBY_REQUEST: &[u8] = &[0xAA, 0x96, 0x01, 0x01]; // answers sub + (mode, on)…: 1 normal, 2 keep ANC
/// Get/set feature (cmd B1) key 4: accidental touch prevention.
const TOUCH_GUARD: u8 = 4;

/// `AA B1 <len> 00` + (key, 0)…; answers (key, len, value)…
fn key_request(key: u8) -> Vec<u8> {
    vec![0xAA, 0xB1, 0x03, 0x00, key, 0x00]
}

/// Device info features (GenDeviceInfoCmd) read at start: firmware, auto off, spatial sound,
/// voice aware, sound effect, mic, speak to chat, auto play/pause, low volume EQ, safe sound,
/// audio limiter, batteries. Features the model lacks are just left out of the answer.
const FEATURES: [u16; 14] = [0x05, 0x0B, 0x0C, 0x0D, 0x14, 0x16, 0x08, 0x11, 0x18, 0x20, 0x23, 0x1A, 0x1B, 0x1C];

/// `AA 06 <len u16> 01` + feature ids (u16 big-endian); answers sub 2 + (id u16, len, value)…
fn feature_request(ids: &[u16]) -> Vec<u8> {
    let n = (1 + 2 * ids.len()) as u16;
    [vec![0xAA, 0x06], n.to_le_bytes().to_vec(), vec![0x01], ids.iter().flat_map(|id| id.to_be_bytes()).collect()].concat()
}

/// Asked right after connecting.
pub fn initial_requests() -> Vec<Vec<u8>> {
    let fixed = [
        INFO_REQUEST,
        MODE_REQUEST,
        BATTERY_REQUEST,
        EQ_REQUEST,
        GESTURES_REQUEST,
        BEEP_REQUEST,
        BALANCE_REQUEST,
        SMART_SWITCH_REQUEST,
        VOICE_PROMPT_REQUEST,
        TIPS_REQUEST,
        SERIAL_REQUEST,
        PERSONIFI_REQUEST,
        STANDBY_REQUEST,
    ];
    fixed.iter().map(|q| q.to_vec()).chain([key_request(TOUCH_GUARD), feature_request(&FEATURES)]).collect()
}

/// [ANC, Ambient, TalkThru] flags — ids 1, 2 and 3 in packet 0x91.
const MODES: [(NoiseMode, [u8; 3]); 4] = [
    (NoiseMode::Anc, [1, 0, 0]),
    (NoiseMode::Ambient, [0, 1, 0]),
    (NoiseMode::TalkThru, [0, 0, 1]),
    (NoiseMode::Off, [0, 0, 0]),
];

pub fn encode(cmd: &Command) -> Vec<u8> {
    match cmd {
        Command::Mode(NoiseMode::Off) => vec![0xAA, 0x91, 0x01, 0x13],
        Command::Mode(m) => {
            let [a, b, t] = MODES.iter().find(|x| x.0 == *m).map_or([0; 3], |x| x.1);
            vec![0xAA, 0x91, 0x07, 0x10, 1, a, 2, b, 3, t]
        }
        Command::Adaptive(on) => vec![0xAA, 0x91, 0x03, 0x20, 0x01, *on as u8],
        Command::AncLevel(n) => vec![0xAA, 0x91, 0x03, 0x20, 0x04, *n],
        Command::AmbientLevel(n) => vec![0xAA, 0x91, 0x03, 0x20, 0xA1, *n],
        Command::Eq(eq) => encode_eq(eq.as_ref().map(|(category, bands)| (*category, bands.as_slice()))),
        Command::Gestures(v) => {
            [vec![0xAA, 0x77, 1 + 2 * v.len() as u8, 0], v.iter().flat_map(|&(g, a)| [g, a]).collect()].concat()
        }
        Command::Beep { left, right } => {
            let side = |s: &Option<bool>| s.map_or(0xFF, |on| on as u8);
            vec![0xAA, 0x23, 0x03, 0x00, side(left), side(right)]
        }
        Command::Write(reg, v) => encode_write(*reg, v),
        Command::PowerOff => vec![0xAA, 0x97, 0x00],
        Command::FactoryReset => vec![0xAA, 0x95, 0x00],
    }
}

/// Empty when there is nothing to send (read-only register, value too short).
fn encode_write(reg: Register, v: &[u8]) -> Vec<u8> {
    let byte = |i: usize| v.get(i).copied();
    match (reg, byte(0), byte(1)) {
        (Register::Feature(id), ..) => {
            let n = (4 + v.len()) as u16;
            [vec![0xAA, 0x06], n.to_le_bytes().to_vec(), vec![0x00], id.to_be_bytes().to_vec(), vec![v.len() as u8], v.to_vec()]
                .concat()
        }
        (Register::Anc(kind), Some(x), _) => vec![0xAA, 0x91, 0x03, 0x20, kind, x],
        (Register::Balance, Some(on), Some(level)) => vec![0xAA, 0xA8, 0x05, 0x00, 0x01, on, 0x02, level],
        (Register::SmartSwitch, ..) if v.len() == 8 => [vec![0xAA, 0x80, 0x09, 0x00], v.to_vec()].concat(),
        (Register::VoicePrompt, Some(x), _) => vec![0xAA, 0x93, 0x02, 0x03, x],
        (Register::Tips, Some(x), _) => vec![0xAA, 0x9C, 0x02, 0x00, x],
        (Register::PersoniFi, Some(x), _) => vec![0xAA, 0xA1, 0x03, 0x00, 0x01, x],
        // standby on = normal mode on + keep-ANC mode off, and the other way round
        (Register::Standby, Some(x), _) => vec![0xAA, 0x96, 0x05, 0x00, 0x01, x, 0x02, (x == 0) as u8],
        (Register::Key(key), Some(x), _) => vec![0xAA, 0xB1, 0x04, 0x01, key, 0x01, x],
        _ => vec![],
    }
}

/// The device only acks these: ask again so the screen shows what stuck.
pub fn follow_up(cmd: &Command) -> Option<Vec<u8>> {
    let q = match cmd {
        Command::Gestures(_) => GESTURES_REQUEST,
        Command::Beep { .. } => BEEP_REQUEST,
        Command::Write(Register::Feature(id), _) => return Some(feature_request(&[*id])),
        Command::Write(Register::Balance, _) => BALANCE_REQUEST,
        Command::Write(Register::SmartSwitch, _) => SMART_SWITCH_REQUEST,
        Command::Write(Register::VoicePrompt, _) => VOICE_PROMPT_REQUEST,
        Command::Write(Register::Tips, _) => TIPS_REQUEST,
        Command::Write(Register::PersoniFi, _) => PERSONIFI_REQUEST,
        Command::Write(Register::Standby, _) => STANDBY_REQUEST,
        Command::Write(Register::Key(key), _) => return Some(key_request(*key)),
        _ => return None,
    };
    Some(q.to_vec())
}

/// EQ "Q×10" (sub 3) in the preset's category (230 = custom), or category 0 (off).
fn encode_eq(eq: Option<(u8, &[(f32, f32)])>) -> Vec<u8> {
    let bands = eq.map(|e| e.1);
    let mut p = vec![3, eq.map_or(0, |e| e.0)];
    // ponytail: calibration 0, what the app sends when libcalibrationLib does not load. The lib
    // subtracts the Design EQ (cat 202) peak from the total peak; port it if +6 dB distorts.
    p.extend(0f32.to_le_bytes());
    p.push(if bands.is_some() { 48 } else { 0 }); // kHz
    p.extend(0f32.to_le_bytes()); // left gain
    p.extend(0f32.to_le_bytes()); // right gain
    match bands {
        None => p.push(0),
        Some(bands) => {
            p.push(bands.len() as u8);
            for &(freq, gain) in bands {
                p.push(1); // peaking
                p.extend(gain.to_le_bytes());
                p.extend(freq.to_le_bytes());
                p.push(10); // Q 1.0 ×10
            }
        }
    }
    let n = p.len() as u16;
    [vec![0xAA, 0xA2], n.to_le_bytes().to_vec(), p].concat()
}

/// Splits a notification into `AA …` packets (sometimes several come together).
fn split(mut d: &[u8]) -> Vec<&[u8]> {
    let mut v = vec![];
    while d.len() >= 3 && d[0] == 0xAA {
        let (start, n) = match d[1] {
            0xA2 | 0x06 if d.len() >= 4 => (4, u16::from_le_bytes([d[2], d[3]]) as usize),
            0xA2 | 0x06 => break,
            _ => (3, d[2] as usize),
        };
        let end = (start + n).min(d.len());
        v.push(&d[..end]);
        d = &d[end..];
    }
    v
}

/// Updates `state` with what the device sent; tells whether anything changed.
pub fn apply(state: &mut DeviceState, data: &[u8]) -> bool {
    let before = state.clone();
    for p in split(data) {
        let pairs = || p.get(4..).unwrap_or_default().chunks_exact(2);
        match (p[1], p.get(3)) {
            (0x91, Some(0x12)) => {
                let mut flags = [0; 3];
                for pair in pairs() {
                    if let 1..=3 = pair[0] {
                        flags[pair[0] as usize - 1] = pair[1];
                    }
                }
                state.mode = MODES.iter().find(|x| x.1 == flags).map(|x| x.0);
            }
            (0x91, Some(0x22)) => {
                for pair in pairs() {
                    state.registers.insert(Register::Anc(pair[0]), vec![pair[1]]);
                    match pair[0] {
                        0x01 => state.adaptive = Some(pair[1] != 0),
                        0x04 => state.anc_level = Some(pair[1]),
                        0xA1 => state.ambient_level = Some(pair[1]),
                        _ => {}
                    }
                }
            }
            // charging L, charging R, level L, level R, charging case, level case
            (0x25, Some(0x01)) if p.len() >= 10 => {
                let b = &p[4..10];
                state.battery = Some(Battery {
                    left: b[2],
                    right: b[3],
                    case: b[5],
                    left_charging: b[0] != 0,
                    right_charging: b[1] != 0,
                    case_charging: b[4] != 0,
                });
            }
            (0x06, _) if p.get(4) == Some(&0x02) => apply_features(state, &p[5..]),
            (0xA8, Some(0x02)) if p.len() >= 8 => _ = state.registers.insert(Register::Balance, vec![p[5], p[7]]),
            (0x80, Some(0x02)) if p.len() >= 12 => _ = state.registers.insert(Register::SmartSwitch, p[4..12].to_vec()),
            (0x93, Some(0x05)) if p.len() >= 5 => _ = state.registers.insert(Register::VoicePrompt, vec![p[4]]),
            (0x9C, Some(0x02)) if p.len() >= 5 => _ = state.registers.insert(Register::Tips, vec![p[4]]),
            (0x94, Some(0x02)) => _ = state.registers.insert(Register::Serial, p[4..].to_vec()),
            (0xA1, Some(0x02)) if p.len() >= 6 && p[4] == 0x01 => _ = state.registers.insert(Register::PersoniFi, vec![p[5]]),
            (0x96, Some(_)) => {
                if let Some(m) = pairs().find(|m| m[0] == 1) {
                    state.registers.insert(Register::Standby, vec![m[1]]);
                }
            }
            // (key, len, value)…
            (0xB1, Some(_)) => {
                let mut d = p.get(4..).unwrap_or_default();
                while let [key, n, rest @ ..] = d {
                    let Some(v) = rest.get(..*n as usize) else { break };
                    if let Some(&x) = v.first() {
                        state.registers.insert(Register::Key(*key), vec![x]);
                    }
                    d = &rest[*n as usize..];
                }
            }
            (0x77, Some(0x02)) => state.gestures = Some(pairs().map(|x| (x[0], x[1])).collect()),
            (0x23, Some(0x02)) if p.len() >= 6 => state.beep = Some((p[4] == 1, p[5] == 1)),
            (0x12, _) => {
                let name = p[3..].split(|&b| b == 0).next().unwrap_or_default();
                state.name = String::from_utf8_lossy(name).trim().to_string();
                if let Some(r) = p.get(3 + name.len() + 1..3 + name.len() + 4) {
                    (state.product_id, state.color) = (Some(u16::from_le_bytes([r[0], r[1]])), Some(r[2]));
                }
            }
            (0xA2, Some(_)) if p.len() >= 20 => {
                let f = |o: usize| f32::from_le_bytes(p[o..o + 4].try_into().unwrap());
                let n = p[19] as usize;
                state.eq = Some(match p[5] {
                    201 | 202 => continue, // Personi-Fi / Design EQ (factory tuning): not the user's
                    0 => None,
                    _ if n > 0 && p.len() >= 20 + n * 10 => Some((0..n).map(|i| f(20 + i * 10 + 1)).collect()),
                    _ => continue,
                });
            }
            _ => {}
        }
    }
    *state != before
}

/// (id u16 big-endian, len, value)… — every one kept raw; the batteries also go to `battery`.
fn apply_features(state: &mut DeviceState, mut d: &[u8]) {
    while d.len() >= 3 {
        let (id, n) = (u16::from_be_bytes([d[0], d[1]]), d[2] as usize);
        let Some(v) = d.get(3..3 + n) else { break };
        if let (0x1A..=0x1C, [charging, level, ..]) = (id, v) {
            let b = state.battery.get_or_insert_default();
            let (c, l) = match id {
                0x1A => (&mut b.left_charging, &mut b.left),
                0x1B => (&mut b.right_charging, &mut b.right),
                _ => (&mut b.case_charging, &mut b.case),
            };
            (*c, *l) = (*charging != 0, *level);
        }
        state.registers.insert(Register::Feature(id), v.to_vec());
        d = &d[3 + n..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANDS: [f32; 10] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn eq(gains: &[f32]) -> Vec<(f32, f32)> {
        BANDS.iter().copied().zip(gains.iter().copied()).collect()
    }

    /// Bytes the device accepted in the Python tests.
    #[test]
    fn packets_match_the_tested_ones() {
        assert_eq!(encode(&Command::Mode(NoiseMode::Anc)), hex("aa 91 07 10 01 01 02 00 03 00"));
        assert_eq!(encode(&Command::Mode(NoiseMode::Off)), hex("aa 91 01 13"));
        assert_eq!(encode(&Command::Adaptive(false)), hex("aa 91 03 20 01 00"));
        assert_eq!(encode(&Command::AncLevel(7)), hex("aa 91 03 20 04 07"));
        assert_eq!(encode(&Command::AmbientLevel(3)), hex("aa 91 03 20 a1 03"));
        assert_eq!(encode(&Command::Eq(None)), hex("aa a2 10 00 03 00 00000000 00 00000000 00000000 00"));
        let mut g = [0.0; 10];
        (g[1], g[2]) = (4.0, 4.0);
        let p = encode(&Command::Eq(Some((230, eq(&g)))));
        assert_eq!(p.len(), 120);
        assert_eq!(p[..20], hex("aa a2 74 00 03 e6 00000000 30 00000000 00000000 0a"));
        assert_eq!(p[30..40], hex("01 00008040 00008042 0a")); // 64 Hz band, +4 dB, Q 1.0
        assert_eq!(encode(&Command::Eq(Some((7, eq(&g)))))[5], 7); // a preset (Studio) goes in its category
        assert_eq!(encode(&Command::Gestures(vec![(6, 0x0b), (9, 8)])), hex("aa 77 05 00 06 0b 09 08"));
        assert_eq!(encode(&Command::Beep { left: Some(true), right: None }), hex("aa 23 03 00 01 ff"));
    }

    /// Real notifications captured from the device.
    #[test]
    fn reads_what_the_device_sends() {
        let mut s = DeviceState::default();
        assert!(apply(&mut s, &hex("aa 91 07 12 01 00 02 01 03 00 aa 91 09 22 01 01 04 06 05 01 a1 06")));
        assert_eq!((s.mode, s.adaptive, s.anc_level, s.ambient_level), (Some(NoiseMode::Ambient), Some(true), Some(6), Some(6)));
        apply(&mut s, &hex("aa 25 0d 01 00 00 64 64 00 28 f8 10 64 00 28 00"));
        let b = s.battery.unwrap();
        assert_eq!((b.left, b.right, b.case, b.case_charging), (100, 100, 40, false));
        apply(&mut s, &hex("aa 12 25 4a 42 4c 20 54 75 6e 65 20 46 6c 65 78 20 32 00 0e 21 00 64 ac bc e4 d5 84 b4 03 05 00"));
        assert_eq!((s.name.as_str(), s.product_id, s.color), ("JBL Tune Flex 2", Some(0x210e), Some(0)));
        // gestures and beep read from his headphones (Nov 8)
        apply(&mut s, &hex("aa 77 11 02 06 0b 07 04 08 00 0c a1 09 08 0a 05 0b 06 0e a1"));
        let g = s.gestures.clone().unwrap();
        assert_eq!((g.len(), g[0], g[7]), (8, (6, 0x0b), (14, 0xa1)));
        apply(&mut s, &hex("aa 23 03 02 00 01 aa 00 02 23 00"));
        assert_eq!(s.beep, Some((false, true)));
        // echo of the EQ turned off (with firmware debug garbage at the end)
        let mut off = hex("aa a2 74 00 04 00 00000000 00 00000000 00000000 00 61 72 61 6d");
        off.resize(120, 0);
        apply(&mut s, &off);
        assert_eq!(s.eq, Some(None));
        // the Design EQ (cat 202) does not touch the user's EQ
        let mut design = hex("aa a2 74 00 04 ca 00000000 00 0000a0c0 0000a0c0 0a");
        design.resize(120, 0);
        assert!(!apply(&mut s, &design));
        // echo of a custom EQ
        let mut g = [0.0; 10];
        g[1] = 4.0;
        let mut echo = encode_eq(Some((7, &eq(&g))));
        echo[4] = 4;
        apply(&mut s, &echo);
        assert_eq!(s.eq, Some(Some(g.to_vec())));
    }

    /// The settings page: device info features and the commands of their own (log of Nov 8).
    #[test]
    fn reads_and_writes_the_settings() {
        let mut s = DeviceState::default();
        let info = "aa 06 3d 00 02 00 05 03 03 05 00 00 06 01 07 00 0b 03 00 1e 00 00 0c 02 00 01 00 0d 02 00 02 \
                    00 14 02 00 00 00 16 03 00 00 00 00 1a 02 00 64 00 1b 02 00 64 00 1c 02 00 1f 00 20 01 00 00 23 01 01";
        assert!(apply(&mut s, &hex(info)));
        let reg = |s: &DeviceState, r| s.registers.get(&r).cloned();
        assert_eq!(reg(&s, Register::Feature(0x05)), Some(vec![3, 5, 0]));
        assert_eq!(reg(&s, Register::Feature(0x0B)), Some(vec![0, 0x1e, 0]));
        assert_eq!(reg(&s, Register::Feature(0x23)), Some(vec![1]));
        assert_eq!(s.battery.map(|b| b.case), Some(0x1f));
        // a bud charging, from the info notification
        apply(&mut s, &hex("aa 06 10 00 02 00 1a 02 01 64 00 1b 02 00 64 00 1c 02 00 3e"));
        let b = s.battery.unwrap();
        assert_eq!((b.left_charging, b.right_charging, b.case), (true, false, 0x3e));
        apply(&mut s, &hex("aa 80 09 02 00 01 2e 00 18 01 ff ff aa 9c 02 02 00"));
        assert_eq!(reg(&s, Register::SmartSwitch), Some(hex("00 01 2e 00 18 01 ff ff")));
        assert_eq!(reg(&s, Register::Tips), Some(vec![0]));
        apply(&mut s, &hex("aa 94 11 02 4d 4d 30 32 34 35 2d 44 50 30 32 34 36 39 36 31"));
        assert_eq!(reg(&s, Register::Serial), Some(b"MM0245-DP0246961".to_vec()));
        apply(&mut s, &hex("aa a8 05 02 01 00 02 64 aa 93 02 05 01"));
        assert_eq!((reg(&s, Register::Balance), reg(&s, Register::VoicePrompt)), (Some(vec![0, 0x64]), Some(vec![1])));

        let write = |r, v: &[u8]| encode(&Command::Write(r, v.to_vec()));
        assert_eq!(write(Register::Feature(0x20), &[1]), hex("aa 06 05 00 00 00 20 01 01"));
        assert_eq!(write(Register::Feature(0x0B), &[1, 0x1e, 0]), hex("aa 06 07 00 00 00 0b 03 01 1e 00"));
        assert_eq!(write(Register::Anc(0x08), &[1]), hex("aa 91 03 20 08 01"));
        assert_eq!(write(Register::Balance, &[1, 40]), hex("aa a8 05 00 01 01 02 28"));
        assert_eq!(write(Register::SmartSwitch, &hex("c5 00 2e 00 78 00 ff ff")), hex("aa 80 09 00 c5 00 2e 00 78 00 ff ff"));
        assert_eq!(write(Register::VoicePrompt, &[0]), hex("aa 93 02 03 00"));
        assert!(write(Register::Serial, b"x").is_empty());
        // from the JBL app's builders (not yet tried on the device)
        assert_eq!(write(Register::PersoniFi, &[1]), hex("aa a1 03 00 01 01"));
        assert_eq!(write(Register::Standby, &[0]), hex("aa 96 05 00 01 00 02 01"));
        assert_eq!(write(Register::Key(4), &[1]), hex("aa b1 04 01 04 01 01"));
        apply(&mut s, &hex("aa 96 05 02 01 01 02 00 aa b1 04 02 04 01 01 aa a1 03 02 01 00"));
        assert_eq!(reg(&s, Register::Standby), Some(vec![1]));
        assert_eq!(reg(&s, Register::Key(4)), Some(vec![1]));
        assert_eq!(reg(&s, Register::PersoniFi), Some(vec![0]));
        assert_eq!(follow_up(&Command::Write(Register::Feature(0x20), vec![1])), Some(hex("aa 06 03 00 01 00 20")));
        assert_eq!(initial_requests().last().unwrap()[..7], hex("aa 06 1d 00 01 00 05"));
    }
}
