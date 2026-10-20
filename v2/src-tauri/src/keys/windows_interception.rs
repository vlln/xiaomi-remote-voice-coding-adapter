//! Optional Windows per-device key mapping through the Interception driver.
//!
//! This backend is compiled only with `--features interception`. It requires the
//! Interception kernel driver and `interception.dll` next to the executable.
//! The default Windows build stays on the SendInput voice-only path.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use interception::raw;
use tracing::{info, warn};

use super::usage;
use super::{BackTarget, KeyMapSettings};

const SCAN_ESC: u16 = 0x01;
const SCAN_BACKSPACE: u16 = 0x0E;

const STATE_E0: u16 = 0x02;

const FILTER_KEY_DOWN: u16 = 0x01;
const FILTER_KEY_UP: u16 = 0x02;

static REMOTE_DEVICE: AtomicI32 = AtomicI32::new(0);

pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

impl Worker {
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

pub fn start(settings: KeyMapSettings) -> Result<Worker, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let handle = std::thread::Builder::new()
        .name("xiaomi-windows-interception".into())
        .spawn(move || run(settings, thread_stop))
        .map_err(|e| format!("cannot start Interception worker: {e}"))?;
    Ok(Worker { stop, handle })
}

unsafe extern "C" fn is_remote(device: i32) -> i32 {
    if device == REMOTE_DEVICE.load(Ordering::Relaxed) {
        1
    } else {
        0
    }
}

fn run(settings: KeyMapSettings, stop: Arc<AtomicBool>) {
    let context = unsafe { raw::interception_create_context() };
    if context.is_null() {
        warn!("Interception driver not installed; Windows key mapping stays voice-only");
        return;
    }

    let remote = match find_remote(context) {
        Some(device) => device,
        None => {
            warn!("Xiaomi Remote not found on an Interception keyboard device");
            unsafe { raw::interception_destroy_context(context) };
            return;
        }
    };

    REMOTE_DEVICE.store(remote, Ordering::Relaxed);
    unsafe {
        raw::interception_set_filter(context, Some(is_remote), FILTER_KEY_DOWN | FILTER_KEY_UP);
    }
    info!(remote, "Interception key mapping active");

    let mut strokes = vec![raw::InterceptionKeyStroke::default(); 32];
    while !stop.load(Ordering::Relaxed) {
        let device = unsafe { raw::interception_wait_with_timeout(context, 100) };
        if device != remote {
            continue;
        }
        let count = unsafe {
            raw::interception_receive(
                context,
                device,
                strokes.as_mut_ptr() as *mut raw::InterceptionStroke,
                strokes.len() as u32,
            )
        };
        if count <= 0 {
            continue;
        }

        let mut output = Vec::with_capacity(count as usize);
        for stroke in strokes.iter().take(count as usize) {
            if let Some(stroke) = translate(*stroke, &settings) {
                output.push(stroke);
            }
        }
        if !output.is_empty() {
            unsafe {
                raw::interception_send(
                    context,
                    device,
                    output.as_ptr() as *const raw::InterceptionStroke,
                    output.len() as u32,
                );
            }
        }
    }

    REMOTE_DEVICE.store(0, Ordering::Relaxed);
    unsafe { raw::interception_destroy_context(context) };
}

fn find_remote(context: raw::InterceptionContext) -> Option<i32> {
    let mut buffer = [0u16; 256];
    for device in 1..=10 {
        if unsafe { raw::interception_is_invalid(device) } != 0 {
            continue;
        }
        let len = unsafe {
            raw::interception_get_hardware_id(
                context,
                device,
                buffer.as_mut_ptr() as *mut _,
                (buffer.len() * 2) as u32,
            )
        } as usize;
        if len == 0 {
            continue;
        }
        let units = (len / 2).min(buffer.len());
        let hardware_id = String::from_utf16_lossy(&buffer[..units]).to_uppercase();
        if hardware_id.contains("VID_2717") && hardware_id.contains("PID_32B8") {
            return Some(device);
        }
    }
    None
}

fn translate(
    mut stroke: raw::InterceptionKeyStroke,
    settings: &KeyMapSettings,
) -> Option<raw::InterceptionKeyStroke> {
    let is_e0 = stroke.state & STATE_E0 != 0;
    let voice = usage::usage_to_scancode(usage::DEFAULT_VOICE_USAGE);
    let home = usage::usage_to_scancode(usage::DEFAULT_HOME_USAGE);
    let tv = usage::usage_to_scancode(usage::DEFAULT_TV_USAGE);
    let back = usage::usage_to_scancode(usage::DEFAULT_BACK_USAGE);

    if settings.voice_to_right_option
        && voice.is_some_and(|(code, e0)| code == stroke.code && e0 == is_e0)
    {
        // The ATVV engine injects the right Alt modifier through SendInput.
        return None;
    }

    let custom_target = settings.custom.iter().find_map(|binding| {
        if !binding.enabled {
            return None;
        }
        let (source, source_e0) = usage::usage_to_scancode(binding.source.usage)?;
        let (target, target_e0) = usage::usage_to_scancode(binding.target.usage)?;
        (source == stroke.code && source_e0 == is_e0).then_some((target, target_e0))
    });
    if let Some((code, e0)) = custom_target {
        stroke.code = code;
        stroke.state &= !(STATE_E0 | 0x04);
        if e0 {
            stroke.state |= STATE_E0;
        }
        return Some(stroke);
    }

    let target = if settings.home_to_escape
        && home.is_some_and(|(code, e0)| code == stroke.code && e0 == is_e0)
    {
        Some(usage::usage_to_scancode(settings.home_target.usage))
    } else if settings.tv_to_backspace
        && tv.is_some_and(|(code, e0)| code == stroke.code && e0 == is_e0)
    {
        Some(usage::usage_to_scancode(settings.tv_target.usage))
    } else if settings.back_target != BackTarget::None
        && back.is_some_and(|(code, e0)| code == stroke.code && e0 == is_e0)
    {
        Some(usage::usage_to_scancode(settings.back_target_key.usage))
    } else {
        None
    };

    let Some(Some((code, e0))) = target else {
        return Some(stroke);
    };

    stroke.code = code;
    stroke.state &= !(STATE_E0 | 0x04);
    if e0 {
        stroke.state |= STATE_E0;
    }
    Some(stroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(code: u16, state: u16) -> raw::InterceptionKeyStroke {
        raw::InterceptionKeyStroke {
            code,
            state,
            information: 0,
        }
    }

    #[test]
    fn drops_voice_f5() {
        let settings = KeyMapSettings::default();
        assert!(translate(stroke(0x3F, 0), &settings).is_none());
    }

    #[test]
    fn maps_extended_home_to_escape() {
        let settings = KeyMapSettings::default();
        let out = translate(stroke(0x47, STATE_E0), &settings).unwrap();
        assert_eq!(out.code, SCAN_ESC);
        assert_eq!(out.state, 0);
    }

    #[test]
    fn maps_tv_grave_to_backspace() {
        let settings = KeyMapSettings::default();
        let out = translate(stroke(0x29, 0), &settings).unwrap();
        assert_eq!(out.code, SCAN_BACKSPACE);
    }

    #[test]
    fn passes_other_keys_through() {
        let settings = KeyMapSettings::default();
        let out = translate(stroke(0x1E, 0), &settings).unwrap();
        assert_eq!(out.code, 0x1E);
    }
}
