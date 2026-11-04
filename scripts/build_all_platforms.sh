#!/usr/bin/env bash
set -euo pipefail

# PhotonForge All-Platforms Packaging Script (macOS, Windows, Linux)
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building PhotonForge v${VERSION} for all platforms"
echo "=========================================="

# 1. macOS Universal
echo "==> 1/3 Building macOS Universal..."
./scripts/build_dmg.sh universal

# 2. Windows x86_64
echo "==> 2/3 Building Windows x86_64..."
cargo xwin build --release --target x86_64-pc-windows-msvc
mkdir -p "$DIST_DIR/win_tmp"
cp "$PROJECT_DIR/target/x86_64-pc-windows-msvc/release/photon_forge.exe" "$DIST_DIR/win_tmp/"
cat << EOF > "$DIST_DIR/win_tmp/README.txt"
PhotonForge v${VERSION} for Windows (x86_64)
=======================================

High-performance astrophotography processing, stacking and calibration platform.

Requirements:
- Windows 10 / 11 (64-bit)
- OpenGL 3.3+ / DirectX compatible GPU

Running:
- Double-click photon_forge.exe to launch.
- Standalone portable binary, no installation required.
EOF
(cd "$DIST_DIR/win_tmp" && zip -r "../PhotonForge-${VERSION}-windows-x64.zip" photon_forge.exe README.txt)
rm -rf "$DIST_DIR/win_tmp"

# 3. Linux x86_64 (glibc 2.17+)
echo "==> 3/3 Building Linux x86_64..."
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.17
mkdir -p "$DIST_DIR/linux_tmp"
cp "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/photon_forge" "$DIST_DIR/linux_tmp/"
chmod +x "$DIST_DIR/linux_tmp/photon_forge"
cp "$PROJECT_DIR/assets/icon_256.png" "$DIST_DIR/linux_tmp/photonforge.png"
cat << EOF > "$DIST_DIR/linux_tmp/photonforge.desktop"
[Desktop Entry]
Name=PhotonForge
Comment=High-performance astrophotography processing, stacking and calibration
Exec=photon_forge
Icon=photonforge
Terminal=false
Type=Application
Categories=Graphics;Photography;Science;Astronomy;
EOF
cat << EOF > "$DIST_DIR/linux_tmp/README.txt"
PhotonForge v${VERSION} for Linux (x86_64)
=====================================

High-performance astrophotography processing, stacking and calibration platform.

Requirements:
- Linux 64-bit (glibc 2.17+; compatible with Ubuntu 16.04+, Debian 8+, Fedora, Arch, etc.)
- Wayland or X11 desktop environment
- OpenGL 3.3+ support (Mesa / proprietary drivers)

Running:
- Make executable: chmod +x photon_forge
- Launch: ./photon_forge
EOF
tar -czvf "$DIST_DIR/PhotonForge-${VERSION}-linux-x86_64.tar.gz" -C "$DIST_DIR/linux_tmp" photon_forge photonforge.png photonforge.desktop README.txt
rm -rf "$DIST_DIR/linux_tmp"

echo "=========================================="
echo " Packaging complete for macOS, Windows and Linux!"
echo " Assets in $DIST_DIR:"
ls -lh "$DIST_DIR"/PhotonForge-${VERSION}*
echo "=========================================="
