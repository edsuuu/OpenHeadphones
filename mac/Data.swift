// The data the Rust app reads too: `devices/<model>/profile.toml` (per-model, see src/profile.rs)
// and `locales/<language>.toml` (see src/i18n.rs), found from the program's folder upward.

import Foundation

// MARK: - TOML, the part those files use

/// A TOML value. Numbers are all Double (the profiles mix 5 and 0.5 in one list).
indirect enum Toml {
    case string(String), number(Double), bool(Bool), array([Toml]), table([String: Toml])

    subscript(key: String) -> Toml? {
        if case .table(let t) = self { return t[key] }
        return nil
    }
    var string: String? { if case .string(let s) = self { return s }; return nil }
    var number: Double? { if case .number(let n) = self { return n }; return nil }
    var int: Int? { number.map { Int($0) } }
    var bool: Bool? { if case .bool(let b) = self { return b }; return nil }
    var array: [Toml] { if case .array(let a) = self { return a }; return [] }
    var table: [String: Toml] { if case .table(let t) = self { return t }; return [:] }
}

struct TomlError: Error, CustomStringConvertible {
    let description: String
}

/// `[table]` headers, `key = value` (bare or quoted keys), strings, numbers (0x… too), booleans,
/// arrays and inline tables, nested and over several lines. Not: `[[tables]]`, dotted keys,
/// multi-line strings, dates.
struct TomlReader {
    private let b: [UInt8]
    private var i = 0

    static func parse(_ text: String) throws -> Toml {
        var r = TomlReader(b: Array(text.utf8))
        return try r.document()
    }

    private init(b: [UInt8]) { self.b = b }

    private var c: UInt8? { i < b.count ? b[i] : nil }

    private func fail(_ what: String) -> TomlError {
        TomlError(description: "line \(b[..<min(i, b.count)].filter { $0 == 0x0A }.count + 1): \(what)")
    }

    /// Spaces and comments; newlines too when `lines`.
    private mutating func skip(lines: Bool) {
        while let ch = c {
            switch ch {
            case 0x20, 0x09, 0x0D: i += 1
            case 0x0A where lines: i += 1
            case UInt8(ascii: "#"): while let ch = c, ch != 0x0A { i += 1 }
            default: return
            }
        }
    }

    private mutating func expect(_ ch: Character) throws {
        guard c == ch.asciiValue else { throw fail("expected \(ch)") }
        i += 1
    }

    private mutating func document() throws -> Toml {
        var root: [String: Toml] = [:], section: String?, current: [String: Toml] = [:]
        func flush() {
            if let section { root[section] = .table(current) } else { root.merge(current) { $1 } }
        }
        while true {
            skip(lines: true)
            guard let ch = c else { break }
            if ch == UInt8(ascii: "[") {
                flush()
                i += 1
                section = try key()
                try expect("]")
                current = root[section!]?.table ?? [:]
                continue
            }
            let k = try key()
            skip(lines: false)
            try expect("=")
            skip(lines: false)
            current[k] = try value()
            skip(lines: false)
            if let ch = c, ch != 0x0A { throw fail("expected end of line") }
        }
        flush()
        return .table(root)
    }

    private mutating func key() throws -> String {
        if c == UInt8(ascii: "\"") { return try string() }
        let start = i
        while let ch = c, ch == UInt8(ascii: "_") || ch == UInt8(ascii: "-") || (ch < 0x80 && Character(Unicode.Scalar(ch)).isLetter) || (0x30...0x39).contains(ch) {
            i += 1
        }
        guard i > start else { throw fail("expected a key") }
        return String(decoding: b[start..<i], as: UTF8.self)
    }

    private mutating func value() throws -> Toml {
        switch c {
        case UInt8(ascii: "\""): return .string(try string())
        case UInt8(ascii: "["):
            i += 1
            var a: [Toml] = []
            while true {
                skip(lines: true)
                if c == UInt8(ascii: "]") { i += 1; return .array(a) }
                a.append(try value())
                skip(lines: true)
                if c == UInt8(ascii: ",") { i += 1 } else if c != UInt8(ascii: "]") { throw fail("expected , or ]") }
            }
        case UInt8(ascii: "{"):
            i += 1
            var t: [String: Toml] = [:]
            while true {
                skip(lines: true)
                if c == UInt8(ascii: "}") { i += 1; return .table(t) }
                let k = try key()
                skip(lines: false)
                try expect("=")
                skip(lines: false)
                t[k] = try value()
                skip(lines: true)
                if c == UInt8(ascii: ",") { i += 1 } else if c != UInt8(ascii: "}") { throw fail("expected , or }") }
            }
        default:
            let start = i
            while let ch = c, !" \t\r\n,]}#".utf8.contains(ch) { i += 1 }
            let word = String(decoding: b[start..<i], as: UTF8.self).replacingOccurrences(of: "_", with: "")
            if word == "true" || word == "false" { return .bool(word == "true") }
            let neg = word.hasPrefix("-"), body = neg || word.hasPrefix("+") ? String(word.dropFirst()) : word
            if body.hasPrefix("0x"), let n = Int(body.dropFirst(2), radix: 16) { return .number(Double(neg ? -n : n)) }
            if let n = Double(word) { return .number(n) }
            throw fail("bad value \(word)")
        }
    }

    private mutating func string() throws -> String {
        i += 1
        var out: [UInt8] = []
        while let ch = c, ch != UInt8(ascii: "\"") {
            guard ch != 0x0A else { throw fail("unterminated string") }
            i += 1
            guard ch == UInt8(ascii: "\\"), let e = c else { out.append(ch); continue }
            i += 1
            switch e {
            case UInt8(ascii: "n"): out.append(0x0A)
            case UInt8(ascii: "t"): out.append(0x09)
            default: out.append(e) // \" and \\
            }
        }
        try expect("\"")
        return String(decoding: out, as: UTF8.self)
    }
}

// MARK: - Data folder

/// The first folder with `devices/`, from the program's folder upward (the repo root in a dev build).
let dataRoot: URL? = {
    var dir = (Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])).resolvingSymlinksInPath()
    while dir.path != "/" {
        dir.deleteLastPathComponent()
        var isDir: ObjCBool = false
        if FileManager.default.fileExists(atPath: dir.appendingPathComponent("devices").path, isDirectory: &isDir), isDir.boolValue {
            return dir
        }
    }
    return nil
}()

// MARK: - Texts

/// Flat keys (`[list] title` → `list.title`): English, then the system language over it.
var texts: [String: String] = [:]

/// One locale file, flattened.
func localeFile(_ name: String) -> [String: String] {
    guard let url = dataRoot?.appendingPathComponent("locales/\(name).toml"),
          let text = try? String(contentsOf: url, encoding: .utf8) else { return [:] }
    do {
        var out: [String: String] = [:]
        for (section, t) in try TomlReader.parse(text).table {
            for (k, v) in t.table { out["\(section).\(k)"] = v.string }
        }
        return out
    } catch {
        log("\(url.path): \(error)")
        return [:]
    }
}

func loadTexts(language: String? = nil) {
    guard let dir = dataRoot?.appendingPathComponent("locales") else { return }
    let files = ((try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? [])
        .filter { $0.hasSuffix(".toml") }.map { String($0.dropLast(5)) }
    // pt-BR → pt-BR.toml; otherwise any file of the same language (pt.toml, pt-PT.toml)
    let wanted = language ?? Locale.preferredLanguages.first ?? "en"
    let lang = { (s: String) in s.split(separator: "-").first.map { $0.lowercased() } ?? "" }
    let pick = files.first { $0.caseInsensitiveCompare(wanted) == .orderedSame } ?? files.first { lang($0) == lang(wanted) }
    texts = localeFile("en").merging(pick.map(localeFile) ?? [:]) { $1 }
}

func t(_ key: String) -> String { texts[key] ?? key }
func maybe(_ key: String) -> String? { texts[key] }
/// `t` with its `{name}` placeholders filled in.
func tf(_ key: String, _ args: [String: Any]) -> String {
    args.reduce(t(key)) { $0.replacingOccurrences(of: "{\($1.key)}", with: "\($1.value)") }
}

// MARK: - Profiles

enum Side: String {
    case left, right
    var key: String { "side.\(rawValue)" }
}

struct Profile {
    struct Eq {
        var maxDb: Double
        var bands: [Double]
        /// Label key `eq.<name>`, the category the device gets (nil: sent as the user's own, 230).
        var presets: [(name: String, category: UInt8?, gains: [Double])]
    }
    struct Gesture { var id: UInt8, side: Side, kind: String }
    struct Action { var id: UInt8, name: String }

    var name = ""
    /// Model ids the device reports about itself.
    var productIds: [UInt16] = []
    /// Bluetooth name prefixes: used to find the device, and to match it before it reports its id.
    var namePrefixes: [String] = []
    var noiseModes: [NoiseMode] = []
    var adaptiveAnc = false
    /// Manual ANC levels 1…n, ambient levels 1…n (0 = none).
    var ancLevels: UInt8 = 0, ambientLevels: UInt8 = 0
    var findBuds = false
    var imageDefault: String?
    /// Color id the device reports → photo file.
    var imageColors: [String: String] = [:]
    var eq: Eq?
    var gestures: [Gesture] = [], actions: [Action] = []
    /// The settings tab, in order.
    var settings: [Setting] = []
    /// Folder the profile came from; photos are relative to it.
    var dir = URL(fileURLWithPath: "/")

    /// Photo for the color the device reports, or the default one.
    func image(color: UInt8?) -> URL? {
        (color.flatMap { imageColors[String($0)] } ?? imageDefault).map { dir.appendingPathComponent($0) }
    }

    init() {}

    init(_ t: Toml, dir: URL) throws {
        self.dir = dir
        name = t["name"]?.string ?? ""
        productIds = t["product_ids"]?.array.compactMap { $0.int.map(UInt16.init) } ?? []
        namePrefixes = t["name_prefixes"]?.array.compactMap(\.string) ?? []
        noiseModes = try (t["noise_modes"]?.array ?? []).map { v in
            guard let m = v.string.flatMap(NoiseMode.init) else { throw TomlError(description: "noise mode \(v)") }
            return m
        }
        adaptiveAnc = t["adaptive_anc"]?.bool ?? false
        ancLevels = UInt8(t["anc_levels"]?.int ?? 0)
        ambientLevels = UInt8(t["ambient_levels"]?.int ?? 0)
        findBuds = t["find_buds"]?.bool ?? false
        imageDefault = t["images"]?["default"]?.string
        imageColors = t["images"]?["colors"]?.table.compactMapValues(\.string) ?? [:]
        if let e = t["eq"] {
            eq = Eq(maxDb: e["max_db"]?.number ?? 0, bands: e["bands"]?.array.compactMap(\.number) ?? [],
                    presets: e["presets"]?.array.map { ($0["name"]?.string ?? "", $0["category"]?.int.map(UInt8.init), $0["gains"]?.array.compactMap(\.number) ?? []) } ?? [])
        }
        gestures = try (t["gestures"]?.array ?? []).map { g in
            guard let id = g["id"]?.int, let side = g["side"]?.string.flatMap(Side.init), let kind = g["kind"]?.string else {
                throw TomlError(description: "gesture \(g)")
            }
            return Gesture(id: UInt8(id), side: side, kind: kind)
        }
        actions = (t["actions"]?.array ?? []).compactMap { a in
            guard let id = a["id"]?.int, let name = a["name"]?.string else { return nil }
            return Action(id: UInt8(id), name: name)
        }
        settings = try (t["settings"]?.array ?? []).map(Setting.init)
    }

    static func loadAll() -> [Profile] {
        guard let root = dataRoot?.appendingPathComponent("devices") else {
            log(t("error.no_data"))
            return []
        }
        let dirs = ((try? FileManager.default.contentsOfDirectory(atPath: root.path)) ?? []).sorted()
        return dirs.compactMap { name in
            let dir = root.appendingPathComponent(name), path = dir.appendingPathComponent("profile.toml")
            guard let text = try? String(contentsOf: path, encoding: .utf8) else { return nil }
            do {
                return try Profile(TomlReader.parse(text), dir: dir)
            } catch {
                log(tf("error.bad_profile", ["path": path.path, "error": error]))
                return nil
            }
        }
    }

    /// By the model id; before the device reports it, by the longest matching name prefix.
    static func find(_ all: [Profile], productId: UInt16?, name: String) -> Profile? {
        if let id = productId, let p = all.first(where: { $0.productIds.contains(id) }) { return p }
        return all.compactMap { p in p.namePrefixes.filter { name.hasPrefix($0) }.map(\.count).max().map { ($0, p) } }
            .max { $0.0 < $1.0 }?.1
    }
}

/// One row of the settings tab. Labels: `setting.<name>` and, if present, `setting.<name>_hint`.
struct Setting {
    enum Kind: String {
        /// On/off: byte `byte` is `on` or `off`.
        case `switch`
        /// One of `options`: `values` (each `width` bytes, little-endian) from byte `byte` on.
        case choice
        /// `min`…`max` in byte `byte`.
        case level
        /// Read only: the register shown as `format`.
        case info
        /// Sends `command` after a confirmation.
        case button
    }
    enum Format: String { case version, text }
    enum ButtonCommand: String { case powerOff = "power_off", factoryReset = "factory_reset" }

    var name: String
    /// Section title key `settings.<section>`; a new title where it changes.
    var section: String
    var kind: Kind
    var register: Register?
    var byte = 0
    var on: UInt8 = 1, off: UInt8 = 0
    /// Label key `option.<name>`, and the values.
    var options: [(name: String, values: [UInt32])] = []
    var width = 1
    var min: UInt8 = 0, max: UInt8 = 0
    /// Level: the neutral value (balance), shown as left/right of it.
    var center: UInt8?
    /// [byte, value] pairs written along with every change (an "on" flag the JBL app sets too).
    var set: [(byte: Int, value: UInt8)] = []
    var format: Format?
    var command: ButtonCommand?
    /// Name of a switch setting: this row only counts while that one is on.
    var when: String?

    init(_ t: Toml) throws {
        guard let name = t["name"]?.string, let section = t["section"]?.string,
              let kind = t["kind"]?.string.flatMap(Kind.init) else { throw TomlError(description: "setting \(t)") }
        (self.name, self.section, self.kind) = (name, section, kind)
        if let r = t["register"] {
            let named: [String: Register] = ["balance": .balance, "smart_switch": .smartSwitch, "voice_prompt": .voicePrompt,
                                             "tips": .tips, "personi_fi": .personiFi, "standby": .standby, "serial": .serial]
            if let s = r.string { register = named[s] }
            if let id = r["feature"]?.int { register = .feature(UInt16(id)) }
            if let n = r["anc"]?.int { register = .anc(UInt8(n)) }
            if let n = r["key"]?.int { register = .key(UInt8(n)) }
            guard register != nil else { throw TomlError(description: "setting \(name): register \(r)") }
        }
        byte = t["byte"]?.int ?? 0
        on = UInt8(t["on"]?.int ?? 1)
        off = UInt8(t["off"]?.int ?? 0)
        options = t["options"]?.array.map { ($0["name"]?.string ?? "", $0["values"]?.array.compactMap { $0.int.map(UInt32.init) } ?? []) } ?? []
        width = t["width"]?.int ?? 1
        min = UInt8(t["min"]?.int ?? 0)
        max = UInt8(t["max"]?.int ?? 0)
        center = t["center"]?.int.map(UInt8.init)
        set = t["set"]?.array.compactMap { p in
            let v = p.array.compactMap(\.int)
            return v.count == 2 ? (v[0], UInt8(v[1])) : nil
        } ?? []
        format = t["format"]?.string.flatMap(Format.init)
        command = t["command"]?.string.flatMap(ButtonCommand.init)
        when = t["when"]?.string
    }

    /// The option's bytes, as they go in the register.
    func optionBytes(_ k: Int) -> [UInt8] {
        options[k].values.flatMap { v in withUnsafeBytes(of: v.littleEndian) { Array($0.prefix(width)) } }
    }

    /// Which option the register holds now.
    func currentOption(_ value: [UInt8]) -> Int? {
        options.indices.first { k in
            let b = optionBytes(k)
            return value.count >= byte + b.count && Array(value[byte..<(byte + b.count)]) == b
        }
    }
}

// MARK: - Self-test: the Rust tests of profile.rs and i18n.rs

func dataSelfTest() {
    let all = Profile.loadAll()
    let dirs = (try? FileManager.default.contentsOfDirectory(atPath: dataRoot!.appendingPathComponent("devices").path))?.count
    check(all.count == dirs, "a profile failed to load (see the error above)")
    let flex = Profile.find(all, productId: 0x210e, name: "")
    check(flex?.name == "JBL Tune Flex 2" && flex?.gestures.count == 8 && flex?.actions.count == 10, "flex profile")
    check(flex?.image(color: 14)?.lastPathComponent == "ghost-black.png", "photo by color")
    check(flex?.image(color: 99)?.lastPathComponent == "black.png", "default photo")
    check(Profile.find(all, productId: nil, name: "JBL Tune Flex 2")?.name == "JBL Tune Flex 2", "by name")
    check(Profile.find(all, productId: 0x1234, name: "JBL Live 770NC")?.name == "JBL", "generic")
    check(Profile.find(all, productId: nil, name: "WH-1000XM5") == nil, "none")
    for p in all {
        if let eq = p.eq { check(eq.presets.allSatisfy { $0.gains.count == eq.bands.count }, "\(p.name): preset size") }
        for s in p.settings {
            let ok = switch s.kind {
            case .choice: !s.options.isEmpty && s.register != nil
            case .level: s.min < s.max && (s.center.map { (s.min...s.max).contains($0) } ?? true)
            case .button: s.command != nil
            case .switch, .info: s.register != nil
            }
            let whenOk = s.when.map { w in p.settings.contains { $0.name == w && $0.kind == .switch } } ?? true
            check(ok && whenOk, "\(p.name): setting \(s.name)")
        }
    }
    let spatial = flex?.settings.first { $0.name == "spatial_mode" }
    check(spatial?.register == .feature(0x0C) && spatial?.byte == 1 && spatial?.set.first?.value == 1, "spatial mode row")
    let smart = flex?.settings.first { $0.name == "smart_audio" }
    check(smart?.optionBytes(0).count == 6 && smart?.currentOption(hex("c5 00 2e 00 18 01 ff ff")) != nil, "smart audio options")

    check(Set(localeFile("pt-BR").keys) == Set(localeFile("en").keys), "every language has every key")
    loadTexts(language: "pt-BR")
    check(t("list.title") == "Fones" && tf("list.many_connected", ["n": 3]) == "3 conectados", "pt-BR texts")
    loadTexts()
}
