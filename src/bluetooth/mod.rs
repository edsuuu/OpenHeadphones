//! Finds headphones and keeps one session per device, on its own thread (tokio, current thread).
//! The per-OS link is in `link`; the bytes on the wire in `protocol::jbl`.

mod link;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Instant, interval_at, sleep};

use crate::device::{Command, DeviceState};
use crate::i18n::t;
use crate::protocol::jbl;
use link::Link;

/// Sends commands to device `id` (the same id that comes with its state).
pub struct Bluetooth(UnboundedSender<(String, Command)>);

impl Bluetooth {
    pub fn send(&self, id: &str, cmd: Command) {
        let _ = self.0.send((id.to_string(), cmd));
    }
}

type Emit = Arc<dyn Fn(String, DeviceState) + Send + Sync>;

/// `--debug`: logs every packet to and from the devices (stderr).
pub static DEBUG: AtomicBool = AtomicBool::new(false);

fn log_packet(id: &str, arrow: &str, data: &[u8]) {
    if DEBUG.load(Ordering::Relaxed) {
        let hex: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
        eprintln!("[bluetooth] {id} {arrow} {}", hex.join(" "));
    }
}

/// Starts the Bluetooth thread; `emit(id, state)` on every change of a device whose name starts
/// with one of `name_prefixes` (from the device profiles).
pub fn start(name_prefixes: Vec<String>, emit: impl Fn(String, DeviceState) + Send + Sync + 'static) -> Bluetooth {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{}: {e}", t("error.runtime")));
        rt.block_on(manager(rx, Arc::new(emit), name_prefixes));
    });
    Bluetooth(tx)
}

async fn adapter() -> Option<Adapter> {
    Manager::new().await.ok()?.adapters().await.ok()?.into_iter().next()
}

fn wanted(name: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| name.starts_with(p.as_str()))
}

/// Finds the headphones (already connected or advertising) and keeps a session for each.
async fn manager(mut rx: UnboundedReceiver<(String, Command)>, emit: Emit, prefixes: Vec<String>) {
    let central = loop {
        match adapter().await {
            Some(a) => break a,
            None => sleep(Duration::from_secs(5)).await, // Bluetooth off
        }
    };
    let mut sessions: HashMap<String, UnboundedSender<Command>> = HashMap::new();
    let mut ended: HashMap<String, Instant> = HashMap::new();
    let (end_tx, mut end_rx) = mpsc::unbounded_channel::<String>();
    let mut scanning = false;
    let mut tick = tokio::time::interval(Duration::from_secs(3));
    let mut round = 0u32;
    loop {
        tokio::select! {
            Some((id, cmd)) = rx.recv() => {
                if let Some(s) = sessions.get(&id) {
                    let _ = s.send(cmd);
                }
            }
            Some(id) = end_rx.recv() => {
                sessions.remove(&id);
                ended.insert(id, Instant::now());
            }
            _ = tick.tick() => {
                round += 1;
                // Scanning fights the headphones' audio for the radio: scan only while there is
                // none (and with one connected, look for another every 30 s).
                if sessions.is_empty() != scanning {
                    scanning = sessions.is_empty();
                    let _ = if scanning { central.start_scan(ScanFilter::default()).await } else { central.stop_scan().await };
                }
                if !sessions.is_empty() && round % 10 != 0 {
                    continue;
                }
                for (p, name) in candidates(&central, &prefixes).await {
                    let id = format!("{:?}", p.id());
                    // still in the case: try again in 10 s, not every round
                    if sessions.contains_key(&id) || ended.get(&id).is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                        continue;
                    }
                    let (tx, rx) = mpsc::unbounded_channel();
                    sessions.insert(id.clone(), tx);
                    let (emit, end) = (emit.clone(), end_tx.clone());
                    tokio::spawn(async move {
                        let mut state = DeviceState { name, ..Default::default() };
                        let r = session(&p, &id, rx, &emit, &mut state).await;
                        eprintln!("[bluetooth] session {id} ({}) ended: {r:?}", state.name);
                        let _ = p.disconnect().await;
                        state.connected = false;
                        emit(id.clone(), state);
                        let _ = end.send(id);
                    });
                }
            }
        }
    }
}

/// Paired on Windows + already connected to the system (macOS does not advertise those) + found by the scan.
async fn candidates(central: &Adapter, prefixes: &[String]) -> Vec<(Peripheral, String)> {
    let mut v = paired(central, prefixes).await;
    let options = RetrievePeripheralsOptions { services: Some(vec![jbl::SERVICE]), ..Default::default() };
    let more = central.retrieve_peripherals(options).await.unwrap_or_default();
    for p in more.into_iter().chain(central.peripherals().await.unwrap_or_default()) {
        let name = advertised_name(&p).await;
        // ponytail: two identical paired headphones only show up through pairing; compare the MAC from 0x12 if needed
        if wanted(&name, prefixes) && !v.iter().any(|(q, n)| q.id() == p.id() || *n == name) {
            v.push((p, name));
        }
    }
    v
}

/// Connected to the PC for audio, the headphones do not advertise on BLE and the scan misses
/// them: go by the address Windows has paired — for audio (classic, same address as BLE) or BLE.
#[cfg(windows)]
async fn paired(central: &Adapter, prefixes: &[String]) -> Vec<(Peripheral, String)> {
    use btleplug::api::BDAddr;
    use btleplug::platform::PeripheralId;
    use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    let mut v: Vec<(Peripheral, String)> = vec![];
    let list = async {
        let mut l = vec![];
        for selector in
            [BluetoothDevice::GetDeviceSelectorFromPairingState(true)?, BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?]
        {
            l.extend(DeviceInformation::FindAllAsyncAqsFilter(&selector)?.await?);
        }
        windows::core::Result::Ok(l)
    };
    for info in list.await.unwrap_or_default() {
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        let name = name.trim_end_matches("-LE").to_string();
        if !wanted(&name, prefixes) {
            continue;
        }
        let address = async {
            match BluetoothDevice::FromIdAsync(&info.Id()?)?.await {
                Ok(d) => d.BluetoothAddress(),
                Err(_) => BluetoothLEDevice::FromIdAsync(&info.Id()?)?.await?.BluetoothAddress(),
            }
        };
        let Ok(Ok(address)) = address.await.map(BDAddr::try_from) else { continue };
        let id = PeripheralId::from(address);
        if v.iter().any(|(p, _)| p.id() == id) {
            continue;
        }
        if let Ok(p) = central.add_peripheral(&id).await {
            v.push((p, name));
        }
    }
    v
}

#[cfg(not(windows))]
async fn paired(_: &Adapter, _: &[String]) -> Vec<(Peripheral, String)> {
    vec![]
}

async fn advertised_name(p: &Peripheral) -> String {
    let name = p.properties().await.ok().flatten().and_then(|x| x.local_name).unwrap_or_default();
    name.trim_end_matches("-LE").to_string()
}

/// Runs while the device stays connected.
async fn session(
    p: &Peripheral,
    id: &str,
    mut rx: UnboundedReceiver<Command>,
    emit: &Emit,
    state: &mut DeviceState,
) -> btleplug::Result<()> {
    let (link, mut notifications) = Link::open(p).await?;
    state.connected = true;
    emit(id.to_string(), state.clone());
    for q in jbl::INITIAL_REQUESTS {
        link.send(q).await?;
    }
    let mut battery = interval_at(Instant::now() + Duration::from_secs(30), Duration::from_secs(30));
    loop {
        tokio::select! {
            n = notifications.next() => match n {
                Some(data) => {
                    log_packet(id, "<-", &data);
                    if jbl::apply(state, &data) {
                        emit(id.to_string(), state.clone());
                    }
                }
                None => return Ok(()),
            },
            c = rx.recv() => match c {
                Some(c) => {
                    let packet = jbl::encode(&c);
                    log_packet(id, "->", &packet);
                    link.send(&packet).await?;
                    if let Some(q) = jbl::follow_up(&c) {
                        link.send(q).await?;
                    }
                }
                None => return Ok(()),
            },
            _ = battery.tick() => {
                if !link.alive().await {
                    return Ok(());
                }
                link.send(jbl::BATTERY_REQUEST).await?;
            }
        }
    }
}
