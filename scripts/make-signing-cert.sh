#!/usr/bin/env bash
# One-time setup: creates a self-signed code-signing certificate ("Dikto Dev")
# and trusts it for code signing on this Mac.
#
# Why this is needed: Dikto's dev builds are ad-hoc signed (`codesign -s -`),
# which bakes a hash of the binary itself into the code-signing identity.
# Every rebuild produces a *different* identity, so macOS silently revokes
# the Accessibility/Microphone TCC grants after each reinstall — even though
# System Settings still shows the toggle as ON. Signing dev builds with a
# stable identity (this certificate) fixes that: the identity stays the same
# across rebuilds, so TCC grants survive reinstalls.
#
# This script is idempotent: if the identity already exists, it exits 0.
set -euo pipefail

CERT_NAME="Dikto Dev"
LOGIN_KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
SYSTEM_KEYCHAIN="/Library/Keychains/System.keychain"

if security find-identity -v -p codesigning 2>/dev/null | grep -q "$CERT_NAME"; then
    echo "Signing identity \"$CERT_NAME\" already present in the login keychain — nothing to do."
    exit 0
fi

echo "Creating self-signed code-signing certificate \"$CERT_NAME\"..."

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

KEY_PEM="$WORKDIR/dikto-dev-key.pem"
CERT_PEM="$WORKDIR/dikto-dev-cert.pem"
P12_PATH="$WORKDIR/dikto-dev.p12"
P12_PASSWORD="$(openssl rand -base64 24)"

openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
    -keyout "$KEY_PEM" -out "$CERT_PEM" \
    -subj "/CN=$CERT_NAME" \
    -addext "extendedKeyUsage=codeSigning" \
    -addext "basicConstraints=critical,CA:false" \
    -addext "keyUsage=critical,digitalSignature"

openssl pkcs12 -export \
    -inkey "$KEY_PEM" -in "$CERT_PEM" \
    -out "$P12_PATH" \
    -passout "pass:$P12_PASSWORD" \
    -name "$CERT_NAME"

echo "Importing into the login keychain (you may see a keychain access prompt)..."
security import "$P12_PATH" \
    -k "$LOGIN_KEYCHAIN" \
    -P "$P12_PASSWORD" \
    -T /usr/bin/codesign

echo
echo "Trusting \"$CERT_NAME\" for code signing system-wide requires admin rights."
echo "macOS will now ask for your account password (sudo)."
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k "$SYSTEM_KEYCHAIN" "$CERT_PEM"

echo
echo "Done. \"$CERT_NAME\" is installed and trusted for code signing."
echo "Run scripts/dev-install.sh to build and install Dikto signed with this identity."
