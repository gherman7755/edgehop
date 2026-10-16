import Foundation

/// The neutral key map (docs/PROTOCOL.md "Key codes", docs/keymap.csv): key events on the wire carry the USB HID
/// usage (keyboard page 0x07) of the physical key, so computers need not share a layout or an operating system.
/// This is the macOS side: macOS virtual keycodes at the edge (event tap, injector), HID everywhere else.
/// The table is tested against docs/keymap.csv, the single source the Rust implementation is tested against too.
public enum Keymap {
    /// Value of `HelloMessage.keymap` from computers that send and understand HID key codes. Computers that do not
    /// announce it use macOS keycodes (the first versions) and must not be sent keys.
    public static let name = "hid"

    /// HID usage of the Escape key (the panic chord).
    public static let hidEscape: UInt16 = 0x29

    /// `(HID usage, macOS keycode)`.
    static let rows: [(hid: UInt16, mac: UInt16)] = [
        (0x0004, 0x00), // A
        (0x0005, 0x0B), // B
        (0x0006, 0x08), // C
        (0x0007, 0x02), // D
        (0x0008, 0x0E), // E
        (0x0009, 0x03), // F
        (0x000A, 0x05), // G
        (0x000B, 0x04), // H
        (0x000C, 0x22), // I
        (0x000D, 0x26), // J
        (0x000E, 0x28), // K
        (0x000F, 0x25), // L
        (0x0010, 0x2E), // M
        (0x0011, 0x2D), // N
        (0x0012, 0x1F), // O
        (0x0013, 0x23), // P
        (0x0014, 0x0C), // Q
        (0x0015, 0x0F), // R
        (0x0016, 0x01), // S
        (0x0017, 0x11), // T
        (0x0018, 0x20), // U
        (0x0019, 0x09), // V
        (0x001A, 0x0D), // W
        (0x001B, 0x07), // X
        (0x001C, 0x10), // Y
        (0x001D, 0x06), // Z
        (0x001E, 0x12), // 1
        (0x001F, 0x13), // 2
        (0x0020, 0x14), // 3
        (0x0021, 0x15), // 4
        (0x0022, 0x17), // 5
        (0x0023, 0x16), // 6
        (0x0024, 0x1A), // 7
        (0x0025, 0x1C), // 8
        (0x0026, 0x19), // 9
        (0x0027, 0x1D), // 0
        (0x0028, 0x24), // Enter
        (0x0029, 0x35), // Escape
        (0x002A, 0x33), // Backspace
        (0x002B, 0x30), // Tab
        (0x002C, 0x31), // Space
        (0x002D, 0x1B), // Minus
        (0x002E, 0x18), // Equal
        (0x002F, 0x21), // LeftBracket
        (0x0030, 0x1E), // RightBracket
        (0x0031, 0x2A), // Backslash
        (0x0033, 0x29), // Semicolon
        (0x0034, 0x27), // Quote
        (0x0035, 0x32), // Grave
        (0x0036, 0x2B), // Comma
        (0x0037, 0x2F), // Period
        (0x0038, 0x2C), // Slash
        (0x0039, 0x39), // CapsLock
        (0x003A, 0x7A), // F1
        (0x003B, 0x78), // F2
        (0x003C, 0x63), // F3
        (0x003D, 0x76), // F4
        (0x003E, 0x60), // F5
        (0x003F, 0x61), // F6
        (0x0040, 0x62), // F7
        (0x0041, 0x64), // F8
        (0x0042, 0x65), // F9
        (0x0043, 0x6D), // F10
        (0x0044, 0x67), // F11
        (0x0045, 0x6F), // F12
        (0x0049, 0x72), // Insert
        (0x004A, 0x73), // Home
        (0x004B, 0x74), // PageUp
        (0x004C, 0x75), // Delete
        (0x004D, 0x77), // End
        (0x004E, 0x79), // PageDown
        (0x004F, 0x7C), // Right
        (0x0050, 0x7B), // Left
        (0x0051, 0x7D), // Down
        (0x0052, 0x7E), // Up
        (0x0053, 0x47), // NumLock
        (0x0054, 0x4B), // KeypadDivide
        (0x0055, 0x43), // KeypadMultiply
        (0x0056, 0x4E), // KeypadMinus
        (0x0057, 0x45), // KeypadPlus
        (0x0058, 0x4C), // KeypadEnter
        (0x0059, 0x53), // Keypad1
        (0x005A, 0x54), // Keypad2
        (0x005B, 0x55), // Keypad3
        (0x005C, 0x56), // Keypad4
        (0x005D, 0x57), // Keypad5
        (0x005E, 0x58), // Keypad6
        (0x005F, 0x59), // Keypad7
        (0x0060, 0x5B), // Keypad8
        (0x0061, 0x5C), // Keypad9
        (0x0062, 0x52), // Keypad0
        (0x0063, 0x41), // KeypadDecimal
        (0x0064, 0x0A), // NonUsBackslash
        (0x0065, 0x6E), // Menu
        (0x0068, 0x69), // F13
        (0x0069, 0x6B), // F14
        (0x006A, 0x71), // F15
        (0x006B, 0x6A), // F16
        (0x006C, 0x40), // F17
        (0x006D, 0x4F), // F18
        (0x006E, 0x50), // F19
        (0x006F, 0x5A), // F20
        (0x00E0, 0x3B), // LeftControl
        (0x00E1, 0x38), // LeftShift
        (0x00E2, 0x3A), // LeftAlt
        (0x00E3, 0x37), // LeftGui
        (0x00E4, 0x3E), // RightControl
        (0x00E5, 0x3C), // RightShift
        (0x00E6, 0x3D), // RightAlt
        (0x00E7, 0x36), // RightGui
    ]

    private static let macToHID: [UInt16: UInt16] = Dictionary(uniqueKeysWithValues: rows.map { ($0.mac, $0.hid) })
    private static let hidToMac: [UInt16: UInt16] = Dictionary(uniqueKeysWithValues: rows.map { ($0.hid, $0.mac) })

    /// The HID usage of the key with this macOS virtual keycode, if it has one (the Fn key, for instance, does not).
    public static func hid(forMac code: UInt16) -> UInt16? { macToHID[code] }

    /// The macOS virtual keycode of the key with this HID usage, if macOS has such a key.
    public static func mac(forHID usage: UInt16) -> UInt16? { hidToMac[usage] }
}
