//! macOS Accessibility permission helpers.
//!
//! A global event tap (used to swallow the remote's F5) and synthetic modifier
//! events both require the app to be trusted in System Settings →
//! Privacy & Security → Accessibility.

use std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
}

#[cfg(target_os = "macos")]
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Ask macOS to show the Accessibility consent dialog (once per app).
#[cfg(target_os = "macos")]
pub fn request() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt"),
        CFBoolean::true_value(),
    )]);
    let trusted = unsafe {
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) != 0
    };
    trusted
}

#[cfg(target_os = "macos")]
pub fn open_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status();
}

#[cfg(not(target_os = "macos"))]
pub fn is_trusted() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn open_settings() {}
