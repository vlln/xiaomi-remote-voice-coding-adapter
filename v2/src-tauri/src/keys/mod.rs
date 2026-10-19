//! Device-level key remapping.
//!
//! macOS uses `hidutil`'s `UserKeyMapping` which is scoped to the remote's
//! vendor/product ID and needs no driver. Linux/Windows backends are separated
//! so the UI can report a platform-specific capability instead of failing the
//! whole app.

use serde::{Deserialize, Serialize};

pub mod accessibility;
pub mod voice_modifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackTarget {
    None,
    Backspace,
    Escape,
}

impl Default for BackTarget {
    fn default() -> Self {
        Self::Backspace
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMapSettings {
    pub voice_to_right_option: bool,
    pub home_to_escape: bool,
    pub tv_to_backspace: bool,
    pub back_target: BackTarget,
}

impl Default for KeyMapSettings {
    fn default() -> Self {
        Self {
            voice_to_right_option: true,
            home_to_escape: true,
            tv_to_backspace: true,
            back_target: BackTarget::Backspace,
        }
    }
}

impl KeyMapSettings {
    pub fn is_empty(&self) -> bool {
        !self.voice_to_right_option
            && !self.home_to_escape
            && !self.tv_to_backspace
            && self.back_target == BackTarget::None
    }
}

pub trait KeyMapper: Send + Sync {
    fn backend(&self) -> &'static str;
    fn apply(&self, settings: &KeyMapSettings) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
    fn supported(&self) -> bool;
}

pub fn create() -> Box<dyn KeyMapper> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacKeyMapper)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxKeyMapper::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsKeyMapper)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(unsupported::UnsupportedKeyMapper)
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos_tap;

#[cfg(target_os = "macos")]
mod macos {
    use std::process::Command;

    use tracing::{debug, info, warn};

    use super::accessibility;
    use super::macos_tap;
    use super::voice_modifier;

    use super::{BackTarget, KeyMapSettings, KeyMapper};

    const VENDOR_ID: u32 = 0x2717;
    const PRODUCT_ID: u32 = 0x32B8;

    pub struct MacKeyMapper;

    impl KeyMapper for MacKeyMapper {
        fn backend(&self) -> &'static str {
            if macos_tap::is_active() {
                "macOS event tap"
            } else {
                "macOS hidutil"
            }
        }

        fn supported(&self) -> bool {
            std::path::Path::new("/usr/bin/hidutil").exists()
        }

        fn apply(&self, settings: &KeyMapSettings) -> Result<(), String> {
            if settings.is_empty() {
                return self.clear();
            }
            if settings.voice_to_right_option && !accessibility::is_trusted() {
                accessibility::request();
                warn!(
                    "Accessibility permission is required for a clean right Option;                      falling back to hidutil F5+Option until it is granted"
                );
            }
            voice_modifier::set_enabled(settings.voice_to_right_option);
            let tap_active = settings.voice_to_right_option && macos_tap::ensure_tap();
            let mappings = build_mappings(settings, tap_active);
            let json = serde_json::json!({ "UserKeyMapping": mappings }).to_string();
            let matching = format!(r#"{{"VendorID":{VENDOR_ID},"ProductID":{PRODUCT_ID}}}"#);
            info!(mappings = %json, "applying macOS key mapping");
            let output = Command::new("/usr/bin/hidutil")
                .args(["property", "--matching", &matching, "--set", &json])
                .output()
                .map_err(|e| format!("failed to run hidutil: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "hidutil failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            debug!("macOS key mapping applied");
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            voice_modifier::set(false);
            let matching = format!(r#"{{"VendorID":{VENDOR_ID},"ProductID":{PRODUCT_ID}}}"#);
            let output = Command::new("/usr/bin/hidutil")
                .args([
                    "property",
                    "--matching",
                    &matching,
                    "--set",
                    r#"{"UserKeyMapping":[]}"#,
                ])
                .output()
                .map_err(|e| format!("failed to run hidutil: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "hidutil clear failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            Ok(())
        }
    }

    fn build_mappings(settings: &KeyMapSettings, tap_active: bool) -> Vec<serde_json::Value> {
        let mut mappings = Vec::new();
        // With the event tap active, the voice key is driven by the ATVV
        // session engine and F5 is swallowed. Without Accessibility, fall back
        // to the old hidutil F5 -> right Option mapping.
        if settings.voice_to_right_option && !tap_active {
            mappings.push(mapping(0x3E, 0xE6));
        }

        if settings.home_to_escape {
            mappings.push(mapping(0x4A, 0x29)); // Home -> Escape
        }
        if settings.tv_to_backspace {
            mappings.push(mapping(0x35, 0x2A)); // `~ (TV) -> Backspace
        }
        match settings.back_target {
            BackTarget::None => {}
            BackTarget::Backspace => mappings.push(mapping(0xF1, 0x2A)),
            BackTarget::Escape => mappings.push(mapping(0xF1, 0x29)),
        }
        mappings
    }

    fn mapping(src_usage: u32, dst_usage: u32) -> serde_json::Value {
        serde_json::json!({
            "HIDKeyboardModifierMappingSrc": 0x7000_0000u32 | src_usage,
            "HIDKeyboardModifierMappingDst": 0x7000_0000u32 | dst_usage,
        })
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::voice_modifier;
    use super::{KeyMapSettings, KeyMapper};

    pub struct WindowsKeyMapper;

    impl KeyMapper for WindowsKeyMapper {
        fn backend(&self) -> &'static str {
            "Windows SendInput (voice)"
        }

        fn supported(&self) -> bool {
            true
        }

        fn apply(&self, settings: &KeyMapSettings) -> Result<(), String> {
            // Voice push-to-talk works via SendInput; Home/TV/back remapping
            // still needs a per-device HID hook/driver.
            voice_modifier::set_enabled(settings.voice_to_right_option);
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            voice_modifier::set(false);
            Ok(())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod unsupported {
    use super::{KeyMapSettings, KeyMapper};

    pub struct UnsupportedKeyMapper;

    impl KeyMapper for UnsupportedKeyMapper {
        fn backend(&self) -> &'static str {
            "unsupported"
        }

        fn supported(&self) -> bool {
            false
        }

        fn apply(&self, _settings: &KeyMapSettings) -> Result<(), String> {
            Err("key mapping is not supported on this platform".into())
        }

        fn clear(&self) -> Result<(), String> {
            Ok(())
        }
    }
}
