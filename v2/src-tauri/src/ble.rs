//! BLE transport for the Xiaomi Remote ATVV service.
//!
//! Uses `btleplug`, which maps to CoreBluetooth on macOS, WinRT on Windows and
//! BlueZ on Linux. The protocol/session state lives in `core::atvv`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, RetrievePeripheralsOptions,
    ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, sleep, timeout, MissedTickBehavior};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::core::atvv::{
    Caps, Session, SessionCommand, SessionSnapshot, GET_CAPS, VOICE_AUDIO, VOICE_CONTROL,
    VOICE_SERVICE, VOICE_TX,
};
use crate::core::AudioSink;
use crate::keys::voice_modifier;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Status {
        state: String,
        message: Option<String>,
        device: Option<String>,
    },
    Session(SessionSnapshot),
    Error {
        message: String,
    },
}

pub async fn run(
    output: Arc<dyn AudioSink>,
    events: mpsc::Sender<EngineEvent>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut attempt = 0u32;
    loop {
        let result = connect_and_stream(output.clone(), &events, &mut stop).await;
        match result {
            Ok(()) => {
                attempt = 0;
                let _ = events
                    .send(EngineEvent::Status {
                        state: "idle".into(),
                        message: None,
                        device: None,
                    })
                    .await;
            }
            Err(err) => {
                attempt = attempt.saturating_add(1);
                warn!(%err, attempt, "BLE connection failed");
                let _ = events
                    .send(EngineEvent::Error {
                        message: err.clone(),
                    })
                    .await;
                let _ = events
                    .send(EngineEvent::Status {
                        state: "error".into(),
                        message: Some(err),
                        device: None,
                    })
                    .await;
            }
        }

        let backoff = Duration::from_millis(1_500 + u64::from(attempt.min(4)) * 1_000);
        tokio::select! {
            _ = &mut stop => break,
            _ = sleep(backoff) => {}
        }
    }
    info!("BLE engine stopped");
}

async fn connect_and_stream(
    output: Arc<dyn AudioSink>,
    events: &mpsc::Sender<EngineEvent>,
    stop: &mut oneshot::Receiver<()>,
) -> Result<(), String> {
    let _ = events
        .send(EngineEvent::Status {
            state: "scanning".into(),
            message: None,
            device: None,
        })
        .await;

    let manager = Manager::new()
        .await
        .map_err(|e| format!("Bluetooth manager: {e}"))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| format!("Bluetooth adapters: {e}"))?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| "No Bluetooth adapter found".to_string())?;

    let service_uuid = Uuid::parse_str(VOICE_SERVICE).expect("valid service uuid");

    // On macOS the remote is already connected as a HID keyboard, so it does
    // not advertise. CoreBluetooth's retrieveConnectedPeripheralsWithServices
    // (exposed by btleplug 0.13) is the only way to reach it.
    let retrieved = match adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![service_uuid]),
        })
        .await
    {
        Ok(peripherals) => {
            debug!(count = peripherals.len(), "retrieved connected peripherals");
            peripherals
        }
        Err(err) => {
            debug!(%err, "retrieve_peripherals failed; falling back to scan");
            Vec::new()
        }
    };

    let peripheral = if let Some(peripheral) = pick_peripheral(&retrieved, &service_uuid).await {
        info!("using already-connected remote");
        peripheral
    } else {
        adapter
            .start_scan(ScanFilter {
                services: vec![service_uuid],
            })
            .await
            .map_err(|e| format!("start scan: {e}"))?;

        let peripheral = loop {
            if let Some(peripheral) = find_remote(&adapter, &service_uuid).await? {
                break peripheral;
            }
            tokio::select! {
                _ = &mut *stop => {
                    let _ = adapter.stop_scan().await;
                    return Ok(());
                }
                _ = sleep(Duration::from_millis(500)) => {}
            }
        };
        let _ = adapter.stop_scan().await;
        peripheral
    };

    let name = peripheral
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|p| p.local_name)
        .unwrap_or_else(|| "Xiaomi Remote".into());
    info!(%name, "remote found");
    let _ = events
        .send(EngineEvent::Status {
            state: "connecting".into(),
            message: None,
            device: Some(name.clone()),
        })
        .await;

    if !peripheral.is_connected().await.unwrap_or(false) {
        timeout(Duration::from_secs(15), peripheral.connect())
            .await
            .map_err(|_| "connect timeout".to_string())?
            .map_err(|e| format!("connect: {e}"))?;
    }
    peripheral
        .discover_services()
        .await
        .map_err(|e| format!("discover services: {e}"))?;

    let (tx, audio, control) = find_characteristics(&peripheral)?;
    peripheral
        .subscribe(&audio)
        .await
        .map_err(|e| format!("subscribe audio: {e}"))?;
    peripheral
        .subscribe(&control)
        .await
        .map_err(|e| format!("subscribe control: {e}"))?;

    let mut notifications = peripheral
        .notifications()
        .await
        .map_err(|e| format!("notifications: {e}"))?;

    let mut session = Session::new(output);
    session.set_connected(true);
    let _ = events
        .send(EngineEvent::Status {
            state: "connected".into(),
            message: None,
            device: Some(name.clone()),
        })
        .await;

    write_packet(&peripheral, &tx, &GET_CAPS).await?;
    info!("GET_CAPS sent");

    let mut watchdog = interval(Duration::from_millis(500));
    watchdog.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = &mut *stop => {
                session.on_disconnect();
                voice_modifier::set(false);
                cleanup(&peripheral, &audio, &control).await;
                return Ok(());
            }
            notification = notifications.next() => {
                match notification {
                    Some(data) => {
                        let uuid = data.uuid;
                        if uuid == audio.uuid {
                            session.on_audio(&data.value);
                            let _ = events.send(EngineEvent::Session(session.snapshot())).await;
                        } else if uuid == control.uuid {
                            let commands = session.on_control(&data.value);
                            // Drive right Option from the actual mic session.
                            // The remote often starts a session with 0x04
                            // without sending a separate 0x08 key event.
                            voice_modifier::set(session.snapshot().mic_open);
                            for command in commands {
                                match command {
                                    SessionCommand::MicOpen(payload) => {
                                        if let Err(err) = write_packet(&peripheral, &tx, &payload).await {
                                            session.mic_open_failed();
                                            voice_modifier::set(false);
                                            let _ = events.send(EngineEvent::Error { message: err }).await;
                                        }
                                    }
                                    SessionCommand::MicClose(payload) => {
                                        if let Err(err) = write_packet(&peripheral, &tx, &payload).await {
                                            let _ = events.send(EngineEvent::Error { message: err }).await;
                                        }
                                    }
                                }
                            }
                            let _ = events.send(EngineEvent::Session(session.snapshot())).await;
                        }
                    }
                    None => {
                        session.on_disconnect();
                        voice_modifier::set(false);
                        cleanup(&peripheral, &audio, &control).await;
                        return Ok(());
                    }
                }
            }
            _ = watchdog.tick() => {
                let commands = session.tick(Instant::now());
                voice_modifier::set(session.snapshot().mic_open);
                for command in commands {
                    if let SessionCommand::MicClose(payload) = command {
                        if let Err(err) = write_packet(&peripheral, &tx, &payload).await {
                            let _ = events.send(EngineEvent::Error { message: err }).await;
                        }
                    }
                }
                let _ = events.send(EngineEvent::Session(session.snapshot())).await;
            }
        }
    }
}

async fn pick_peripheral(peripherals: &[Peripheral], service: &Uuid) -> Option<Peripheral> {
    for peripheral in peripherals {
        if let Ok(Some(properties)) = peripheral.properties().await {
            let name = properties
                .local_name
                .as_deref()
                .unwrap_or_default()
                .to_lowercase();
            let looks_like_remote = name.contains("小米")
                || name.contains("xiaomi")
                || name.contains("mi rc")
                || name.contains("mi remote");
            if properties.services.contains(service) || looks_like_remote {
                return Some(peripheral.clone());
            }
        }
    }
    peripherals.first().cloned()
}

async fn find_remote(adapter: &Adapter, service: &Uuid) -> Result<Option<Peripheral>, String> {
    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| format!("list peripherals: {e}"))?;
    for peripheral in peripherals {
        let properties = peripheral
            .properties()
            .await
            .map_err(|e| format!("peripheral properties: {e}"))?;
        let Some(properties) = properties else {
            continue;
        };
        let name = properties
            .local_name
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        let looks_like_remote = name.contains("小米")
            || name.contains("xiaomi")
            || name.contains("mi rc")
            || name.contains("mi remote");
        if properties.services.contains(service) || looks_like_remote {
            if peripheral.is_connected().await.unwrap_or(false) {
                debug!("using already-connected remote {name}");
            }
            return Ok(Some(peripheral));
        }
    }
    Ok(None)
}

fn find_characteristics(
    peripheral: &Peripheral,
) -> Result<(Characteristic, Characteristic, Characteristic), String> {
    let tx_uuid = Uuid::parse_str(VOICE_TX).expect("valid tx uuid");
    let audio_uuid = Uuid::parse_str(VOICE_AUDIO).expect("valid audio uuid");
    let control_uuid = Uuid::parse_str(VOICE_CONTROL).expect("valid control uuid");

    let characteristics = peripheral.characteristics();
    let mut tx = None;
    let mut audio = None;
    let mut control = None;
    for characteristic in characteristics {
        if characteristic.uuid == tx_uuid {
            tx = Some(characteristic);
        } else if characteristic.uuid == audio_uuid {
            audio = Some(characteristic);
        } else if characteristic.uuid == control_uuid {
            control = Some(characteristic);
        }
    }
    Ok((
        tx.ok_or_else(|| "TX characteristic missing".to_string())?,
        audio.ok_or_else(|| "Audio characteristic missing".to_string())?,
        control.ok_or_else(|| "Control characteristic missing".to_string())?,
    ))
}

async fn write_packet(
    peripheral: &Peripheral,
    characteristic: &Characteristic,
    payload: &[u8],
) -> Result<(), String> {
    let write_type = if characteristic.properties.contains(CharPropFlags::WRITE) {
        WriteType::WithResponse
    } else {
        WriteType::WithoutResponse
    };
    peripheral
        .write(characteristic, payload, write_type)
        .await
        .map_err(|e| format!("BLE write: {e}"))
}

async fn cleanup(peripheral: &Peripheral, audio: &Characteristic, control: &Characteristic) {
    let _ = peripheral.unsubscribe(audio).await;
    let _ = peripheral.unsubscribe(control).await;
    let _ = peripheral.disconnect().await;
}

/// Capability probe used by the UI before starting the engine.
pub fn known_caps() -> Option<Caps> {
    None
}
