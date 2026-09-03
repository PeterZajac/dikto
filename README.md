# Dikto

Hold a hotkey, speak, and the transcribed text is typed at your cursor in
whatever app you're in. Speech-to-text runs through a free Groq API key,
optional Claude cleanup through your own [Meridian](https://github.com/rynfar/meridian)
instance, and everything else stays on your machine. Built with Tauri v2,
React, and Rust.

## Features

- **Global hotkey dictation** — hold to talk (push-to-talk) or double-tap to
  lock recording on/off. Default hotkey is right Option on macOS and right
  Ctrl on Windows; remappable in Settings.
- **Speech-to-text via Groq** — `whisper-large-v3-turbo`, free tier, with
  Slovak/Czech/English or auto-detect language modes.
- **Optional Claude cleanup via Meridian** — fixes punctuation, capitalization
  and filler words before pasting; "light" or "strong" (light rephrasing)
  style. Off by default; falls back to the raw transcript automatically if
  Meridian isn't running.
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
  Delete individual entries or clear it all. Completed dictations (text and
  audio) are deleted after a configurable window (default 7 days, checked at
  startup, hourly and after each dictation); failed takes are kept until you
  delete them.
- **Tray icon** — quick access to language switching, settings, and quit;
  optional launch-on-login.
- **First-run wizard** — walks through permissions, the Groq key, and Meridian
  on first launch.

The UI is in Slovak.

## Install a release

Grab the latest build from the [Releases page](../../releases).

### macOS

1. Download the `.dmg` and drag Dikto to Applications.
2. The build isn't notarized, so Gatekeeper refuses to open it the first
   time. Clear the quarantine flag from a terminal:

   ```sh
   xattr -d com.apple.quarantine "/Applications/Dikto.app"
   ```

3. Open Dikto. Grant **Microphone** and **Accessibility** when asked; the
   wizard links straight to the right System Settings pane. Without
   Accessibility the hotkey and text insertion don't work.

Every new build has a different ad-hoc code signature, so after updating you
may find Accessibility silently dead even though the toggle shows ON. Remove
Dikto from the Accessibility list and add it again.

### Windows (experimental)

1. Download the `.exe` installer. SmartScreen will flag it as unrecognized
   since it isn't code-signed — click "More info" → "Run anyway".
2. Open Dikto and allow microphone access when Windows asks.

**Windows has not been run by anyone yet.** It compiles and passes
`cargo check` in CI, but hotkey capture, paste injection, the tray and the
bubble window are all unverified — see [`docs/WINDOWS.md`](docs/WINDOWS.md).
If you try it, please report what worked.

### Uninstall

**macOS** — quit Dikto from the tray, then:

```sh
rm -rf /Applications/Dikto.app
rm -rf ~/Library/Application\ Support/com.peterzajac.dikto   # settings, history, recordings
rm -rf ~/Library/WebKit/com.peterzajac.dikto ~/Library/Caches/com.peterzajac.dikto
tccutil reset All com.peterzajac.dikto                       # forget the permission grants
```

If launch-on-login was enabled, also remove
`~/Library/LaunchAgents/com.peterzajac.dikto.plist`.

**Windows** — uninstall from Settings → Apps, then delete
`%APPDATA%\com.peterzajac.dikto` if you want the history gone too.

## First-run setup

1. **Permissions** (macOS) — grant Microphone and Accessibility access.
2. **Groq API key** — sign up for a free key at
   [console.groq.com](https://console.groq.com) and paste it in. It's stored
   in the app's local `settings.json` (plain text). Without a key dictation
   doesn't work.
3. **Meridian (optional)** — if you run [Meridian](https://github.com/rynfar/meridian)
   locally (default `http://127.0.0.1:3456`), Dikto detects it and can use it
   to clean up dictated text via Claude. Settings has an "Otestovať" button
   that round-trips a real completion. Skip it and raw transcripts get
   pasted instead.
4. **Try it** — the last wizard step has a text field: click into it, hold the
   hotkey, say a few words, release. The text should appear in the field.

All of this can be revisited later from the Settings page.

## Build from source

### 1. Prerequisites

**macOS**

- Xcode Command Line Tools: `xcode-select --install`
- Rust via [rustup](https://rustup.rs). The toolchain version is pinned in
  `rust-toolchain.toml`; the first `cargo` command inside the repo installs it.
- Node.js 22 with corepack: `corepack enable` (installs the pinned pnpm on
  first use).

**Windows**

- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with the "Desktop development with C++" workload.
- WebView2 Runtime (already present on Windows 10/11).
- Rust via [rustup](https://rustup.rs) (MSVC toolchain).
- Node.js 22, then `corepack enable`.

### 2. Clone and install

```sh
git clone https://github.com/PeterZajac/dikto.git
cd dikto
corepack enable
pnpm install
```

### 3. Run in development mode

```sh
pnpm tauri dev
```

The first run compiles all Rust dependencies and takes several minutes;
later runs are incremental. The app window opens with the first-run wizard.

On macOS, in dev mode the Microphone and Accessibility grants belong to the
**terminal** you launched from (Terminal, iTerm, VS Code), not to Dikto.
Grant them to that app in System Settings → Privacy & Security, otherwise the
hotkey never fires and nothing is pasted.

You can also skip the wizard's key step by putting the key in a `.env` file
at the repo root (gitignored):

```sh
echo 'GROQ_API_KEY=gsk_...' > .env
```

### 4. Verify it works

Open any text field, hold the hotkey (right Option on macOS, right Ctrl on
Windows), say a sentence, release. The bubble shows a waveform while you
speak and the text lands at the cursor. If nothing happens, the app's
Settings page shows a warning banner with the missing permission.

For a headless check of the whole pipeline without the GUI:

```sh
# macOS, after a build
./src-tauri/target/release/dikto --selftest path/to/some.wav
```

It prints one `[PASS]`/`[FAIL]`/`[SKIP]` line per stage (settings + Groq key,
WAV decoding, Groq transcription, Meridian cleanup, clipboard round-trip,
paste-event construction) and exits non-zero if a mandatory stage fails.

### 5. Build an installable bundle

```sh
pnpm tauri build
```

Output lands in `src-tauri/target/release/bundle/` (`dmg/` on macOS,
`nsis/` on Windows).

### macOS: install a dev build without losing permissions

Ad-hoc signatures embed a hash of the binary, so every rebuild changes the
signing identity and macOS silently drops the Accessibility/Microphone grants
tied to it. Signing with a fixed local certificate keeps the grants alive.

One-time setup, creates and trusts a self-signed "Dikto Dev" certificate:

```sh
scripts/make-signing-cert.sh
```

Then, for every iteration:

```sh
scripts/dev-install.sh
```

This builds, signs with "Dikto Dev", installs to `/Applications` and clears
the quarantine flag. After switching from an ad-hoc build to the signed one,
reset the stale grants once and re-grant them:

```sh
tccutil reset All com.peterzajac.dikto
```

### Tests and lint

```sh
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

CI runs the same on macOS plus `cargo check` on Windows for every push.

## Releasing

Push a tag and the release workflow builds a universal macOS `.dmg` and a
Windows `.exe`, attaching both to a **draft** GitHub release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Review the draft on the Releases page and publish it.

## Privacy

- **Audio** is sent to Groq's API for transcription. A copy is also written
  to `audio/` in the app's local data directory so a failed transcription can
  be retried or exported; it's deleted together with the history entry per
  the retention setting (default 7 days for successful takes, kept until you
  delete them for failed ones).
- **Transcribed text** is sent to Meridian (and from there, Anthropic) only
  if cleanup is enabled and Meridian is reachable.
- **Everything else** — settings (including the Groq key, stored in plain
  text), dictation history, hotkey config — stays in local files. Nothing
  else leaves the machine.

Local data lives in the OS app-data directory under `com.peterzajac.dikto`
(`~/Library/Application Support/com.peterzajac.dikto` on macOS,
`%APPDATA%\com.peterzajac.dikto` on Windows).

## Architecture

The Tauri backend (`src-tauri/`) owns a hotkey listener (a raw CGEventTap on
macOS, `rdev` elsewhere), an audio recorder (`cpal`), a small state machine
driving the dictation phases, a SQLite history store paired with an on-disk
recording store, a client-side rate limiter, and clients for Groq (STT) and
Meridian (cleanup). Text is delivered by copying it to the clipboard and
then typing it as unicode key events on macOS or sending `Ctrl+V` via
`enigo` on Windows. The React frontend (`src/`) renders the floating bubble
and the settings/history/wizard windows, and talks to the backend entirely
through Tauri commands.

## License

MIT — see [LICENSE](LICENSE).
