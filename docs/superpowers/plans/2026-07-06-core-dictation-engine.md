# Core Dictation Engine + Bubble — Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Working push-to-talk dictation on macOS: hold a key → speak (SK/CS/EN) → Groq transcribes → Claude-via-Meridian cleans → text lands at the cursor of any app, with a floating bubble showing waveform + live transcript.

**Architecture:** Tauri v2 app. All system work (mic capture, global hotkey, HTTP, paste) lives in Rust; two webview windows (hidden main + floating bubble) render UI in React/TS. Rust emits events (`dictation:state`, `dictation:amplitude`, `dictation:partial`) that the bubble renders. Spec: `docs/superpowers/specs/2026-07-06-local-wispr-flow-design.md`.

**Tech Stack:** Tauri v2, Rust (cpal, rdev, hound, reqwest, enigo, arboard, keyring, thiserror, wiremock for tests), React 18 + TypeScript + Vite (multi-page), pnpm.

## Global Constraints

- Package manager: `pnpm`. Node ≥ 20, Rust stable (≥ 1.77), Tauri v2.
- App identifier: `com.peterzajac.localwisprflow`. Product name: `Local Wispr Flow`.
- Primary dev platform: macOS (Windows arrives in Plan 2 — keep all platform-specific code behind `#[cfg(target_os = ...)]`).
- Event names (exact): `dictation:state`, `dictation:amplitude`, `dictation:partial`.
- Window labels (exact): `main`, `bubble`.
- Groq model: `whisper-large-v3-turbo`. Cleanup default model: `claude-sonnet-5`. Meridian default URL: `http://127.0.0.1:3456`.
- Cleanup timeout: 5 s. Double-tap window: 300 ms.
- Principle from spec §6: **dictated text must never be lost** — every failure path ends with text in the clipboard or audio retained for retry.
- Bubble UI strings are Slovak (e.g. `✨ upravujem text…`, `✓ vložené`, `nič som nepočul`).
- Groq API key resolution order: env `GROQ_API_KEY` → OS keyring (service `local-wispr-flow`, user `groq`).
- Commit after every task; conventional commit messages (`feat:`, `test:`, `chore:`).

---

### Task 1: Scaffold Tauri v2 project with two windows

**Files:**
- Create: entire scaffold (via `create-tauri-app`), then modify:
- Modify: `package.json`, `src-tauri/tauri.conf.json`, `vite.config.ts`
- Create: `bubble.html`, `src/windows/main/main.tsx`, `src/windows/bubble/main.tsx`, `src-tauri/Info.plist`

**Interfaces:**
- Produces: running Tauri app with window labels `main` (visible) and `bubble` (hidden, transparent, always-on-top); Vite entry `bubble.html` for the bubble webview.

- [ ] **Step 1: Scaffold into a temp dir and merge into the repo root**

```bash
cd /Users/peterzajac/Documents/dev/personal/local-wispr-flow
pnpm create tauri-app@latest tmp-scaffold --template react-ts --manager pnpm --yes
cp -R tmp-scaffold/. .
rm -rf tmp-scaffold
```

- [ ] **Step 2: Fix project identity**

In `package.json` set `"name": "local-wispr-flow"`. In `src-tauri/tauri.conf.json` set:

```json
{
  "productName": "Local Wispr Flow",
  "identifier": "com.peterzajac.localwisprflow"
}
```

In `src-tauri/Cargo.toml` set `name = "local-wispr-flow"` (and the `[lib] name = "local_wispr_flow_lib"` that the scaffold generated stays as-is, just make it consistent with `src-tauri/src/main.rs`'s `local_wispr_flow_lib::run()` call).

- [ ] **Step 3: Configure the two windows + macOS private API**

In `src-tauri/tauri.conf.json` replace the `app` section:

```json
"app": {
  "macOSPrivateApi": true,
  "windows": [
    {
      "label": "main",
      "title": "Local Wispr Flow",
      "url": "index.html",
      "width": 900,
      "height": 620,
      "resizable": true
    },
    {
      "label": "bubble",
      "url": "bubble.html",
      "width": 340,
      "height": 64,
      "decorations": false,
      "transparent": true,
      "alwaysOnTop": true,
      "resizable": false,
      "visible": false,
      "skipTaskbar": true,
      "shadow": false,
      "focus": false
    }
  ],
  "security": { "csp": null }
}
```

(`macOSPrivateApi: true` is required for `transparent: true` on macOS.)

- [ ] **Step 4: Vite multi-page setup**

Create `bubble.html` in the repo root:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>bubble</title>
  </head>
  <body style="background: transparent">
    <div id="root"></div>
    <script type="module" src="/src/windows/bubble/main.tsx"></script>
  </body>
</html>
```

Restructure frontend sources:

```bash
mkdir -p src/windows/main src/windows/bubble src/shared
git mv src/App.tsx src/windows/main/App.tsx 2>/dev/null || mv src/App.tsx src/windows/main/App.tsx
mv src/main.tsx src/windows/main/main.tsx
rm -f src/App.css src/styles.css 2>/dev/null || true
```

Fix the import path inside `src/windows/main/main.tsx` (`./App`) and point `index.html`'s script tag to `/src/windows/main/main.tsx`. Create a minimal `src/windows/bubble/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <div style={{ color: "white" }}>bubble placeholder</div>
  </React.StrictMode>,
);
```

In `vite.config.ts` add the multi-page input:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        bubble: resolve(__dirname, "bubble.html"),
      },
    },
  },
});
```

- [ ] **Step 5: macOS Info.plist for microphone permission**

Create `src-tauri/Info.plist` (Tauri v2 merges it into the bundle automatically):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>NSMicrophoneUsageDescription</key>
  <string>Local Wispr Flow potrebuje mikrofón na diktovanie.</string>
</dict>
</plist>
```

Note for dev: `pnpm tauri dev` runs unbundled — macOS attributes the mic prompt to your terminal app. Grant it once when prompted.

- [ ] **Step 6: Run and verify**

Run: `pnpm install && pnpm tauri dev`
Expected: main window opens with the scaffold page; no bubble visible (it's `visible: false`); no compile errors. Verify the bubble page builds: open `http://localhost:1420/bubble.html` in a browser — shows "bubble placeholder".

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: scaffold Tauri v2 app with main + bubble windows"
```

---

### Task 2: Settings store + Groq API key resolution

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod settings;`), `src-tauri/Cargo.toml`

**Interfaces:**
- Produces (used by Tasks 6, 7, 9):
  - `struct Settings { hotkey: String, language: LanguageMode, cleanup_enabled: bool, cleanup_model: String, meridian_url: String, groq_url: String }`
  - `enum LanguageMode { Auto, Sk, Cs, En }` with `fn code(&self) -> Option<&'static str>`
  - `fn load(path: &Path) -> Settings`, `fn save(path: &Path, s: &Settings) -> std::io::Result<()>`
  - `fn groq_api_key() -> Option<String>`, `fn set_groq_api_key(key: &str) -> Result<(), keyring::Error>`

- [ ] **Step 1: Add dependencies**

In `src-tauri/Cargo.toml` add to `[dependencies]` (keep the scaffold's existing tauri/serde lines):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
keyring = { version = "3", features = ["apple-native", "windows-native"] }
thiserror = "2"
```

and add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/settings.rs` with tests at the bottom:

```rust
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageMode {
    Auto,
    Sk,
    Cs,
    En,
}

impl LanguageMode {
    /// ISO code passed to Groq; None = let Whisper auto-detect.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            LanguageMode::Auto => None,
            LanguageMode::Sk => Some("sk"),
            LanguageMode::Cs => Some("cs"),
            LanguageMode::En => Some("en"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// rdev key as its Debug string, e.g. "AltGr" (right Option on mac).
    pub hotkey: String,
    pub language: LanguageMode,
    pub cleanup_enabled: bool,
    pub cleanup_model: String,
    pub meridian_url: String,
    pub groq_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "AltGr".into(),
            language: LanguageMode::Auto,
            cleanup_enabled: true,
            cleanup_model: "claude-sonnet-5".into(),
            meridian_url: "http://127.0.0.1:3456".into(),
            groq_url: "https://api.groq.com".into(),
        }
    }
}

pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(s)?)
}

const KEYRING_SERVICE: &str = "local-wispr-flow";
const KEYRING_USER: &str = "groq";

pub fn groq_api_key() -> Option<String> {
    if let Ok(k) = std::env::var("GROQ_API_KEY") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
}

pub fn set_groq_api_key(key: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?.set_password(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings::default();
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn missing_file_yields_default() {
        assert_eq!(load(Path::new("/nonexistent/nope.json")), Settings::default());
    }

    #[test]
    fn corrupt_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), Settings::default());
    }

    #[test]
    fn partial_file_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"language":"sk"}"#).unwrap();
        let s = load(&p);
        assert_eq!(s.language, LanguageMode::Sk);
        assert_eq!(s.hotkey, "AltGr");
    }

    #[test]
    fn language_codes() {
        assert_eq!(LanguageMode::Auto.code(), None);
        assert_eq!(LanguageMode::Sk.code(), Some("sk"));
        assert_eq!(LanguageMode::Cs.code(), Some("cs"));
        assert_eq!(LanguageMode::En.code(), Some("en"));
    }
}
```

Add `mod settings;` to `src-tauri/src/lib.rs` (top of file, alongside existing items).

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test settings -- --nocapture`
Expected: 5 tests PASS (module compiles first try since code and tests land together; if compile fails, fix and re-run).

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: settings store with JSON persistence and keyring-backed Groq key"
```

---

### Task 3: Dictation state machine

**Files:**
- Create: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod state;`)

**Interfaces:**
- Produces (used by Task 9):
  - `enum Phase { Idle, Recording, Transcribing, Cleaning, Injecting, Error }` (serializes to snake_case)
  - `enum Event { StartRequested, StopRequested, Cancel, TranscriptReady, CleanupDone, Injected, Failed }`
  - `fn transition(p: Phase, e: Event) -> Option<Phase>` — `None` = illegal transition, ignore the event.

- [ ] **Step 1: Write the failing test + implementation**

Create `src-tauri/src/state.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Injecting,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    StartRequested,
    StopRequested,
    Cancel,
    TranscriptReady,
    CleanupDone,
    Injected,
    Failed,
}

/// Returns the next phase, or None when the event is illegal in this phase
/// (caller ignores it — e.g. a stray key-up while Idle).
pub fn transition(p: Phase, e: Event) -> Option<Phase> {
    use Event::*;
    use Phase::*;
    match (p, e) {
        (Idle, StartRequested) => Some(Recording),
        (Error, StartRequested) => Some(Recording),
        (Recording, StopRequested) => Some(Transcribing),
        (Recording, Cancel) => Some(Idle),
        (Transcribing, TranscriptReady) => Some(Cleaning),
        (Transcribing, Cancel) => Some(Idle),
        (Cleaning, CleanupDone) => Some(Injecting),
        (Cleaning, Cancel) => Some(Idle),
        (Injecting, Injected) => Some(Idle),
        (_, Failed) => Some(Error),
        (Error, Cancel) => Some(Idle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;
    use Phase::*;

    #[test]
    fn happy_path() {
        let mut p = Idle;
        for (e, want) in [
            (StartRequested, Recording),
            (StopRequested, Transcribing),
            (TranscriptReady, Cleaning),
            (CleanupDone, Injecting),
            (Injected, Idle),
        ] {
            p = transition(p, e).unwrap();
            assert_eq!(p, want);
        }
    }

    #[test]
    fn cancel_returns_to_idle_from_active_phases() {
        for p in [Recording, Transcribing, Cleaning, Error] {
            assert_eq!(transition(p, Cancel), Some(Idle));
        }
    }

    #[test]
    fn failure_from_any_phase_goes_to_error() {
        for p in [Recording, Transcribing, Cleaning, Injecting] {
            assert_eq!(transition(p, Failed), Some(Error));
        }
    }

    #[test]
    fn error_can_restart() {
        assert_eq!(transition(Error, StartRequested), Some(Recording));
    }

    #[test]
    fn illegal_events_are_none() {
        assert_eq!(transition(Idle, StopRequested), None);
        assert_eq!(transition(Idle, Injected), None);
        assert_eq!(transition(Recording, StartRequested), None);
    }

    #[test]
    fn phase_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Transcribing).unwrap(), "\"transcribing\"");
    }
}
```

Add `mod state;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test state::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "feat: dictation phase state machine"
```

---

### Task 4: Hotkey interpreter (push-to-talk + double-tap toggle) and rdev listener

**Files:**
- Create: `src-tauri/src/hotkey.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod hotkey;`), `src-tauri/Cargo.toml` (add `rdev = "0.5"`)

**Interfaces:**
- Produces (used by Task 9):
  - `enum Action { Start, Stop, None }` — output of the pure interpreter
  - `struct Interpreter` with `fn new() -> Self`, `fn key_down(&mut self, t_ms: u128) -> Action`, `fn key_up(&mut self, t_ms: u128) -> Action`, `fn tick(&mut self, t_ms: u128) -> Action`
  - `enum HotkeySignal { Start, Stop, Cancel }` — sent over an `std::sync::mpsc::Sender<HotkeySignal>`
  - `fn spawn(hotkey: Arc<RwLock<String>>, tx: mpsc::Sender<HotkeySignal>)` — spawns the rdev listener thread + 50 ms tick thread. Matches keys via `format!("{:?}", key) == *hotkey.read()`. `Escape` while active sends `Cancel`.
  - Const `TAP_MS: u128 = 300`.

**Semantics (from spec §4):** hold > 300 ms = push-to-talk (Start on down, Stop on up). Quick tap (< 300 ms) arms a double-tap window: second down within 300 ms locks recording on (toggle); the next down stops it. A quick tap with no second tap stops recording at window expiry (yields "nič som nepočul" for near-empty audio).

- [ ] **Step 1: Write the failing tests + implementation**

Create `src-tauri/src/hotkey.rs`:

```rust
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TAP_MS: u128 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    None,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Idle,
    /// Key held after initial down; recording is running.
    Ptt { down: u128 },
    /// Quick tap released; recording continues while we wait for a 2nd tap.
    TapArmed { up: u128 },
    /// Double-tap lock: recording until next key-down.
    Locked,
    /// Stop was emitted on key-down; swallow the matching key-up.
    Stopping,
}

pub struct Interpreter {
    mode: Mode,
}

impl Interpreter {
    pub fn new() -> Self {
        Self { mode: Mode::Idle }
    }

    pub fn key_down(&mut self, t: u128) -> Action {
        match self.mode {
            Mode::Idle => {
                self.mode = Mode::Ptt { down: t };
                Action::Start
            }
            Mode::TapArmed { .. } => {
                self.mode = Mode::Locked;
                Action::None
            }
            Mode::Locked => {
                self.mode = Mode::Stopping;
                Action::Stop
            }
            _ => Action::None,
        }
    }

    pub fn key_up(&mut self, t: u128) -> Action {
        match self.mode {
            Mode::Ptt { down } if t.saturating_sub(down) < TAP_MS => {
                self.mode = Mode::TapArmed { up: t };
                Action::None
            }
            Mode::Ptt { .. } => {
                self.mode = Mode::Idle;
                Action::Stop
            }
            Mode::Stopping => {
                self.mode = Mode::Idle;
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Call every ~50 ms; resolves an expired double-tap window.
    pub fn tick(&mut self, t: u128) -> Action {
        if let Mode::TapArmed { up } = self.mode {
            if t.saturating_sub(up) >= TAP_MS {
                self.mode = Mode::Idle;
                return Action::Stop;
            }
        }
        Action::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySignal {
    Start,
    Stop,
    Cancel,
}

pub fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

/// Spawns the global rdev listener + tick thread. Never returns handles —
/// both threads live for the app's lifetime.
pub fn spawn(hotkey: Arc<RwLock<String>>, tx: mpsc::Sender<HotkeySignal>) {
    let interp = Arc::new(std::sync::Mutex::new(Interpreter::new()));

    {
        let interp = interp.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let send = move |a: Action| match a {
                Action::Start => { let _ = tx.send(HotkeySignal::Start); }
                Action::Stop => { let _ = tx.send(HotkeySignal::Stop); }
                Action::None => {}
            };
            let result = rdev::listen(move |ev| {
                let key_name = match ev.event_type {
                    rdev::EventType::KeyPress(k) => Some((format!("{k:?}"), true)),
                    rdev::EventType::KeyRelease(k) => Some((format!("{k:?}"), false)),
                    _ => None,
                };
                if let Some((name, is_down)) = key_name {
                    if name == "Escape" && is_down {
                        // Cancel handled unconditionally; pipeline ignores it when Idle.
                        // (send() above only covers Start/Stop)
                        return;
                    }
                    let target = hotkey.read().unwrap().clone();
                    if name == target {
                        let t = now_ms();
                        let mut i = interp.lock().unwrap();
                        let a = if is_down { i.key_down(t) } else { i.key_up(t) };
                        send(a);
                    }
                }
            });
            if let Err(e) = result {
                eprintln!("hotkey listener failed: {e:?} (missing Accessibility permission?)");
            }
        });
    }

    // tick thread
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let a = interp.lock().unwrap().tick(now_ms());
        if a == Action::Stop {
            let _ = tx.send(HotkeySignal::Stop);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_is_push_to_talk() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.tick(200), Action::None);
        assert_eq!(i.key_up(500), Action::Stop);
    }

    #[test]
    fn double_tap_locks_and_third_press_stops() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.key_up(100), Action::None); // quick tap → armed
        assert_eq!(i.key_down(200), Action::None); // 2nd tap → locked
        assert_eq!(i.key_up(280), Action::None);
        assert_eq!(i.tick(1000), Action::None); // locked: no timeout stop
        assert_eq!(i.key_down(5000), Action::Stop); // 3rd press stops
        assert_eq!(i.key_up(5050), Action::None); // its release swallowed
    }

    #[test]
    fn single_quick_tap_stops_on_window_expiry() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.key_up(100), Action::None);
        assert_eq!(i.tick(350), Action::None); // 250 ms after up: still armed
        assert_eq!(i.tick(401), Action::Stop); // window expired
    }

    #[test]
    fn after_stop_cycle_can_start_again() {
        let mut i = Interpreter::new();
        i.key_down(0);
        i.key_up(500); // ptt stop
        assert_eq!(i.key_down(1000), Action::Start);
    }
}
```

Add `rdev = "0.5"` to `[dependencies]` in `src-tauri/Cargo.toml` and `mod hotkey;` to `lib.rs`.

Note: the `Escape` branch above intentionally does nothing yet — Task 12 wires `Cancel` through it. The `spawn` listener sends only Start/Stop for now.

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test hotkey::`
Expected: 4 tests PASS.

- [ ] **Step 3: Manual smoke of the listener (macOS Accessibility)**

Temporarily add to `lib.rs` `run()` setup closure:

```rust
let hk = std::sync::Arc::new(std::sync::RwLock::new("AltGr".to_string()));
let (tx, rx) = std::sync::mpsc::channel();
crate::hotkey::spawn(hk, tx);
std::thread::spawn(move || {
    while let Ok(sig) = rx.recv() {
        println!("HOTKEY SIGNAL: {sig:?}");
    }
});
```

Run `pnpm tauri dev`. macOS will require Accessibility permission for your terminal (System Settings → Privacy & Security → Accessibility — add your terminal app, restart `tauri dev`). Then:
- Hold right Option ~1 s, release → console prints `Start` then `Stop`.
- Double-tap right Option, wait, tap again → `Start` … `Stop`.

Keep this wiring — Task 9 replaces the `println!` loop with the pipeline.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: global hotkey with push-to-talk and double-tap toggle"
```

---

### Task 5: Audio capture, resampling, WAV encoding

**Files:**
- Create: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod audio;`), `src-tauri/Cargo.toml` (add `cpal = "0.15"`, `hound = "3.5"`)

**Interfaces:**
- Produces (used by Tasks 9, 11):
  - `fn to_mono(samples: &[f32], channels: u16) -> Vec<f32>`
  - `fn resample_linear(mono: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32>`
  - `fn prepare_wav(samples: &[f32], src_rate: u32, channels: u16) -> Vec<u8>` — 16 kHz mono 16-bit WAV bytes
  - `struct Recorder` with `fn new() -> Self`, `fn start(&self, on_amplitude: Box<dyn Fn(f32) + Send>) -> Result<(), String>`, `fn snapshot(&self) -> Option<(Vec<f32>, u32, u16)>`, `fn stop(&self) -> Option<(Vec<f32>, u32, u16)>`, `fn duration_secs(&self) -> f32`

- [ ] **Step 1: Write failing tests + pure functions**

Create `src-tauri/src/audio.rs`:

```rust
use std::io::Cursor;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

pub fn resample_linear(mono: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || mono.is_empty() {
        return mono.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = (mono.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono[idx];
            let b = *mono.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

pub const TARGET_RATE: u32 = 16_000;

pub fn prepare_wav(samples: &[f32], src_rate: u32, channels: u16) -> Vec<u8> {
    let mono = to_mono(samples, channels);
    let resampled = resample_linear(&mono, src_rate, TARGET_RATE);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("wav writer");
        for s in resampled {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v).expect("wav sample");
        }
        writer.finalize().expect("wav finalize");
    }
    cursor.into_inner()
}

pub struct Recorder {
    buf: Arc<Mutex<Vec<f32>>>,
    meta: Mutex<Option<(u32, u16)>>,
    stop_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            buf: Arc::new(Mutex::new(Vec::new())),
            meta: Mutex::new(None),
            stop_tx: Mutex::new(None),
        }
    }

    /// Starts capture on the default input device. `on_amplitude` receives an
    /// RMS value (0..~1) per audio callback; the caller throttles UI emits.
    pub fn start(&self, on_amplitude: Box<dyn Fn(f32) + Send>) -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let mut stop_guard = self.stop_tx.lock().unwrap();
        if stop_guard.is_some() {
            return Err("already recording".into());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no input device".to_string())?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("input config: {e}"))?;
        let rate = config.sample_rate().0;
        let channels = config.channels();

        self.buf.lock().unwrap().clear();
        *self.meta.lock().unwrap() = Some((rate, channels));

        let buf = self.buf.clone();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        *stop_guard = Some(stop_tx);
        drop(stop_guard);

        // cpal::Stream is !Send on macOS → own it on a dedicated thread.
        std::thread::spawn(move || {
            let stream = device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let rms = (data.iter().map(|s| s * s).sum::<f32>()
                        / data.len().max(1) as f32)
                        .sqrt();
                    on_amplitude(rms);
                    buf.lock().unwrap().extend_from_slice(data);
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            );
            match stream {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        let _ = ready_tx.send(Err(format!("play: {e}")));
                        return;
                    }
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv(); // park until stop; dropping ends stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("build stream: {e}")));
                }
            }
        });

        ready_rx
            .recv()
            .map_err(|_| "audio thread died".to_string())?
    }

    /// Copy of everything captured so far (used for live partials).
    pub fn snapshot(&self) -> Option<(Vec<f32>, u32, u16)> {
        let (rate, ch) = (*self.meta.lock().unwrap())?;
        Some((self.buf.lock().unwrap().clone(), rate, ch))
    }

    pub fn duration_secs(&self) -> f32 {
        match *self.meta.lock().unwrap() {
            Some((rate, ch)) => {
                self.buf.lock().unwrap().len() as f32 / (rate as f32 * ch as f32)
            }
            None => 0.0,
        }
    }

    /// Stops capture and returns the full take.
    pub fn stop(&self) -> Option<(Vec<f32>, u32, u16)> {
        if let Some(tx) = self.stop_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        let out = self.snapshot();
        *self.meta.lock().unwrap() = None;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_passthrough_and_stereo_average() {
        assert_eq!(to_mono(&[0.5, 0.5, 1.0], 1), vec![0.5, 0.5, 1.0]);
        assert_eq!(to_mono(&[0.0, 1.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_halves_length_from_32k() {
        let src: Vec<f32> = vec![0.25; 3200];
        let out = resample_linear(&src, 32_000, 16_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let src = vec![0.1_f32, 0.2, 0.3];
        assert_eq!(resample_linear(&src, 16_000, 16_000), src);
    }

    #[test]
    fn wav_has_riff_header_and_correct_data_size() {
        let samples = vec![0.0_f32; 16_000]; // 1s @ 16k mono
        let wav = prepare_wav(&samples, 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 44-byte canonical header + 2 bytes per sample
        assert_eq!(wav.len(), 44 + 16_000 * 2);
    }
}
```

Add `cpal = "0.15"` and `hound = "3.5"` to `[dependencies]`; `mod audio;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test audio::`
Expected: 4 tests PASS.

- [ ] **Step 3: Manual capture smoke**

Temporarily extend the Task 4 debug loop in `lib.rs`: on `Start` call `recorder.start(Box::new(|_|{}))`, on `Stop` call `recorder.stop()` and print `prepare_wav(...)` byte length + `duration_secs`. Run `pnpm tauri dev`, grant mic permission, hold hotkey, speak 2 s, release. Expected: printed WAV length ≈ `44 + 32000*2` for ~2 s. Remove the print, keep recorder available for Task 9.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: cpal recorder with 16k mono wav export and rms amplitude"
```

---

### Task 6: Groq STT client

**Files:**
- Create: `src-tauri/src/stt.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod stt;`), `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: WAV bytes from `audio::prepare_wav` (Task 5), `Settings.groq_url` + `settings::groq_api_key()` (Task 2).
- Produces (used by Tasks 9, 11):
  - `struct SttClient` with `fn new(base_url: String, api_key: String) -> Self`
  - `async fn transcribe(&self, wav: Vec<u8>, language: Option<&str>) -> Result<Transcript, SttError>`
  - `struct Transcript { text: String, language: Option<String> }`
  - `enum SttError { Network(String), Api { status: u16, body: String } }`

- [ ] **Step 1: Add dependencies**

In `src-tauri/Cargo.toml` `[dependencies]` add:

```toml
reqwest = { version = "0.12", features = ["json", "multipart"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
```

and in `[dev-dependencies]` add:

```toml
wiremock = "0.6"
```

- [ ] **Step 2: Write failing tests + implementation**

Create `src-tauri/src/stt.rs`:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("network: {0}")]
    Network(String),
    #[error("groq api {status}: {body}")]
    Api { status: u16, body: String },
}

#[derive(Deserialize)]
struct GroqResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

pub struct SttClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

pub const GROQ_MODEL: &str = "whisper-large-v3-turbo";

impl SttClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            http: reqwest::Client::new(),
        }
    }

    pub async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
    ) -> Result<Transcript, SttError> {
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| SttError::Network(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", GROQ_MODEL)
            .text("response_format", "verbose_json");
        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/openai/v1/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| SttError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(SttError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let g: GroqResponse = resp
            .json()
            .await
            .map_err(|e| SttError::Network(e.to_string()))?;
        Ok(Transcript {
            text: g.text.trim().to_string(),
            language: g.language,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn transcribes_and_reads_language() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/v1/audio/transcriptions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "  ahoj svet  ",
                "language": "sk"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "test-key".into());
        let t = client.transcribe(vec![1, 2, 3], None).await.unwrap();
        assert_eq!(t.text, "ahoj svet");
        assert_eq!(t.language.as_deref(), Some("sk"));
    }

    #[tokio::test]
    async fn api_error_surfaces_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        match client.transcribe(vec![0], Some("sk")).await {
            Err(SttError::Api { status: 429, body }) => assert_eq!(body, "rate limited"),
            other => panic!("expected 429, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn network_error_when_server_unreachable() {
        let client = SttClient::new("http://127.0.0.1:1".into(), "k".into());
        assert!(matches!(
            client.transcribe(vec![0], None).await,
            Err(SttError::Network(_))
        ));
    }
}
```

Add `mod stt;` to `lib.rs`.

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test stt::`
Expected: 3 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: groq whisper stt client with language auto-detect"
```

---

### Task 7: Meridian (Claude) cleanup client

**Files:**
- Create: `src-tauri/src/cleanup.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod cleanup;`)

**Interfaces:**
- Consumes: `Settings.meridian_url`, `Settings.cleanup_model` (Task 2).
- Produces (used by Task 9):
  - `struct CleanupClient` with `fn new(base_url: String, model: String) -> Self` (5 s timeout) and `fn with_timeout(base_url: String, model: String, timeout: Duration) -> Self` (for tests)
  - `async fn clean(&self, raw: &str) -> Result<String, CleanupError>`
  - `enum CleanupError { Network(String), Api { status: u16, body: String }, Empty }` — **any** error means the caller falls back to raw text (spec §6).

- [ ] **Step 1: Write failing tests + implementation**

Create `src-tauri/src/cleanup.rs`:

```rust
use serde::Deserialize;
use std::time::Duration;

pub const SYSTEM_PROMPT: &str = "You clean up dictated text. Fix punctuation and capitalization, \
remove filler words (e.g. \"ehm\", \"\u{e9}\", \"proste\", \"ako\u{17e}e\", \"um\", \"like\", \"you know\"), \
and fix obvious mis-transcriptions. Keep the original language (Slovak, Czech or English). \
Keep the meaning and wording otherwise unchanged. Never add new information. \
Never answer questions contained in the text \u{2014} only clean it. \
Output ONLY the cleaned text, with no quotes and no commentary.";

#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("network: {0}")]
    Network(String),
    #[error("meridian api {status}: {body}")]
    Api { status: u16, body: String },
    #[error("empty response")]
    Empty,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

pub struct CleanupClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

impl CleanupClient {
    pub fn new(base_url: String, model: String) -> Self {
        Self::with_timeout(base_url, model, CLEANUP_TIMEOUT)
    }

    pub fn with_timeout(base_url: String, model: String, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn clean(&self, raw: &str) -> Result<String, CleanupError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": SYSTEM_PROMPT,
            "messages": [{ "role": "user", "content": raw }]
        });
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", "local")
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| CleanupError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(CleanupError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let parsed: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| CleanupError::Network(e.to_string()))?;
        let text = parsed
            .content
            .iter()
            .find(|b| b.kind == "text")
            .map(|b| b.text.trim().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            return Err(CleanupError::Empty);
        }
        Ok(text)
    }

    /// Cheap reachability probe used for the settings UI later (Plan 2)
    /// and by the pipeline to decide whether to even attempt cleanup.
    pub async fn is_reachable(&self) -> bool {
        self.http.get(&self.base_url).send().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn cleans_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(body_partial_json(serde_json::json!({
                "model": "claude-sonnet-5",
                "messages": [{ "role": "user", "content": "no proste ahoj svet akoze" }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "Ahoj, svet." }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "claude-sonnet-5".into());
        assert_eq!(
            c.clean("no proste ahoj svet akoze").await.unwrap(),
            "Ahoj, svet."
        );
    }

    #[tokio::test]
    async fn timeout_is_a_network_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(500))
                    .set_body_json(serde_json::json!({"content": []})),
            )
            .mount(&server)
            .await;

        let c = CleanupClient::with_timeout(
            server.uri(),
            "m".into(),
            Duration::from_millis(50),
        );
        assert!(matches!(c.clean("x").await, Err(CleanupError::Network(_))));
    }

    #[tokio::test]
    async fn empty_content_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "   " }]
            })))
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "m".into());
        assert!(matches!(c.clean("x").await, Err(CleanupError::Empty)));
    }

    #[tokio::test]
    async fn api_error_surfaces_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "m".into());
        assert!(matches!(
            c.clean("x").await,
            Err(CleanupError::Api { status: 500, .. })
        ));
    }
}
```

Add `mod cleanup;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test cleanup::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri
git commit -m "feat: claude-via-meridian text cleanup client with 5s timeout"
```

---

### Task 8: Text injection at cursor (clipboard swap + paste keystroke)

**Files:**
- Create: `src-tauri/src/inject.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod inject;`), `src-tauri/Cargo.toml` (add `arboard = "3"`, `enigo = "0.2"`)

**Interfaces:**
- Produces (used by Tasks 9, 12):
  - `fn inject_text(text: &str) -> Result<(), InjectError>` — pastes at cursor, restores previous clipboard.
  - `fn copy_only(text: &str) -> Result<(), InjectError>` — fallback path: leaves text in clipboard.
  - `enum InjectError { Clipboard(String), Keystroke(String) }`

- [ ] **Step 1: Implementation**

Create `src-tauri/src/inject.rs`:

```rust
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error("keystroke: {0}")]
    Keystroke(String),
}

pub fn copy_only(text: &str) -> Result<(), InjectError> {
    let mut cb = arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    cb.set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))
}

/// Saves the clipboard, puts `text` in it, simulates Cmd/Ctrl+V into the
/// frontmost app, then restores the original clipboard.
pub fn inject_text(text: &str) -> Result<(), InjectError> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut cb = arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    let previous = cb.get_text().ok();
    cb.set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))?;

    // Give the OS clipboard a beat before pasting.
    std::thread::sleep(Duration::from_millis(80));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| InjectError::Keystroke(e.to_string()))?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let press = |e: &mut Enigo| -> Result<(), InjectError> {
        e.key(modifier, Direction::Press)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        e.key(Key::Unicode('v'), Direction::Click)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        e.key(modifier, Direction::Release)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        Ok(())
    };
    let result = press(&mut enigo);

    // Let the paste land before restoring the old clipboard.
    std::thread::sleep(Duration::from_millis(150));
    if let Some(prev) = previous {
        let _ = cb.set_text(prev);
    }
    result
}
```

Add `arboard = "3"` and `enigo = "0.2"` to `[dependencies]`; `mod inject;` to `lib.rs`.

- [ ] **Step 2: Build check**

Run: `cd src-tauri && cargo build`
Expected: compiles clean (no unit tests here — behavior is purely OS-side; the module is exercised end-to-end in Task 9's manual verification).

- [ ] **Step 3: Manual verification**

Temporarily add to the `lib.rs` setup closure:

```rust
std::thread::spawn(|| {
    std::thread::sleep(std::time::Duration::from_secs(5));
    if let Err(e) = crate::inject::inject_text("Ahoj z Local Wispr Flow!") {
        eprintln!("inject failed: {e}");
    }
});
```

Run `pnpm tauri dev`, within 5 s click into TextEdit/Notes. Expected: the sentence appears at the cursor; whatever was in your clipboard before is still there afterwards (test by copying something first). macOS asks for Accessibility permission for the terminal if not yet granted. Remove the temp block after verifying.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: paste-at-cursor injection with clipboard preservation"
```

---

### Task 9: Pipeline orchestration — headless end-to-end dictation (MILESTONE)

**Files:**
- Create: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/src/lib.rs` (full rewrite of `run()` shown below)

**Interfaces:**
- Consumes: everything from Tasks 2–8.
- Produces (used by Tasks 10–12):
  - `struct AppCtx { phase: Mutex<Phase>, recorder: audio::Recorder, settings: RwLock<Settings>, pending_wav: Mutex<Option<Vec<u8>>>, partial_inflight: AtomicBool, app: AppHandle }`
  - `fn handle_signal(ctx: Arc<AppCtx>, sig: HotkeySignal)` — entry point from the hotkey channel
  - `fn set_phase(ctx: &AppCtx, phase: Phase, message: Option<&str>)` — updates state + emits `dictation:state` with payload `{ "phase": "...", "message": "...|null" }`
  - Tauri events emitted: `dictation:state`, `dictation:amplitude` (payload `{ "value": f32 }`)

- [ ] **Step 1: Write the pipeline**

Create `src-tauri/src/pipeline.rs`:

```rust
use crate::audio::{self, Recorder};
use crate::cleanup::CleanupClient;
use crate::hotkey::HotkeySignal;
use crate::settings::{self, Settings};
use crate::state::{transition, Event, Phase};
use crate::stt::SttClient;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppCtx {
    pub phase: Mutex<Phase>,
    pub recorder: Recorder,
    pub settings: RwLock<Settings>,
    pub pending_wav: Mutex<Option<Vec<u8>>>,
    pub partial_inflight: AtomicBool,
    pub app: AppHandle,
}

pub fn set_phase(ctx: &AppCtx, phase: Phase, message: Option<&str>) {
    *ctx.phase.lock().unwrap() = phase;
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": phase, "message": message }),
    );
}

fn apply(ctx: &AppCtx, ev: Event) -> Option<Phase> {
    let mut guard = ctx.phase.lock().unwrap();
    let next = transition(*guard, ev)?;
    *guard = next;
    Some(next)
}

fn show_bubble(ctx: &AppCtx) {
    if let Some(w) = ctx.app.get_webview_window("bubble") {
        let _ = w.show();
    }
}

fn hide_bubble_after(ctx: &Arc<AppCtx>, ms: u64) {
    let ctx = ctx.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        // Only hide when we're back to Idle (a new take may have started).
        if *ctx.phase.lock().unwrap() == Phase::Idle {
            if let Some(w) = ctx.app.get_webview_window("bubble") {
                let _ = w.hide();
            }
        }
    });
}

pub fn handle_signal(ctx: Arc<AppCtx>, sig: HotkeySignal) {
    match sig {
        HotkeySignal::Start => start_recording(&ctx),
        HotkeySignal::Stop => {
            if apply(&ctx, Event::StopRequested).is_some() {
                set_phase(&ctx, Phase::Transcribing, None);
                let ctx2 = ctx.clone();
                tauri::async_runtime::spawn(async move { finish(ctx2).await });
            }
        }
        HotkeySignal::Cancel => cancel(&ctx),
    }
}

fn start_recording(ctx: &Arc<AppCtx>) {
    if apply(ctx, Event::StartRequested).is_none() {
        return;
    }
    ctx.pending_wav.lock().unwrap().take();
    let app = ctx.app.clone();
    let last_emit = Arc::new(Mutex::new(std::time::Instant::now()));
    let on_amp = Box::new(move |rms: f32| {
        let mut last = last_emit.lock().unwrap();
        if last.elapsed().as_millis() >= 100 {
            *last = std::time::Instant::now();
            let _ = app.emit("dictation:amplitude", serde_json::json!({ "value": rms }));
        }
    });
    match ctx.recorder.start(on_amp) {
        Ok(()) => {
            set_phase(ctx, Phase::Recording, None);
            show_bubble(ctx);
        }
        Err(e) => {
            apply(ctx, Event::Failed);
            set_phase(ctx, Phase::Error, Some(&format!("mikrofón: {e}")));
            show_bubble(ctx);
        }
    }
}

pub fn cancel(ctx: &Arc<AppCtx>) {
    let _ = ctx.recorder.stop();
    if apply(ctx, Event::Cancel).is_some() {
        set_phase(ctx, Phase::Idle, None);
        hide_bubble_after(ctx, 0);
    }
}

async fn finish(ctx: Arc<AppCtx>) {
    let Some((samples, rate, ch)) = ctx.recorder.stop() else {
        set_phase(&ctx, Phase::Idle, None);
        return;
    };
    // < 0.4 s of audio → treat as silence.
    if samples.len() < (rate as usize * ch as usize) * 2 / 5 {
        set_phase(&ctx, Phase::Idle, Some("nič som nepočul"));
        hide_bubble_after(&ctx, 1200);
        return;
    }
    let wav = audio::prepare_wav(&samples, rate, ch);
    transcribe_and_deliver(ctx, wav).await;
}

pub async fn transcribe_and_deliver(ctx: Arc<AppCtx>, wav: Vec<u8>) {
    let (groq_url, lang, cleanup_enabled, meridian_url, model) = {
        let s = ctx.settings.read().unwrap();
        (
            s.groq_url.clone(),
            s.language.code(),
            s.cleanup_enabled,
            s.meridian_url.clone(),
            s.cleanup_model.clone(),
        )
    };
    let Some(api_key) = settings::groq_api_key() else {
        *ctx.pending_wav.lock().unwrap() = Some(wav);
        apply(&ctx, Event::Failed);
        set_phase(&ctx, Phase::Error, Some("chýba Groq API kľúč"));
        return;
    };

    // 1. STT
    let stt = SttClient::new(groq_url, api_key);
    let transcript = match stt.transcribe(wav.clone(), lang).await {
        Ok(t) => t,
        Err(e) => {
            *ctx.pending_wav.lock().unwrap() = Some(wav);
            apply(&ctx, Event::Failed);
            set_phase(&ctx, Phase::Error, Some(&format!("prepis zlyhal: {e}")));
            return;
        }
    };
    if transcript.text.is_empty() {
        set_phase(&ctx, Phase::Idle, Some("nič som nepočul"));
        hide_bubble_after(&ctx, 1200);
        return;
    }
    if apply(&ctx, Event::TranscriptReady).is_none() {
        return; // cancelled meanwhile
    }

    // 2. Cleanup (best-effort — spec: never lose text)
    let mut note: Option<&str> = None;
    let final_text = if cleanup_enabled {
        set_phase(&ctx, Phase::Cleaning, Some("✨ upravujem text…"));
        match CleanupClient::new(meridian_url, model).clean(&transcript.text).await {
            Ok(cleaned) => cleaned,
            Err(_) => {
                note = Some("vložené bez úprav");
                transcript.text.clone()
            }
        }
    } else {
        transcript.text.clone()
    };
    if apply(&ctx, Event::CleanupDone).is_none() {
        return; // cancelled meanwhile
    }

    // 3. Inject
    set_phase(&ctx, Phase::Injecting, None);
    let inject_result =
        tauri::async_runtime::spawn_blocking(move || crate::inject::inject_text(&final_text))
            .await
            .unwrap_or_else(|e| Err(crate::inject::InjectError::Keystroke(e.to_string())));
    match inject_result {
        Ok(()) => {
            apply(&ctx, Event::Injected);
            set_phase(&ctx, Phase::Idle, Some(note.unwrap_or("✓ vložené")));
            hide_bubble_after(&ctx, 1200);
        }
        Err(e) => {
            apply(&ctx, Event::Failed);
            set_phase(&ctx, Phase::Error, Some(&format!("vloženie zlyhalo: {e}")));
        }
    }
}
```

- [ ] **Step 2: Wire everything in `lib.rs`**

Replace `src-tauri/src/lib.rs` with:

```rust
mod audio;
mod cleanup;
mod hotkey;
mod inject;
mod pipeline;
mod settings;
mod state;
mod stt;

use pipeline::AppCtx;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("app config dir");
            let s = settings::load(&config_dir.join("settings.json"));
            let hotkey_name = Arc::new(RwLock::new(s.hotkey.clone()));

            let ctx = Arc::new(AppCtx {
                phase: Mutex::new(state::Phase::Idle),
                recorder: audio::Recorder::new(),
                settings: RwLock::new(s),
                pending_wav: Mutex::new(None),
                partial_inflight: AtomicBool::new(false),
                app: app.handle().clone(),
            });
            app.manage(ctx.clone());

            let (tx, rx) = mpsc::channel::<hotkey::HotkeySignal>();
            hotkey::spawn(hotkey_name, tx);
            std::thread::spawn(move || {
                while let Ok(sig) = rx.recv() {
                    pipeline::handle_signal(ctx.clone(), sig);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

(Remove all temporary debug blocks from Tasks 4/5/8.)

- [ ] **Step 3: Build + unit tests still green**

Run: `cd src-tauri && cargo test`
Expected: all tests from Tasks 2–7 PASS; `cargo build` clean.

- [ ] **Step 4: MILESTONE — manual end-to-end verification**

Prereqs: `export GROQ_API_KEY=gsk_...` in the shell before `pnpm tauri dev`; terminal has Accessibility + Microphone permissions.

1. Run `pnpm tauri dev`.
2. Click into TextEdit. Hold right Option, say "toto je test diktovania jedna dva tri", release.
3. Expected: within ~2–4 s the cleaned sentence appears in TextEdit. (Bubble window pops up as an empty/placeholder rectangle — its real UI is Task 10.)
4. With Meridian running (`meridian` in another terminal): text arrives cleaned. Stop Meridian, dictate again: raw text arrives (fallback works).
5. Double-tap test: double-tap right Option, speak two sentences, tap once — text arrives.

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat: end-to-end headless dictation pipeline (hotkey->groq->meridian->paste)"
```

---

### Task 10: Bubble UI — waveform, states, macOS non-activating panel

**Files:**
- Create: `src/windows/bubble/Bubble.tsx`, `src/windows/bubble/bubble.css`, `src/shared/events.ts`
- Modify: `src/windows/bubble/main.tsx`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` (macOS-only `tauri-nspanel`), `src-tauri/src/pipeline.rs` (position bubble on show)
- Modify: `package.json` (add `@tauri-apps/api`)

**Interfaces:**
- Consumes: events `dictation:state` `{ phase, message }`, `dictation:amplitude` `{ value }` (Task 9); `dictation:partial` `{ text }` arrives in Task 11 — the component already renders it.
- Produces: `src/shared/events.ts` exporting `EVENT_STATE = "dictation:state"`, `EVENT_AMPLITUDE = "dictation:amplitude"`, `EVENT_PARTIAL = "dictation:partial"` and payload types `StatePayload { phase: Phase; message: string | null }`, `AmplitudePayload { value: number }`, `PartialPayload { text: string }`, `type Phase = "idle" | "recording" | "transcribing" | "cleaning" | "injecting" | "error"`.

- [ ] **Step 1: Shared event contract**

```bash
pnpm add @tauri-apps/api
```

Create `src/shared/events.ts`:

```ts
export type Phase =
  | "idle"
  | "recording"
  | "transcribing"
  | "cleaning"
  | "injecting"
  | "error";

export const EVENT_STATE = "dictation:state";
export const EVENT_AMPLITUDE = "dictation:amplitude";
export const EVENT_PARTIAL = "dictation:partial";

export interface StatePayload {
  phase: Phase;
  message: string | null;
}
export interface AmplitudePayload {
  value: number;
}
export interface PartialPayload {
  text: string;
}
```

- [ ] **Step 2: Bubble component**

Create `src/windows/bubble/Bubble.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AmplitudePayload,
  EVENT_AMPLITUDE,
  EVENT_PARTIAL,
  EVENT_STATE,
  PartialPayload,
  Phase,
  StatePayload,
} from "../../shared/events";
import "./bubble.css";

const BAR_COUNT = 24;

export default function Bubble() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [partial, setPartial] = useState("");
  const [bars, setBars] = useState<number[]>(Array(BAR_COUNT).fill(0));
  const [seconds, setSeconds] = useState(0);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<StatePayload>(EVENT_STATE, (e) => {
      setPhase(e.payload.phase);
      setMessage(e.payload.message);
      if (e.payload.phase === "recording") {
        setPartial("");
        setSeconds(0);
      }
    }).then((u) => unsubs.push(u));
    listen<AmplitudePayload>(EVENT_AMPLITUDE, (e) => {
      setBars((prev) => [...prev.slice(1), Math.min(1, e.payload.value * 6)]);
    }).then((u) => unsubs.push(u));
    listen<PartialPayload>(EVENT_PARTIAL, (e) => {
      setPartial(e.payload.text);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, []);

  useEffect(() => {
    if (phase === "recording") {
      timerRef.current = window.setInterval(() => setSeconds((s) => s + 1), 1000);
    } else if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
    return () => {
      if (timerRef.current !== null) window.clearInterval(timerRef.current);
    };
  }, [phase]);

  const mmss = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

  return (
    <div className={`bubble bubble--${phase}`} data-tauri-drag-region>
      {phase === "recording" && (
        <>
          <Waveform bars={bars} />
          {partial ? (
            <span className="bubble__partial">{partial}</span>
          ) : (
            <span className="bubble__timer">● {mmss}</span>
          )}
        </>
      )}
      {phase === "transcribing" && <span className="bubble__status">prepisujem…</span>}
      {phase === "cleaning" && (
        <span className="bubble__status">{message ?? "✨ upravujem text…"}</span>
      )}
      {phase === "injecting" && <span className="bubble__status">vkladám…</span>}
      {phase === "idle" && message && (
        <span className="bubble__status bubble__status--done">{message}</span>
      )}
      {phase === "error" && (
        <span className="bubble__status bubble__status--error">⚠ {message}</span>
      )}
    </div>
  );
}

function Waveform({ bars }: { bars: number[] }) {
  return (
    <div className="waveform" aria-hidden>
      {bars.map((v, i) => (
        <div
          key={i}
          className="waveform__bar"
          style={{ height: `${Math.max(8, v * 100)}%` }}
        />
      ))}
    </div>
  );
}
```

Create `src/windows/bubble/bubble.css`:

```css
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body,
#root {
  background: transparent;
  height: 100%;
  overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  user-select: none;
  -webkit-user-select: none;
  cursor: default;
}

.bubble {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 48px;
  margin: 8px;
  padding: 0 16px;
  border-radius: 24px;
  background: rgba(24, 24, 27, 0.78);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.92);
  font-size: 13px;
  overflow: hidden;
  white-space: nowrap;
}

.bubble--error {
  border-color: rgba(248, 113, 113, 0.5);
}

.waveform {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 26px;
  flex-shrink: 0;
}

.waveform__bar {
  width: 3px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.85);
  transition: height 90ms linear;
}

.bubble__timer {
  color: rgba(255, 255, 255, 0.6);
  font-variant-numeric: tabular-nums;
}

.bubble__partial {
  overflow: hidden;
  text-overflow: ellipsis;
  direction: rtl; /* keep the newest words visible */
  text-align: left;
}

.bubble__status--done {
  color: #4ade80;
}

.bubble__status--error {
  color: #f87171;
}
```

Replace `src/windows/bubble/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import Bubble from "./Bubble";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Bubble />
  </React.StrictMode>,
);
```

- [ ] **Step 3: macOS non-activating panel + bottom-center positioning**

Add to `src-tauri/Cargo.toml`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
tauri-nspanel = { git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2" }
```

In `src-tauri/src/lib.rs` `setup`, after `app.manage(ctx.clone())`, add:

```rust
if let Some(bubble) = app.get_webview_window("bubble") {
    position_bubble(&bubble);
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::WebviewWindowExt;
        // NSWindowStyleMaskNonactivatingPanel = 1 << 7,
        // NSStatusWindowLevel = 25,
        // collection: canJoinAllSpaces (1<<0) | fullScreenAuxiliary (1<<8)
        if let Ok(panel) = bubble.to_panel() {
            panel.set_style_mask(1 << 7);
            panel.set_level(25);
            panel.set_collection_behaviour(
                tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior::from_bits_retain(
                    (1 << 0) | (1 << 8),
                ),
            );
        }
    }
}
```

and register the plugin on the builder (macOS only):

```rust
let builder = tauri::Builder::default();
#[cfg(target_os = "macos")]
let builder = builder.plugin(tauri_nspanel::init());
builder
    .setup(|app| { /* existing setup */ })
    ...
```

Add the positioning helper to `lib.rs`:

```rust
fn position_bubble(win: &tauri::WebviewWindow) {
    if let (Ok(Some(monitor)), Ok(size)) = (win.primary_monitor(), win.outer_size()) {
        let m = monitor.size();
        let x = (m.width.saturating_sub(size.width)) / 2;
        let y = m.height.saturating_sub(size.height + 120);
        let _ = win.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}
```

Note: if the `tauri-nspanel` v2 API differs on the pinned commit (crate is community-maintained), adjust names per its README — the three required effects are: nonactivating style mask, status-window level, visible on all spaces. Verify behavior in Step 4, not by reading code.

- [ ] **Step 4: Manual verification**

Run `pnpm tauri dev`:
1. Click into TextEdit, hold hotkey and speak — bubble appears bottom-center, bars dance with your voice volume, timer runs.
2. Release — "prepisujem…" → (Meridian on: "✨ upravujem text…") → "✓ vložené" → bubble fades away; text lands in TextEdit.
3. Critical: while the bubble is visible, the TextEdit window must keep its focused (active) title bar — the bubble must never steal focus.
4. Drag the bubble somewhere else — it moves (drag region).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: floating bubble with live waveform and dictation states"
```

---

### Task 11: Live partial transcription in the bubble

**Files:**
- Modify: `src-tauri/src/pipeline.rs`

**Interfaces:**
- Consumes: `Recorder::snapshot()` (Task 5), `SttClient` (Task 6), `AppCtx.partial_inflight` (Task 9).
- Produces: emits `dictation:partial` `{ "text": String }` every ~2.5 s while recording (already rendered by Task 10's bubble).

- [ ] **Step 1: Add the partial loop**

In `src-tauri/src/pipeline.rs`, at the end of the `Ok(())` arm of `start_recording` (after `show_bubble(ctx);`), add:

```rust
spawn_partial_loop(ctx.clone());
```

and add these items to the file:

```rust
const PARTIAL_INTERVAL_MS: u64 = 2500;
/// Don't bother transcribing less than 1 s of audio.
const PARTIAL_MIN_SECS: f32 = 1.0;

fn spawn_partial_loop(ctx: Arc<AppCtx>) {
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(PARTIAL_INTERVAL_MS));
        interval.tick().await; // first tick fires immediately; skip it
        loop {
            interval.tick().await;
            if *ctx.phase.lock().unwrap() != Phase::Recording {
                return; // recording ended — loop dies with it
            }
            if ctx.recorder.duration_secs() < PARTIAL_MIN_SECS {
                continue;
            }
            // One partial request at a time; skip a beat when Groq is slow.
            if ctx
                .partial_inflight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }
            let Some((samples, rate, ch)) = ctx.recorder.snapshot() else {
                ctx.partial_inflight.store(false, Ordering::SeqCst);
                continue;
            };
            let (groq_url, lang) = {
                let s = ctx.settings.read().unwrap();
                (s.groq_url.clone(), s.language.code())
            };
            let Some(api_key) = settings::groq_api_key() else {
                ctx.partial_inflight.store(false, Ordering::SeqCst);
                continue;
            };
            let ctx2 = ctx.clone();
            tauri::async_runtime::spawn(async move {
                let wav = audio::prepare_wav(&samples, rate, ch);
                let stt = SttClient::new(groq_url, api_key);
                if let Ok(t) = stt.transcribe(wav, lang).await {
                    // Only show it if we're still recording.
                    if *ctx2.phase.lock().unwrap() == Phase::Recording && !t.text.is_empty() {
                        let _ = ctx2
                            .app
                            .emit("dictation:partial", serde_json::json!({ "text": t.text }));
                    }
                }
                ctx2.partial_inflight.store(false, Ordering::SeqCst);
            });
        }
    });
}
```

- [ ] **Step 2: Build + tests**

Run: `cd src-tauri && cargo test && cargo build`
Expected: all green.

- [ ] **Step 3: Manual verification**

Run `pnpm tauri dev`, double-tap the hotkey (toggle lock) and speak continuously for ~10 s. Expected: after ~3–4 s the bubble starts showing your words, refreshing every ~2.5 s, newest words visible at the right edge. Tap once to stop — final (more accurate) text lands at the cursor.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat: live partial transcription streamed to the bubble"
```

---

### Task 12: Error handling, retry, and cancel — "text is never lost"

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/hotkey.rs`, `src-tauri/src/pipeline.rs`, `src/windows/bubble/Bubble.tsx`

**Interfaces:**
- Consumes: `AppCtx.pending_wav` (Task 9), `inject::copy_only` (Task 8).
- Produces:
  - Tauri commands: `cancel_dictation()`, `retry_transcription()`, `set_groq_key(key: String)`
  - `HotkeySignal::Cancel` now emitted on Escape while dictation is active.
  - Error payload convention: on `phase: "error"`, `message` is Slovak and actionable; when text could not be pasted it is left in the clipboard.

- [ ] **Step 1: Escape → Cancel in the hotkey listener**

In `src-tauri/src/hotkey.rs`, replace the `Escape` branch inside the rdev callback:

```rust
if name == "Escape" && is_down {
    let _ = tx_esc.send(HotkeySignal::Cancel);
    return;
}
```

To make `tx` available under two names, clone it before `rdev::listen`:

```rust
let tx_esc = tx.clone();
```

(The pipeline ignores `Cancel` while `Idle`, so unconditional sending is safe.)

- [ ] **Step 2: Clipboard fallback on inject failure**

In `src-tauri/src/pipeline.rs`, in `transcribe_and_deliver`, replace the entire `// 3. Inject` section (from `set_phase(&ctx, Phase::Injecting, None);` to the end of the `match`) with this — on paste failure the text must still reach the user via the clipboard:

```rust
// 3. Inject
set_phase(&ctx, Phase::Injecting, None);
let text_for_inject = final_text.clone();
let inject_result =
    tauri::async_runtime::spawn_blocking(move || crate::inject::inject_text(&text_for_inject))
        .await
        .unwrap_or_else(|e| Err(crate::inject::InjectError::Keystroke(e.to_string())));
match inject_result {
    Ok(()) => {
        apply(&ctx, Event::Injected);
        set_phase(&ctx, Phase::Idle, Some(note.unwrap_or("✓ vložené")));
        hide_bubble_after(&ctx, 1200);
    }
    Err(e) => {
        // Never lose text: leave it in the clipboard at minimum.
        let _ = crate::inject::copy_only(&final_text);
        apply(&ctx, Event::Failed);
        set_phase(
            &ctx,
            Phase::Error,
            Some(&format!("vloženie zlyhalo — text je v schránke (Cmd+V). {e}")),
        );
    }
}
```

- [ ] **Step 3: Commands for cancel, retry and key setup**

Create `src-tauri/src/commands.rs`:

```rust
use crate::pipeline::{self, AppCtx};
use crate::settings;
use crate::state::Phase;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn cancel_dictation(ctx: State<'_, Arc<AppCtx>>) {
    pipeline::cancel(ctx.inner());
}

/// Re-runs STT on the audio kept from a failed attempt (spec §6: "skúsiť znova").
#[tauri::command]
pub async fn retry_transcription(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    let wav = ctx.pending_wav.lock().unwrap().take();
    let Some(wav) = wav else {
        return Err("žiadne audio na zopakovanie".into());
    };
    {
        let mut phase = ctx.phase.lock().unwrap();
        *phase = Phase::Transcribing;
    }
    pipeline::set_phase(&ctx, Phase::Transcribing, None);
    pipeline::transcribe_and_deliver(ctx.inner().clone(), wav).await;
    Ok(())
}

/// Dev/setup helper until Plan 2 ships the settings UI.
#[tauri::command]
pub fn set_groq_key(key: String) -> Result<(), String> {
    settings::set_groq_api_key(&key).map_err(|e| e.to_string())
}
```

In `lib.rs` add `mod commands;` and register on the builder:

```rust
.invoke_handler(tauri::generate_handler![
    commands::cancel_dictation,
    commands::retry_transcription,
    commands::set_groq_key
])
```

- [ ] **Step 4: Bubble error UI with Retry + click-to-cancel**

In `src/windows/bubble/Bubble.tsx`:

Add imports and handlers:

```tsx
import { invoke } from "@tauri-apps/api/core";

const cancel = () => void invoke("cancel_dictation");
const retry = () => void invoke("retry_transcription");
```

Replace the recording branch's wrapper so a click cancels (keep drag on the container):

```tsx
{phase === "recording" && (
  <button className="bubble__hit" onClick={cancel} title="Zrušiť (Esc)">
    <Waveform bars={bars} />
    {partial ? (
      <span className="bubble__partial">{partial}</span>
    ) : (
      <span className="bubble__timer">● {mmss}</span>
    )}
  </button>
)}
```

Replace the error branch:

```tsx
{phase === "error" && (
  <>
    <span className="bubble__status bubble__status--error">⚠ {message}</span>
    {message?.startsWith("prepis zlyhal") && (
      <button className="bubble__retry" onClick={retry}>
        skúsiť znova
      </button>
    )}
    <button className="bubble__retry" onClick={cancel}>
      ✕
    </button>
  </>
)}
```

Add to `bubble.css`:

```css
.bubble__hit {
  all: unset;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.bubble__retry {
  all: unset;
  cursor: pointer;
  padding: 4px 10px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.12);
  font-size: 12px;
  flex-shrink: 0;
}

.bubble__retry:hover {
  background: rgba(255, 255, 255, 0.22);
}
```

- [ ] **Step 5: Build + full test suite**

Run: `cd src-tauri && cargo test && cargo build && cd .. && pnpm build`
Expected: all Rust tests PASS, both frontends compile.

- [ ] **Step 6: Manual failure-path verification**

1. `unset GROQ_API_KEY`, remove keyring entry if set → dictate → bubble shows "chýba Groq API kľúč". Set the key via devtools on the main window: `window.__TAURI__.core.invoke("set_groq_key", { key: "gsk_..." })` (or re-export the env var) → click "skúsiť znova" is not shown for this error type; restart dictation instead.
2. Turn off Wi-Fi → dictate → "prepis zlyhal…" with "skúsiť znova"; turn Wi-Fi on → click it → text lands at cursor. **The audio was not lost.**
3. Kill Meridian → dictate → raw text arrives, bubble says "vložené bez úprav".
4. During recording press Esc → bubble disappears, nothing is pasted.
5. Click the bubble mid-recording → same as Esc.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: cancel, retry and never-lose-text failure handling"
```

---

## Plan 1 Completion Checklist

- [ ] All `cargo test` suites green (`settings`, `state`, `hotkey`, `audio`, `stt`, `cleanup`)
- [ ] `pnpm build` clean (main + bubble pages)
- [ ] Manual E2E on macOS: PTT, double-tap toggle, live partials, Meridian cleanup + fallback, offline retry, Esc cancel, clipboard preservation
- [ ] Bubble never steals focus (title bar of target app stays active)
- [ ] Use superpowers:verification-before-completion before declaring done

## Deferred to Plan 2 (`main app, windows, distribution`)

Per spec §5/§9/§10: History (SQLite + UI + search), Settings UI (hotkey capture, language mode, cleanup style jemné/silné, Groq key management, Meridian status), Setup wizard, tray icon + autostart, bubble position persistence, Windows adaptation + verification, GitHub Actions release builds (.dmg/.msi), README.
