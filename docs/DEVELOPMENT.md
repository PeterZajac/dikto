# Development notes

## Tests and lint

```sh
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

CI runs the same on macOS plus `cargo check` on Windows for every push.

## Self-test

A headless check of the whole pipeline without the GUI:

```sh
./src-tauri/target/release/dikto --selftest path/to/some.wav
```

It prints one `[PASS]`/`[FAIL]`/`[SKIP]` line per stage (settings + Groq key,
WAV decoding, Groq transcription, Meridian cleanup, clipboard round-trip,
paste-event construction) and exits non-zero if a mandatory stage fails.

## Groq key in dev mode

Put the key in a `.env` at the repo root (gitignored) instead of typing it
into the wizard:

```sh
echo 'GROQ_API_KEY=gsk_...' > .env
```

## macOS: permissions and code signing

Release builds are ad-hoc signed (`bundle.macOS.signingIdentity: "-"` in
`tauri.conf.json`). An ad-hoc signature embeds a hash of the binary, so every
rebuild has a new identity and macOS drops the Accessibility/Microphone
grants tied to the old one: the toggle still shows ON but the permission is
dead. Remove Dikto from the Accessibility list and add it again after an
update.

For local iteration, sign with a fixed self-signed certificate so grants
survive rebuilds. One-time setup:

```sh
scripts/make-signing-cert.sh
```

Then for every iteration:

```sh
scripts/dev-install.sh
```

This builds, signs with "Dikto Dev", installs to `/Applications` and clears
the quarantine flag. After switching from an ad-hoc build to the signed one,
reset the stale grants once and re-grant them:

```sh
tccutil reset All com.peterzajac.dikto
```

In `pnpm tauri dev` the grants belong to the terminal you launched from, not
to Dikto.

## Releasing

Bump `version` in `package.json`, `src-tauri/tauri.conf.json` and
`src-tauri/Cargo.toml`, add a `CHANGELOG.md` entry, then:

```sh
git tag v0.1.2
git push origin v0.1.2
```

The release workflow builds a universal macOS `.dmg` and a Windows `.exe`
and attaches them to a **draft** GitHub release. Review it and publish.
