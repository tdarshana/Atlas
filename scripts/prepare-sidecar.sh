#!/usr/bin/env bash
# Build atlasd in release mode and stage it as the Tauri sidecar the bundler expects:
# src-tauri/binaries/atlasd-<target triple>. Run this before `bun run tauri build`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
triple="$(rustc -vV | sed -n 's/host: //p')"
if [ -z "$triple" ]; then
  echo "could not read the host target triple from rustc -vV" >&2
  exit 1
fi

cargo build --release -p atlasd --manifest-path "$root/Cargo.toml"

src="$root/target/release/atlasd"
dest_dir="$root/src-tauri/binaries"
dest="$dest_dir/atlasd-$triple"

mkdir -p "$dest_dir"
cp "$src" "$dest"
chmod +x "$dest"
echo "staged $dest"
