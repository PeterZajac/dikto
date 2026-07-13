#!/usr/bin/env bash
# Builds Dikto, signs it with the stable "Dikto Dev" identity (see
# scripts/make-signing-cert.sh), and installs it to /Applications.
#
# Why sign at all for local dev: an ad-hoc signature (`codesign -s -`) embeds
# a hash of the binary in the code-signing identity, so it's different on
# every build. macOS ties TCC grants (Accessibility, Microphone) to that
# identity, so each rebuild silently invalidates them — the System Settings
# toggle stays ON but the permission is dead. Signing with a fixed identity
# keeps the identity stable across rebuilds, so TCC grants survive.
#
# Run scripts/make-signing-cert.sh once first. After that, run this script
# for every iteration instead of `pnpm tauri build` + manual install.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CERT_NAME="Dikto Dev"
APP_NAME="Dikto.app"
BUNDLE_DIR="$REPO_ROOT/src-tauri/target/release/bundle"
DEST="/Applications/$APP_NAME"

cd "$REPO_ROOT"
echo "Building Dikto (pnpm tauri build)..."
pnpm tauri build

# Tauri removes the staged .app directory under bundle/macos/ after it has
# packaged the .dmg, so the .app may or may not still be there depending on
# bundle order — fall back to extracting it from the .dmg when it's gone.
APP_SRC="$BUNDLE_DIR/macos/$APP_NAME"
if [[ ! -d "$APP_SRC" ]]; then
    echo "Staged .app not found at $APP_SRC (tauri cleaned it up after DMG creation) — extracting from the .dmg instead."
    DMG_PATH="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' -print -quit)"
    if [[ -z "$DMG_PATH" ]]; then
        echo "error: no .app in bundle/macos and no .dmg in bundle/dmg — build output not where expected." >&2
        exit 1
    fi
    MOUNT_DIR="$(mktemp -d)"
    trap 'hdiutil detach "$MOUNT_DIR" -quiet 2>/dev/null || true; rm -rf "$MOUNT_DIR"' EXIT
    hdiutil attach "$DMG_PATH" -mountpoint "$MOUNT_DIR" -nobrowse -quiet
    cp -R "$MOUNT_DIR/$APP_NAME" "$REPO_ROOT/src-tauri/target/release/bundle/macos/"
    hdiutil detach "$MOUNT_DIR" -quiet
    trap - EXIT
    APP_SRC="$BUNDLE_DIR/macos/$APP_NAME"
fi

if security find-identity -v -p codesigning 2>/dev/null | grep -q "$CERT_NAME"; then
    echo "Signing $APP_SRC with \"$CERT_NAME\"..."
    # No hardened runtime: Dikto loads unsigned dylibs (cpal/enigo backends);
    # hardened runtime would refuse to load them.
    codesign --force --deep -s "$CERT_NAME" "$APP_SRC"
    echo "Verifying signature..."
    codesign -dv "$APP_SRC" 2>&1 | grep -E "Authority" || true
else
    echo
    echo "warning: signing identity \"$CERT_NAME\" not found — installing UNSIGNED."
    echo "Every rebuild will get a new ad-hoc identity, silently revoking your"
    echo "Accessibility/Microphone grants. Run scripts/make-signing-cert.sh once"
    echo "to fix this."
    echo
fi

echo "Installing to $DEST..."
rm -rf "$DEST"
cp -R "$APP_SRC" "$DEST"
xattr -dr com.apple.quarantine "$DEST"

echo
echo "Installed. Signature:"
codesign -dv --verbose=2 "$DEST" 2>&1 | grep -E "Identifier|Authority|Signature"
