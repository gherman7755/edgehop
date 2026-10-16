Pre-release of EdgeHop for **macOS** and **Windows**: share one keyboard and mouse between computers on the same network, by pushing the cursor through the edge of the screen. This version has a new desktop app (the same screens on both systems) and works between a Mac and a Windows PC.

## Install

**macOS (Apple Silicon, macOS 13 or later)**: open `EdgeHop-@VERSION@-mac-arm64.dmg` and drag **EdgeHop** into **Applications**.
1. The first launch is blocked because the app is not signed by a registered Apple developer. Either right-click the app, choose **Open** and confirm, or run:
   ```bash
   xattr -dr com.apple.quarantine /Applications/EdgeHop.app
   ```
2. Grant **Accessibility** and **Input Monitoring** when asked (the app has Grant buttons), then quit and reopen it once. Allow **Local Network** if prompted: that is how the computers find each other.

**Windows (64-bit, Windows 10 or later)**: run `EdgeHop-@VERSION@-win-x64-setup.exe`.
1. SmartScreen warns about an unknown publisher because the installer is not signed: choose **More info**, then **Run anyway**.
2. The first time you start listening or connecting, Windows Firewall asks about EdgeHop: allow it on **private networks** (it needs the port and UDP 5353 for finding the other computer).

## Use

On the computer that will be controlled choose **Be controlled** and press **Start listening**: it shows a token and its addresses. On the other computer choose **Control another computer**, pick the first one from the list (or type its address), paste the token, say which side it is on and press **Connect**. Push the cursor through that edge of the screen; push back through the same edge to return.

## What is new since 0.1.0

- A new desktop app for macOS and Windows (Electron) with a status header, a drawn layout picker, nearby computers, the token, a log and a menu bar / tray icon. It keeps sharing from the tray when the window is closed.
- **Windows support**: a Windows PC can control a Mac and be controlled by one, mouse, scroll and (optionally) keyboard.
- **Keyboard sharing** on both systems with a shared key map, so layouts do not have to match, and without keys sticking when the cursor crosses with a modifier held.
- Computers on the network are **found by name** on both systems.
- The link is now **encrypted** (computers running 0.1.0 cannot connect to 0.2.0: update both).
- Dragging with a button held stays on the computer it started on, and a jitter right after crossing no longer bounces you back.
- The panic chord now works on Windows too.

## Known limits

- **Encrypted, but the token is the key.** The connection is encrypted and authenticated (Noise, with the shared token as the pre-shared key), so someone on your network can neither read nor change the traffic, and cannot connect without the token. Anyone who knows the token can connect, so keep it private (the generated 20-character token is strong; a short one you invent is not). Keyboard sharing is still **off by default** and has to be enabled on both computers.
- **Not signed or notarized**, hence the warnings above. Because the macOS signature is ad-hoc, macOS asks for Accessibility and Input Monitoring again after every reinstall or update.
- **Panic chord** on either computer: **Ctrl + Option + Cmd + Esc** on a Mac, **Ctrl + Alt + Win + Esc** on Windows; it stops sharing.
- Intel Macs are not supported by this build. Caps Lock is not shared reliably; JIS-only and media keys are not shared.
- Windows does not let EdgeHop type into windows running as administrator, and Ctrl+Alt+Del and Win+L cannot be shared.
- Not yet tried on many setups: please report problems with the log (**Log → Copy all**) from the app.

## Verify the download

```bash
shasum -a 256 -c SHA256SUMS --ignore-missing      # macOS
```
On Windows: `Get-FileHash .\EdgeHop-@VERSION@-win-x64-setup.exe` and compare with `SHA256SUMS`.

## Source

Built by the release workflow from the commit this tag points to. See the README for the architecture, the wire protocol (`docs/PROTOCOL.md`), the core IPC (`docs/IPC.md`) and the app (`docs/UI.md`).
