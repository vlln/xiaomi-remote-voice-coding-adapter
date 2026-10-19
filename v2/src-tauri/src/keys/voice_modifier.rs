//! Emits the right-Option modifier from ATVV voice-key events.
//!
//! The remote's voice key is more reliable over the ATVV control channel than
//! as a HID key: the HID F5 event is sometimes absent or filtered. The session
//! engine therefore drives the modifier directly when it sees `0x08`
//! (voice-key down) and the end of the session.
//!
//! macOS posts a `flagsChanged` event (keycode 61). Windows uses `SendInput`
//! with a virtual right-menu key; Linux creates a tiny uinput keyboard and
//! emits `KEY_RIGHTALT`.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static DOWN: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        set(false);
    }
}

#[cfg(target_os = "macos")]
pub fn is_down() -> bool {
    DOWN.load(Ordering::Relaxed)
}

pub fn set(down: bool) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if DOWN.swap(down, Ordering::Relaxed) == down {
        return;
    }
    tracing::info!(down, "voice modifier changed");
    post(down);
}

#[cfg(target_os = "macos")]
fn post(down: bool) {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const RIGHT_OPTION_KEYCODE: u16 = 61;
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::Private) else {
        return;
    };
    let Ok(event) = CGEvent::new_keyboard_event(source, RIGHT_OPTION_KEYCODE, down) else {
        return;
    };
    event.set_type(CGEventType::FlagsChanged);
    event.set_flags(if down {
        CGEventFlags::CGEventFlagAlternate
    } else {
        CGEventFlags::empty()
    });
    event.post(CGEventTapLocation::HID);
    tracing::debug!(down, "posted right Option");
}

#[cfg(target_os = "windows")]
fn post(down: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VK_RMENU,
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_RMENU,
                wScan: 0,
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        tracing::warn!("SendInput failed for right Alt");
    }
}

#[cfg(target_os = "linux")]
fn post(down: bool) {
    use std::sync::OnceLock;

    use evdev::uinput::VirtualDevice;
    use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
    use parking_lot::Mutex;

    static DEVICE: OnceLock<Mutex<Option<VirtualDevice>>> = OnceLock::new();

    let device = DEVICE.get_or_init(|| {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_RIGHTALT);
        let created = (|| -> std::io::Result<VirtualDevice> {
            VirtualDevice::builder()?
                .name("Xiaomi Remote Voice Modifier")
                .with_keys(&keys)?
                .build()
        })();
        match created {
            Ok(device) => {
                tracing::info!("uinput voice modifier ready");
                Mutex::new(Some(device))
            }
            Err(err) => {
                tracing::warn!(%err, "cannot create /dev/uinput voice modifier");
                Mutex::new(None)
            }
        }
    });

    if let Some(device) = device.lock().as_mut() {
        let event = InputEvent::new(EventType::KEY.0, KeyCode::KEY_RIGHTALT.0, i32::from(down));
        if let Err(err) = device.emit(&[event]) {
            tracing::warn!(%err, "uinput emit failed");
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn post(_down: bool) {}
