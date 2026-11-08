//! The BLE link to one device: write packets, receive notifications.

pub use imp::Link;

pub type Notifications = futures::stream::BoxStream<'static, Vec<u8>>;

/// Linux/macOS: btleplug handles it.
#[cfg(not(windows))]
mod imp {
    use std::time::Duration;

    use btleplug::api::{Characteristic, Peripheral as _, WriteType};
    use btleplug::platform::Peripheral;
    use futures::StreamExt;
    use tokio::time::timeout;

    use super::Notifications;
    use crate::protocol::jbl::{NOTIFY, WRITE};

    pub struct Link {
        p: Peripheral,
        write: Characteristic,
    }

    impl Link {
        pub async fn open(p: &Peripheral) -> btleplug::Result<(Link, Notifications)> {
            if !p.is_connected().await? {
                let limit = Duration::from_secs(15);
                timeout(limit, p.connect()).await.map_err(|_| btleplug::Error::TimedOut(limit))??;
            }
            p.discover_services().await?;
            let find = |u| p.characteristics().into_iter().find(|c| c.uuid == u).ok_or(btleplug::Error::NoSuchCharacteristic);
            let (notify, write) = (find(NOTIFY)?, find(WRITE)?);
            p.subscribe(&notify).await?;
            let stream = p.notifications().await?.filter_map(|n| async move { (n.uuid == NOTIFY).then_some(n.value) }).boxed();
            Ok((Link { p: p.clone(), write }, stream))
        }

        pub async fn send(&self, b: &[u8]) -> btleplug::Result<()> {
            self.p.write(&self.write, b, WriteType::WithResponse).await
        }

        pub async fn alive(&self) -> bool {
            self.p.is_connected().await.unwrap_or(false)
        }
    }
}

/// Windows: straight to WinRT, like bleak. Paired only for audio, the BLE link only comes up
/// with a GattSession asking to keep it, and btleplug's discover-everything hangs forever on one
/// of the device's services before reaching JBL's — here only JBL's service is asked for.
#[cfg(windows)]
mod imp {
    use std::time::Duration;

    use btleplug::api::Peripheral as _;
    use btleplug::platform::Peripheral;
    use futures::StreamExt;
    use tokio::time::{sleep, timeout};
    use uuid::Uuid;
    use windows::Devices::Bluetooth::GenericAttributeProfile::*;
    use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice};
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::{DataReader, DataWriter};
    use windows::core::{GUID, Ref};

    use super::Notifications;
    use crate::protocol::jbl::{NOTIFY, SERVICE, WRITE};

    pub struct Link {
        device: BluetoothLEDevice,
        session: GattSession,
        notify: GattCharacteristic,
        write: GattCharacteristic,
        token: i64,
    }

    impl Link {
        pub async fn open(p: &Peripheral) -> btleplug::Result<(Link, Notifications)> {
            let address = u64::from(p.address());
            let r = async {
                let device = BluetoothLEDevice::FromBluetoothAddressAsync(address)?.await?;
                let session = GattSession::FromDeviceIdAsync(&device.BluetoothDeviceId()?)?.await?;
                session.SetMaintainConnection(true)?;
                for _ in 0..50 {
                    if session.SessionStatus()? == GattSessionStatus::Active {
                        break;
                    }
                    sleep(Duration::from_millis(200)).await;
                }
                let guid = |u: Uuid| GUID::from_u128(u.as_u128());
                let service = device
                    .GetGattServicesForUuidWithCacheModeAsync(guid(SERVICE), BluetoothCacheMode::Uncached)?
                    .await?
                    .Services()?
                    .GetAt(0)?;
                // uncached, asking for the characteristics hangs; the service request above filled the cache
                let all = service.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Cached)?.await?;
                let find = |u: Uuid| {
                    all.Characteristics()?.into_iter().find(|c| c.Uuid().ok() == Some(guid(u))).ok_or(windows::core::Error::empty())
                };
                let (notify, write) = (find(NOTIFY)?, find(WRITE)?);
                let (tx, rx) = futures::channel::mpsc::unbounded();
                // the handler is not Send: built and handed over without an await in between
                let token = notify.ValueChanged(&TypedEventHandler::new(
                    move |_: Ref<GattCharacteristic>, a: Ref<GattValueChangedEventArgs>| {
                        if let Ok(a) = a.ok() {
                            let r = DataReader::FromBuffer(&a.CharacteristicValue()?)?;
                            let mut v = vec![0; r.UnconsumedBufferLength()? as usize];
                            r.ReadBytes(&mut v)?;
                            let _ = tx.unbounded_send(v);
                        }
                        Ok(())
                    },
                ))?;
                let link = Link { device, session, notify, write, token };
                let status = link
                    .notify
                    .WriteClientCharacteristicConfigurationDescriptorAsync(GattClientCharacteristicConfigurationDescriptorValue::Notify)?
                    .await?;
                windows::core::Result::Ok((link, status, rx))
            };
            let limit = Duration::from_secs(25);
            let (link, status, rx) = timeout(limit, r)
                .await
                .map_err(|_| btleplug::Error::TimedOut(limit))?
                .map_err(|e| btleplug::Error::Other(e.into()))?;
            if status != GattCommunicationStatus::Success {
                return Err(btleplug::Error::NotConnected);
            }
            Ok((link, rx.boxed()))
        }

        pub async fn send(&self, b: &[u8]) -> btleplug::Result<()> {
            // the buffer is not Send: build the operation before the await
            let op = || {
                let w = DataWriter::new()?;
                w.WriteBytes(b)?;
                self.write.WriteValueWithOptionAsync(&w.DetachBuffer()?, GattWriteOption::WriteWithResponse)
            };
            let r = match op() {
                Ok(op) => op.await,
                Err(e) => Err(e),
            };
            match r {
                Ok(GattCommunicationStatus::Success) => Ok(()),
                Ok(_) => Err(btleplug::Error::NotConnected),
                Err(e) => Err(btleplug::Error::Other(e.into())),
            }
        }

        pub async fn alive(&self) -> bool {
            self.device.ConnectionStatus() == Ok(BluetoothConnectionStatus::Connected)
        }
    }

    /// Lets go of the link: without it Windows keeps the device connected on BLE.
    impl Drop for Link {
        fn drop(&mut self) {
            let _ = self.notify.RemoveValueChanged(self.token);
            let _ = self.session.Close();
            let _ = self.device.Close();
        }
    }
}
