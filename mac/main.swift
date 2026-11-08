// The macOS app: a menu bar panel for JBL headphones in AppKit + SwiftUI. The panel is drawn once
// and Core Animation moves it on the GPU; the genie is a Metal mesh (compiled at runtime: only the
// Command Line Tools are needed). Bluetooth: Bluetooth.swift; the bytes: Protocol.swift; per-model
// data and texts: Data.swift (the same devices/ and locales/ files as the Rust app).
// Run: ./run.sh [--open] [--debug] [--selftest] [--snapshot <dir>]

import AppKit
import MetalKit
import SwiftUI

// MARK: - Look

let panelWidth: CGFloat = 360
let detailHeight: CGFloat = 650
let windowHeight: CGFloat = 670

extension Color {
    init(hex: UInt32, alpha: Double = 1) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: alpha)
    }
}

let ink = Color(hex: 0xF2F3F7)
let ink2 = Color(hex: 0x9AA0AE)
let ink3 = Color(hex: 0x646B79)
let green = Color(hex: 0x42D99B)
let warning = Color(hex: 0xFFC247)
let accent = LinearGradient(colors: [Color(hex: 0xFF9244), Color(hex: 0xFF4D6D)], startPoint: .leading, endPoint: .trailing)
let springy = Animation.spring(response: 0.42, dampingFraction: 0.86)

func modeIcon(_ m: NoiseMode) -> String {
    switch m {
    case .anc: "shield.lefthalf.filled"
    case .ambient: "waveform"
    case .talkThru: "bubble.left"
    case .off: "power"
    }
}

enum Tab {
    case sound, gestures, find, settings
    var key: String { "tab.\(self)" }
    var icon: String {
        switch self {
        case .sound: "speaker.wave.2"
        case .gestures: "hand.tap"
        case .find: "location"
        case .settings: "gearshape"
        }
    }
}

/// Bottom sheet over the device page.
enum Sheet: Equatable {
    /// Picking the action of this gesture id.
    case gesture(UInt8)
    /// Confirming the beep on this side.
    case beep(Side)
    /// Picking an option of setting n, confirming the button of setting n.
    case options(Int), confirm(Int)
}

func displayName(_ s: DeviceState, _ p: Profile) -> String {
    !s.name.isEmpty ? s.name : !p.name.isEmpty ? p.name : t("device.unknown")
}

/// 32 → "32", 1000 → "1k", 16000 → "16k".
func bandLabel(_ f: Double) -> String { f >= 1000 ? String(format: "%gk", f / 1000) : String(format: "%g", f) }

// MARK: - Model

final class Model: ObservableObject {
    struct Device {
        let id: String
        var state: DeviceState
    }

    let profiles: [Profile]
    var bluetooth: Bluetooth?
    @Published var devices: [Device] = []
    @Published var current = 0
    @Published var detail = false
    @Published var tab = Tab.sound
    @Published var sheet: Sheet?
    /// The equalizer as on screen: the device's echo does not fight what the user is doing.
    @Published var gains: [Double] = []
    @Published var eqOn = false
    @Published var dragging: Int?
    /// The user's own curve, kept while trying presets.
    var custom: [Double] = []
    var eqSent = Date.distantPast
    /// A level slider being dragged (setting name, value): it sends only on release.
    @Published var slider: (setting: String, value: UInt8)?
    /// How far the tab's content is scrolled (drawn as is in the genie's still image).
    var scroll: CGFloat = 0
    private var photos: [URL: NSImage?] = [:]

    init(profiles: [Profile]) { self.profiles = profiles }

    var state: DeviceState? { devices.indices.contains(current) ? devices[current].state : nil }
    var connected: Bool { state?.connected == true }
    func profile(_ s: DeviceState?) -> Profile { s.flatMap { Profile.find(profiles, productId: $0.productId, name: $0.name) } ?? Profile() }
    var profile: Profile { profile(state) }

    /// The model's photo in the device's color (nil: the system's headphones symbol).
    func photo(_ s: DeviceState) -> NSImage? {
        guard let url = profile(s).image(color: s.color) else { return nil }
        if let cached = photos[url] { return cached }
        let image = NSImage(contentsOf: url)
        photos[url] = image
        return image
    }

    func update(_ id: String, _ s: DeviceState) {
        if let i = devices.firstIndex(where: { $0.id == id }) {
            devices[i].state = s
            if i == current { syncEq() }
        } else {
            devices.append(Device(id: id, state: s))
        }
    }

    private func edit(_ i: Int? = nil, _ f: (inout DeviceState) -> Void) {
        let i = i ?? current
        if devices.indices.contains(i) { f(&devices[i].state) }
    }

    private func send(_ c: Command, to i: Int? = nil) {
        let i = i ?? current
        if devices.indices.contains(i) { bluetooth?.send(devices[i].id, c) }
    }

    func open(_ i: Int) {
        (current, detail, tab, sheet, dragging) = (i, true, .sound, nil, nil)
        gains = []
        syncEq()
    }

    // the screen changes right away; the device confirms with a notification
    func setMode(_ m: NoiseMode, device i: Int? = nil) {
        edit(i) { $0.mode = m }
        send(.mode(m), to: i)
    }

    func toggleAdaptive() {
        let on = !(state?.adaptive ?? false)
        edit { $0.adaptive = on }
        send(.adaptive(on))
    }

    func setLevel(_ n: UInt8, ambient: Bool) {
        edit { if ambient { $0.ambientLevel = n } else { $0.ancLevel = n } }
        send(ambient ? .ambientLevel(n) : .ancLevel(n))
    }

    // MARK: equalizer

    func presetIndex() -> Int? {
        profile.eq?.presets.firstIndex { $0.gains.count == gains.count && zip($0.gains, gains).allSatisfy { abs($0 - $1) < 0.05 } }
    }

    /// Brings the device's EQ to the screen without fighting what the user is doing.
    func syncEq() {
        guard let eq = profile.eq else { return }
        if gains.count != eq.bands.count { (gains, custom) = (Array(repeating: 0, count: eq.bands.count), Array(repeating: 0, count: eq.bands.count)) }
        guard dragging == nil, -eqSent.timeIntervalSinceNow > 1.5, let e = state?.eq else { return }
        switch e {
        case nil: eqOn = false
        case let g? where g.count == eq.bands.count:
            eqOn = true
            gains = g.map { (Double($0) * 2).rounded() / 2 }
            if presetIndex() == nil { custom = gains }
        default: break
        }
    }

    func toggleEq() {
        eqOn.toggle()
        sendEq()
    }

    /// A preset, or the user's own curve (nil).
    func pick(preset: Int?) {
        guard let eq = profile.eq else { return }
        (gains, eqOn) = (preset.map { eq.presets[$0].gains } ?? custom, true)
        sendEq()
    }

    func sendEq() {
        eqSent = Date()
        guard let eq = profile.eq else { return }
        let g = eqOn ? gains.map(Float.init) : nil
        edit { $0.eq = .some(g) }
        let category = presetIndex().flatMap { eq.presets[$0].category } ?? 230
        send(.eq(g.map { (category, zip(eq.bands, $0).map { (Float($0), $1) }) }))
    }

    func drag(band: Int, gain: Double) {
        let first = dragging == nil
        dragging = band
        let changed = gains[band] != gain
        gains[band] = gain
        // at most every 120 ms while dragging; releasing sends the final value
        if first || (changed && -eqSent.timeIntervalSinceNow > 0.12) { sendEq() }
    }

    func endDrag() {
        dragging = nil
        if presetIndex() == nil { custom = gains }
        sendEq()
    }

    // MARK: gestures and find

    /// Changes one gesture and sends the whole list, as the JBL app does.
    func setGesture(_ g: UInt8, _ action: UInt8) {
        sheet = nil
        guard var list = state?.gestures else { return }
        if let k = list.firstIndex(where: { $0.gesture == g }) { list[k].action = action } else { list.append(Touch(gesture: g, action: action)) }
        edit { $0.gestures = list }
        send(.gestures(list))
    }

    func beeping(_ side: Side) -> Bool { state?.beep?[side == .left ? 0 : 1] ?? false }

    func beep(_ side: Side, _ on: Bool) {
        sheet = nil
        edit {
            var b = $0.beep ?? [false, false]
            b[side == .left ? 0 : 1] = on
            $0.beep = b
        }
        send(.beep(left: side == .left ? on : nil, right: side == .right ? on : nil))
    }

    // MARK: settings

    func value(_ s: Setting) -> [UInt8]? { s.register.flatMap { state?.registers[$0] } }
    func byte(_ s: Setting) -> UInt8? { value(s)?[at: s.byte] }

    /// What the device has: once it answered the feature request (the last one asked at connect),
    /// a row it never gave a value for is a feature it lacks.
    // ponytail: "answered" = a feature other than the batteries came (the earbuds also send those alone);
    // an answer slower than the feature request shows its row late, which is fine
    func shown(_ s: Setting) -> Bool {
        let answered = state?.registers.keys.contains { if case .feature(let id) = $0 { !(0x1A...0x1C).contains(id) } else { false } } ?? false
        return s.kind == .button || value(s) != nil || !answered
    }

    /// Connected, read, and its `when` switch is on (or it has none).
    func live(_ s: Setting) -> Bool {
        guard connected, s.kind == .button || value(s) != nil else { return false }
        guard let w = s.when, let sw = profile.settings.first(where: { $0.name == w }) else { return true }
        return byte(sw) == sw.on
    }

    func toggle(_ s: Setting) {
        let next = byte(s) == s.on ? s.off : s.on
        write(s) { $0[s.byte] = next }
    }

    func pick(_ s: Setting, option k: Int) {
        sheet = nil
        let b = s.optionBytes(k)
        write(s) { $0.replaceSubrange(s.byte..<(s.byte + b.count), with: b) }
    }

    func setLevel(_ s: Setting, _ n: UInt8) { write(s) { $0[s.byte] = n } }

    func slide(_ s: Setting, _ n: UInt8) { slider = (s.name, n) }

    /// Sends once, on release: every step of the drag would flood the link.
    func endSlide(_ s: Setting, _ n: UInt8) {
        slider = nil
        setLevel(s, n)
    }

    func confirm(_ s: Setting) {
        sheet = nil
        switch s.command {
        case .powerOff: send(.powerOff)
        case .factoryReset: send(.factoryReset)
        case nil: break
        }
    }

    /// Changes the register as last read (padded when shorter) and sends it whole; the screen
    /// shows the change right away, the device's answer confirms it.
    private func write(_ s: Setting, _ f: (inout [UInt8]) -> Void) {
        guard let reg = s.register, var v = state.map({ $0.registers[reg] ?? [] }) else { return }
        let width = s.options.map { $0.values.count * s.width }.max() ?? 1
        let need = s.set.map { $0.byte + 1 }.reduce(s.byte + width, Swift.max)
        if v.count < need { v += Array(repeating: 0, count: need - v.count) }
        f(&v)
        for (b, x) in s.set { v[b] = x }
        edit { $0.registers[reg] = v }
        send(.write(reg, v))
    }
}

// MARK: - Pieces

struct Glass: ViewModifier {
    var radius: CGFloat
    var on = false
    // what `@State` expands to: the Command Line Tools have no plugin for SwiftUI's macros
    private let hover = State(initialValue: false)

    func body(content: Content) -> some View {
        content
            .background {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(on ? AnyShapeStyle(accent) : AnyShapeStyle(Color.white.opacity(hover.wrappedValue ? 0.10 : 0.055)))
            }
            .overlay {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(Color.white.opacity(on ? 0.22 : 0.08), lineWidth: 1)
            }
            .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover.wrappedValue = h } }
    }
}

/// A row of a card that is a button: a soft pill under the pointer.
struct Hover: ViewModifier {
    private let hover = State(initialValue: false)

    func body(content: Content) -> some View {
        content
            .contentShape(Rectangle())
            .background {
                RoundedRectangle(cornerRadius: 11, style: .continuous).fill(Color.white.opacity(hover.wrappedValue ? 0.05 : 0)).padding(3)
            }
            .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover.wrappedValue = h } }
    }
}

extension View {
    func glass(_ radius: CGFloat, on: Bool = false) -> some View { modifier(Glass(radius: radius, on: on)) }
    /// Dimmed and not clickable when off.
    func live(_ on: Bool, dim: Double = 0.4) -> some View { opacity(on ? 1 : dim).disabled(!on) }
}

struct Photo: View {
    let image: NSImage?
    let size: CGFloat
    var body: some View {
        Group {
            if let image {
                Image(nsImage: image).resizable().interpolation(.high).scaledToFit()
            } else {
                Image(systemName: "headphones").resizable().scaledToFit().foregroundStyle(ink2).padding(size * 0.22)
            }
        }
        .frame(width: size, height: size)
    }
}

struct SectionTitle: View {
    let text: String
    var body: some View {
        Text(text.uppercased()).font(.system(size: 10.5, weight: .semibold)).tracking(0.7).foregroundStyle(ink3)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Battery level: green while charging, red when low.
struct Bar: View {
    let level: Int
    var charging = false
    var width: CGFloat? = 22
    var body: some View {
        Capsule().fill(Color.white.opacity(0.10))
            .overlay(alignment: .leading) {
                GeometryReader { g in
                    Capsule().fill(charging ? AnyShapeStyle(green) : level <= 20 ? AnyShapeStyle(Color.red) : AnyShapeStyle(accent))
                        .frame(width: g.size.width * CGFloat(min(level, 100)) / 100)
                }
            }
            .frame(width: width, height: 8)
    }
}

/// One control, one pill that slides between the segments.
struct Segmented<Label: View>: View {
    let count: Int
    let selected: Int?
    var height: CGFloat = 30
    var pill: AnyShapeStyle = AnyShapeStyle(accent)
    let pick: (Int) -> Void
    @ViewBuilder let label: (Int) -> Label
    @Namespace private var ns

    var body: some View {
        let big = height > 44
        HStack(spacing: 2) {
            ForEach(0..<count, id: \.self) { i in
                Button { withAnimation(springy) { pick(i) } } label: {
                    label(i)
                        .frame(maxWidth: .infinity)
                        .frame(height: height)
                        .foregroundStyle(selected == i ? Color.white : ink2)
                        .background {
                            if selected == i {
                                RoundedRectangle(cornerRadius: big ? 11 : 8, style: .continuous).fill(pill)
                                    .matchedGeometryEffect(id: "pill", in: ns)
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(3)
        .background(RoundedRectangle(cornerRadius: big ? 14 : 11, style: .continuous).fill(Color.black.opacity(0.28)))
    }
}

struct ModePicker: View {
    let modes: [NoiseMode]
    let selected: NoiseMode?
    var labels = false
    let pick: (NoiseMode) -> Void

    var body: some View {
        Segmented(count: modes.count, selected: selected.flatMap(modes.firstIndex), height: labels ? 52 : 30, pick: { pick(modes[$0]) }) { i in
            VStack(spacing: 5) {
                Image(systemName: modeIcon(modes[i])).font(.system(size: labels ? 16 : 13, weight: .medium))
                if labels { Text(t("noise.\(modes[i].rawValue)")).font(.system(size: 11, weight: .medium)) }
            }
            .help(t("noise.\(modes[i].rawValue)_long"))
        }
    }
}

struct Switch: View {
    let on: Bool
    let toggle: () -> Void
    var body: some View {
        Capsule().fill(on ? AnyShapeStyle(accent) : AnyShapeStyle(Color.white.opacity(0.12)))
            .frame(width: 36, height: 21)
            .overlay(alignment: on ? .trailing : .leading) { Circle().fill(.white).padding(2.5).shadow(radius: 1) }
            .contentShape(Capsule())
            .onTapGesture { withAnimation(springy) { toggle() } }
    }
}

/// A button per step, the bar growing with it (ANC intensity, short levels).
struct Steps: View {
    let range: ClosedRange<UInt8>
    let current: UInt8?
    let pick: (UInt8) -> Void
    var body: some View {
        HStack(spacing: 5) {
            ForEach(Array(range), id: \.self) { n in
                let on = current.map { $0 >= n } ?? false
                let bar = 3 + 12 * CGFloat(Int(n - range.lowerBound) + 1) / CGFloat(range.count)
                Button { withAnimation(springy) { pick(n) } } label: {
                    Color.clear.frame(width: 18, height: 26)
                        .glass(6, on: on)
                        .overlay(alignment: .bottom) {
                            RoundedRectangle(cornerRadius: 1.5).fill(Color.white.opacity(on ? 0.8 : 0.3)).frame(width: 6, height: bar).padding(.bottom, 5)
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }
}

struct Hairline: View {
    var body: some View { Divider().overlay(Color.white.opacity(0.06)).padding(.leading, 14) }
}

/// Rows in one glass card, with hairlines between them.
struct Card<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        VStack(spacing: 0) { content }.glass(14)
    }
}

/// Title and, if it has one, the hint under it.
struct TitleHint: View {
    let title: String
    var hint: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.system(size: 13)).foregroundStyle(ink)
            if let hint { Text(hint).font(.system(size: 10.5)).foregroundStyle(ink2) }
        }
    }
}

/// Wraps its children into rows, left to right (the preset chips).
struct Flow: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let f = frames(proposal.width ?? .infinity, subviews)
        return CGSize(width: proposal.width ?? f.map(\.maxX).max() ?? 0, height: f.map(\.maxY).max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (v, f) in zip(subviews, frames(bounds.width, subviews)) {
            v.place(at: CGPoint(x: bounds.minX + f.minX, y: bounds.minY + f.minY), proposal: ProposedViewSize(f.size))
        }
    }

    private func frames(_ width: CGFloat, _ subviews: Subviews) -> [CGRect] {
        var (x, y, row) = (CGFloat(0), CGFloat(0), CGFloat(0))
        return subviews.map { v in
            let s = v.sizeThatFits(.unspecified)
            if x > 0 && x + s.width > width { (x, y, row) = (0, y + row + spacing, 0) }
            defer { (x, row) = (x + s.width + spacing, max(row, s.height)) }
            return CGRect(origin: CGPoint(x: x, y: y), size: s)
        }
    }
}

/// Pulsing rings (searching, beeping).
struct Rings: View {
    let count: Int
    let from: CGFloat, to: CGFloat
    var speed = 0.9
    var color = Color.white
    var body: some View {
        TimelineView(.animation) { ctx in
            let time = ctx.date.timeIntervalSinceReferenceDate
            ZStack {
                ForEach(0..<count, id: \.self) { k in
                    let f = (time * speed + Double(k) / Double(count)).truncatingRemainder(dividingBy: 1)
                    Circle().stroke(color.opacity((1 - f) * 0.5), lineWidth: 1.5).frame(width: 2 * (from + f * (to - from)), height: 2 * (from + f * (to - from)))
                }
            }
        }
    }
}

/// Catmull-Rom through the points; `floor` closes it down to that y (for the fill).
func curve(_ p: [CGPoint], floor: CGFloat? = nil) -> Path {
    Path { path in
        path.move(to: p[0])
        for i in 0..<p.count - 1 {
            let (p0, p1, p2, p3) = (p[max(i - 1, 0)], p[i], p[i + 1], p[min(i + 2, p.count - 1)])
            path.addCurve(to: p2, control1: CGPoint(x: p1.x + (p2.x - p0.x) / 6, y: p1.y + (p2.y - p0.y) / 6),
                          control2: CGPoint(x: p2.x - (p3.x - p1.x) / 6, y: p2.y - (p3.y - p1.y) / 6))
        }
        if let floor {
            path.addLine(to: CGPoint(x: p[p.count - 1].x, y: floor))
            path.addLine(to: CGPoint(x: p[0].x, y: floor))
            path.closeSubpath()
        }
    }
}

/// The curve: drag anywhere, the nearest band follows (half-dB steps, never past the model's limit).
struct Equalizer: View {
    @ObservedObject var m: Model
    let eq: Profile.Eq

    var body: some View {
        GeometryReader { g in
            let (w, h, pad, labels) = (g.size.width, g.size.height, CGFloat(12), CGFloat(18))
            let n = m.gains.count
            let mid = (h - labels) / 2, half = mid - pad
            let x = { (i: Int) in pad + (w - 2 * pad) * CGFloat(i) / CGFloat(max(n - 1, 1)) }
            let pts = (0..<n).map { CGPoint(x: x($0), y: mid - CGFloat(m.gains[$0] / eq.maxDb) * half) }
            ZStack(alignment: .topLeading) {
                Path { p in
                    for k in [-1.0, -0.5, 0.5, 1.0] {
                        p.move(to: CGPoint(x: 0, y: mid + k * half))
                        p.addLine(to: CGPoint(x: w, y: mid + k * half))
                    }
                }
                .stroke(Color.white.opacity(0.05))
                Path { p in
                    p.move(to: CGPoint(x: 0, y: mid))
                    p.addLine(to: CGPoint(x: w, y: mid))
                }
                .stroke(Color.white.opacity(0.16), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                if n >= 2 {
                    curve(pts, floor: mid).fill(LinearGradient(colors: [Color(hex: 0xFF6A5A, alpha: 0.30), Color(hex: 0xFF6A5A, alpha: 0.04)], startPoint: .top, endPoint: .bottom))
                    curve(pts).stroke(accent, style: StrokeStyle(lineWidth: 2.4, lineCap: .round))
                    ForEach(0..<n, id: \.self) { i in
                        let big = m.dragging == i
                        Circle().fill(big ? Color.white : Color(hex: 0x16171D)).overlay(Circle().stroke(.white, lineWidth: 1.8))
                            .frame(width: big ? 13 : 9, height: big ? 13 : 9).position(pts[i])
                        Text(bandLabel(eq.bands[i])).font(.system(size: 9.5)).monospacedDigit().foregroundStyle(ink3)
                            .fixedSize().position(x: pts[i].x, y: h - labels / 2)
                    }
                    if let b = m.dragging, b < n { tip(b, pts[b], w) }
                }
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                let band = m.dragging ?? Int(((v.startLocation.x - pad) / ((w - 2 * pad) / CGFloat(max(n - 1, 1)))).rounded()).clamped(0, n - 1)
                let gain = (Double((mid - v.location.y) / half) * eq.maxDb * 2).rounded().clamped(-2 * eq.maxDb, 2 * eq.maxDb) / 2
                if n > 0 { m.drag(band: band, gain: gain) }
            }.onEnded { _ in m.endDrag() })
        }
    }

    func tip(_ b: Int, _ p: CGPoint, _ w: CGFloat) -> some View {
        let g = m.gains[b]
        let text = tf("eq.point", ["freq": bandLabel(eq.bands[b]), "gain": g > 0 ? String(format: "+%g", g) : String(format: "%g", g)])
        return Text(text).font(.system(size: 11.5, weight: .semibold)).foregroundStyle(Color(hex: 0x111216))
            .padding(.horizontal, 9).frame(height: 24)
            .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(.white))
            .fixedSize()
            .position(x: min(max(p.x, 60), w - 60), y: p.y - 30 < 4 ? p.y + 26 : p.y - 26)
    }
}

extension Comparable {
    func clamped(_ lo: Self, _ hi: Self) -> Self { min(max(self, lo), hi) }
}

// MARK: - List page

struct ListPage: View {
    @ObservedObject var m: Model

    var body: some View {
        VStack(spacing: 14) {
            HStack(alignment: .firstTextBaseline) {
                Text(t("list.title")).font(.system(size: 17, weight: .bold)).foregroundStyle(ink)
                Spacer()
                Text(subtitle).font(.system(size: 12)).foregroundStyle(ink2)
            }
            if m.devices.isEmpty {
                VStack(spacing: 10) {
                    ZStack {
                        Rings(count: 3, from: 30, to: 110, speed: 0.55, color: Color(hex: 0xFF9244))
                        Photo(image: nil, size: 84)
                    }
                    .frame(height: 220)
                    Text(t("list.searching")).font(.system(size: 14, weight: .semibold)).foregroundStyle(ink)
                    Text(t("list.searching_hint")).font(.system(size: 11)).foregroundStyle(ink2)
                }
                .padding(.bottom, 16)
            }
            ForEach(m.devices.indices, id: \.self) { i in DeviceCard(m: m, i: i) }
        }
        .padding(16)
    }

    var subtitle: String {
        switch m.devices.filter(\.state.connected).count {
        case 0: t("list.none_connected")
        case 1: t("list.one_connected")
        case let n: tf("list.many_connected", ["n": n])
        }
    }
}

struct Batteries: View {
    let b: Battery
    var body: some View {
        // space between: the first against the card's left edge, the last against its right
        HStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { i, r in
                if i > 0 { Spacer(minLength: 12) }
                VStack(alignment: .leading, spacing: 6) {
                    Text(t(r.0)).font(.system(size: 11)).foregroundStyle(ink2)
                    HStack(spacing: 6) {
                        Bar(level: r.1, charging: r.2)
                        Text("\(r.1)%").font(.system(size: 13, weight: .semibold)).monospacedDigit().foregroundStyle(ink)
                    }
                }
            }
        }
    }

    var rows: [(String, Int, Bool)] {
        [("battery.left", b.left, b.leftCharging), ("battery.right", b.right, b.rightCharging), ("battery.case", b.caseLevel, b.caseCharging)]
    }
}

struct DeviceCard: View {
    @ObservedObject var m: Model
    let i: Int

    var body: some View {
        let s = m.devices[i].state, p = m.profile(s)
        VStack(spacing: 16) {
            Button { withAnimation(springy) { m.open(i) } } label: {
                HStack(spacing: 14) {
                    Photo(image: m.photo(s), size: 64).background(Circle().fill(Color.white.opacity(0.04)))
                    VStack(alignment: .leading, spacing: 5) {
                        Text(displayName(s, p)).font(.system(size: 15, weight: .semibold)).foregroundStyle(ink).lineLimit(1)
                        HStack(spacing: 6) {
                            Circle().fill(s.connected ? green : ink3).frame(width: 6, height: 6)
                            Text(status(s)).font(.system(size: 12)).foregroundStyle(ink2).lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                    Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(ink3)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if s.connected, let b = s.battery { Batteries(b: b) }
            if !p.noiseModes.isEmpty {
                ModePicker(modes: p.noiseModes, selected: s.mode) { m.setMode($0, device: i) }.live(s.connected, dim: 1)
            }
        }
        .padding(14)
        .glass(16)
        .opacity(s.connected ? 1 : 0.55)
    }

    func status(_ s: DeviceState) -> String {
        switch (s.connected, s.mode) {
        case (true, let m?): t("noise.\(m.rawValue)_long")
        case (true, nil): t("device.connected")
        case (false, _): t("list.take_out")
        }
    }
}

// MARK: - Device page

struct DetailPage: View {
    @ObservedObject var m: Model
    /// For an ImageRenderer still, which leaves a ScrollView blank: the content as it is scrolled now.
    var still = false

    var body: some View {
        let s = m.state ?? DeviceState(), p = m.profile
        let tabs = tabsOf(p)
        let tab = tabs.contains(m.tab) ? m.tab : .sound
        VStack(spacing: 14) {
            ZStack {
                VStack(spacing: 3) {
                    Text(displayName(s, p)).font(.system(size: 15, weight: .semibold)).foregroundStyle(ink).lineLimit(1).padding(.horizontal, 40)
                    HStack(spacing: 5) {
                        Circle().fill(s.connected ? green : ink3).frame(width: 5, height: 5)
                        Text(t(s.connected ? "device.connected" : "device.disconnected")).font(.system(size: 10.5)).foregroundStyle(ink2)
                    }
                }
                HStack {
                    Button { withAnimation(springy) { m.detail = false } } label: {
                        Image(systemName: "chevron.left").font(.system(size: 12, weight: .bold)).foregroundStyle(ink)
                            .frame(width: 28, height: 28).glass(14)
                    }
                    .buttonStyle(.plain)
                    Spacer()
                }
            }
            HStack(spacing: 20) {
                Photo(image: m.photo(s), size: 104)
                    .background(Circle().fill(RadialGradient(colors: [Color(hex: 0xFF9244, alpha: 0.09), .clear],
                                                             center: .center, startRadius: 0, endRadius: 64)))
                VStack(spacing: 11) {
                    ForEach(batteryRows(s.connected ? s.battery : nil), id: \.0) { r in
                        HStack(spacing: 10) {
                            Text(t(r.0)).font(.system(size: 12)).foregroundStyle(ink2).frame(width: 58, alignment: .leading)
                            Bar(level: r.1 ?? 0, charging: r.2, width: nil)
                            Text(r.1.map { "\($0)%" } ?? "–").font(.system(size: 12.5, weight: .semibold)).monospacedDigit().foregroundStyle(ink)
                                .frame(width: 38, alignment: .trailing)
                        }
                    }
                }
            }
            .padding(.horizontal, 4)
            if tabs.count > 1 {
                Segmented(count: tabs.count, selected: tabs.firstIndex(of: tab), pill: AnyShapeStyle(Color.white.opacity(0.12)),
                          pick: { m.tab = tabs[$0] }) { i in
                    HStack(spacing: 6) {
                        Image(systemName: tabs[i].icon).font(.system(size: 12, weight: .medium))
                        Text(t(tabs[i].key)).font(.system(size: 12, weight: .medium))
                    }
                }
            }
            let content = Group {
                switch tab {
                case .sound: SoundTab(m: m)
                case .gestures: GesturesTab(m: m)
                case .find: FindTab(m: m)
                case .settings: SettingsTab(m: m)
                }
            }
            // sound and find fit (the curve takes what is left); the lists scroll
            if tab == .sound || tab == .find {
                content.frame(maxHeight: .infinity, alignment: .top).transition(.opacity).id(tab)
            } else if still {
                content.fixedSize(horizontal: false, vertical: true).offset(y: -m.scroll)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top).clipped()
            } else {
                ScrollView(.vertical, showsIndicators: false) { content }
                    .onScrollGeometryChange(for: CGFloat.self, of: { $0.contentOffset.y }) { m.scroll = $1 }
                    .scrollDisabled(m.dragging != nil || m.slider != nil)
                    .transition(.opacity)
                    .id(tab)
            }
        }
        .padding(16)
        .frame(width: panelWidth, height: detailHeight, alignment: .top)
        .overlay(alignment: .bottom) {
            ZStack(alignment: .bottom) {
                if m.sheet != nil {
                    Color.black.opacity(0.55).contentShape(Rectangle()).onTapGesture { m.sheet = nil }.transition(.opacity)
                }
                if let sheet = m.sheet {
                    SheetPanel(m: m, sheet: sheet).transition(.move(edge: .bottom))
                }
            }
            .animation(springy, value: m.sheet)
        }
    }

    /// Tabs the model has: sound always, touch, find and settings when the profile has them.
    func tabsOf(_ p: Profile) -> [Tab] {
        [.sound] + (p.gestures.isEmpty ? [] : [.gestures]) + (p.findBuds ? [.find] : []) + (p.settings.isEmpty ? [] : [.settings])
    }

    func batteryRows(_ b: Battery?) -> [(String, Int?, Bool)] {
        [("battery.left", b?.left, b?.leftCharging ?? false), ("battery.right", b?.right, b?.rightCharging ?? false),
         ("battery.case", b?.caseLevel, b?.caseCharging ?? false)]
    }
}

/// Noise control, the ANC options and the equalizer.
struct SoundTab: View {
    @ObservedObject var m: Model

    var body: some View {
        let s = m.state ?? DeviceState(), p = m.profile
        VStack(spacing: 8) {
            if !p.noiseModes.isEmpty {
                SectionTitle(text: t("noise.section")).padding(.top, 4)
                ModePicker(modes: p.noiseModes, selected: s.mode, labels: true) { m.setMode($0) }.live(s.connected)
            }
            anc(s, p).padding(.top, 4)
            if let eq = p.eq {
                HStack {
                    SectionTitle(text: t("eq.section"))
                    Switch(on: m.eqOn) { m.toggleEq() }.live(s.connected)
                }
                .padding(.top, 8)
                VStack(spacing: 10) {
                    let selected = m.presetIndex() ?? eq.presets.count
                    Flow {
                        ForEach(0...eq.presets.count, id: \.self) { i in
                            let on = i == selected && m.eqOn
                            Button { withAnimation(springy) { m.pick(preset: i < eq.presets.count ? i : nil) } } label: {
                                Text(t(i < eq.presets.count ? "eq.\(eq.presets[i].name)" : "eq.custom"))
                                    .font(.system(size: 12, weight: i == selected ? .semibold : .medium))
                                    .foregroundStyle(i == selected ? Color.white : ink2)
                                    .padding(.horizontal, 12).frame(height: 26)
                                    .glass(13, on: on)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    Equalizer(m: m, eq: eq).frame(minHeight: 80, maxHeight: .infinity).padding(.horizontal, 6).padding(.vertical, 6).glass(14)
                }
                .live(m.eqOn && s.connected, dim: 0.35)
            }
        }
    }

    /// Adaptive on/off, then one row of levels: the ambient level in ambient mode, the ANC
    /// intensity otherwise.
    @ViewBuilder func anc(_ s: DeviceState, _ p: Profile) -> some View {
        let anc = s.mode == .anc
        let auto = p.adaptiveAnc && s.adaptive == true
        let ambient = s.mode == .ambient && p.ambientLevels > 0
        let (max, current, title) = ambient ? (p.ambientLevels, s.ambientLevel, "anc.ambient_level") : (p.ancLevels, s.ancLevel, "anc.intensity")
        if p.adaptiveAnc || max > 0 {
            Card {
                if p.adaptiveAnc {
                    HStack {
                        TitleHint(title: t("anc.adaptive"), hint: t(anc ? "anc.adaptive_hint" : "anc.only_anc"))
                        Spacer()
                        Switch(on: auto) { m.toggleAdaptive() }.disabled(!anc || !s.connected)
                    }
                    .padding(.horizontal, 14).frame(height: 46)
                    .opacity(anc ? 1 : 0.35)
                }
                if p.adaptiveAnc && max > 0 { Hairline() }
                if max > 0 {
                    let manual = auto && !ambient
                    HStack {
                        TitleHint(title: t(title), hint: manual ? t("anc.automatic") : tf("anc.level", ["n": current.map { "\($0)" } ?? "–", "max": max]))
                        Spacer()
                        Steps(range: 1...max, current: current) { m.setLevel($0, ambient: ambient) }
                            .opacity(manual ? 0.45 : 1).disabled(!s.connected || !(ambient || (anc && !auto)))
                    }
                    .padding(.horizontal, 14).frame(height: 46)
                    .opacity(anc || ambient ? 1 : 0.35)
                }
            }
        }
    }
}

/// What each touch does, per earbud; a row opens the sheet to pick another action.
struct GesturesTab: View {
    @ObservedObject var m: Model

    var body: some View {
        let s = m.state ?? DeviceState(), p = m.profile
        if let pairs = s.gestures {
            VStack(spacing: 10) {
                ForEach([Side.left, .right], id: \.self) { side in
                    let gs = p.gestures.filter { $0.side == side }
                    if !gs.isEmpty {
                        SectionTitle(text: t(side.key)).padding(.top, side == .right ? 6 : 0)
                        Card {
                            ForEach(gs.indices, id: \.self) { k in
                                if k > 0 { Hairline() }
                                Button { m.sheet = .gesture(gs[k].id) } label: {
                                    HStack {
                                        Text(t("gesture.\(gs[k].kind)")).font(.system(size: 13)).foregroundStyle(ink)
                                        Spacer()
                                        Text(actionLabel(p, pairs.first { $0.gesture == gs[k].id }?.action)).font(.system(size: 12)).foregroundStyle(ink2).lineLimit(1)
                                        Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold)).foregroundStyle(ink3)
                                    }
                                    .padding(.horizontal, 14).frame(height: 44)
                                    .modifier(Hover())
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
            .live(s.connected)
        } else {
            Text(t("gestures.loading")).font(.system(size: 13)).foregroundStyle(ink2).padding(.top, 60)
        }
    }
}

func actionLabel(_ p: Profile, _ id: UInt8?) -> String {
    guard let id else { return "–" }
    return p.actions.first { $0.id == id }.map { t("action.\($0.name)") } ?? tf("action.unknown", ["id": String(format: "0x%02x", id)])
}

/// Makes an earbud beep, after a warning to take them out of the ears (it is loud).
struct FindTab: View {
    @ObservedObject var m: Model

    var body: some View {
        VStack(spacing: 16) {
            // the warning is not a button: solid, no glass
            HStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle").font(.system(size: 20, weight: .medium)).foregroundStyle(warning)
                VStack(alignment: .leading, spacing: 3) {
                    Text(t("find.warning")).font(.system(size: 13.5, weight: .semibold)).foregroundStyle(ink)
                    Text(t("find.warning_hint")).font(.system(size: 11.5)).foregroundStyle(ink2)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16).frame(height: 62)
            .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(warning.opacity(0.10)))
            .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(warning.opacity(0.35)))
            HStack(spacing: 10) {
                ForEach([Side.left, .right], id: \.self) { side in
                    let on = m.beeping(side)
                    Button { on ? m.beep(side, false) : (m.sheet = .beep(side)) } label: {
                        VStack(spacing: 6) {
                            ZStack {
                                if on { Rings(count: 2, from: 22, to: 48) }
                                Image(systemName: "speaker.wave.3").font(.system(size: 24, weight: .medium)).foregroundStyle(on ? Color.white : ink2)
                            }
                            .frame(height: 76)
                            Text(t(side.key)).font(.system(size: 14, weight: .semibold)).foregroundStyle(ink)
                            Text(t(on ? "find.playing" : "find.idle")).font(.system(size: 11.5)).foregroundStyle(on ? Color.white : ink2)
                            Text(t(on ? "find.stop" : "find.play")).font(.system(size: 12.5, weight: .semibold)).foregroundStyle(ink).padding(.top, 8)
                        }
                        .frame(maxWidth: .infinity).frame(height: 172)
                        .glass(20, on: on)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .live(m.connected)
        }
    }
}

/// Everything else the model has, row by row from the profile's `settings`, in sections.
struct SettingsTab: View {
    @ObservedObject var m: Model

    var body: some View {
        let all = m.profile.settings
        // a section: its title, then its rows in one card
        let sections = all.indices.filter { m.shown(all[$0]) }.reduce(into: [[Int]]()) { out, i in
            if let last = out.last?.last, all[last].section == all[i].section { out[out.count - 1].append(i) } else { out.append([i]) }
        }
        VStack(spacing: 10) {
            ForEach(sections, id: \.self) { rows in
                SectionTitle(text: t("settings.\(all[rows[0]].section)")).padding(.top, rows[0] != sections[0][0] ? 6 : 0)
                Card {
                    ForEach(rows, id: \.self) { i in
                        if i != rows[0] { Hairline() }
                        row(i, all[i]).live(m.live(all[i]))
                    }
                }
            }
        }
        .padding(.bottom, 8)
    }

    @ViewBuilder func row(_ i: Int, _ s: Setting) -> some View {
        let label = t("setting.\(s.name)"), hint = maybe("setting.\(s.name)_hint")
        switch s.kind {
        case .switch:
            HStack {
                TitleHint(title: label, hint: hint)
                Spacer()
                Switch(on: m.byte(s) == s.on) { m.toggle(s) }
            }
            .padding(.horizontal, 14).padding(.vertical, 9).frame(minHeight: 46)
        // up to three options sit in the row; more open a sheet
        case .choice where s.options.count <= 3:
            VStack(alignment: .leading, spacing: 9) {
                TitleHint(title: label, hint: hint)
                Segmented(count: s.options.count, selected: m.value(s).flatMap(s.currentOption), height: 28, pick: { m.pick(s, option: $0) }) { k in
                    Text(t("option.\(s.options[k].name)")).font(.system(size: 11.5, weight: .medium)).lineLimit(1)
                }
            }
            .padding(14)
        case .choice:
            Button { m.sheet = .options(i) } label: {
                HStack {
                    Text(label).font(.system(size: 13)).foregroundStyle(ink)
                    Spacer()
                    Text(m.value(s).flatMap(s.currentOption).map { t("option.\(s.options[$0].name)") } ?? "–").font(.system(size: 12)).foregroundStyle(ink2)
                    Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold)).foregroundStyle(ink3)
                }
                .padding(.horizontal, 14).frame(height: 44)
                .modifier(Hover())
            }
            .buttonStyle(.plain)
        // up to ten steps a level is a row of buttons; more, a slider
        case .level where s.max - s.min < 10:
            HStack {
                TitleHint(title: label, hint: m.byte(s).map { levelText(s, $0) } ?? "–")
                Spacer()
                Steps(range: s.min...s.max, current: m.byte(s)) { m.setLevel(s, $0) }
            }
            .padding(.horizontal, 14).frame(height: 50)
        case .level:
            let value = m.slider?.setting == s.name ? m.slider?.value : m.byte(s)
            VStack(spacing: 10) {
                HStack {
                    Text(label).font(.system(size: 13)).foregroundStyle(ink)
                    Spacer()
                    Text(value.map { levelText(s, $0) } ?? "–").font(.system(size: 12)).foregroundStyle(ink2)
                }
                LevelSlider(s: s, value: value, slide: { m.slide(s, $0) }, done: { m.endSlide(s, $0) })
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
        case .info:
            HStack {
                Text(label).font(.system(size: 13)).foregroundStyle(ink2)
                Spacer()
                Text(m.value(s).map { infoText(s, $0) } ?? "–").font(.system(size: 13, weight: .semibold)).foregroundStyle(ink).textSelection(.enabled)
            }
            .padding(.horizontal, 14).frame(height: 42)
        case .button:
            Button { m.sheet = .confirm(i) } label: {
                Text(label).font(.system(size: 13, weight: .semibold)).foregroundStyle(s.command == .factoryReset ? warning : ink)
                    .frame(maxWidth: .infinity).frame(height: 44)
                    .modifier(Hover())
            }
            .buttonStyle(.plain)
        }
    }
}

/// What an info setting shows.
func infoText(_ s: Setting, _ v: [UInt8]) -> String {
    s.format == .version ? v.map(String.init).joined(separator: ".")
        : String(decoding: v, as: UTF8.self).trimmingCharacters(in: CharacterSet(charactersIn: "\0").union(.whitespaces))
}

/// "Level n of max"; with a center (balance), how far to which side of it.
func levelText(_ s: Setting, _ n: UInt8) -> String {
    guard let c = s.center else { return tf("anc.level", ["n": Int(n) - Int(s.min) + 1, "max": Int(s.max) - Int(s.min) + 1]) }
    return n < c ? tf("setting.\(s.name)_low", ["n": c - n]) : n > c ? tf("setting.\(s.name)_high", ["n": n - c]) : t("setting.\(s.name)_center")
}

/// The bar grows from the center (balance) or from the start; sends only on release.
struct LevelSlider: View {
    let s: Setting
    let value: UInt8?
    let slide: (UInt8) -> Void
    let done: (UInt8) -> Void

    var body: some View {
        GeometryReader { g in
            let w = g.size.width, span = CGFloat(max(Int(s.max) - Int(s.min), 1))
            let at = { (n: UInt8) in CGFloat(Int(n.clamped(s.min, s.max)) - Int(s.min)) / span * w }
            let step = { (x: CGFloat) in s.min + UInt8(((x / w).clamped(0, 1) * span).rounded()) }
            let origin = s.center.map(at) ?? 0, kx = value.map(at) ?? origin
            ZStack(alignment: .leading) {
                Capsule().fill(Color.white.opacity(0.12)).frame(height: 4)
                if s.center != nil {
                    Capsule().fill(Color.white.opacity(0.25)).frame(width: 1.5, height: 12).offset(x: origin - 0.75)
                }
                Capsule().fill(accent).frame(width: abs(kx - origin), height: 4).offset(x: min(kx, origin))
                Circle().fill(.white).frame(width: 18, height: 18).shadow(color: .black.opacity(0.3), radius: 1.5, y: 1).offset(x: kx - 9)
            }
            .frame(height: 24)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { slide(step($0.location.x)) }.onEnded { done(step($0.location.x)) })
        }
        .frame(height: 24)
        .padding(.horizontal, 9)
    }
}

// MARK: - Sheets

struct SheetPanel: View {
    @ObservedObject var m: Model
    let sheet: Sheet

    var body: some View {
        let p = m.profile
        VStack(spacing: 0) {
            Capsule().fill(Color.white.opacity(0.18)).frame(width: 36, height: 4).padding(.top, 8)
            Group {
                switch sheet {
                case .gesture(let id):
                    let g = p.gestures.first { $0.id == id }
                    let current = m.state?.gestures?.first { $0.gesture == id }?.action
                    list(title: g.map { "\(t($0.side.key)) · \(t("gesture.\($0.kind)"))" } ?? "",
                         items: p.actions.map { (t("action.\($0.name)"), $0.id == current) }) { m.setGesture(id, p.actions[$0].id) }
                case .options(let i):
                    let s = p.settings[i]
                    let current = m.value(s).flatMap(s.currentOption)
                    list(title: t("setting.\(s.name)"), items: s.options.indices.map { (t("option.\(s.options[$0].name)"), $0 == current) }) { m.pick(s, option: $0) }
                case .beep(let side):
                    confirm(title: t("find.confirm_title"), lines: [t("find.confirm_line1"), t("find.confirm_line2")],
                            cancel: t("find.cancel"), ok: t("find.confirm")) { m.beep(side, true) }
                case .confirm(let i):
                    let s = p.settings[i]
                    confirm(title: t("setting.\(s.name)"), lines: [t("setting.\(s.name)_confirm")],
                            cancel: t("settings.cancel"), ok: t("settings.confirm")) { m.confirm(s) }
                }
            }
            .padding(16)
            .padding(.bottom, 4)
        }
        .frame(maxWidth: .infinity)
        .background(UnevenRoundedRectangle(topLeadingRadius: 20, topTrailingRadius: 20, style: .continuous).fill(Color(hex: 0x1E1F26)))
        .overlay(UnevenRoundedRectangle(topLeadingRadius: 20, topTrailingRadius: 20, style: .continuous).stroke(Color.white.opacity(0.09)))
    }

    func list(title: String, items: [(String, Bool)], pick: @escaping (Int) -> Void) -> some View {
        VStack(spacing: 6) {
            Text(title).font(.system(size: 15, weight: .bold)).foregroundStyle(ink).padding(.bottom, 10)
            ForEach(items.indices, id: \.self) { k in
                Button { pick(k) } label: {
                    HStack {
                        Text(items[k].0).font(.system(size: 13, weight: items[k].1 ? .semibold : .regular))
                        Spacer()
                        if items[k].1 { Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)) }
                    }
                    .foregroundStyle(items[k].1 ? Color.white : ink)
                    .padding(.horizontal, 16).frame(height: 38)
                    .glass(13, on: items[k].1)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    func confirm(title: String, lines: [String], cancel: String, ok: String, action: @escaping () -> Void) -> some View {
        VStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle").font(.system(size: 32, weight: .medium)).foregroundStyle(warning).padding(.vertical, 8)
            Text(title).font(.system(size: 16, weight: .bold)).foregroundStyle(ink)
            ForEach(lines, id: \.self) { Text($0).font(.system(size: 12.5)).foregroundStyle(ink2).multilineTextAlignment(.center) }
            HStack(spacing: 10) {
                Button { m.sheet = nil } label: { sheetButton(cancel, primary: false) }.buttonStyle(.plain)
                Button(action: action) { sheetButton(ok, primary: true) }.buttonStyle(.plain)
            }
            .padding(.top, 14)
        }
    }

    func sheetButton(_ text: String, primary: Bool) -> some View {
        Text(text).font(.system(size: 13.5, weight: .semibold)).foregroundStyle(.white)
            .frame(maxWidth: .infinity).frame(height: 44).glass(16, on: primary).contentShape(Rectangle())
    }
}

struct PanelView: View {
    @ObservedObject var m: Model
    var still = false

    var body: some View {
        ZStack(alignment: .top) {
            if m.detail {
                DetailPage(m: m, still: still).transition(.move(edge: .trailing).combined(with: .opacity))
            } else {
                ListPage(m: m).transition(.move(edge: .leading).combined(with: .opacity))
            }
        }
        .frame(width: panelWidth, alignment: .top)
        .fixedSize(horizontal: false, vertical: true)
        .background(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(Color(hex: 0x15161B)))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Color.white.opacity(0.09)))
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .frame(width: panelWidth, height: windowHeight, alignment: .top)
        .environment(\.colorScheme, .dark)
    }
}

// MARK: - Genie (Metal)

let shaderSource = """
#include <metal_stdlib>
using namespace metal;

struct U { float p; float tipX; float tipW; float w; float h; float rows; };
struct V { float4 pos [[position]]; float2 uv; float d; };

static float smooth01(float x) { x = clamp(x, 0.0, 1.0); return x * x * (3.0 - 2.0 * x); }

// Same curve as src/render/genie.rs: the sides bend toward the icon, then the content slides down the funnel.
vertex V genie_v(uint vid [[vertex_id]], constant U &u [[buffer(0)]]) {
    float side = float(vid % 2);
    float v = float(vid / 2) / u.rows;
    float bend = smooth01(u.p / 0.45);
    float slide = smooth01((u.p - 0.2) / 0.8);
    float d = v - slide;                      // distance from the icon, in window heights
    float dc = max(d, 0.0);
    float f = bend * (1.0 - cos(M_PI_F * (1.0 - dc))) / 2.0;
    float left = f * (u.tipX - u.tipW / 2.0);
    float right = u.w + f * (u.tipX + u.tipW / 2.0 - u.w);
    float x = left + side * (right - left);
    V o;
    o.pos = float4(x / u.w * 2.0 - 1.0, 1.0 - dc * 2.0, 0.0, 1.0);
    o.uv = float2(side, v);
    o.d = d;
    return o;
}

fragment float4 genie_f(V in [[stage_in]], texture2d<float> t [[texture(0)]]) {
    if (in.d < 0.0) discard_fragment();      // already down the funnel
    constexpr sampler s(filter::linear, address::clamp_to_edge);
    return t.sample(s, in.uv);
}
"""

struct GenieUniforms { var p, tipX, tipW, w, h, rows: Float }

final class Genie: NSObject, MTKViewDelegate {
    let view: MTKView
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private var texture: MTLTexture?
    private var start: CFTimeInterval = 0
    private var duration: CFTimeInterval = 0.8
    private var opening = true
    private var done: (() -> Void)?
    var tipX: Float = 0
    private let rows = 160

    init(frame: NSRect) {
        device = MTLCreateSystemDefaultDevice()!
        view = MTKView(frame: frame, device: device)
        queue = device.makeCommandQueue()!
        let lib = try! device.makeLibrary(source: shaderSource, options: nil)
        let d = MTLRenderPipelineDescriptor()
        d.vertexFunction = lib.makeFunction(name: "genie_v")
        d.fragmentFunction = lib.makeFunction(name: "genie_f")
        d.colorAttachments[0].pixelFormat = .bgra8Unorm
        pipeline = try! device.makeRenderPipelineState(descriptor: d)
        super.init()
        view.colorPixelFormat = .bgra8Unorm
        view.clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)
        view.layer?.isOpaque = false
        (view.layer as? CAMetalLayer)?.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        view.isPaused = true
        view.enableSetNeedsDisplay = false
        view.preferredFramesPerSecond = 120
        view.delegate = self
    }

    /// Where the running animation is: 0 = open … 1 = inside the icon.
    var progress: Float {
        let x = Float(min(max((CACurrentMediaTime() - start) / duration, 0), 1))
        return opening ? 1 - x * x * (3 - 2 * x) : x * x
    }

    func run(_ image: CGImage, opening: Bool, from p: Float, done: @escaping () -> Void) {
        texture = makeTexture(image)
        self.opening = opening
        self.done = done
        duration = opening ? 0.8 : 0.55
        // closing halfway through the opening carries on from there (closing is x², so x = √p)
        start = CACurrentMediaTime() - (opening ? 0 : Double(p.squareRoot()) * duration)
        view.isPaused = false
    }

    func draw(in view: MTKView) {
        let finished = CACurrentMediaTime() - start >= duration
        guard let texture, let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable,
              let cb = queue.makeCommandBuffer(), let enc = cb.makeRenderCommandEncoder(descriptor: pass) else { return }
        var u = GenieUniforms(p: progress, tipX: tipX, tipW: 26, w: Float(view.bounds.width), h: Float(view.bounds.height), rows: Float(rows))
        enc.setRenderPipelineState(pipeline)
        enc.setVertexBytes(&u, length: MemoryLayout<GenieUniforms>.stride, index: 0)
        enc.setFragmentTexture(texture, index: 0)
        enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: (rows + 1) * 2)
        enc.endEncoding()
        cb.present(drawable)
        cb.commit()
        if finished {
            view.isPaused = true
            let d = done
            done = nil
            DispatchQueue.main.async { d?() }
        }
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    /// Premultiplied RGBA in sRGB, top row first: what the shader samples.
    private func makeTexture(_ image: CGImage) -> MTLTexture? {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        bytes.withUnsafeMutableBytes { buf in
            let ctx = CGContext(data: buf.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            ctx?.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm, width: w, height: h, mipmapped: false)
        let t = device.makeTexture(descriptor: desc)
        t?.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: bytes, bytesPerRow: w * 4)
        return t
    }
}

// MARK: - Window

final class KeyPanel: NSPanel {
    var onEscape: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onEscape() }
}

@MainActor
final class PanelController: NSObject, NSWindowDelegate {
    enum Phase { case closed, opening, open, closing }

    let model: Model
    let window: KeyPanel
    let host: NSHostingView<PanelView>
    let genie: Genie
    var phase = Phase.closed
    var closedAt = CACurrentMediaTime() - 1

    init(model: Model) {
        self.model = model
        let frame = NSRect(x: 0, y: 0, width: panelWidth, height: windowHeight)
        window = KeyPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.level = .statusBar
        host = NSHostingView(rootView: PanelView(m: model))
        host.frame = frame
        genie = Genie(frame: frame)
        let root = NSView(frame: frame)
        root.addSubview(host)
        root.addSubview(genie.view)
        window.contentView = root
        super.init()
        window.delegate = self
        // Esc closes the sheet first, then the panel
        window.onEscape = { [weak self] in
            guard let self else { return }
            if self.model.sheet != nil { self.model.sheet = nil } else { self.close() }
        }
    }

    func toggle(anchor: NSRect) {
        switch phase {
        case .opening, .open: close()
        // the click on the icon already took the focus away and closed it: closing was the point
        case .closed, .closing: if CACurrentMediaTime() - closedAt > 0.4 { open(anchor) }
        }
    }

    private func snapshot() -> CGImage? {
        let r = ImageRenderer(content: PanelView(m: model, still: true))
        r.scale = window.backingScaleFactor
        r.isOpaque = false
        return r.cgImage
    }

    func open(_ a: NSRect) {
        let screen = NSScreen.screens.first { $0.frame.contains(NSPoint(x: a.midX, y: a.midY)) } ?? NSScreen.main!
        let x = min(max(a.midX - panelWidth / 2, screen.visibleFrame.minX + 8), screen.visibleFrame.maxX - panelWidth - 8)
        window.setFrameOrigin(NSPoint(x: x, y: a.minY - windowHeight - 6))
        genie.tipX = Float(a.midX - x)
        guard let image = snapshot() else { return }
        phase = .opening
        host.alphaValue = 0
        genie.view.isHidden = false
        window.orderFrontRegardless()
        window.makeKey()
        NSApp.activate()
        genie.run(image, opening: true, from: 1) { [weak self] in
            guard let self, self.phase == .opening else { return }
            self.phase = .open
            self.host.alphaValue = 1
            self.genie.view.isHidden = true
        }
    }

    func close() {
        guard phase == .opening || phase == .open, let image = snapshot() else { return }
        let p = phase == .opening ? genie.progress : 0
        phase = .closing
        closedAt = CACurrentMediaTime()
        genie.view.isHidden = false
        host.alphaValue = 0
        genie.run(image, opening: false, from: p) { [weak self] in
            guard let self, self.phase == .closing else { return }
            self.phase = .closed
            self.window.orderOut(nil)
            (self.model.detail, self.model.sheet) = (false, nil)
        }
    }

    func windowDidResignKey(_ notification: Notification) { close() }
}

// MARK: - App

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model: Model
    var item: NSStatusItem!
    var panel: PanelController!
    var bluetooth: Bluetooth?

    init(model: Model) { self.model = model }

    func applicationDidFinishLaunching(_ notification: Notification) {
        panel = PanelController(model: model)
        let model = model
        bluetooth = Bluetooth(prefixes: model.profiles.flatMap(\.namePrefixes)) { id, state in model.update(id, state) }
        model.bluetooth = bluetooth
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        item.button?.image = NSImage(systemSymbolName: "headphones", accessibilityDescription: t("app.name"))
        item.button?.target = self
        item.button?.action = #selector(click)
        item.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
        if CommandLine.arguments.contains("--open") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.click() }
        }
    }

    @objc func click() {
        // right click: the menu with Quit (set only for this click, so the left click still opens the panel)
        if NSApp.currentEvent?.type == .rightMouseUp {
            let menu = NSMenu()
            menu.addItem(withTitle: t("app.quit"), action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
            item.menu = menu
            item.button?.performClick(nil)
            item.menu = nil
            return
        }
        guard let frame = item.button?.window?.frame else { return }
        panel.toggle(anchor: frame)
    }
}

/// `--snapshot <dir>`: every screen with a fake device as PNGs, no window (to check the look).
@MainActor
func snapshot(to dir: String, profiles: [Profile]) {
    let m = Model(profiles: profiles)
    func save(_ name: String) {
        let r = ImageRenderer(content: PanelView(m: m, still: true))
        r.scale = 2
        if let image = r.cgImage, let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: "\(dir)/\(name).png"))
        }
    }
    save("empty")
    // the first model with an id stands in for a real device, with the settings it sent (Nov 8)
    let p = profiles.first { !$0.productIds.isEmpty } ?? Profile()
    var s = DeviceState(connected: true, name: p.name, productId: p.productIds.first, color: 0, mode: .anc, adaptive: false,
                        ancLevel: 5, ambientLevel: 3, battery: Battery(left: 96, right: 97, caseLevel: 40, caseCharging: true),
                        eq: p.eq.flatMap { $0.presets.count > 2 ? .some($0.presets[2].gains.map(Float.init)) : nil },
                        gestures: zip(p.gestures, (0..<p.gestures.count).map { p.actions[($0 + 6) % max(p.actions.count, 1)] }).map { Touch(gesture: $0.id, action: $1.id) },
                        beep: [false, true])
    for packet in ["aa 06 3d 00 02 00 05 03 03 05 00 00 06 01 07 00 0b 03 00 1e 00 00 0c 02 00 01 00 0d 02 00 02 00 14 02 00 00 " +
                   "00 16 03 00 00 00 00 1a 02 00 60 00 1b 02 00 61 00 1c 02 01 28 00 20 01 00 00 23 01 01",
                   "aa 91 09 22 01 00 04 05 05 01 a1 03", "aa 80 09 02 c5 00 2e 00 18 01 ff ff",
                   "aa 94 11 02 4d 4d 30 32 34 35 2d 44 50 30 32 34 36 39 36 31", "aa a8 05 02 01 01 02 61 aa 93 02 05 01 aa 9c 02 02 00"] {
        JBL.apply(&s, hex(packet))
    }
    m.update("fake", s)
    save("list")
    m.open(0)
    for (name, tab, sheet) in [("sound", Tab.sound, nil), ("gestures", .gestures, Sheet.gesture(p.gestures.first?.id ?? 0)),
                               ("find", .find, .beep(.left)), ("settings", .settings, p.settings.firstIndex { $0.kind == .button }.map(Sheet.confirm))] {
        (m.tab, m.sheet) = (tab, nil)
        save(name)
        if let sheet {
            m.sheet = sheet
            save("\(name)-sheet")
        }
    }
    (m.tab, m.sheet) = (.sound, nil)
    m.setMode(.ambient)
    save("sound-ambient")
}

MainActor.assumeIsolated {
    let args = CommandLine.arguments
    loadTexts(language: args.firstIndex(of: "--lang").flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil })
    if args.contains("--selftest") {
        JBL.selfTest()
        dataSelfTest()
        print("selftest ok")
        exit(0)
    }
    let profiles = Profile.loadAll()
    if let i = args.firstIndex(of: "--snapshot") {
        snapshot(to: i + 1 < args.count ? args[i + 1] : ".", profiles: profiles)
        exit(0)
    }
    let app = NSApplication.shared
    let delegate = AppDelegate(model: Model(profiles: profiles))
    app.delegate = delegate
    app.setActivationPolicy(.accessory)
    withExtendedLifetime(delegate) { app.run() }
}
