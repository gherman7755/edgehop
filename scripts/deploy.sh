#!/usr/bin/env bash
# Build edgehop-link (release) and copy it to the other Mac with scp.
#   scripts/deploy.sh user@other-mac.local [remote_dir]
# scp does not add the quarantine attribute, so the binary runs without a Gatekeeper prompt.
set -euo pipefail

target="${1:?usage: scripts/deploy.sh user@host [remote_dir]}"
remote_dir="${2:-edgehop}"
cd "$(dirname "$0")/.."

local_arch="$(uname -m)"
remote_arch="$(ssh "$target" uname -m)"
if [[ "$local_arch" != "$remote_arch" ]]; then
	echo "Architecture mismatch: this Mac is $local_arch, $target is $remote_arch." >&2
	echo "Build on the target, or cross-build with --arch." >&2
	exit 1
fi

swift build -c release --product edgehop-link
bin_dir="$(swift build -c release --show-bin-path)"

ssh "$target" "mkdir -p '$remote_dir'"
scp "$bin_dir/edgehop-link" "$target:$remote_dir/"
echo "Copied to $target:$remote_dir/edgehop-link"
echo "On the other Mac: grant Accessibility + Input Monitoring to its Terminal, then:"
echo "  EDGEHOP_TOKEN=<token> $remote_dir/edgehop-link serve"
