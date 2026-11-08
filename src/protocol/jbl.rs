//! JBL headphones over BLE, as the JBL Headphones app 5.38.2 speaks it (TuneFlex2Control,
//! HmProtocol3DeviceStatusControl, CmdGen, EQCmd, AdvancedAncCmd, GestureCmd, BeepingCmd).
//!
//! Service 65786365-…-2e636f6d0000: write to …0002, the device notifies on …0001.
//! Packet: `AA <cmd> <len> <payload>`; only the EQ (cmd A2) has a 2-byte little-endian length.

use uuid::Uuid;

use crate::device::{Battery, Command, DeviceState, NoiseMode};

pub const SERVICE: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0000);
pub const NOTIFY: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0001);
pub const WRITE: Uuid = Uuid::from_u128(0x65786365_6c70_6f69_6e74_2e636f6d0002);

const INFO_REQUEST: &[u8] = &[0xAA, 0x11, 0x00]; // answers 0x12: name, product id, color, MAC…
const MODE_REQUEST: &[u8] = &[0xAA, 0x91, 0x01, 0x11]; // answers 0x12 (modes) and 0x22 (advanced ANC)
pub const BATTERY_REQUEST: &[u8] = &[0xAA, 0x25, 0x01, 0x00];
const EQ_REQUEST: &[u8] = &[0xAA, 0xA2, 0x02, 0x01, 0xFF]; // 255 = the EQ that is playing
const GESTURES_REQUEST: &[u8] = &[0xAA, 0x77, 0x02, 0x01, 0xFF]; // answers sub 2 + (gesture, action) pairs
const BEEP_REQUEST: &[u8] = &[0xAA, 0x23, 0x01, 0x01]; // answers sub 2 + left, right (1 = beeping)

/// Asked right after connecting.
pub const INITIAL_REQUESTS: [&[u8]; 6] =
    [INFO_REQUEST, MODE_REQUEST, BATTERY_REQUEST, EQ_REQUEST, GESTURES_REQUEST, BEEP_REQUEST];

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
        Command::Eq(bands) => encode_eq(bands.as_deref()),
        Command::Gestures(v) => {
            [vec![0xAA, 0x77, 1 + 2 * v.len() as u8, 0], v.iter().flat_map(|&(g, a)| [g, a]).collect()].concat()
        }
        Command::Beep { left, right } => {
            let side = |s: &Option<bool>| s.map_or(0xFF, |on| on as u8);
            vec![0xAA, 0x23, 0x03, 0x00, side(left), side(right)]
        }
    }
}

/// The device only acks these: ask again so the screen shows what stuck.
pub fn follow_up(cmd: &Command) -> Option<&'static [u8]> {
    match cmd {
        Command::Gestures(_) => Some(GESTURES_REQUEST),
        Command::Beep { .. } => Some(BEEP_REQUEST),
        _ => None,
    }
}

/// EQ "Q×10" (sub 3) in category 230 (custom) or 0 (off).
fn encode_eq(bands: Option<&[(f32, f32)]>) -> Vec<u8> {
    let mut p = vec![3, if bands.is_some() { 230 } else { 0 }];
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
            0xA2 if d.len() >= 4 => (4, u16::from_le_bytes([d[2], d[3]]) as usize),
            0xA2 => break,
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
        let p = encode(&Command::Eq(Some(eq(&g))));
        assert_eq!(p.len(), 120);
        assert_eq!(p[..20], hex("aa a2 74 00 03 e6 00000000 30 00000000 00000000 0a"));
        assert_eq!(p[30..40], hex("01 00008040 00008042 0a")); // 64 Hz band, +4 dB, Q 1.0
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
        let mut echo = encode_eq(Some(&eq(&g)));
        echo[4] = 4;
        apply(&mut s, &echo);
        assert_eq!(s.eq, Some(Some(g.to_vec())));
    }
}
