#!/usr/bin/env bash
set -euo pipefail

# Build PhotonForge for Windows (x86_64-pc-windows-msvc) using cargo-xwin

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building PhotonForge v${VERSION} for Windows (x86_64-pc-windows-msvc)"
echo "=========================================="

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
cargo xwin build --release --bin photon_forge --target x86_64-pc-windows-msvc

STAGE_DIR="$DIST_DIR/PhotonForge-${VERSION}-windows-x64"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

cp "$PROJECT_DIR/target/x86_64-pc-windows-msvc/release/photon_forge.exe" "$STAGE_DIR/PhotonForge.exe"

cat << EOF > "$STAGE_DIR/README.txt"
PhotonForge v${VERSION} (Windows x64)
=====================================
High-Performance Astrophotography Stacking, Post-Processing & Smart Telescope Suite.

Usage:
  1. Extract this zip archive into any folder (e.g. C:\PhotonForge).
  2. Double-click PhotonForge.exe to start.
  3. Toggle UI language between English (EN) and Finnish (FI) in the top-left toolbar.

First Launch (Windows Defender SmartScreen):
  If the blue "Windows protected your PC" screen appears:
  - Click "More info"
  - Click "Run anyway"
EOF

ZIP_PATH="$DIST_DIR/PhotonForge-${VERSION}-windows-x64.zip"
rm -f "$ZIP_PATH"
(cd "$STAGE_DIR" && zip -r "$ZIP_PATH" PhotonForge.exe README.txt)
rm -rf "$STAGE_DIR"

echo "==> Windows package created: $ZIP_PATH"
