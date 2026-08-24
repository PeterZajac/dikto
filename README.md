# Dikto

A small, self-hosted alternative to [Wispr Flow](https://wisprflow.ai/): hold a
hotkey, speak, and the transcribed (and optionally cleaned-up) text is pasted
at your cursor in any app. Built with Tauri v2 + React on the frontend and
Rust on the backend.

Why: Wispr Flow is a subscription SaaS that streams your audio to its own
servers. This app does the same job with services you control — a free Groq
API key for transcription, and (optionally) your own [Meridian](https://github.com/rynfar/meridian)
instance for text cleanup — with nothing else leaving your machine.

## Features

- **Global hotkey dictation** — hold to talk (push-to-talk) or double-tap to
  lock recording on/off; default hotkey is right Option (`AltGr`), remappable
  in Settings.
- **Speech-to-text via Groq** — `whisper-large-v3-turbo`, free tier, with
  Slovak/Czech/English or auto-detect language modes.
- **Optional Claude cleanup via Meridian** — fixes punctuation, capitalization
  and filler words before pasting; "light" or "strong" (light rephrasing)
  style. Falls back to the raw transcript automatically if Meridian isn't
  running.
- **Floating bubble** — shows a live waveform and transcript while recording,
  remembers where you last dragged it, stays out of the way of fullscreen
  apps.
- **Nothing gets lost** — the recording is written to disk and its history
  row created *before* transcription is attempted, so a rate limit, a network
  drop or a crash can never cost you a dictation. Failed takes stay in History
  with a "Prepísať znova" button and a download for the raw WAV.
- **Rate-limit resilience** — Groq 429s and server errors are retried with
  backoff honouring `Retry-After`, and a client-side throttle keeps the live
  preview from spending the quota the real transcription needs.
- **History** — every dictation is saved locally (SQLite) and searchable.
  Delete individual entries or clear it all. Audio is pruned after a
  configurable window (default 7 days); the text is kept indefinitely, and
  failed takes keep their audio until you delete them.
- **Tray icon** — quick access to language switching, settings, and quit;
  optional launch-on-login.
- **First-run wizard** — walks through permissions, the Groq key, and Meridian
  on first launch.

## Install

Grab the latest release from the [Releases page](../../releases).

### macOS

Download the `.dmg`, drag the app to Applications. Because the build isn't
notarized, Gatekeeper will refuse to open it the first time — clear the
quarantine flag from a terminal:

```sh
xattr -d com.apple.quarantine "/Applications/Dikto.app"
```

Then open it normally. You'll be asked for Microphone and Accessibility
permissions on first launch (needed to record audio and to paste into other
apps).

### Windows

Download the `.exe` installer. SmartScreen will likely flag it as unrecognized
since it isn't code-signed — click "More info" → "Run anyway" to proceed.

**Windows support is untested at runtime.** The code compiles and passes
`cargo check` in CI on `windows-latest`, but no one has run this app on an
actual Windows machine yet. Expect rough edges (see
[`docs/WINDOWS.md`](docs/WINDOWS.md) for the full list of what's unverified —
hotkey capture, clipboard injection, tray behavior, autostart, and the bubble
window's visual styling in particular).

## First-run setup

1. **Permissions** (macOS) — grant Microphone and Accessibility access when
   prompted; the wizard links straight to the right System Settings pane.
2. **Groq API key** — sign up for a free key at
   [console.groq.com](https://console.groq.com) and paste it in; it's stored
   in the app's local `settings.json` config file (plain text). You can skip
   this step, but dictation won't work without a key.
3. **Meridian (optional)** — if you run [Meridian](https://github.com/rynfar/meridian)
   locally (default `http://127.0.0.1:3456`), the app will detect it and use
   it to clean up dictated text via Claude. Settings has an "Otestovať" button
   that round-trips a real completion, so you can tell "answering" apart from
   "merely listening on the port". Skip it and raw transcripts get pasted
   instead.

All of this can be revisited later from the Settings page.

## Privacy

- **Audio** is sent to Groq's API for transcription. A copy is also written
  to `audio/` in the app's local data directory so a failed transcription can
  be retried or exported; it's pruned per the retention setting (default 7
  days for successful takes, kept until you delete them for failed ones).
- **Transcribed text** is sent to Meridian (and from there, Anthropic) only
  if cleanup is enabled and Meridian is reachable.
- **Everything else** — settings (including the Groq key, stored in plain
  text), dictation history, hotkey config — stays in local files. Nothing
  else leaves the machine.

## Development

Requires [pnpm](https://pnpm.io) (via `corepack enable`) and a Rust toolchain
(pinned in `rust-toolchain.toml`; run any `cargo`/`rustup` command inside the
repo and it auto-installs).

```sh
corepack enable
pnpm install
pnpm tauri dev
```

`pnpm build` builds the frontend only; `pnpm tauri build` produces a full
platform bundle. Backend tests: `cd src-tauri && cargo test`.

### Development install (macOS)

Dev builds are ad-hoc signed by default, and ad-hoc signatures embed a hash
of the binary itself — so the signing identity changes on *every* rebuild.
macOS ties TCC grants (Accessibility, Microphone) to that identity, so each
reinstall silently revokes them: System Settings still shows the toggle as
ON, but the permission is dead, and dictation/paste stop working until you
manually re-grant it. Signing dev builds with a fixed local identity instead
keeps the identity stable across rebuilds, so grants survive.

One-time setup:

```sh
scripts/make-signing-cert.sh
```

Creates and trusts a self-signed "Dikto Dev" code-signing certificate. Idempotent — safe to re-run.

For every iteration after that:

```sh
scripts/dev-install.sh
```

Builds, signs the app with "Dikto Dev", and installs it to `/Applications`,
clearing the quarantine flag. If the certificate isn't installed, it falls
back to an unsigned install with a warning instead of failing.

After switching from an unsigned/ad-hoc build to the signed one, reset the
stale TCC grants once and re-grant them:

```sh
tccutil reset All com.peterzajac.dikto
```

Then open Dikto and grant Accessibility/Microphone one final time — from
then on, rebuilds via `dev-install.sh` won't invalidate them again.

### Self-test

`Dikto --selftest <path-to-wav>` runs a headless pipeline check (settings +
Groq key, WAV decoding, Groq transcription, Meridian cleanup, clipboard
round-trip, and paste-event construction) without launching the GUI, printing
one `[PASS]`/`[FAIL]`/`[SKIP]` line per stage. Useful for verifying a signed
build end-to-end, especially the Groq API key and TCC-gated bits, without
having to dictate through the UI. Exits non-zero if any mandatory stage
fails.

## Architecture

The Tauri backend (`src-tauri/`) owns a hotkey listener (a raw CGEventTap on
macOS, `rdev` elsewhere), an audio
recorder (`cpal`), a small state machine driving the dictation phases, a
SQLite history store paired with an on-disk recording store, a client-side
rate limiter, and clients for Groq (STT) and Meridian (cleanup); a paste step
(`enigo`) injects the final text via the clipboard. The React frontend (`src/`) renders the
floating bubble, the settings/history/wizard windows, and talks to the
backend entirely through Tauri commands.

## License

MIT — see [LICENSE](LICENSE).
