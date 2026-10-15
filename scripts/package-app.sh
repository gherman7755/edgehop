#!/usr/bin/env bash
# Build dist/EdgeHop.app (ad-hoc signed). Open it, or copy it to /Applications.
#   scripts/package-app.sh
# Ad-hoc signing means macOS treats every rebuild as a "new" app: re-grant Accessibility / Input
# Monitoring after rebuilding (System Settings -> Privacy & Security).
set -euo pipefail
cd "$(dirname "$0")/.."

swift build -c release --product EdgeHopApp
bin_dir="$(swift build -c release --show-bin-path)"

app="dist/EdgeHop.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/EdgeHopApp" "$app/Contents/MacOS/EdgeHop"
cp Resources/Info.plist "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - --identifier dev.edgehop.app "$app"
codesign --verify --verbose=2 "$app"

echo
echo "Built $app"
echo "  open $app"
echo "  scp -r $app user@other-mac.local:~/Applications/   # for the other Mac (same CPU type)"
