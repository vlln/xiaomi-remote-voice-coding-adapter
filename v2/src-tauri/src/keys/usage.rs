//! Canonical key identifiers.
//!
//! Bindings are stored as USB HID keyboard usages (usage page 0x07), because
//! that is what macOS `hidutil` and the remote's HID reports use. Linux and
//! Windows backends translate the usage to their native key/scan code.

/// Friendly names for the keys a remote is likely to expose.
pub fn usage_name(usage: u16) -> String {
    let name = match usage {
        0x04..=0x1D => {
            const LETTERS: [&str; 26] = [
                "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
                "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
            ];
            return LETTERS[(usage - 0x04) as usize].to_string();
        }
        0x1E..=0x26 => return ((usage - 0x1D) as u8).to_string(),
        0x27 => "0",
        0x28 => "Return",
        0x29 => "Esc",
        0x2A => "Backspace",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "-",
        0x2E => "=",
        0x2F => "[",
        0x30 => "]",
        0x31 => "\\",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",
        0x39 => "Caps Lock",
        0x3A..=0x45 => {
            return format!("F{}", usage - 0x39);
        }
        0x46 => "Print Screen",
        0x47 => "Scroll Lock",
        0x48 => "Pause",
        0x49 => "Insert",
        0x4A => "Home",
        0x4B => "Page Up",
        0x4C => "Delete",
        0x4D => "End",
        0x4E => "Page Down",
        0x4F => "Right",
        0x50 => "Left",
        0x51 => "Down",
        0x52 => "Up",
        0x53 => "Num Lock",
        0xE0 => "Left Control",
        0xE1 => "Left Shift",
        0xE2 => "Left Option",
        0xE3 => "Left Command",
        0xE4 => "Right Control",
        0xE5 => "Right Shift",
        0xE6 => "Right Option",
        0xE7 => "Right Command",
        0x65 => "Menu",
        0x66 => "Power",
        0x7F => "Mute",
        0x80 => "Volume Up",
        0x81 => "Volume Down",
        0xF1 => "Back (AC Back)",
        _ => return format!("Key 0x{usage:02X}"),
    };
    name.to_string()
}

/// macOS virtual keycode for a HID usage, used by the event tap to suppress
/// the voice key while a session is active.
pub fn usage_to_macos_keycode(usage: u16) -> Option<u16> {
    let code = match usage {
        0x04 => 0,  // A
        0x05 => 11, // B
        0x06 => 8,  // C
        0x07 => 2,  // D
        0x08 => 14, // E
        0x09 => 3,  // F
        0x0A => 5,  // G
        0x0B => 4,  // H
        0x0C => 34, // I
        0x0D => 38, // J
        0x0E => 40, // K
        0x0F => 37, // L
        0x10 => 46, // M
        0x11 => 45, // N
        0x12 => 31, // O
        0x13 => 35, // P
        0x14 => 12, // Q
        0x15 => 15, // R
        0x16 => 1,  // S
        0x17 => 17, // T
        0x18 => 32, // U
        0x19 => 9,  // V
        0x1A => 13, // W
        0x1B => 7,  // X
        0x1C => 16, // Y
        0x1D => 6,  // Z
        0x1E => 18,
        0x1F => 19,
        0x20 => 20,
        0x21 => 21,
        0x22 => 23,
        0x23 => 22,
        0x24 => 26,
        0x25 => 28,
        0x26 => 25,
        0x27 => 29,
        0x28 => 36, // Return
        0x29 => 53, // Esc
        0x2A => 51, // Backspace
        0x2B => 48, // Tab
        0x2C => 49, // Space
        0x35 => 50, // `
        0x3A => 122,
        0x3B => 120,
        0x3C => 99,
        0x3D => 118,
        0x3E => 96, // F5 (voice default)
        0x3F => 97,
        0x40 => 98,
        0x41 => 100,
        0x42 => 101,
        0x43 => 109,
        0x44 => 103,
        0x45 => 111,
        0x4A => 115, // Home
        0x4B => 116,
        0x4C => 117,
        0x4D => 119,
        0x4E => 121,
        0x4F => 124,
        0x50 => 123,
        0x51 => 125,
        0x52 => 126,
        0xE0 => 59,
        0xE1 => 56,
        0xE2 => 58,
        0xE3 => 55,
        0xE4 => 62,
        0xE5 => 60,
        0xE6 => 61,
        0xE7 => 54,
        0x65 => 110,
        0x66 => 127,
        0x7F => 74,
        0x80 => 72,
        0x81 => 73,
        _ => return None,
    };
    Some(code)
}

/// Linux evdev key code for a HID usage.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn usage_to_evdev(usage: u16) -> Option<u16> {
    let code = match usage {
        0x04 => 30,
        0x05 => 48,
        0x06 => 46,
        0x07 => 32,
        0x08 => 18,
        0x09 => 33,
        0x0A => 34,
        0x0B => 35,
        0x0C => 23,
        0x0D => 36,
        0x0E => 37,
        0x0F => 38,
        0x10 => 50,
        0x11 => 49,
        0x12 => 24,
        0x13 => 25,
        0x14 => 16,
        0x15 => 19,
        0x16 => 31,
        0x17 => 20,
        0x18 => 22,
        0x19 => 47,
        0x1A => 17,
        0x1B => 45,
        0x1C => 21,
        0x1D => 44,
        0x1E => 2,
        0x1F => 3,
        0x20 => 4,
        0x21 => 5,
        0x22 => 6,
        0x23 => 7,
        0x24 => 8,
        0x25 => 9,
        0x26 => 10,
        0x27 => 11,
        0x28 => 28,
        0x29 => 1,
        0x2A => 14,
        0x2B => 15,
        0x2C => 57,
        0x35 => 41,
        0x3A..=0x43 => 59 + (usage - 0x3A),
        0x44 => 87,
        0x45 => 88,
        0x49 => 110,
        0x4A => 102,
        0x4B => 104,
        0x4C => 111,
        0x4D => 107,
        0x4E => 109,
        0x4F => 106,
        0x50 => 105,
        0x51 => 108,
        0x52 => 103,
        0xF1 => 158,
        0xE0 => 29,
        0xE1 => 42,
        0xE2 => 56,
        0xE3 => 125,
        0xE4 => 97,
        0xE5 => 54,
        0xE6 => 100,
        0xE7 => 126,
        0x65 => 139,
        0x66 => 116,
        0x7F => 113,
        0x80 => 115,
        0x81 => 114,
        _ => return None,
    };
    Some(code)
}

/// Windows set-1 scancode and whether the E0 prefix is required.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn usage_to_scancode(usage: u16) -> Option<(u16, bool)> {
    let value = match usage {
        0x04 => (0x1E, false),
        0x05 => (0x30, false),
        0x06 => (0x2E, false),
        0x07 => (0x20, false),
        0x08 => (0x12, false),
        0x09 => (0x21, false),
        0x0A => (0x22, false),
        0x0B => (0x23, false),
        0x0C => (0x17, false),
        0x0D => (0x24, false),
        0x0E => (0x25, false),
        0x0F => (0x26, false),
        0x10 => (0x32, false),
        0x11 => (0x31, false),
        0x12 => (0x18, false),
        0x13 => (0x19, false),
        0x14 => (0x10, false),
        0x15 => (0x13, false),
        0x16 => (0x1F, false),
        0x17 => (0x14, false),
        0x18 => (0x16, false),
        0x19 => (0x2F, false),
        0x1A => (0x11, false),
        0x1B => (0x2D, false),
        0x1C => (0x15, false),
        0x1D => (0x2C, false),
        0x1E => (0x02, false),
        0x1F => (0x03, false),
        0x20 => (0x04, false),
        0x21 => (0x05, false),
        0x22 => (0x06, false),
        0x23 => (0x07, false),
        0x24 => (0x08, false),
        0x25 => (0x09, false),
        0x26 => (0x0A, false),
        0x27 => (0x0B, false),
        0x28 => (0x1C, false),
        0x29 => (0x01, false),
        0x2A => (0x0E, false),
        0x2B => (0x0F, false),
        0x2C => (0x39, false),
        0x35 => (0x29, false),
        0x3A => (0x3B, false),
        0x3B => (0x3C, false),
        0x3C => (0x3D, false),
        0x3D => (0x3E, false),
        0x3E => (0x3F, false),
        0x3F => (0x40, false),
        0x40 => (0x41, false),
        0x41 => (0x42, false),
        0x42 => (0x43, false),
        0x43 => (0x44, false),
        0x44 => (0x57, false),
        0x45 => (0x58, false),
        0x4A => (0x47, true),
        0x4B => (0x49, true),
        0x4C => (0x53, true),
        0x4D => (0x4F, true),
        0x4E => (0x51, true),
        0x4F => (0x4D, true),
        0x50 => (0x4B, true),
        0x51 => (0x50, true),
        0x52 => (0x48, true),
        0xF1 => (0x6A, true),
        0xE0 => (0x1D, false),
        0xE1 => (0x2A, false),
        0xE2 => (0x38, false),
        0xE3 => (0x5B, true),
        0xE4 => (0x1D, true),
        0xE5 => (0x36, false),
        0xE6 => (0x38, true),
        0xE7 => (0x5C, true),
        0x65 => (0x5D, true),
        0x66 => (0x5E, true),
        0x7F => (0x20, true),
        0x80 => (0x30, true),
        0x81 => (0x2E, true),
        _ => return None,
    };
    Some(value)
}

/// macOS HID usage for the default remote keys, exposed for tests/docs.
pub const DEFAULT_VOICE_USAGE: u16 = 0x3E;
pub const DEFAULT_HOME_USAGE: u16 = 0x4A;
pub const DEFAULT_TV_USAGE: u16 = 0x35;
pub const DEFAULT_BACK_USAGE: u16 = 0xF1;
pub const DEFAULT_MENU_USAGE: u16 = 0x65;
pub const DEFAULT_VOICE_TARGET: u16 = 0xE6;

/// Reverse mapping used when recording a key through the macOS event tap.
pub fn macos_keycode_to_usage(keycode: u16) -> Option<u16> {
    let usage = match keycode {
        0 => 0x04,
        11 => 0x05,
        8 => 0x06,
        2 => 0x07,
        14 => 0x08,
        3 => 0x09,
        5 => 0x0A,
        4 => 0x0B,
        34 => 0x0C,
        38 => 0x0D,
        40 => 0x0E,
        37 => 0x0F,
        46 => 0x10,
        45 => 0x11,
        31 => 0x12,
        35 => 0x13,
        12 => 0x14,
        15 => 0x15,
        1 => 0x16,
        17 => 0x17,
        32 => 0x18,
        9 => 0x19,
        13 => 0x1A,
        7 => 0x1B,
        16 => 0x1C,
        6 => 0x1D,
        18 => 0x1E,
        19 => 0x1F,
        20 => 0x20,
        21 => 0x21,
        23 => 0x22,
        22 => 0x23,
        26 => 0x24,
        28 => 0x25,
        25 => 0x26,
        29 => 0x27,
        36 => 0x28,
        53 => 0x29,
        51 => 0x2A,
        48 => 0x2B,
        49 => 0x2C,
        50 => 0x35,
        122 => 0x3A,
        120 => 0x3B,
        99 => 0x3C,
        118 => 0x3D,
        96 => 0x3E,
        97 => 0x3F,
        98 => 0x40,
        100 => 0x41,
        101 => 0x42,
        109 => 0x43,
        103 => 0x44,
        111 => 0x45,
        115 => 0x4A,
        116 => 0x4B,
        117 => 0x4C,
        119 => 0x4D,
        121 => 0x4E,
        124 => 0x4F,
        123 => 0x50,
        125 => 0x51,
        126 => 0x52,
        54 => 0xE7,
        55 => 0xE3,
        56 => 0xE1,
        57 => 0x39,
        58 => 0xE2,
        59 => 0xE0,
        60 => 0xE5,
        61 => 0xE6,
        62 => 0xE4,
        110 => 0x65,
        127 => 0x66,
        74 => 0x7F,
        72 => 0x80,
        73 => 0x81,
        _ => return None,
    };
    Some(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_maps_common_keycodes() {
        assert_eq!(macos_keycode_to_usage(96), Some(DEFAULT_VOICE_USAGE));
        assert_eq!(macos_keycode_to_usage(115), Some(DEFAULT_HOME_USAGE));
        assert_eq!(macos_keycode_to_usage(50), Some(DEFAULT_TV_USAGE));
        assert_eq!(macos_keycode_to_usage(0), Some(0x04));
    }

    #[test]
    fn names_and_platform_codes() {
        assert_eq!(usage_name(0x4A), "Home");
        assert_eq!(usage_to_evdev(0x3E), Some(63));
        assert_eq!(usage_to_scancode(0x4A), Some((0x47, true)));
    }
}
