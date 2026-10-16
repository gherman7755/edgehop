//! The neutral key map (`docs/PROTOCOL.md`, "Key codes", and `docs/keymap.csv`).
//!
//! Key events carry the **USB HID usage** (keyboard page 0x07) of the physical key, so the computers need not share
//! a layout or an operating system: each side translates between HID and its own codes at its edge. This module is
//! the Windows side of that translation. The table is checked against `docs/keymap.csv`, the single source both
//! the Swift and the Rust implementations are tested against.

use crate::Modifiers;

/// Value of the `keymap` field of `hello` messages from computers that send and understand HID key codes. Computers
/// that do not announce it use macOS virtual keycodes (protocol 1, `edgehop-link` 0.1.0 and older) and must not be
/// sent keys.
pub const KEYMAP_NAME: &str = "hid";

/// HID usage of the Escape key (the panic chord).
pub const HID_ESCAPE: u16 = 0x29;

/// A Windows scan code (set 1) and whether the key is an extended (`E0`-prefixed) one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKey {
    pub scan: u16,
    pub extended: bool,
}

/// `(HID usage, Windows scan code)`, with `0xE000` added to the scan code of extended keys. Sorted by HID usage.
const TABLE: &[(u16, u16)] = &[
    (0x04, 0x001E), // A
    (0x05, 0x0030), // B
    (0x06, 0x002E), // C
    (0x07, 0x0020), // D
    (0x08, 0x0012), // E
    (0x09, 0x0021), // F
    (0x0A, 0x0022), // G
    (0x0B, 0x0023), // H
    (0x0C, 0x0017), // I
    (0x0D, 0x0024), // J
    (0x0E, 0x0025), // K
    (0x0F, 0x0026), // L
    (0x10, 0x0032), // M
    (0x11, 0x0031), // N
    (0x12, 0x0018), // O
    (0x13, 0x0019), // P
    (0x14, 0x0010), // Q
    (0x15, 0x0013), // R
    (0x16, 0x001F), // S
    (0x17, 0x0014), // T
    (0x18, 0x0016), // U
    (0x19, 0x002F), // V
    (0x1A, 0x0011), // W
    (0x1B, 0x002D), // X
    (0x1C, 0x0015), // Y
    (0x1D, 0x002C), // Z
    (0x1E, 0x0002), // 1
    (0x1F, 0x0003), // 2
    (0x20, 0x0004), // 3
    (0x21, 0x0005), // 4
    (0x22, 0x0006), // 5
    (0x23, 0x0007), // 6
    (0x24, 0x0008), // 7
    (0x25, 0x0009), // 8
    (0x26, 0x000A), // 9
    (0x27, 0x000B), // 0
    (0x28, 0x001C), // Enter
    (0x29, 0x0001), // Escape
    (0x2A, 0x000E), // Backspace
    (0x2B, 0x000F), // Tab
    (0x2C, 0x0039), // Space
    (0x2D, 0x000C), // Minus
    (0x2E, 0x000D), // Equal
    (0x2F, 0x001A), // LeftBracket
    (0x30, 0x001B), // RightBracket
    (0x31, 0x002B), // Backslash
    (0x33, 0x0027), // Semicolon
    (0x34, 0x0028), // Quote
    (0x35, 0x0029), // Grave
    (0x36, 0x0033), // Comma
    (0x37, 0x0034), // Period
    (0x38, 0x0035), // Slash
    (0x39, 0x003A), // CapsLock
    (0x3A, 0x003B), // F1
    (0x3B, 0x003C), // F2
    (0x3C, 0x003D), // F3
    (0x3D, 0x003E), // F4
    (0x3E, 0x003F), // F5
    (0x3F, 0x0040), // F6
    (0x40, 0x0041), // F7
    (0x41, 0x0042), // F8
    (0x42, 0x0043), // F9
    (0x43, 0x0044), // F10
    (0x44, 0x0057), // F11
    (0x45, 0x0058), // F12
    (0x46, 0xE037), // PrintScreen
    (0x47, 0x0046), // ScrollLock
    (0x49, 0xE052), // Insert
    (0x4A, 0xE047), // Home
    (0x4B, 0xE049), // PageUp
    (0x4C, 0xE053), // Delete
    (0x4D, 0xE04F), // End
    (0x4E, 0xE051), // PageDown
    (0x4F, 0xE04D), // Right
    (0x50, 0xE04B), // Left
    (0x51, 0xE050), // Down
    (0x52, 0xE048), // Up
    (0x53, 0x0045), // NumLock
    (0x54, 0xE035), // KeypadDivide
    (0x55, 0x0037), // KeypadMultiply
    (0x56, 0x004A), // KeypadMinus
    (0x57, 0x004E), // KeypadPlus
    (0x58, 0xE01C), // KeypadEnter
    (0x59, 0x004F), // Keypad1
    (0x5A, 0x0050), // Keypad2
    (0x5B, 0x0051), // Keypad3
    (0x5C, 0x004B), // Keypad4
    (0x5D, 0x004C), // Keypad5
    (0x5E, 0x004D), // Keypad6
    (0x5F, 0x0047), // Keypad7
    (0x60, 0x0048), // Keypad8
    (0x61, 0x0049), // Keypad9
    (0x62, 0x0052), // Keypad0
    (0x63, 0x0053), // KeypadDecimal
    (0x64, 0x0056), // NonUsBackslash
    (0x65, 0xE05D), // Menu
    (0x68, 0x0064), // F13
    (0x69, 0x0065), // F14
    (0x6A, 0x0066), // F15
    (0x6B, 0x0067), // F16
    (0x6C, 0x0068), // F17
    (0x6D, 0x0069), // F18
    (0x6E, 0x006A), // F19
    (0x6F, 0x006B), // F20
    (0xE0, 0x001D), // LeftControl
    (0xE1, 0x002A), // LeftShift
    (0xE2, 0x0038), // LeftAlt
    (0xE3, 0xE05B), // LeftGui
    (0xE4, 0xE01D), // RightControl
    (0xE5, 0x0036), // RightShift
    (0xE6, 0xE038), // RightAlt
    (0xE7, 0xE05C), // RightGui
];

const EXTENDED: u16 = 0xE000;

/// The Windows scan code of the key with this HID usage, if Windows has such a key.
pub fn hid_to_windows(hid: u16) -> Option<WinKey> {
    let index = TABLE.binary_search_by_key(&hid, |&(h, _)| h).ok()?;
    let code = TABLE[index].1;
    Some(WinKey {
        scan: code & !EXTENDED,
        extended: code & EXTENDED != 0,
    })
}

/// The HID usage of the key with this Windows scan code (`extended`: the `E0` prefix), if it has one.
pub fn windows_to_hid(scan: u16, extended: bool) -> Option<u16> {
    let code = if extended { scan | EXTENDED } else { scan };
    TABLE.iter().find(|&&(_, w)| w == code).map(|&(h, _)| h)
}

const VK_PAUSE: u32 = 0x13;
const VK_NUMLOCK: u32 = 0x90;

/// The HID usage for a key event from a Windows keyboard hook (`vk`: virtual-key code, `scan`: scan code,
/// `extended`: the `E0` flag). Scan codes name the physical key, which is what the wire carries; two keys need
/// care: Pause has no usable single scan code and is ignored, and NumLock is reported with a misleading
/// extended flag on some keyboards, so it is recognised by its virtual-key code. Fake Shift events that Windows
/// adds around some numpad and navigation keys (`E0 2A` / `E0 AA`) are not in the table and so are ignored.
pub fn windows_key_to_hid(vk: u32, scan: u16, extended: bool) -> Option<u16> {
    match vk {
        VK_PAUSE => None,
        VK_NUMLOCK => Some(0x53),
        _ => windows_to_hid(scan, extended),
    }
}

/// The panic chord: Ctrl + Option/Alt + Command/Win + Escape, pressed on either computer, stops sharing. `event` is
/// what capture reports (key events carry the modifier state in effect when the key went down).
pub fn is_panic_chord(event: &crate::InputEvent) -> bool {
    let crate::InputEvent::Key {
        code,
        is_down,
        modifiers,
    } = event
    else {
        return false;
    };
    *is_down
        && *code == HID_ESCAPE
        && modifiers.contains(Modifiers::CONTROL | Modifiers::OPTION | Modifiers::COMMAND)
}

/// The modifier a HID usage stands for, if it is one of the eight modifier keys (usages 0xE0 to 0xE7). Left and
/// right keys give the same bit; command is the Windows / Meta key.
pub fn modifier_for(hid: u16) -> Option<Modifiers> {
    match hid {
        0xE0 | 0xE4 => Some(Modifiers::CONTROL),
        0xE1 | 0xE5 => Some(Modifiers::SHIFT),
        0xE2 | 0xE6 => Some(Modifiers::OPTION),
        0xE3 | 0xE7 => Some(Modifiers::COMMAND),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(hid, mac keycode, windows code)` from the shared table; blank columns are `None`.
    fn csv() -> Vec<(u16, Option<u16>, Option<u16>)> {
        let text = include_str!("../../../../docs/keymap.csv");
        let hex = |s: &str| (!s.is_empty()).then(|| u16::from_str_radix(s, 16).expect("hex"));
        text.lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .skip(1)
            .map(|line| {
                let cells: Vec<&str> = line.split(',').collect();
                assert_eq!(cells.len(), 4, "{line}");
                (hex(cells[0]).expect("hid"), hex(cells[2]), hex(cells[3]))
            })
            .collect()
    }

    #[test]
    fn the_table_is_sorted_and_has_no_duplicates() {
        assert!(TABLE.windows(2).all(|w| w[0].0 < w[1].0), "sorted by HID");
        let mut windows: Vec<u16> = TABLE.iter().map(|&(_, w)| w).collect();
        windows.sort_unstable();
        windows.dedup();
        assert_eq!(windows.len(), TABLE.len(), "one HID usage per Windows key");
    }

    #[test]
    fn it_agrees_with_the_shared_csv_in_both_directions() {
        let rows = csv();
        assert!(rows.len() > 100);
        for (hid, _, win) in &rows {
            match win {
                Some(code) => {
                    let key = hid_to_windows(*hid).unwrap_or_else(|| panic!("{hid:04X} missing"));
                    assert_eq!(
                        key.scan | if key.extended { EXTENDED } else { 0 },
                        *code,
                        "{hid:04X}"
                    );
                    assert_eq!(windows_to_hid(key.scan, key.extended), Some(*hid));
                }
                None => assert_eq!(hid_to_windows(*hid), None, "{hid:04X} has no Windows key"),
            }
        }
        let with_windows = rows.iter().filter(|r| r.2.is_some()).count();
        assert_eq!(
            with_windows,
            TABLE.len(),
            "the table has no rows the CSV does not"
        );
    }

    #[test]
    fn extended_keys_are_told_apart_from_their_keypad_twins() {
        // Right Control and Left Control share scan code 0x1D; Enter and KeypadEnter share 0x1C.
        assert_eq!(windows_to_hid(0x1D, false), Some(0xE0));
        assert_eq!(windows_to_hid(0x1D, true), Some(0xE4));
        assert_eq!(windows_to_hid(0x1C, false), Some(0x28));
        assert_eq!(windows_to_hid(0x1C, true), Some(0x58));
        assert_eq!(windows_to_hid(0x7F, false), None);
    }

    #[test]
    fn the_panic_chord_needs_all_three_modifiers_and_escape_going_down() {
        use crate::InputEvent;
        let all = Modifiers::CONTROL | Modifiers::OPTION | Modifiers::COMMAND;
        let key = |code, is_down, modifiers| InputEvent::Key {
            code,
            is_down,
            modifiers,
        };
        assert!(is_panic_chord(&key(HID_ESCAPE, true, all)));
        assert!(!is_panic_chord(&key(HID_ESCAPE, false, all)));
        assert!(!is_panic_chord(&key(
            HID_ESCAPE,
            true,
            Modifiers::CONTROL | Modifiers::OPTION
        )));
        assert!(!is_panic_chord(&key(0x14, true, all)));
        assert!(!is_panic_chord(&InputEvent::Scroll {
            horizontal: 0,
            vertical: 1
        }));
    }

    #[test]
    fn hook_events_map_with_the_quirks_of_pause_numlock_and_fake_shifts() {
        assert_eq!(windows_key_to_hid(0x41, 0x1E, false), Some(0x04)); // A
        assert_eq!(windows_key_to_hid(0x25, 0x4B, true), Some(0x50)); // Left arrow
        assert_eq!(windows_key_to_hid(0x64, 0x4B, false), Some(0x5C)); // Numpad 4
        assert_eq!(windows_key_to_hid(0x13, 0x45, false), None); // Pause
        assert_eq!(windows_key_to_hid(0x90, 0x45, true), Some(0x53)); // NumLock, whatever the flag says
        assert_eq!(windows_key_to_hid(0x10, 0x2A, true), None); // the fake Shift around Insert
    }

    #[test]
    fn modifier_keys_map_to_their_bits() {
        assert_eq!(modifier_for(0xE1), Some(Modifiers::SHIFT));
        assert_eq!(modifier_for(0xE5), Some(Modifiers::SHIFT));
        assert_eq!(modifier_for(0xE3), Some(Modifiers::COMMAND));
        assert_eq!(modifier_for(0x04), None);
    }
}
