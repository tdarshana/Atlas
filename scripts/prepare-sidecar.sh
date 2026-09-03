#!/usr/bin/env bash
# Build atlasd in release mode and stage it as the Tauri sidecar the bundler expects:
# src-tauri/binaries/atlasd-<target triple>. `tauri.conf.json` runs this as part of
# beforeBuildCommand, so `bun run tauri build` works with no separate step; running it
# by hand first is still fine, since a second run does nothing.
#
# An optional first argument cross-compiles for that target triple instead of the host's
# own (`rustc -vV`'s "host:" line), for a CI job building macOS aarch64 and x86_64 from
# one runner; the caller is responsible for that target's toolchain being installed
# (`rustup target add`).
set -euo pipefail

# `tauri.conf.json`'s `beforeBuildCommand` also runs this, with no target argument, on
# every `tauri build`. The release workflow already stages the right triple explicitly
# (see release.yml) before calling `tauri-action`, which reruns beforeBuildCommand once
# per matrix job; without this a cross-compiling job (building x86_64-apple-darwin on an
# aarch64-apple-darwin runner, say) would rebuild and stage an unwanted host-triple
# sidecar on top of the one already staged for the requested target. The workflow sets
# this env var right after its own explicit staging step so that rerun is a no-op; a
# plain local `bun run tauri build` never sets it, so this still runs there as before.
if [ -n "${ATLAS_SIDECAR_STAGED:-}" ]; then
  exit 0
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
triple="${1:-}"
if [ -z "$triple" ]; then
  triple="$(rustc -vV | sed -n 's/host: //p')"
fi
if [ -z "$triple" ]; then
  echo "could not read the host target triple from rustc -vV" >&2
  exit 1
fi

# Cargo is already incremental, but a no-op rebuild still writes a "Finished" line into
# every frontend build. Hold its output and show it only when the build fails, so a
# fresh binary stages silently.
log="$(mktemp)"
trap 'rm -f "$log"' EXIT
build_args=(--release -p atlasd --manifest-path "$root/Cargo.toml")
target_dir="$root/target/release"
if [ -n "${1:-}" ]; then
  build_args+=(--target "$triple")
  target_dir="$root/target/$triple/release"
fi
if ! cargo build "${build_args[@]}" >"$log" 2>&1; then
  cat "$log" >&2
  exit 1
fi

# Windows binaries carry `.exe`; cargo produces it on the built binary, and Tauri's
# `externalBin` resolver expects it on the staged sidecar's name too (mirrors
# `std::env::consts::EXE_SUFFIX`, which `sidecar_atlasd` in `src-tauri/src/lib.rs` uses
# at runtime to find it).
ext=""
case "$triple" in
  *-pc-windows-*) ext=".exe" ;;
esac

src="$target_dir/atlasd$ext"
dest_dir="$root/src-tauri/binaries"
dest="$dest_dir/atlasd-$triple$ext"

# Nothing to do when the staged sidecar is already this binary, so a repeated run neither
# rewrites it nor claims to have staged anything.
if [ -f "$dest" ] && cmp -s "$src" "$dest"; then
  exit 0
fi

mkdir -p "$dest_dir"
cp "$src" "$dest"
chmod +x "$dest"
echo "staged $dest"
