#!/usr/bin/env bash
# Build dist/EdgeHop.dmg: the app plus an Applications shortcut for drag-and-drop install.
#   scripts/make-dmg.sh
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/package-app.sh >/dev/null

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp -R dist/EdgeHop.app "$stage/"
ln -s /Applications "$stage/Applications"

rm -f dist/EdgeHop.dmg
hdiutil create -volname "EdgeHop" -srcfolder "$stage" -fs HFS+ -format UDZO -ov dist/EdgeHop.dmg >/dev/null
hdiutil verify dist/EdgeHop.dmg >/dev/null

echo "Built dist/EdgeHop.dmg ($(du -h dist/EdgeHop.dmg | cut -f1), $(uname -m) only)"
