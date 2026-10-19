//! Minimal macOS event tap used to turn the remote's voice key into a clean
//! right-Option modifier.
//!
//! `hidutil` alone can map F5 to the right-Option usage, but on macOS the
//! resulting event may still carry the F5 keycode. This tap is deliberately
//! narrow: `hidutil` maps only the remote's F5 to F18, then the tap turns F18
//! into a `flagsChanged` event for right Option (keycode 61).
//!
//! The tap requires Accessibility permission. If it cannot be created, callers
//! fall back to the plain `hidutil` F5 -> right Option mapping.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use tracing::{debug, info, warn};

const VOICE_KEYCODE: u16 = 96;

static TAP_ACTIVE: AtomicBool = AtomicBool::new(false);
static TAP_INIT: parking_lot::Mutex<Option<bool>> = parking_lot::Mutex::new(None);
static TAP_PORT: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

pub fn ensure_tap() -> bool {
    if let Some(true) = *TAP_INIT.lock() {
        return true;
    }
    let active = start_tap();
    *TAP_INIT.lock() = Some(active);
    active
}

pub fn is_active() -> bool {
    TAP_ACTIVE.load(Ordering::Relaxed)
}

fn start_tap() -> bool {
    let (ready_tx, ready_rx) = mpsc::sync_channel::<bool>(1);
    let builder = std::thread::Builder::new().name("macos-key-event-tap".into());
    let spawned = builder.spawn(move || {
        let tap = match CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::KeyDown, CGEventType::KeyUp],
            |_proxy, event_type, event| {
                if matches!(
                    event_type,
                    CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                ) {
                    let port = TAP_PORT.load(Ordering::Relaxed) as *mut c_void;
                    if !port.is_null() {
                        unsafe { CGEventTapEnable(port, true) };
                        warn!("macOS key event tap was disabled; re-enabled");
                    }
                    return CallbackResult::Keep;
                }
                if !matches!(event_type, CGEventType::KeyDown | CGEventType::KeyUp) {
                    return CallbackResult::Keep;
                }
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                if keycode != VOICE_KEYCODE {
                    return CallbackResult::Keep;
                }
                if !super::voice_modifier::is_down() {
                    // A plain F5 from a normal keyboard: leave it alone.
                    return CallbackResult::Keep;
                }
                let down = matches!(event_type, CGEventType::KeyDown);
                debug!(down, "suppressing F5 while voice session is active");
                CallbackResult::Drop
            },
        ) {
            Ok(tap) => tap,
            Err(_) => {
                warn!("macOS key event tap could not be created (Accessibility permission?)");
                let _ = ready_tx.send(false);
                return;
            }
        };

        TAP_PORT.store(
            tap.mach_port().as_concrete_TypeRef() as usize,
            Ordering::Relaxed,
        );
        let source = match tap.mach_port().create_runloop_source(0) {
            Ok(source) => source,
            Err(_) => {
                warn!("macOS key event tap runloop source failed");
                let _ = ready_tx.send(false);
                return;
            }
        };
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        info!("macOS key event tap enabled");
        let _ = ready_tx.send(true);
        CFRunLoop::run_current();
        drop(tap);
    });

    if spawned.is_err() {
        return false;
    }

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(active) => {
            TAP_ACTIVE.store(active, Ordering::Relaxed);
            active
        }
        Err(_) => {
            warn!("macOS key event tap did not become ready");
            false
        }
    }
}
