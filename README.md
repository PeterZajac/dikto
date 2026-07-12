# Local Wispr Flow

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
- **History** — every dictation is saved locally (SQLite) and searchable.
  Delete individual entries or clear it all.
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
xattr -d com.apple.quarantine "/Applications/Local Wispr Flow.app"
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
   in your OS keychain, never in a config file. You can skip this step, but
   dictation won't work without a key.
3. **Meridian (optional)** — if you run [Meridian](https://github.com/rynfar/meridian)
   locally (default `http://127.0.0.1:3456`), the app will detect it and use
   it to clean up dictated text via Claude. Skip it and raw transcripts get
   pasted instead.

All of this can be revisited later from the Settings page.

## Privacy

- **Audio** is sent to Groq's API for transcription only while you're
  recording; it isn't stored beyond that.
- **Transcribed text** is sent to Meridian (and from there, Anthropic) only
  if cleanup is enabled and Meridian is reachable.
- **Everything else** — settings, dictation history, hotkey config — stays
  in local files and your OS keychain (Groq key). Nothing else leaves the
  machine.

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

## Architecture

The Tauri backend (`src-tauri/`) owns a hotkey listener (`rdev`), an audio
recorder (`cpal`), a small state machine driving the dictation phases, and
clients for Groq (STT) and Meridian (cleanup); a paste step (`enigo`) injects
the final text via the clipboard. The React frontend (`src/`) renders the
floating bubble, the settings/history/wizard windows, and talks to the
backend entirely through Tauri commands.

## License

MIT — see [LICENSE](LICENSE).
