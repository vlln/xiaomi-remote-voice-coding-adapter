//! Device-level key remapping.
//!
//! Bindings are stored as USB HID usages and can be recorded from the UI.
//! macOS applies them with `hidutil`'s `UserKeyMapping` scoped to the remote's
//! vendor/product ID; Linux uses evdev grab + uinput; Windows uses SendInput and
//! an optional Interception backend.

use serde::{Deserialize, Serialize};

pub mod accessibility;
pub mod capture;
pub mod usage;
pub mod voice_modifier;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeySource {
    pub usage: u16,
    #[serde(default)]
    pub name: String,
}

impl KeySource {
    pub fn new(usage: u16) -> Self {
        Self {
            usage,
            name: usage::usage_name(usage),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.usage != 0
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomBinding {
    pub source: KeySource,
    pub target: KeySource,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMapSettings {
    pub voice_to_right_option: bool,
    pub home_to_escape: bool,
    pub tv_to_backspace: bool,
    pub back_target: BackTarget,
    /// Target for the fixed Home button.
    #[serde(default = "default_home_target")]
    pub home_target: KeySource,
    /// Target for the fixed TV button.
    #[serde(default = "default_tv_target")]
    pub tv_target: KeySource,
    /// Target for the fixed Back button.
    #[serde(default = "default_back_target_key")]
    pub back_target_key: KeySource,
    #[serde(default)]
    pub custom: Vec<CustomBinding>,
}

fn default_home_target() -> KeySource {
    KeySource::new(0x29) // Escape
}

fn default_tv_target() -> KeySource {
    KeySource::new(0x2A) // Backspace
}

fn default_back_target_key() -> KeySource {
    KeySource::new(0x2A) // Backspace
}

impl Default for KeyMapSettings {
    fn default() -> Self {
        Self {
            voice_to_right_option: true,
            home_to_escape: true,
            tv_to_backspace: true,
            back_target: BackTarget::Backspace,
            home_target: default_home_target(),
            tv_target: default_tv_target(),
            back_target_key: default_back_target_key(),
            custom: Vec::new(),
        }
    }
}

impl KeyMapSettings {
    pub fn is_empty(&self) -> bool {
        !self.voice_to_right_option
            && !self.home_to_escape
            && !self.tv_to_backspace
            && self.back_target == BackTarget::None
            && self.custom.iter().all(|binding| !binding.enabled)
    }
}

pub trait KeyMapper: Send + Sync {
    fn backend(&self) -> &'static str;
    fn apply(&self, settings: &KeyMapSettings) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
    fn supported(&self) -> bool;
    /// Whether Home/TV/Back remapping works on this platform without extra drivers.
    fn full_mapping(&self) -> bool {
        true
    }
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
        Box::new(windows::WindowsKeyMapper::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(unsupported::UnsupportedKeyMapper)
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(all(target_os = "windows", feature = "interception"))]
mod windows_interception;

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
            let voice_keycode =
                super::usage::usage_to_macos_keycode(super::usage::DEFAULT_VOICE_USAGE)
                    .unwrap_or(0);
            let tap_active = settings.voice_to_right_option && macos_tap::ensure_tap();
            macos_tap::set_voice_keycode(if tap_active { voice_keycode } else { 0 });
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
        const VOICE_SOURCE: u32 = super::usage::DEFAULT_VOICE_USAGE as u32;
        const HOME_SOURCE: u32 = super::usage::DEFAULT_HOME_USAGE as u32;
        const TV_SOURCE: u32 = super::usage::DEFAULT_TV_USAGE as u32;
        const BACK_SOURCE: u32 = super::usage::DEFAULT_BACK_USAGE as u32;

        if settings.voice_to_right_option && !tap_active {
            mappings.push(mapping(VOICE_SOURCE, 0xE6));
        }
        if settings.home_to_escape && settings.home_target.is_valid() {
            mappings.push(mapping(HOME_SOURCE, settings.home_target.usage as u32));
        }
        if settings.tv_to_backspace && settings.tv_target.is_valid() {
            mappings.push(mapping(TV_SOURCE, settings.tv_target.usage as u32));
        }
        if settings.back_target != BackTarget::None && settings.back_target_key.is_valid() {
            mappings.push(mapping(BACK_SOURCE, settings.back_target_key.usage as u32));
        }
        for binding in &settings.custom {
            if binding.enabled && binding.source.is_valid() && binding.target.is_valid() {
                mappings.push(mapping(
                    binding.source.usage as u32,
                    binding.target.usage as u32,
                ));
            }
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
    use super::{BackTarget, KeyMapSettings, KeyMapper};

    pub struct WindowsKeyMapper {
        #[cfg(feature = "interception")]
        worker: parking_lot::Mutex<Option<super::windows_interception::Worker>>,
    }

    impl WindowsKeyMapper {
        pub fn new() -> Self {
            Self {
                #[cfg(feature = "interception")]
                worker: parking_lot::Mutex::new(None),
            }
        }

        #[cfg(feature = "interception")]
        fn stop_worker(&self) {
            if let Some(worker) = self.worker.lock().take() {
                worker.stop();
            }
        }
    }

    impl Default for WindowsKeyMapper {
        fn default() -> Self {
            Self::new()
        }
    }

    impl KeyMapper for WindowsKeyMapper {
        fn backend(&self) -> &'static str {
            if cfg!(feature = "interception") {
                "Windows Interception"
            } else {
                "Windows SendInput (voice)"
            }
        }

        fn supported(&self) -> bool {
            true
        }

        fn full_mapping(&self) -> bool {
            cfg!(feature = "interception")
        }

        fn apply(&self, settings: &KeyMapSettings) -> Result<(), String> {
            voice_modifier::set_enabled(settings.voice_to_right_option);

            #[cfg(feature = "interception")]
            {
                self.stop_worker();
                let wants_remap = settings.home_to_escape
                    || settings.tv_to_backspace
                    || settings.back_target != BackTarget::None;
                if wants_remap {
                    match super::windows_interception::start(settings.clone()) {
                        Ok(worker) => *self.worker.lock() = Some(worker),
                        Err(err) => {
                            tracing::warn!(%err, "cannot start Interception key mapper");
                        }
                    }
                }
            }

            #[cfg(not(feature = "interception"))]
            if settings.home_to_escape
                || settings.tv_to_backspace
                || settings.back_target != BackTarget::None
            {
                tracing::warn!(
                    "Windows Home/TV/Back mapping needs the optional `interception` feature and driver; only voice is active"
                );
            }

            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            #[cfg(feature = "interception")]
            self.stop_worker();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_without_key_sources_get_defaults() {
        let json = r#"{
            "voice_to_right_option": true,
            "home_to_escape": true,
            "tv_to_backspace": true,
            "back_target": "backspace"
        }"#;
        let parsed: KeyMapSettings = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.home_target.usage, 0x29);
        assert_eq!(parsed.tv_target.usage, 0x2A);
        assert_eq!(parsed.back_target_key.usage, 0x2A);
        assert!(!parsed.home_target.name.is_empty());
    }
}
