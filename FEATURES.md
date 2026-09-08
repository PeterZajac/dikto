# Features

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
  with a retry button and a download for the raw WAV.
- **Rate-limit resilience** — Groq 429s and server errors are retried with
  backoff honouring `Retry-After`, and a client-side throttle keeps the live
  preview from spending the quota the real transcription needs.
- **History** — every dictation is saved locally (SQLite) and searchable.
  Delete individual entries or clear it all. Completed dictations (text and
  audio) are deleted after a configurable window (default 7 days, checked at
  startup, hourly and after each dictation); failed takes are kept until you
  delete them.
- **Notes** — a plain-text notepad inside the app: a list on the left, an
  editor on the right, autosaved as you type. Click into a note and dictate —
  the text is typed at the caret like anywhere else, and the same dictation
  still lands in History. Notes live in their own table, so History's "delete
  all" and the retention sweep never touch them.
- **In-app updates** — Dikto checks for a new release, shows what changed, and
  installs it when you click. Never silently, never in the background.
- **Tray icon** — quick access to dictation-language switching, settings, and
  quit; optional launch-on-login.
- **First-run wizard** — walks through permissions, the Groq key, and Meridian
  on first launch.
- **Interface language** — English by default, Slovak switchable in
  Settings → Interface language. Applies to all windows and the tray.

## Privacy

- **Audio** is sent to Groq's API for transcription. A copy is also written
  to `audio/` in the app's local data directory so a failed transcription can
  be retried or exported; it's deleted together with the history entry per
  the retention setting.
- **Transcribed text** is sent to Meridian (and from there, Anthropic) only
  if cleanup is enabled and Meridian is reachable.
- **Everything else** — settings (including the Groq key, stored in plain
  text), dictation history, notes, hotkey config — stays in local files.
- **Update checks** ask GitHub for the latest release manifest; nothing about
  you is sent with the request.

Local data lives under `com.peterzajac.dikto` in the OS app-data directory
(`~/Library/Application Support/com.peterzajac.dikto` on macOS,
`%APPDATA%\com.peterzajac.dikto` on Windows).

## Architecture

The Tauri backend (`src-tauri/`) owns a hotkey listener (a raw CGEventTap on
macOS, `rdev` elsewhere), an audio recorder (`cpal`), a small state machine
driving the dictation phases, a SQLite store holding both history and notes,
paired with an on-disk recording store, a client-side rate limiter, and clients for Groq (STT) and
Meridian (cleanup). Text is delivered by copying it to the clipboard and
then typing it as unicode key events on macOS or sending `Ctrl+V` via
`enigo` on Windows. The React frontend (`src/`) renders the floating bubble
and the settings/history/notes/wizard windows, and talks to the backend
entirely through Tauri commands. Updates go through `tauri-plugin-updater`,
which verifies each package against a minisign public key compiled into the
binary.
