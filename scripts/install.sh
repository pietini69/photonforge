#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

echo "==> Rakennetaan optimoitu PhotonForge release-binaari..."
cargo build --release

APP_PATH="/Applications/PhotonForge.app"
echo "==> Asennetaan kohteeseen $APP_PATH..."
rm -rf "$APP_PATH"
mkdir -p "$APP_PATH/Contents/MacOS"
mkdir -p "$APP_PATH/Contents/Resources"

cp "$SCRIPT_DIR/target/release/photon_forge" "$APP_PATH/Contents/MacOS/photon_forge"
chmod +x "$APP_PATH/Contents/MacOS/photon_forge"

cp "$SCRIPT_DIR/Info.plist" "$APP_PATH/Contents/Info.plist"
cp "$SCRIPT_DIR/assets/photonforge.icns" "$APP_PATH/Contents/Resources/photonforge.icns"

echo "==> Allekirjoitetaan ad-hoc -koodiallekirjoituksella..."
codesign --force --deep --sign - "$APP_PATH"
touch "$APP_PATH"

echo "==> Rekisteröidään macOS LaunchServicesiin..."
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP_PATH"

echo "==> Kopioidaan komentonäkymän binääri ~/.cargo/bin/photon_forge..."
cp "$SCRIPT_DIR/target/release/photon_forge" "$HOME/.cargo/bin/photon_forge"

echo "✨ PhotonForge on asennettu onnistuneesti! Löydät sen nyt Applications-kansiosta ja Spotlightista."
