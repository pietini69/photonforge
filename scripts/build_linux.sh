#!/usr/bin/env bash
set -euo pipefail

# Build PhotonForge for Linux (x86_64-unknown-linux-gnu) using cargo-zigbuild

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building PhotonForge v${VERSION} for Linux (x86_64-unknown-linux-gnu)"
echo "=========================================="

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
export BINDGEN_EXTRA_CLANG_ARGS="-I/tmp/linux_headers/usr/include -I/tmp/linux_headers/usr/include/x86_64-linux-gnu"

cargo zigbuild --release --bin photon_forge --target x86_64-unknown-linux-gnu

STAGE_DIR="$DIST_DIR/PhotonForge-${VERSION}-linux-x86_64"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

cp "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/photon_forge" "$STAGE_DIR/photonforge"
chmod +x "$STAGE_DIR/photonforge"
cp "$PROJECT_DIR/assets/icon_512.png" "$STAGE_DIR/photonforge.png"

cat << 'EOF' > "$STAGE_DIR/PhotonForge.desktop"
[Desktop Entry]
Name=PhotonForge
Comment=Astrophotography Stacking, Processing & Smart Telescope Suite
Exec=photonforge
Icon=photonforge
Terminal=false
Type=Application
Categories=Graphics;Science;Astronomy;Photography;
EOF

cat << EOF > "$STAGE_DIR/README.txt"
PhotonForge v${VERSION} (Linux x86_64)
======================================
High-Performance Astrophotography Stacking, Post-Processing & Smart Telescope Suite.

Running PhotonForge:
  1. Ensure execution permissions:
     chmod +x photonforge
  2. Launch directly from terminal or file manager:
     ./photonforge
  3. Toggle UI language between English (EN) and Finnish (FI) in the top-left toolbar.
EOF

TAR_PATH="$DIST_DIR/PhotonForge-${VERSION}-linux-x86_64.tar.gz"
rm -f "$TAR_PATH"
(cd "$DIST_DIR" && tar -czvf "$TAR_PATH" "PhotonForge-${VERSION}-linux-x86_64")
rm -rf "$STAGE_DIR"

echo "==> Linux package created: $TAR_PATH"
