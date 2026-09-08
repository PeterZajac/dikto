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

# Sign during the build, not after it. tauri.conf.json says "-" so a bare
# `pnpm tauri build` works on a machine without the certificate; this env var
# overrides it, exactly as the release workflow does. Signing afterwards would
# leave the .app.tar.gz the updater ships holding an ad-hoc signature — and an
# ad-hoc update kills the Accessibility and Microphone grants it was supposed
# to preserve.
if security find-identity -v -p codesigning 2>/dev/null | grep -q "$CERT_NAME"; then
    export APPLE_SIGNING_IDENTITY="$CERT_NAME"
else
    echo
    echo "warning: signing identity \"$CERT_NAME\" not found — building UNSIGNED."
    echo "Every rebuild will get a new ad-hoc identity, silently revoking your"
    echo "Accessibility/Microphone grants. Run scripts/make-signing-cert.sh once"
    echo "to fix this."
    echo
fi

echo "Building Dikto (pnpm tauri build)..."
pnpm tauri build

APP_SRC="$BUNDLE_DIR/macos/$APP_NAME"

# No hardened runtime because it buys nothing without notarization, not
# because it can't work: the bundle ships zero dylibs or frameworks and
# `otool -L` on the binary lists only /System and /usr. cpal talks to
# CoreAudio, and enigo is cfg(not(target_os = "macos")) — not compiled here
# at all. A Developer ID move stays open.
echo "Signature as built:"
codesign -dv "$APP_SRC" 2>&1 | grep -E "Authority|flags" || true

echo "Installing to $DEST..."
rm -rf "$DEST"
cp -R "$APP_SRC" "$DEST"
xattr -dr com.apple.quarantine "$DEST"

echo
echo "Installed. Signature:"
codesign -dv --verbose=2 "$DEST" 2>&1 | grep -E "Identifier|Authority|Signature"
