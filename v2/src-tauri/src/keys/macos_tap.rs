//! Narrow macOS event tap.
//!
//! It has two jobs:
//! - swallow the configured voice key while an ATVV session is active, so the
//!   target app only receives the right-Option modifier emitted by the session;
//! - while the UI is recording a shortcut, swallow the next key and report its
//!   keycode back to the capture command.
//!
//! The tap requires Accessibility permission. If it cannot be created, callers
//! fall back to the plain `hidutil` mapping.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use tokio::sync::oneshot;

use core_foundation::base::TCFType;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use tracing::{debug, info, warn};

static VOICE_KEYCODE: AtomicU16 = AtomicU16::new(96);
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static CAPTURE_SWALLOW: AtomicBool = AtomicBool::new(true);
static CAPTURE_TX: parking_lot::Mutex<Option<oneshot::Sender<u16>>> = parking_lot::Mutex::new(None);

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

pub fn set_voice_keycode(keycode: u16) {
    VOICE_KEYCODE.store(keycode, Ordering::Relaxed);
}

async fn wait_for_key(timeout: Duration, swallow: bool) -> Result<u16, String> {
    if !is_active() && !ensure_tap() {
        return Err("需要键盘控制权限才能录制按键".into());
    }
    if CAPTURE_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("正在等待按键".into());
    }

    let (tx, rx) = oneshot::channel();
    CAPTURE_SWALLOW.store(swallow, Ordering::SeqCst);
    *CAPTURE_TX.lock() = Some(tx);
    let result = tokio::time::timeout(timeout, rx).await;
    CAPTURE_ACTIVE.store(false, Ordering::SeqCst);
    CAPTURE_TX.lock().take();

    match result {
        Ok(Ok(keycode)) => Ok(keycode),
        Ok(Err(_)) => Err("按键捕获已取消".into()),
        Err(_) => Err("等待按键超时，请重试".into()),
    }
}

pub async fn capture_next_key(timeout: Duration) -> Result<u16, String> {
    // Capture for a binding: swallow the key and allow Escape as a target.
    wait_for_key(timeout, true).await
}

pub async fn observe_next_key(timeout: Duration) -> Result<u16, String> {
    // Test mode: let the key through so the user can see its real effect.
    wait_for_key(timeout, false).await
}

pub fn cancel_capture() {
    CAPTURE_TX.lock().take();
    CAPTURE_ACTIVE.store(false, Ordering::SeqCst);
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
                if CAPTURE_ACTIVE.load(Ordering::Relaxed) {
                    // Swallow the captured key so it does not trigger a
                    // shortcut in the app behind the capture sheet.
                    if matches!(event_type, CGEventType::KeyDown) {
                        if let Some(tx) = CAPTURE_TX.lock().take() {
                            let _ = tx.send(keycode);
                        }
                    }
                    if CAPTURE_SWALLOW.load(Ordering::SeqCst) {
                        return CallbackResult::Drop;
                    }
                    return CallbackResult::Keep;
                }
                let voice_keycode = VOICE_KEYCODE.load(Ordering::Relaxed);
                if voice_keycode == 0 || keycode != voice_keycode {
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
