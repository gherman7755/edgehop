import Foundation
import Testing
@testable import EdgeHopCore

/// The rows of docs/keymap.csv (the file the Rust implementation is tested against as well).
private func csvRows() throws -> [(hid: UInt16, name: String, mac: UInt16?, win: String)] {
    let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let text = try String(contentsOf: repo.appendingPathComponent("docs/keymap.csv"), encoding: .utf8)
    return text.split(separator: "\n").filter { !$0.hasPrefix("#") }.dropFirst().map { line in
        let cells = line.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        return (UInt16(cells[0], radix: 16)!, cells[1], UInt16(cells[2], radix: 16), cells[3])
    }
}

@Suite struct KeymapTests {
    @Test func theTableAgreesWithTheSharedCSVInBothDirections() throws {
        let rows = try csvRows()
        #expect(rows.count > 100)
        for row in rows {
            if let mac = row.mac {
                #expect(Keymap.mac(forHID: row.hid) == mac, "\(row.name)")
                #expect(Keymap.hid(forMac: mac) == row.hid, "\(row.name)")
            } else {
                #expect(Keymap.mac(forHID: row.hid) == nil, "\(row.name) has no macOS key")
            }
        }
        #expect(Keymap.rows.count == rows.filter { $0.mac != nil }.count, "the table has no rows the CSV does not")
    }

    @Test func letterAKeepsItsIdentityAcrossTheTranslation() {
        #expect(Keymap.hid(forMac: 0x00) == 0x04) // kVK_ANSI_A
        #expect(Keymap.mac(forHID: 0x04) == 0x00)
        #expect(Keymap.hid(forMac: 0x35) == Keymap.hidEscape)
    }

    @Test func keysWithoutAnOtherSideAreDropped() {
        #expect(Keymap.hid(forMac: 0x3F) == nil) // Fn
        #expect(Keymap.mac(forHID: 0x46) == nil) // PrintScreen: no such key on a Mac
    }

    @Test func modifierKeysAreTheOnesTheEventTapAndInjectorTreatAsFlags() throws {
        // HID 0xE0...0xE7 are exactly the keys whose macOS keycodes change modifier flags.
        for row in try csvRows() {
            let isHIDModifier = (0xE0...0xE7).contains(row.hid)
            guard let mac = row.mac else { continue }
            let isFlagKey = Modifiers.flag(forKeyCode: mac) != nil
            // CapsLock (HID 0x39) is a flag key on macOS but not a modifier on the wire.
            #expect(isFlagKey == (isHIDModifier || row.hid == 0x39), "\(row.name)")
        }
    }
}
