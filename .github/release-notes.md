Pre-release of EdgeHop for macOS: share one mouse (and optionally the keyboard) between two Macs on the same network.

## Install

1. Open `EdgeHop-@VERSION@-arm64.dmg` and drag **EdgeHop** into **Applications**.
2. The first launch is blocked because the app is not signed by a registered Apple developer. Either right-click the app, choose **Open** and confirm, or run:
   ```bash
   xattr -dr com.apple.quarantine /Applications/EdgeHop.app
   ```
3. Grant **Accessibility** and **Input Monitoring** when asked (the app has Grant buttons), then quit and reopen it once. Allow **Local Network** if prompted: that is how the Macs find each other.

On the Mac that will be controlled choose **Be controlled** and press Start (it shows a token). On the other Mac choose **Control another computer**, pick the first Mac, paste the token, say which side it is on and press Connect.

## Requirements

Apple Silicon Mac, macOS 13 or later. Intel Macs and Windows are not supported by this build.

## Known limits

- **Not encrypted.** The connection is authenticated with the shared token (the token itself never crosses the network) but the traffic is not encrypted: use it on a network you trust. Keyboard sharing is therefore **off by default** and has to be enabled on both Macs.
- **Not signed or notarized.** Hence the Gatekeeper warning above. Because the signature is ad-hoc, macOS asks for Accessibility and Input Monitoring again after every reinstall or update.
- Panic chord on either Mac: **Ctrl + Option + Cmd + Esc** stops sharing.

## Verify the download

```bash
shasum -a 256 -c SHA256SUMS
```

## Source

Built by the release workflow from the commit this tag points to, with `scripts/make-dmg.sh`. See the README for the architecture, the wire protocol (`docs/PROTOCOL.md`) and the core IPC (`docs/IPC.md`).
