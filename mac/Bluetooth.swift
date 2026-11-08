// Finds this Mac's headphones and keeps one session per device: the logic of src/bluetooth/mod.rs
// on CoreBluetooth. Everything runs on the main queue, so the callbacks touch the model directly.

import CoreBluetooth
import IOBluetooth

/// `--debug`: every packet to and from the devices, on stderr (the Rust app's format).
let debug = CommandLine.arguments.contains("--debug")

func log(_ s: String) { FileHandle.standardError.write(Data((s + "\n").utf8)) }

/// The advertised name without the "-LE" some models add to their BLE name.
func bleName(_ s: String) -> String { s.hasSuffix("-LE") ? String(s.dropLast(3)) : s }

/// Names of the devices paired with this Mac and connected to it now (audio included).
func connectedHere() -> Set<String> {
    Set((IOBluetoothDevice.pairedDevices() as? [IOBluetoothDevice] ?? []).filter { $0.isConnected() }.compactMap(\.name))
}

final class Bluetooth: NSObject, CBCentralManagerDelegate {
    typealias Emit = (String, DeviceState) -> Void
    private var central: CBCentralManager!
    private let prefixes: [String]
    private let emit: Emit
    private var sessions: [String: Session] = [:]
    private var ended: [String: Date] = [:]
    /// What the scan found, by BLE id (held: CoreBluetooth forgets what nobody holds).
    private var scanned: [UUID: (CBPeripheral, String)] = [:]
    private var timer: Timer?
    private var round = 0

    /// `emit(id, state)` on every change of a device whose name starts with one of `prefixes`.
    init(prefixes: [String], emit: @escaping Emit) {
        (self.prefixes, self.emit) = (prefixes, emit)
        super.init()
        central = CBCentralManager(delegate: self, queue: .main)
    }

    /// Sends a command to device `id` (the same id that comes with its state).
    func send(_ id: String, _ cmd: Command) { sessions[id]?.send(cmd) }

    private func wanted(_ name: String) -> Bool { prefixes.contains { name.hasPrefix($0) } }

    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        if debug { log("[bluetooth] adapter state \(c.state.rawValue) (5 = on, 3 = no permission)") }
        switch c.state {
        case .poweredOn:
            guard timer == nil else { return }
            timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.tick() }
            tick()
        case .unauthorized:
            log("[bluetooth] no permission: allow Bluetooth for the app that started this one (System Settings › Privacy & Security › Bluetooth)")
        default: // off: CoreBluetooth drops every link without telling each one
            timer?.invalidate()
            timer = nil
            for s in Array(sessions.values) { s.end("Bluetooth is off") }
        }
    }

    private func tick() {
        round += 1
        // Scanning fights the headphones' audio for the radio: scan only while there is none
        // (and with one connected, look for another every 30 s).
        if sessions.isEmpty != central.isScanning {
            if sessions.isEmpty { central.scanForPeripherals(withServices: nil) } else { central.stopScan() }
        }
        if !sessions.isEmpty && round % 10 != 0 { return }
        // by name: the same headphones show up under more than one BLE id (one per earbud, and
        // macOS hands out a new one after a while), so the id would list them twice
        var found: [String: [CBPeripheral]] = [:]
        for (p, name) in candidates() { found[name, default: []].append(p) }
        if debug { log("[bluetooth] round \(round): \(found.mapValues(\.count)), sessions \(sessions.keys.sorted())") }
        for (name, ps) in found {
            // failed: next round, not 10 s later (right after connecting for audio the headphones
            // can drop the first BLE link)
            if sessions[name] != nil || ended[name].map({ -$0.timeIntervalSinceNow < 3 }) == true { continue }
            let s = Session(name: name, peripherals: ps, central: central, emit: emit) { [weak self] in
                self?.sessions[name] = nil
                self?.ended[name] = Date()
            }
            sessions[name] = s
            s.start()
        }
    }

    /// Only this Mac's headphones: connected to the system on BLE (macOS does not advertise those),
    /// or advertising under the name of one connected here for audio. GATT needs no pairing:
    /// anything else the scan finds is someone else's, nearby.
    private func candidates() -> [(CBPeripheral, String)] {
        let here = connectedHere()
        let connected = central.retrieveConnectedPeripherals(withServices: [JBL.service]).map { ($0, bleName($0.name ?? ""), true) }
        var v: [(CBPeripheral, String)] = []
        for (p, name, ours) in connected + scanned.values.map({ ($0.0, $0.1, false) }) {
            if !wanted(name) || v.contains(where: { $0.0.identifier == p.identifier }) { continue }
            // ponytail: by name, so the same model nearby also passes while ours is connected, and a device
            // renamed in the Bluetooth settings does not match; compare the MAC from 0x12 if needed
            if !ours && !here.contains(name) {
                if debug { log("[bluetooth] skipped \"\(name)\": not connected to this Mac (connected: \(here.sorted()))") }
                continue
            }
            v.append((p, name))
        }
        return v
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral, advertisementData ad: [String: Any], rssi: NSNumber) {
        let name = bleName(ad[CBAdvertisementDataLocalNameKey] as? String ?? p.name ?? "")
        if wanted(name) { scanned[p.identifier] = (p, name) }
    }

    private func session(of p: CBPeripheral) -> Session? { sessions.values.first { $0.owns(p) } }
    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) { session(of: p)?.connected(p) }
    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) { session(of: p)?.failed(p, error) }
    func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) { session(of: p)?.failed(p, error) }
}

/// One device while it stays connected. It opens all of the device's BLE ids at once and keeps the
/// first that answers: one at a time, a silent one held the right one back for its whole timeout.
final class Session: NSObject, CBPeripheralDelegate {
    let name: String
    private let peripherals: [CBPeripheral]
    private unowned let central: CBCentralManager
    private let emit: Bluetooth.Emit
    private let onEnd: () -> Void
    private var state: DeviceState
    private var failures = Set<UUID>()
    /// The one that answered, and where to write.
    private var link: (p: CBPeripheral, write: CBCharacteristic)?
    /// Writes wait for the previous one's response, as in the Rust app.
    private var queue: [[UInt8]] = []
    private var writing = false
    private var timers: [Timer] = []
    private var done = false

    init(name: String, peripherals: [CBPeripheral], central: CBCentralManager, emit: @escaping Bluetooth.Emit, onEnd: @escaping () -> Void) {
        (self.name, self.peripherals, self.central, self.emit, self.onEnd) = (name, peripherals, central, emit, onEnd)
        state = DeviceState(name: name)
    }

    func owns(_ p: CBPeripheral) -> Bool { peripherals.contains { $0.identifier == p.identifier } }

    func start() {
        for p in peripherals {
            p.delegate = self
            central.connect(p)
        }
        // connect() never gives up on its own (still in the case, nothing answers)
        timers.append(Timer.scheduledTimer(withTimeInterval: 15, repeats: false) { [weak self] _ in
            if self?.link == nil { self?.end("no BLE id answered in 15 s") }
        })
    }

    func connected(_ p: CBPeripheral) { p.discoverServices([JBL.service]) }

    func failed(_ p: CBPeripheral, _ error: Error?) {
        if p.identifier == link?.p.identifier { return end("disconnected \(error?.localizedDescription ?? "")") }
        if link != nil { return } // one that lost: it just goes
        failures.insert(p.identifier)
        if failures.count == peripherals.count { end("no BLE id answered: \(error?.localizedDescription ?? "?")") }
    }

    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard let s = p.services?.first(where: { $0.uuid == JBL.service }) else { return failed(p, error) }
        p.discoverCharacteristics([JBL.notify, JBL.write], for: s)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor s: CBService, error: Error?) {
        let all = s.characteristics ?? []
        guard let n = all.first(where: { $0.uuid == JBL.notify }), all.contains(where: { $0.uuid == JBL.write }) else { return failed(p, error) }
        p.setNotifyValue(true, for: n)
    }

    /// Subscribed: this BLE id answers. The first one wins; the others are let go only when the
    /// session ends (disconnecting them right away dropped this one too).
    func peripheral(_ p: CBPeripheral, didUpdateNotificationStateFor c: CBCharacteristic, error: Error?) {
        guard error == nil, c.isNotifying else { return failed(p, error) }
        guard link == nil, !done, let w = c.service?.characteristics?.first(where: { $0.uuid == JBL.write }) else { return }
        link = (p, w)
        if debug { log("[bluetooth] \(name): \(peripherals.count) BLE id(s), \(p.identifier) answered") }
        state.connected = true
        emit(name, state)
        JBL.initialRequests().forEach(write)
        timers.append(Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            guard let self, let l = self.link else { return }
            if l.p.state != .connected { return self.end("gone") }
            self.write(JBL.batteryRequest)
        })
    }

    func peripheral(_ p: CBPeripheral, didUpdateValueFor c: CBCharacteristic, error: Error?) {
        guard p.identifier == link?.p.identifier, c.uuid == JBL.notify, let v = c.value else { return }
        let data = [UInt8](v)
        if debug { log("[bluetooth] \(name) <- \(hexString(data))") }
        if JBL.apply(&state, data) { emit(name, state) }
    }

    func send(_ c: Command) {
        let packet = JBL.encode(c)
        guard link != nil, !packet.isEmpty else { return }
        write(packet)
        if let q = JBL.followUp(c) { write(q) }
    }

    private func write(_ packet: [UInt8]) {
        queue.append(packet)
        pump()
    }

    private func pump() {
        guard !writing, let l = link, !queue.isEmpty else { return }
        let packet = queue.removeFirst()
        if debug { log("[bluetooth] \(name) -> \(hexString(packet))") }
        writing = true
        l.p.writeValue(Data(packet), for: l.write, type: .withResponse)
    }

    func peripheral(_ p: CBPeripheral, didWriteValueFor c: CBCharacteristic, error: Error?) {
        guard p.identifier == link?.p.identifier else { return }
        if let error { log("[bluetooth] \(name): write failed: \(error.localizedDescription)") }
        writing = false
        pump()
    }

    func end(_ why: String) {
        guard !done else { return }
        done = true
        log("[bluetooth] session \(name) ended: \(why)")
        timers.forEach { $0.invalidate() }
        for p in peripherals { central.cancelPeripheralConnection(p) }
        // never connected (still in the case): nothing to list
        if state.connected {
            state.connected = false
            emit(name, state)
        }
        onEnd()
    }
}
