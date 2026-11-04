#!/usr/bin/env bash
set -euo pipefail

# PhotonForge macOS Packaging Script
# Supports:
#   ./scripts/build_dmg.sh universal   (Builds Universal 2 binary: Apple Silicon + Intel) [DEFAULT]
#   ./scripts/build_dmg.sh arm64       (Apple Silicon only)
#   ./scripts/build_dmg.sh intel       (Intel x86_64 only)

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DEFAULT_SIGNING_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | grep "Apple Development" | head -n 1 | cut -d '"' -f 2 || true)
SIGNING_IDENTITY="${SIGNING_IDENTITY:-${DEFAULT_SIGNING_IDENTITY:-}}"
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

MODE="${1:-universal}"
export PATH="$HOME/.cargo/bin:$PATH"

package_target() {
    local target_name="$1"
    local bin_source="$2"
    local vol_name="PhotonForge ${VERSION}"
    local app_dir="$DIST_DIR/PhotonForge.app"
    local dmg_root="$DIST_DIR/dmg_${target_name}_root"
    local dmg_path="$DIST_DIR/PhotonForge-${VERSION}-${target_name}.dmg"
    local zip_path="$DIST_DIR/PhotonForge-${VERSION}-macos-${target_name}.zip"

    echo "==> Constructing ${target_name} PhotonForge.app bundle..."
    rm -rf "$app_dir" "$dmg_root"
    mkdir -p "$app_dir/Contents/MacOS"
    mkdir -p "$app_dir/Contents/Resources"

    cp "$bin_source" "$app_dir/Contents/MacOS/photon_forge"
    chmod +x "$app_dir/Contents/MacOS/photon_forge"
    cp "$PROJECT_DIR/Info.plist" "$app_dir/Contents/Info.plist"
    cp "$PROJECT_DIR/assets/photonforge.icns" "$app_dir/Contents/Resources/photonforge.icns"

    echo "==> Code signing ${target_name} bundle..."
    if [ -n "$SIGNING_IDENTITY" ]; then
        codesign --force --deep --sign "$SIGNING_IDENTITY" "$app_dir"
        codesign --verify --deep --strict "$app_dir"
    else
        echo "    (No Apple Development signing identity found; signing ad-hoc)"
        codesign --force --deep --sign - "$app_dir"
    fi

    echo "==> Packaging DMG: $dmg_path..."
    mkdir -p "$dmg_root"
    cp -R "$app_dir" "$dmg_root/PhotonForge.app"
    ln -s /Applications "$dmg_root/Applications"
    rm -f "$dmg_path"
    hdiutil create -volname "$vol_name" -srcfolder "$dmg_root" -ov -format UDZO "$dmg_path"

    echo "==> Packaging ZIP: $zip_path..."
    rm -f "$zip_path"
    (cd "$DIST_DIR" && zip -r "$zip_path" "PhotonForge.app")

    if [[ "$target_name" == "universal" || "$target_name" == "arm64" ]]; then
        echo "==> Installing latest build into /Applications/PhotonForge.app..."
        rm -rf /Applications/PhotonForge.app
        cp -R "$app_dir" /Applications/PhotonForge.app
        touch /Applications/PhotonForge.app
    fi

    rm -rf "$dmg_root" "$app_dir"
    echo "==> Done: $dmg_path"
}

build_arm64() {
    echo "==> Building arm64 release binary..."
    cargo build --release --bin photon_forge --target aarch64-apple-darwin
}

build_intel() {
    echo "==> Building x86_64 (Intel) release binary..."
    cargo build --release --bin photon_forge --target x86_64-apple-darwin
}

case "$MODE" in
    arm64)
        build_arm64
        package_target "arm64" "$PROJECT_DIR/target/aarch64-apple-darwin/release/photon_forge"
        ;;
    intel)
        build_intel
        package_target "intel-x86_64" "$PROJECT_DIR/target/x86_64-apple-darwin/release/photon_forge"
        ;;
    universal)
        build_arm64
        build_intel
        mkdir -p "$DIST_DIR"
        UNIV_BIN="$DIST_DIR/photon_forge_universal_tmp"
        echo "==> Creating Universal 2 binary with lipo..."
        lipo -create -output "$UNIV_BIN" \
            "$PROJECT_DIR/target/aarch64-apple-darwin/release/photon_forge" \
            "$PROJECT_DIR/target/x86_64-apple-darwin/release/photon_forge"
        package_target "universal" "$UNIV_BIN"
        rm -f "$UNIV_BIN"
        ;;
    *)
        echo "Usage: $0 [universal|arm64|intel]"
        exit 1
        ;;
esac

echo "=========================================="
echo " PhotonForge macOS Packaging Complete!"
echo "=========================================="
