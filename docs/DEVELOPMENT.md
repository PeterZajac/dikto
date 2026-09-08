# Development notes

## Tests and lint

```sh
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

CI runs the same on macOS plus `cargo check` on Windows for every push.

`src-tauri/src/pipeline_e2e.rs` runs the real dictation pipeline end to end
on Tauri's `MockRuntime`: a generated WAV goes through a mock Groq, a mock
Meridian, the SQLite history and the recording store, and ends in the
clipboard. Delivery is always driven past its paste deadline so the test
never types into the app that has focus; the hotkey and microphone are the
only stages it does not exercise.

## UI tests (Playwright)

```sh
pnpm exec playwright install chromium   # once
pnpm test:e2e
```

They run the React windows (settings, history, wizard, bubble) against the
Vite dev server with the Tauri runtime replaced by an in-page mock
(`e2e/tauri-mock.ts`), once with a macOS user agent and once with a Windows
one. They cover UI flows and the IPC contract, not the Rust pipeline; that
is `cargo test`. `tauri-driver` (real WebDriver E2E) is not used because it
does not support macOS. Traces of failed runs land in `test-results/`.

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

Measured on the installed 0.1.6 build — `codesign -d -r-
/Applications/Dikto.app`:

```
designated => cdhash H"712cd364…" or cdhash H"7b5ab1b6…"
flags=0x2(adhoc)   TeamIdentifier=not set
```

TCC stores that requirement when you grant a permission and re-checks it on
every launch, and a cdhash is a hash of the code — hence the dead toggle. The
same command on a binary signed with "Dikto Dev" gives `identifier "…" and
certificate leaf H"…"` instead, which survives rebuilds.

`tauri.conf.json` keeps `signingIdentity: "-"` on purpose: a named identity
makes `tauri build` abort on any machine that doesn't have it in its keychain,
which would break a fresh clone and hide `dev-install.sh`'s own "identity not
found" warning. The real identity is supplied out of band — `dev-install.sh`
re-signs after the build, and the release workflow passes
`APPLE_SIGNING_IDENTITY`.

The updater delivers a bundle signed with whatever identity built it. "Dikto
Dev" is a self-signed root trusted only on the Mac that ran
`make-signing-cert.sh`, so on anyone else's machine that signature has an
invalid authority. Before updates go to anyone but the author, this needs a
Developer ID certificate — which in turn forces `hardenedRuntime: true` and
notarization.

Hardened runtime is off because it buys nothing without notarization, **not**
because the app can't take it: the bundle contains no dylibs or frameworks
and `otool -L` on the binary lists only `/System` and `/usr` paths. cpal uses
CoreAudio, and `enigo` is a `cfg(not(target_os = "macos"))` dependency that
is never compiled on macOS. A Developer ID move stays open.

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

```sh
pnpm bump 0.1.7
```

That rewrites `version` in `package.json`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`, verifies each one landed,
and turns the `## Unreleased` section of `CHANGELOG.md` into a dated one for
this version (or opens an empty section if there is none). It refuses to run
on a dirty tree and never commits or tags.

Write the bullets as the work lands, under `## Unreleased` — they become the
GitHub release body *and* the notes the in-app update banner shows, so they
are user-facing. Then:

```sh
git commit -am "chore: release v0.1.7"
git tag v0.1.7
git push origin main v0.1.7
```

The release workflow builds a universal macOS `.dmg` and a Windows `.exe`
and attaches them to a **draft** GitHub release. Review it and publish.
