//! Emits the right-Option modifier from ATVV voice-key events.
//!
//! The remote's voice key is more reliable over the ATVV control channel than
//! as a HID key: the HID F5 event is sometimes absent or filtered. The session
//! engine therefore drives the modifier directly when it sees `0x08`
//! (voice-key down) and the end of the session.
//!
//! On macOS this posts a `flagsChanged` event for keycode 61. Other platforms
//! are no-ops until their native modifier backends land.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static DOWN: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        set(false);
    }
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

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
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType,
    };
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

#[cfg(not(target_os = "macos"))]
fn post(_down: bool) {}
