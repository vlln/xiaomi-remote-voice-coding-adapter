//! Linux device-level key mapping: evdev grab + uinput passthrough.
//!
//! The remote's HID usage codes arrive as Linux key codes. We grab its event
//! node, translate a handful of keys, and re-emit everything through a virtual
//! keyboard, so Home/TV/Back no longer reach the desktop with their original
//! codes. The voice key is dropped here and emitted by the ATVV session engine.
//!
//! Requires read access to `/dev/input/event*` and write access to
//! `/dev/uinput` (input group / udev rule).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, BusType, Device, EventType, InputEvent, InputId, KeyCode};
use parking_lot::Mutex;
use tracing::{debug, info, warn};

use super::usage;
use super::voice_modifier;
use super::{BackTarget, KeyMapSettings, KeyMapper};

const VENDOR: u16 = 0x2717;
const PRODUCT: u16 = 0x32B8;

struct Worker {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

pub struct LinuxKeyMapper {
    worker: Mutex<Option<Worker>>,
}

impl LinuxKeyMapper {
    pub fn new() -> Self {
        Self {
            worker: Mutex::new(None),
        }
    }

    fn stop_worker(&self) {
        let worker = self.worker.lock().take();
        if let Some(worker) = worker {
            worker.stop.store(true, Ordering::Relaxed);
            let _ = worker.handle.join();
        }
    }
}

impl Default for LinuxKeyMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyMapper for LinuxKeyMapper {
    fn backend(&self) -> &'static str {
        "Linux evdev/uinput"
    }

    fn supported(&self) -> bool {
        true
    }

    fn apply(&self, settings: &KeyMapSettings) -> Result<(), String> {
        self.stop_worker();
        let voice_alt = settings.voice_target.usage == usage::DEFAULT_VOICE_TARGET;
        voice_modifier::set_enabled(settings.voice_to_right_option && voice_alt);
        if settings.is_empty() {
            return Ok(());
        }

        let settings = settings.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::Builder::new()
            .name("xiaomi-linux-keymap".into())
            .spawn(move || run_worker(settings, thread_stop))
            .map_err(|e| format!("cannot start Linux key mapper: {e}"))?;
        *self.worker.lock() = Some(Worker { stop, handle });
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        self.stop_worker();
        voice_modifier::set(false);
        Ok(())
    }
}

fn run_worker(settings: KeyMapSettings, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        let Some((path, mut device)) = find_remote() else {
            debug!("xiaomi remote evdev node not found; retrying");
            sleep_stop(&stop, Duration::from_secs(2));
            continue;
        };
        info!(path = %path.display(), "grabbing Xiaomi Remote evdev node");
        if let Err(err) = device.grab() {
            warn!(%err, "cannot grab remote (input group/permissions?)");
            sleep_stop(&stop, Duration::from_secs(5));
            continue;
        }

        let mut virtual_device = match build_virtual_keyboard(&device) {
            Ok(device) => device,
            Err(err) => {
                warn!(%err, "cannot create uinput keyboard");
                let _ = device.ungrab();
                sleep_stop(&stop, Duration::from_secs(5));
                continue;
            }
        };

        set_nonblocking(&device);
        info!("Linux key mapping active");

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match device.fetch_events() {
                Ok(events) => {
                    let events: Vec<InputEvent> = events.collect();
                    let translated = translate(events, &settings);
                    if !translated.is_empty() {
                        if let Err(err) = virtual_device.emit(&translated) {
                            warn!(%err, "uinput emit failed");
                            break;
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => {
                    warn!(%err, "evdev read failed; reconnecting");
                    break;
                }
            }
        }

        let _ = device.ungrab();
        drop(virtual_device);
        debug!("Linux key mapping worker disconnected");
        sleep_stop(&stop, Duration::from_millis(500));
    }
}

fn find_remote() -> Option<(std::path::PathBuf, Device)> {
    for (path, device) in evdev::enumerate() {
        let id = device.input_id();
        if id.vendor() == VENDOR && id.product() == PRODUCT {
            let is_keyboard = device.supported_keys().is_some_and(|keys| {
                keys.contains(KeyCode::KEY_F5) || keys.contains(KeyCode::KEY_HOME)
            });
            if is_keyboard {
                return Some((path, device));
            }
        }
    }
    None
}

fn build_virtual_keyboard(device: &Device) -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    if let Some(supported) = device.supported_keys() {
        for key in supported.iter() {
            keys.insert(key);
        }
    }
    keys.insert(KeyCode::KEY_ESC);
    keys.insert(KeyCode::KEY_BACKSPACE);
    keys.insert(KeyCode::KEY_RIGHTALT);

    VirtualDevice::builder()?
        .name("Xiaomi Remote Virtual Keyboard")
        .input_id(InputId::new(BusType::BUS_USB, VENDOR, PRODUCT, 1))
        .with_keys(&keys)?
        .build()
}

fn translate(events: Vec<InputEvent>, settings: &KeyMapSettings) -> Vec<InputEvent> {
    let voice = KeyCode::KEY_F5.0;
    let home = KeyCode::KEY_HOME.0;
    let tv = KeyCode::KEY_GRAVE.0;
    let back = KeyCode::KEY_BACK.0;

    let home_target =
        usage::usage_to_evdev(settings.home_target.usage).unwrap_or(KeyCode::KEY_ESC.0);
    let tv_target =
        usage::usage_to_evdev(settings.tv_target.usage).unwrap_or(KeyCode::KEY_BACKSPACE.0);
    let back_target =
        usage::usage_to_evdev(settings.back_target_key.usage).unwrap_or(KeyCode::KEY_BACKSPACE.0);
    let voice_target =
        usage::usage_to_evdev(settings.voice_target.usage).unwrap_or(KeyCode::KEY_RIGHTALT.0);
    let menu = usage::usage_to_evdev(usage::DEFAULT_MENU_USAGE).unwrap_or(KeyCode::KEY_MENU.0);
    let menu_target =
        usage::usage_to_evdev(settings.menu_target.usage).unwrap_or(KeyCode::KEY_MENU.0);

    let mut output = Vec::with_capacity(events.len());
    for event in events {
        if event.event_type() == EventType::KEY {
            let code = event.code();
            let value = event.value();

            if settings.voice_to_right_option && code == voice {
                if voice_target == KeyCode::KEY_RIGHTALT.0 {
                    // The ATVV engine emits the right-Alt modifier.
                    continue;
                }
                output.push(key_event(voice_target, value));
                continue;
            }
            if settings.home_to_escape && code == home {
                output.push(key_event(home_target, value));
                continue;
            }
            if settings.tv_to_backspace && code == tv {
                output.push(key_event(tv_target, value));
                continue;
            }
            if settings.back_target != BackTarget::None
                && (code == back || code == KeyCode::KEY_HOMEPAGE.0)
            {
                output.push(key_event(back_target, value));
                continue;
            }
            if settings.menu_to_context && code == menu {
                output.push(key_event(menu_target, value));
                continue;
            }
        }
        output.push(event);
    }
    output
}

fn key_event(code: u16, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code, value)
}

fn set_nonblocking(device: &Device) {
    use std::os::fd::AsRawFd;
    let fd = device.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

fn sleep_stop(stop: &AtomicBool, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeySource;

    fn key(code: KeyCode) -> InputEvent {
        key_event(code.0, 1)
    }

    #[test]
    fn drops_voice_key_when_enabled() {
        let settings = KeyMapSettings::default();
        let out = translate(vec![key(KeyCode::KEY_F5)], &settings);
        assert!(out.is_empty());
    }

    #[test]
    fn maps_home_tv_and_back() {
        let mut settings = KeyMapSettings::default();
        settings.back_target = BackTarget::Backspace;
        settings.back_target_key = KeySource::new(0x29);
        let out = translate(
            vec![
                key(KeyCode::KEY_HOME),
                key(KeyCode::KEY_GRAVE),
                key(KeyCode::KEY_BACK),
                key(KeyCode::KEY_A),
            ],
            &settings,
        );
        assert_eq!(out[0].code(), KeyCode::KEY_ESC.0);
        assert_eq!(out[1].code(), KeyCode::KEY_BACKSPACE.0);
        assert_eq!(out[2].code(), KeyCode::KEY_ESC.0);
        assert_eq!(out[3].code(), KeyCode::KEY_A.0);
    }
}
