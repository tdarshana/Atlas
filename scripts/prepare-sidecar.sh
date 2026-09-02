#!/usr/bin/env bash
# Build atlasd in release mode and stage it as the Tauri sidecar the bundler expects:
# src-tauri/binaries/atlasd-<target triple>. `tauri.conf.json` runs this as part of
# beforeBuildCommand, so `bun run tauri build` works with no separate step; running it
# by hand first is still fine, since a second run does nothing.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
triple="$(rustc -vV | sed -n 's/host: //p')"
if [ -z "$triple" ]; then
  echo "could not read the host target triple from rustc -vV" >&2
  exit 1
fi

# Cargo is already incremental, but a no-op rebuild still writes a "Finished" line into
# every frontend build. Hold its output and show it only when the build fails, so a
# fresh binary stages silently.
log="$(mktemp)"
trap 'rm -f "$log"' EXIT
if ! cargo build --release -p atlasd --manifest-path "$root/Cargo.toml" >"$log" 2>&1; then
  cat "$log" >&2
  exit 1
fi

src="$root/target/release/atlasd"
dest_dir="$root/src-tauri/binaries"
dest="$dest_dir/atlasd-$triple"

# Nothing to do when the staged sidecar is already this binary, so a repeated run neither
# rewrites it nor claims to have staged anything.
if [ -f "$dest" ] && cmp -s "$src" "$dest"; then
  exit 0
fi

mkdir -p "$dest_dir"
cp "$src" "$dest"
chmod +x "$dest"
echo "staged $dest"
