//! Press-to-record key capture.
//!
//! macOS uses the existing Accessibility event tap: while recording, the tap
//! swallows keys and reports the next keycode. This keeps the feature on the
//! permission the app already needs, instead of asking for Input Monitoring.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use super::usage;

#[derive(Debug, Clone, Serialize)]
pub struct CapturedKey {
    pub page: u16,
    pub usage: u16,
    pub name: String,
}

static CAPTURE_BUSY: AtomicBool = AtomicBool::new(false);

pub async fn capture_next_key() -> Result<CapturedKey, String> {
    wait_for_key(true).await
}

/// Wait for the next remote key without swallowing it, for testing.
pub async fn observe_next_key() -> Result<CapturedKey, String> {
    wait_for_key(false).await
}

async fn wait_for_key(swallow: bool) -> Result<CapturedKey, String> {
    if CAPTURE_BUSY.swap(true, Ordering::SeqCst) {
        return Err("正在等待按键，请先按下一个键".into());
    }

    let result = async {
        #[cfg(target_os = "macos")]
        {
            let keycode = if swallow {
                crate::keys::macos_tap::capture_next_key(Duration::from_secs(10)).await?
            } else {
                crate::keys::macos_tap::observe_next_key(Duration::from_secs(10)).await?
            };
            let usage = usage::macos_keycode_to_usage(keycode).ok_or_else(|| {
                format!("无法识别这个按键（keycode {keycode}），请换一个标准按键")
            })?;
            Ok(CapturedKey {
                page: 0x07,
                usage,
                name: usage::usage_name(usage),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = swallow;
            Err("当前平台暂不支持按键捕获".into())
        }
    }
    .await;

    CAPTURE_BUSY.store(false, Ordering::SeqCst);
    result
}

pub fn cancel() {
    #[cfg(target_os = "macos")]
    crate::keys::macos_tap::cancel_capture();
}
