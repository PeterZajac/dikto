#!/usr/bin/env bash
# Bumps Dikto's version in the four places that must agree, and opens a
# CHANGELOG entry for it.
#
# Why a script: the version lives in package.json, tauri.conf.json,
# Cargo.toml and (pinned) Cargo.lock. Miss one and the built app reports a
# version different from the tag, which quietly breaks the updater — it
# compares the installed version against the release manifest.
#
# Deliberately stops before git: the changelog bullets are the one part a
# human must write, so this never commits and never tags.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VERSION="${1:-}"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: pnpm bump <MAJOR.MINOR.PATCH>   (e.g. pnpm bump 0.1.7)" >&2
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "Working tree is dirty — commit or stash first." >&2
    exit 1
fi

sed -i '' -E "s/^(  \"version\": \")[^\"]+(\",)$/\1$VERSION\2/" package.json
sed -i '' -E "s/^(  \"version\": \")[^\"]+(\",)$/\1$VERSION\2/" src-tauri/tauri.conf.json
sed -i '' -E "s/^version = \".*\"$/version = \"$VERSION\"/" src-tauri/Cargo.toml

# Cargo.lock pins the crate's own version too; patch only the line inside
# the `dikto` package block.
awk -v v="$VERSION" '
    /^name = "dikto"$/ { in_dikto = 1 }
    in_dikto && /^version = / { print "version = \"" v "\""; in_dikto = 0; next }
    { print }
' src-tauri/Cargo.lock > src-tauri/Cargo.lock.tmp && mv src-tauri/Cargo.lock.tmp src-tauri/Cargo.lock

# Re-read every file: a sed that silently matched nothing is exactly the
# failure this script exists to prevent.
assert_version() {
    grep -qx "$2" "$1" || { echo "$1 was not updated to $VERSION" >&2; exit 1; }
}
assert_version package.json "  \"version\": \"$VERSION\","
assert_version src-tauri/tauri.conf.json "  \"version\": \"$VERSION\","
assert_version src-tauri/Cargo.toml "version = \"$VERSION\""
grep -A 1 -x 'name = "dikto"' src-tauri/Cargo.lock | grep -qx "version = \"$VERSION\"" \
    || { echo "src-tauri/Cargo.lock was not updated to $VERSION" >&2; exit 1; }

TODAY="$(date +%F)"
if grep -q "^## $VERSION " CHANGELOG.md; then
    echo "CHANGELOG.md already has a $VERSION section — leaving it alone."
elif grep -q '^## Unreleased$' CHANGELOG.md; then
    # Bullets written while the work landed become this release's section.
    sed -i '' "s/^## Unreleased\$/## $VERSION — $TODAY/" CHANGELOG.md
    echo "CHANGELOG.md: promoted the Unreleased section to $VERSION."
else
    awk -v v="$VERSION" -v today="$TODAY" '
        { print }
        !inserted && /^# Changelog$/ {
            print ""; print "## " v " — " today; print ""; print "- "
            inserted = 1
        }
    ' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md
fi

cat <<EOF

Bumped to $VERSION. Next:

  1. Check the CHANGELOG.md bullets for $VERSION read like release notes —
     they become the GitHub release body and the in-app update banner.
  2. git commit -am "chore: release v$VERSION"
  3. git tag v$VERSION && git push origin main v$VERSION
EOF
