// JBL headphones over BLE: a port of src/protocol/jbl.rs and src/device.rs, which stay the source
// of truth (keep both in step; `--selftest` checks the same captured packets as the Rust tests).
//
// Service 65786365-…-2e636f6d0000: write to …0002, the device notifies on …0001.
// Packet: `AA <cmd> <len> <payload>`; the EQ (cmd A2) and the device info (cmd 06) have a
// 2-byte little-endian length.

import CoreBluetooth

enum NoiseMode: String {
    case anc, ambient, talkThru = "talk_thru", off
}

/// Where a setting's bytes live on the device (named in the profiles' `settings`).
enum Register: Hashable {
    /// Device info feature (id), the value as the device stores it.
    case feature(UInt16)
    /// One advanced ANC value (type).
    case anc(UInt8)
    /// [on, level]
    case balance
    /// AAC, SBC and two latencies, u16 little-endian each.
    case smartSwitch
    /// [0 = tones, 1 = voice]
    case voicePrompt
    case tips, personiFi, standby
    /// [value] of a key of the get/set feature command.
    case key(UInt8)
    /// Serial number, text (read only).
    case serial
}

struct Battery: Equatable {
    var left = 0, right = 0, caseLevel = 0
    var leftCharging = false, rightCharging = false, caseCharging = false
}

/// A gesture and the action it triggers.
struct Touch: Equatable {
    var gesture: UInt8, action: UInt8
}

struct DeviceState: Equatable {
    var connected = false
    var name = ""
    /// Model id and color id the device reports about itself.
    var productId: UInt16?
    var color: UInt8?
    var mode: NoiseMode?
    var adaptive: Bool?
    var ancLevel: UInt8?
    /// How much of the outside comes in, in ambient mode.
    var ambientLevel: UInt8?
    var battery: Battery?
    /// Gain per band in dB; `.some(nil)` = EQ off.
    var eq: [Float]??
    var gestures: [Touch]?
    /// [left, right] beeping (find my buds).
    var beep: [Bool]?
    /// Raw values behind the settings tab, as last read.
    var registers: [Register: [UInt8]] = [:]
}

enum Command {
    case mode(NoiseMode), adaptive(Bool), ancLevel(UInt8), ambientLevel(UInt8)
    /// (frequency Hz, gain dB) per band; nil turns the EQ off.
    case eq([(freq: Float, gain: Float)]?)
    /// The whole list, as the JBL app sends it.
    case gestures([Touch])
    /// true beeps, false stops, nil leaves that side alone.
    case beep(left: Bool?, right: Bool?)
    /// The whole value of a register.
    case write(Register, [UInt8])
    case powerOff, factoryReset
}

extension Array where Element == UInt8 {
    /// Bounds-checked: what the device sends is never trusted to be as long as it should.
    subscript(at i: Int) -> UInt8? { indices.contains(i) ? self[i] : nil }
    /// From `i` to the end (empty past it).
    func from(_ i: Int) -> [UInt8] { i < count ? Array(self[i...]) : [] }
}

func hexString(_ b: [UInt8]) -> String { b.map { String(format: "%02x", $0) }.joined(separator: " ") }

enum JBL {
    static let service = CBUUID(string: "65786365-6C70-6F69-6E74-2E636F6D0000")
    static let notify = CBUUID(string: "65786365-6C70-6F69-6E74-2E636F6D0001")
    static let write = CBUUID(string: "65786365-6C70-6F69-6E74-2E636F6D0002")

    static let infoRequest: [UInt8] = [0xAA, 0x11, 0x00] // answers 0x12: name, product id, color, MAC…
    static let modeRequest: [UInt8] = [0xAA, 0x91, 0x01, 0x11] // answers 0x12 (modes) and 0x22 (advanced ANC)
    static let batteryRequest: [UInt8] = [0xAA, 0x25, 0x01, 0x00]
    static let eqRequest: [UInt8] = [0xAA, 0xA2, 0x02, 0x01, 0xFF] // 255 = the EQ that is playing
    static let gesturesRequest: [UInt8] = [0xAA, 0x77, 0x02, 0x01, 0xFF]
    static let beepRequest: [UInt8] = [0xAA, 0x23, 0x01, 0x01]
    static let balanceRequest: [UInt8] = [0xAA, 0xA8, 0x01, 0x01]
    static let smartSwitchRequest: [UInt8] = [0xAA, 0x80, 0x01, 0x01]
    static let voicePromptRequest: [UInt8] = [0xAA, 0x93, 0x01, 0x04]
    static let tipsRequest: [UInt8] = [0xAA, 0x9C, 0x01, 0x01]
    static let serialRequest: [UInt8] = [0xAA, 0x94, 0x01, 0x01]
    static let personiFiRequest: [UInt8] = [0xAA, 0xA1, 0x01, 0x01]
    static let standbyRequest: [UInt8] = [0xAA, 0x96, 0x01, 0x01]
    /// Get/set feature (cmd B1) key 4: accidental touch prevention.
    static let touchGuard: UInt8 = 4
    /// Device info features read at start: firmware, auto off, spatial sound, voice aware, sound
    /// effect, mic, speak to chat, auto play/pause, low volume EQ, safe sound, audio limiter, batteries.
    static let features: [UInt16] = [0x05, 0x0B, 0x0C, 0x0D, 0x14, 0x16, 0x08, 0x11, 0x18, 0x20, 0x23, 0x1A, 0x1B, 0x1C]

    static func le16(_ n: Int) -> [UInt8] { [UInt8(n & 0xFF), UInt8((n >> 8) & 0xFF)] }
    static func f32(_ x: Float) -> [UInt8] { withUnsafeBytes(of: x.bitPattern.littleEndian, Array.init) }

    /// `AA B1 <len> 00` + (key, 0)…; answers (key, len, value)…
    static func keyRequest(_ key: UInt8) -> [UInt8] { [0xAA, 0xB1, 0x03, 0x00, key, 0x00] }

    /// `AA 06 <len u16> 01` + feature ids (u16 big-endian); answers sub 2 + (id u16, len, value)…
    static func featureRequest(_ ids: [UInt16]) -> [UInt8] {
        [0xAA, 0x06] + le16(1 + 2 * ids.count) + [0x01] + ids.flatMap { [UInt8($0 >> 8), UInt8($0 & 0xFF)] }
    }

    /// Asked right after connecting.
    static func initialRequests() -> [[UInt8]] {
        [infoRequest, modeRequest, batteryRequest, eqRequest, gesturesRequest, beepRequest, balanceRequest,
         smartSwitchRequest, voicePromptRequest, tipsRequest, serialRequest, personiFiRequest, standbyRequest,
         keyRequest(touchGuard), featureRequest(features)]
    }

    /// [ANC, Ambient, TalkThru] flags — ids 1, 2 and 3 in packet 0x91.
    static let modes: [(NoiseMode, [UInt8])] = [(.anc, [1, 0, 0]), (.ambient, [0, 1, 0]), (.talkThru, [0, 0, 1]), (.off, [0, 0, 0])]

    static func encode(_ c: Command) -> [UInt8] {
        switch c {
        case .mode(.off): return [0xAA, 0x91, 0x01, 0x13]
        case .mode(let m):
            let f = modes.first { $0.0 == m }?.1 ?? [0, 0, 0]
            return [0xAA, 0x91, 0x07, 0x10, 1, f[0], 2, f[1], 3, f[2]]
        case .adaptive(let on): return [0xAA, 0x91, 0x03, 0x20, 0x01, on ? 1 : 0]
        case .ancLevel(let n): return [0xAA, 0x91, 0x03, 0x20, 0x04, n]
        case .ambientLevel(let n): return [0xAA, 0x91, 0x03, 0x20, 0xA1, n]
        case .eq(let bands): return encodeEq(bands)
        case .gestures(let v): return [0xAA, 0x77, UInt8(1 + 2 * v.count), 0] + v.flatMap { [$0.gesture, $0.action] }
        case .beep(let left, let right):
            let side = { (s: Bool?) -> UInt8 in s.map { $0 ? 1 : 0 } ?? 0xFF }
            return [0xAA, 0x23, 0x03, 0x00, side(left), side(right)]
        case .write(let reg, let v): return encodeWrite(reg, v)
        case .powerOff: return [0xAA, 0x97, 0x00]
        case .factoryReset: return [0xAA, 0x95, 0x00]
        }
    }

    /// Empty when there is nothing to send (read-only register, value too short).
    static func encodeWrite(_ reg: Register, _ v: [UInt8]) -> [UInt8] {
        switch (reg, v.first) {
        case (.feature(let id), _):
            return [0xAA, 0x06] + le16(4 + v.count) + [0x00, UInt8(id >> 8), UInt8(id & 0xFF), UInt8(v.count)] + v
        case (.anc(let kind), let x?): return [0xAA, 0x91, 0x03, 0x20, kind, x]
        case (.balance, let on?) where v.count >= 2: return [0xAA, 0xA8, 0x05, 0x00, 0x01, on, 0x02, v[1]]
        case (.smartSwitch, _) where v.count == 8: return [0xAA, 0x80, 0x09, 0x00] + v
        case (.voicePrompt, let x?): return [0xAA, 0x93, 0x02, 0x03, x]
        case (.tips, let x?): return [0xAA, 0x9C, 0x02, 0x00, x]
        case (.personiFi, let x?): return [0xAA, 0xA1, 0x03, 0x00, 0x01, x]
        // standby on = normal mode on + keep-ANC mode off, and the other way round
        case (.standby, let x?): return [0xAA, 0x96, 0x05, 0x00, 0x01, x, 0x02, x == 0 ? 1 : 0]
        case (.key(let key), let x?): return [0xAA, 0xB1, 0x04, 0x01, key, 0x01, x]
        default: return []
        }
    }

    /// The device only acks these: ask again so the screen shows what stuck.
    static func followUp(_ c: Command) -> [UInt8]? {
        switch c {
        case .gestures: return gesturesRequest
        case .beep: return beepRequest
        case .write(.feature(let id), _): return featureRequest([id])
        case .write(.balance, _): return balanceRequest
        case .write(.smartSwitch, _): return smartSwitchRequest
        case .write(.voicePrompt, _): return voicePromptRequest
        case .write(.tips, _): return tipsRequest
        case .write(.personiFi, _): return personiFiRequest
        case .write(.standby, _): return standbyRequest
        case .write(.key(let key), _): return keyRequest(key)
        default: return nil
        }
    }

    /// EQ "Q×10" (sub 3) in category 230 (custom) or 0 (off).
    static func encodeEq(_ bands: [(freq: Float, gain: Float)]?) -> [UInt8] {
        var p: [UInt8] = [3, bands == nil ? 0 : 230]
        // ponytail: calibration 0, what the app sends when libcalibrationLib does not load (see jbl.rs)
        p += f32(0)
        p.append(bands == nil ? 0 : 48) // kHz
        p += f32(0) + f32(0) // left, right gain
        if let bands {
            p.append(UInt8(bands.count))
            for b in bands { p += [1] + f32(b.gain) + f32(b.freq) + [10] } // peaking, Q 1.0 ×10
        } else {
            p.append(0)
        }
        return [0xAA, 0xA2] + le16(p.count) + p
    }

    /// Splits a notification into `AA …` packets (sometimes several come together).
    static func split(_ data: [UInt8]) -> [[UInt8]] {
        var v: [[UInt8]] = [], d = data[...]
        while d.count >= 3, d.first == 0xAA {
            let b = Array(d)
            let (start, n): (Int, Int)
            switch b[1] {
            case 0xA2, 0x06:
                guard b.count >= 4 else { return v }
                (start, n) = (4, Int(b[2]) | Int(b[3]) << 8)
            default: (start, n) = (3, Int(b[2]))
            }
            let end = min(start + n, b.count)
            v.append(Array(b[..<end]))
            d = d.dropFirst(end)
        }
        return v
    }

    /// Updates `s` with what the device sent; tells whether anything changed.
    @discardableResult
    static func apply(_ s: inout DeviceState, _ data: [UInt8]) -> Bool {
        let before = s
        for p in split(data) {
            let pairs = stride(from: 4, to: p.count - 1, by: 2).map { (p[$0], p[$0 + 1]) }
            switch (p[1], p[at: 3]) {
            case (0x91, 0x12?):
                var flags: [UInt8] = [0, 0, 0]
                for (k, v) in pairs where (1...3).contains(k) { flags[Int(k) - 1] = v }
                s.mode = modes.first { $0.1 == flags }?.0
            case (0x91, 0x22?):
                for (k, v) in pairs {
                    s.registers[.anc(k)] = [v]
                    switch k {
                    case 0x01: s.adaptive = v != 0
                    case 0x04: s.ancLevel = v
                    case 0xA1: s.ambientLevel = v
                    default: break
                    }
                }
            // charging L, charging R, level L, level R, charging case, level case
            case (0x25, 0x01?) where p.count >= 10:
                s.battery = Battery(left: Int(p[6]), right: Int(p[7]), caseLevel: Int(p[9]),
                                    leftCharging: p[4] != 0, rightCharging: p[5] != 0, caseCharging: p[8] != 0)
            case (0x06, _) where p[at: 4] == 0x02: applyFeatures(&s, p.from(5))
            case (0xA8, 0x02?) where p.count >= 8: s.registers[.balance] = [p[5], p[7]]
            case (0x80, 0x02?) where p.count >= 12: s.registers[.smartSwitch] = Array(p[4..<12])
            case (0x93, 0x05?) where p.count >= 5: s.registers[.voicePrompt] = [p[4]]
            case (0x9C, 0x02?) where p.count >= 5: s.registers[.tips] = [p[4]]
            case (0x94, 0x02?): s.registers[.serial] = p.from(4)
            case (0xA1, 0x02?) where p.count >= 6 && p[4] == 0x01: s.registers[.personiFi] = [p[5]]
            case (0x96, _?):
                if let m = pairs.first(where: { $0.0 == 1 }) { s.registers[.standby] = [m.1] }
            // (key, len, value)…
            case (0xB1, _?):
                var i = 4
                while i + 2 <= p.count, i + 2 + Int(p[i + 1]) <= p.count {
                    let n = Int(p[i + 1])
                    if n > 0 { s.registers[.key(p[i])] = [p[i + 2]] }
                    i += 2 + n
                }
            case (0x77, 0x02?): s.gestures = pairs.map { Touch(gesture: $0.0, action: $0.1) }
            case (0x23, 0x02?) where p.count >= 6: s.beep = [p[4] == 1, p[5] == 1]
            case (0x12, _):
                let name = p.from(3).prefix { $0 != 0 }
                s.name = String(decoding: name, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                let r = 3 + name.count + 1
                if r + 3 <= p.count { (s.productId, s.color) = (UInt16(p[r]) | UInt16(p[r + 1]) << 8, p[r + 2]) }
            case (0xA2, _?) where p.count >= 20:
                let f = { (o: Int) in Float(bitPattern: UInt32(p[o]) | UInt32(p[o + 1]) << 8 | UInt32(p[o + 2]) << 16 | UInt32(p[o + 3]) << 24) }
                let n = Int(p[19])
                switch p[5] {
                case 201, 202: continue // Personi-Fi / Design EQ (factory tuning): not the user's
                case 0: s.eq = .some(nil)
                case _ where n > 0 && p.count >= 20 + n * 10: s.eq = .some((0..<n).map { f(20 + $0 * 10 + 1) })
                default: continue
                }
            default: break
            }
        }
        return s != before
    }

    /// (id u16 big-endian, len, value)… — every one kept raw; the batteries also go to `battery`.
    static func applyFeatures(_ s: inout DeviceState, _ d: [UInt8]) {
        var i = 0
        while i + 3 <= d.count {
            let (id, n) = (UInt16(d[i]) << 8 | UInt16(d[i + 1]), Int(d[i + 2]))
            guard i + 3 + n <= d.count else { break }
            let v = Array(d[(i + 3)..<(i + 3 + n)])
            if (0x1A...0x1C).contains(id), v.count >= 2 {
                var b = s.battery ?? Battery()
                switch id {
                case 0x1A: (b.leftCharging, b.left) = (v[0] != 0, Int(v[1]))
                case 0x1B: (b.rightCharging, b.right) = (v[0] != 0, Int(v[1]))
                default: (b.caseCharging, b.caseLevel) = (v[0] != 0, Int(v[1]))
                }
                s.battery = b
            }
            s.registers[.feature(id)] = v
            i += 3 + n
        }
    }
}

// MARK: - Self-test (`--selftest`): the Rust tests of jbl.rs, on the same bytes

func check(_ ok: Bool, _ what: String, line: Int = #line) {
    if !ok {
        print("FAIL line \(line): \(what)")
        exit(1)
    }
}

func hex(_ s: String) -> [UInt8] {
    let s = s.filter { !$0.isWhitespace }
    return stride(from: 0, to: s.count, by: 2).map { i in
        UInt8(s[s.index(s.startIndex, offsetBy: i)..<s.index(s.startIndex, offsetBy: i + 2)], radix: 16)!
    }
}

extension JBL {
    static let bands: [Float] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
    static func eqOf(_ gains: [Float]) -> [(freq: Float, gain: Float)] { zip(bands, gains).map { ($0, $1) } }

    static func selfTest() {
        // bytes the device accepted in the Python tests
        check(encode(.mode(.anc)) == hex("aa 91 07 10 01 01 02 00 03 00"), "mode anc")
        check(encode(.mode(.off)) == hex("aa 91 01 13"), "mode off")
        check(encode(.adaptive(false)) == hex("aa 91 03 20 01 00"), "adaptive")
        check(encode(.ancLevel(7)) == hex("aa 91 03 20 04 07"), "anc level")
        check(encode(.ambientLevel(3)) == hex("aa 91 03 20 a1 03"), "ambient level")
        check(encode(.eq(nil)) == hex("aa a2 10 00 03 00 00000000 00 00000000 00000000 00"), "eq off")
        var g = [Float](repeating: 0, count: 10)
        (g[1], g[2]) = (4, 4)
        let p = encode(.eq(eqOf(g)))
        check(p.count == 120, "eq length")
        check(Array(p[..<20]) == hex("aa a2 74 00 03 e6 00000000 30 00000000 00000000 0a"), "eq head")
        check(Array(p[30..<40]) == hex("01 00008040 00008042 0a"), "64 Hz band, +4 dB, Q 1.0")
        check(encode(.gestures([Touch(gesture: 6, action: 0x0b), Touch(gesture: 9, action: 8)])) == hex("aa 77 05 00 06 0b 09 08"), "gestures")
        check(encode(.beep(left: true, right: nil)) == hex("aa 23 03 00 01 ff"), "beep")

        // real notifications captured from the device
        var s = DeviceState()
        check(apply(&s, hex("aa 91 07 12 01 00 02 01 03 00 aa 91 09 22 01 01 04 06 05 01 a1 06")), "modes changed")
        check(s.mode == .ambient && s.adaptive == true && s.ancLevel == 6 && s.ambientLevel == 6, "modes")
        apply(&s, hex("aa 25 0d 01 00 00 64 64 00 28 f8 10 64 00 28 00"))
        check(s.battery == Battery(left: 100, right: 100, caseLevel: 40), "battery")
        apply(&s, hex("aa 12 25 4a 42 4c 20 54 75 6e 65 20 46 6c 65 78 20 32 00 0e 21 00 64 ac bc e4 d5 84 b4 03 05 00"))
        check(s.name == "JBL Tune Flex 2" && s.productId == 0x210e && s.color == 0, "info")
        apply(&s, hex("aa 77 11 02 06 0b 07 04 08 00 0c a1 09 08 0a 05 0b 06 0e a1"))
        check(s.gestures?.count == 8 && s.gestures?.first == Touch(gesture: 6, action: 0x0b) && s.gestures?.last == Touch(gesture: 14, action: 0xa1), "gestures read")
        apply(&s, hex("aa 23 03 02 00 01 aa 00 02 23 00"))
        check(s.beep == [false, true], "beep read")
        // echo of the EQ turned off (with firmware debug garbage at the end)
        var off = hex("aa a2 74 00 04 00 00000000 00 00000000 00000000 00 61 72 61 6d")
        off += [UInt8](repeating: 0, count: 120 - off.count)
        apply(&s, off)
        check(s.eq == .some(nil), "eq off read")
        // the Design EQ (cat 202) does not touch the user's EQ
        var design = hex("aa a2 74 00 04 ca 00000000 00 0000a0c0 0000a0c0 0a")
        design += [UInt8](repeating: 0, count: 120 - design.count)
        check(!apply(&s, design), "design eq ignored")
        var g1 = [Float](repeating: 0, count: 10)
        g1[1] = 4
        var echo = encodeEq(eqOf(g1))
        echo[4] = 4
        apply(&s, echo)
        check(s.eq == .some(g1), "custom eq read")
        // short and garbled packets never crash
        for junk in ["aa", "aa 25 05 01 00", "aa 06 05", "aa a2 30 00 04", "aa b1 09 02 04 09", "aa 12 02 4a", "aa 77 03 02 06"] {
            apply(&s, hex(junk))
        }

        // the settings tab: device info features and the commands of their own (log of Nov 8)
        s = DeviceState()
        let info = "aa 06 3d 00 02 00 05 03 03 05 00 00 06 01 07 00 0b 03 00 1e 00 00 0c 02 00 01 00 0d 02 00 02 " +
            "00 14 02 00 00 00 16 03 00 00 00 00 1a 02 00 64 00 1b 02 00 64 00 1c 02 00 1f 00 20 01 00 00 23 01 01"
        check(apply(&s, hex(info)), "features changed")
        check(s.registers[.feature(0x05)] == [3, 5, 0], "firmware")
        check(s.registers[.feature(0x0B)] == [0, 0x1e, 0], "auto off")
        check(s.registers[.feature(0x23)] == [1], "limiter")
        check(s.battery?.caseLevel == 0x1f, "case from features")
        apply(&s, hex("aa 06 10 00 02 00 1a 02 01 64 00 1b 02 00 64 00 1c 02 00 3e"))
        check(s.battery?.leftCharging == true && s.battery?.rightCharging == false && s.battery?.caseLevel == 0x3e, "charging")
        apply(&s, hex("aa 80 09 02 00 01 2e 00 18 01 ff ff aa 9c 02 02 00"))
        check(s.registers[.smartSwitch] == hex("00 01 2e 00 18 01 ff ff") && s.registers[.tips] == [0], "smart switch, tips")
        apply(&s, hex("aa 94 11 02 4d 4d 30 32 34 35 2d 44 50 30 32 34 36 39 36 31"))
        check(s.registers[.serial] == Array("MM0245-DP0246961".utf8), "serial")
        apply(&s, hex("aa a8 05 02 01 00 02 64 aa 93 02 05 01"))
        check(s.registers[.balance] == [0, 0x64] && s.registers[.voicePrompt] == [1], "balance, voice prompt")

        let w = { (r: Register, v: [UInt8]) in encode(.write(r, v)) }
        check(w(.feature(0x20), [1]) == hex("aa 06 05 00 00 00 20 01 01"), "write feature")
        check(w(.feature(0x0B), [1, 0x1e, 0]) == hex("aa 06 07 00 00 00 0b 03 01 1e 00"), "write auto off")
        check(w(.anc(0x08), [1]) == hex("aa 91 03 20 08 01"), "write anc")
        check(w(.balance, [1, 40]) == hex("aa a8 05 00 01 01 02 28"), "write balance")
        check(w(.smartSwitch, hex("c5 00 2e 00 78 00 ff ff")) == hex("aa 80 09 00 c5 00 2e 00 78 00 ff ff"), "write smart switch")
        check(w(.voicePrompt, [0]) == hex("aa 93 02 03 00"), "write voice prompt")
        check(w(.serial, Array("x".utf8)).isEmpty, "serial is read only")
        check(w(.personiFi, [1]) == hex("aa a1 03 00 01 01"), "write personi-fi")
        check(w(.standby, [0]) == hex("aa 96 05 00 01 00 02 01"), "write standby")
        check(w(.key(4), [1]) == hex("aa b1 04 01 04 01 01"), "write key")
        apply(&s, hex("aa 96 05 02 01 01 02 00 aa b1 04 02 04 01 01 aa a1 03 02 01 00"))
        check(s.registers[.standby] == [1] && s.registers[.key(4)] == [1] && s.registers[.personiFi] == [0], "standby, key, personi-fi")
        check(followUp(.write(.feature(0x20), [1])) == hex("aa 06 03 00 01 00 20"), "follow up")
        check(Array(initialRequests().last!.prefix(7)) == hex("aa 06 1d 00 01 00 05"), "feature request")
    }
}
