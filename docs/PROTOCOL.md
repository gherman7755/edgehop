# EdgeHop link protocol (v1, plaintext)

What another implementation (for example a Windows agent) must speak to join a session.
**Status: not encrypted.** Authentication only. Noise XX arrives in milestone M6, which will wrap this
exact framing.

## Discovery and transport
* TCP, default port **47474**. The receiver advertises Bonjour service **`_edgehop._tcp`** (name = computer name).
  Manual connection by IP is always possible. Disable Nagle (`TCP_NODELAY`): records are tiny.
* One controller at a time per receiver; extra connections are closed.

## Frame
8-byte header then payload, all numbers big-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 2 | magic `0x45 0x48` ("EH") |
| 2 | 1 | type: 1 hello, 2 layout (reserved), 3 input, 4 control, 5 ping, 6 pong |
| 3 | 1 | flags (0) |
| 4 | 4 | payload length, max 16 MiB |

Bad magic / unknown type / oversize length: drop the connection.

## Handshake (hello frames, JSON UTF-8)
1. Receiver -> controller: `{"version":1,"name":"<computer>","nonce":"<base64 of 32 random bytes>"}`
2. Controller -> receiver: `{"version":1,"name":"<computer>","proof":"<base64>","monitors":[...]}`
   where `proof = HMAC-SHA256(key = UTF-8 token, message = "edgehop-link-v1|" || nonce_bytes)`.
3. Receiver -> controller: `{"version":1,"name":"<computer>","accepted":true,"monitors":[...]}`
   or `"accepted":false` and close. Frames other than hello before acceptance close the connection (5 s limit).

`monitors` = `[{"id":"d123","x":0,"y":0,"width":1512,"height":982}, ...]` in global points, the primary display
at (0,0). The controller uses the monitor at (0,0) as the remote screen size.

## Input frames (type 3), fixed 16-byte records
| Offset | Meaning |
|---|---|
| 0 | kind: 0 move absolute, 1 move delta, 2 button, 3 scroll, 4 key |
| 1 | flag: 1 = down (button/key) |
| 2 | aux: button (0 left, 1 right, 2 middle, 3 back, 4 forward) or key modifiers (bit0 shift, 1 control, 2 option, 3 command) |
| 3 | 0 |
| 4..8 | int32: x / dx / horizontal scroll / key code |
| 8..12 | int32: y / dy / vertical scroll |
| 12..16 | 0 |

* Coordinates are in the **receiver's** global points (origin top-left of the primary display).
* Scroll values are 1/120 of a wheel notch (Windows `WHEEL_DELTA` units); positive vertical = scroll up.
* **Key codes are currently macOS virtual keycodes.** A Windows agent needs a translation table; the neutral
  keymap is milestone M7. Until then a Windows agent should support mouse and scroll only.
* The receiver tags every event it injects (macOS: `kCGEventSourceUserData = 0x45484F50`) and ignores its
  own tag when capturing, to avoid feedback loops. A Windows agent can use `dwExtraInfo` for the same purpose.

## Control frames (type 4, JSON)
* `{"t":"release_all"}`: release every key and button pressed through this link (sent when control returns).

## Ping / pong
Controller sends type 5 with an 8-byte payload (its monotonic milliseconds); receiver answers type 6 with the
same payload. Every 2 s.

## Receiver behaviour that matters
* Absolute moves: large jumps (> 150 px) warp the cursor; small ones become relative moves.
* Relative moves start from the real cursor position if the receiver has none yet (or after 0.5 s idle).
* On disconnect: release everything that was pressed.
* Panic chord Ctrl+Option+Cmd+Esc (macOS keycode 53) stops sharing on either computer.
