# Main App, Polish & Distribution — Implementation Plan (Plan 2 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the working dictation engine (Plan 1, merged) into a complete app: settings UI with hotkey capture, local history, first-run wizard with permission guidance, tray icon, review-backlog fixes, Windows build support, and GitHub Releases CI.

**Architecture:** Same Tauri v2 app. New Rust modules `history.rs` (SQLite via rusqlite-bundled) and expanded `commands.rs`; settings become hot-applied (hotkey RwLock updated live). Main window becomes a real React app (sidebar: Nastavenia / História; wizard overlay on first run). Spec: `docs/superpowers/specs/2026-07-06-local-wispr-flow-design.md` §5, §9, §10.

**Tech Stack:** additions — rusqlite (bundled), macos-accessibility-client (macOS-only), tauri-plugin-autostart, tauri-plugin-opener (re-registered), GitHub Actions (macos-14 + windows-latest).

## Global Constraints

- Same as Plan 1: pnpm, Rust pinned 1.96, identifiers/labels/event names unchanged, Slovak UI copy, "text never lost".
- New IPC events: `settings:changed` (payload = full Settings JSON), `hotkey:captured` `{ key: string }`, `dictation:pipeline-dead` `{ message }` (rdev listener failure).
- New commands (exact names): `get_settings`, `set_settings`, `has_groq_key`, `set_groq_key` (exists), `test_groq_key`, `meridian_status`, `history_list`, `history_delete`, `history_clear`, `permissions_status`, `open_privacy_settings`, `hotkey_capture_start`, `finish_wizard`.
- History DB: `<app_data_dir>/history.sqlite`, table `dictations(id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, raw TEXT NOT NULL, clean TEXT NOT NULL, language TEXT, duration_ms INTEGER NOT NULL)`.
- Settings JSON gains fields (serde defaults keep old files valid): `cleanup_style: "light"|"strong"` (default light), `wizard_done: bool` (default false), `bubble_pos: Option<(i32,i32)>` (default None), `autostart: bool` (default false).
- Design: modern-minimalist (Linear/Raycast vibe), dark+light via `prefers-color-scheme`, single accent `#6E56CF`, CSS custom properties in `src/shared/tokens.css`, no UI library. UI-heavy tasks MUST load the `frontend-design` skill before writing components.
- macOS deep links: `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` and `?Privacy_Microphone`.
- Every task: tests where testable, `cargo test` + `cargo build` + `pnpm build` green, conventional commit.
- GUI cannot be driven by subagents — GUI steps are listed as deferred manual checks in reports, verified by the user at the end.

---

### Task 1: Settings v2 — new fields, first-run write, hot-apply commands

**Files:** Modify `src-tauri/src/settings.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/pipeline.rs`

**Interfaces produced:**
- `Settings` gains: `cleanup_style: CleanupStyle` (`Light|Strong`, serde lowercase), `wizard_done: bool`, `bubble_pos: Option<(i32, i32)>`, `autostart: bool` — all `#[serde(default)]`-compatible; extend the `partial_file_fills_defaults` test for the new fields.
- `AppCtx` gains `pub hotkey_name: Arc<RwLock<String>>` and `pub settings_path: PathBuf` (lib.rs passes both; hotkey::spawn keeps using the same Arc).
- Commands:

```rust
#[tauri::command]
pub fn get_settings(ctx: State<'_, Arc<AppCtx>>) -> Settings {
    ctx.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(ctx: State<'_, Arc<AppCtx>>, new: Settings) -> Result<(), String> {
    *ctx.hotkey_name.write().unwrap() = new.hotkey.clone();
    settings::save(&ctx.settings_path, &new).map_err(|e| e.to_string())?;
    *ctx.settings.write().unwrap() = new.clone();
    let _ = ctx.app.emit("settings:changed", &new);
    Ok(())
}

#[tauri::command]
pub fn has_groq_key() -> bool {
    settings::groq_api_key().is_some()
}

#[tauri::command]
pub async fn test_groq_key(ctx: State<'_, Arc<AppCtx>>) -> Result<bool, String> {
    let (url, key) = { let s = ctx.settings.read().unwrap();
        (s.groq_url.clone(), settings::groq_api_key().ok_or("chýba kľúč")?) };
    let resp = reqwest::Client::new()
        .get(format!("{}/openai/v1/models", url.trim_end_matches('/')))
        .bearer_auth(key).timeout(std::time::Duration::from_secs(10))
        .send().await.map_err(|e| e.to_string())?;
    Ok(resp.status().is_success())
}

#[tauri::command]
pub async fn meridian_status(ctx: State<'_, Arc<AppCtx>>) -> bool {
    let (url, model) = { let s = ctx.settings.read().unwrap();
        (s.meridian_url.clone(), s.cleanup_model.clone()) };
    crate::cleanup::CleanupClient::new(url, model).is_reachable().await
}

#[tauri::command]
pub fn finish_wizard(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    let mut s = ctx.settings.write().unwrap();
    s.wizard_done = true;
    settings::save(&ctx.settings_path, &s).map_err(|e| e.to_string())
}
```

- lib.rs: on startup, if settings file missing, `settings::save(&path, &Settings::default())` (first-run template). Register all new commands. Fix the `is_reachable` dead-code warning (now used).
- pipeline.rs: cleanup call passes style — `CleanupClient` gains `with_style(CleanupStyle)`; `Strong` appends to SYSTEM_PROMPT: `" You may lightly rephrase sentences for fluency, keeping the language and meaning."` Add a wiremock test asserting the strong-style system prompt is sent.

Steps: write tests (settings defaults roundtrip incl. new fields; cleanup strong-prompt test) → implement → `cargo test` → wire lib.rs/commands → build → commit `feat: settings v2 with hot-apply, groq/meridian probes, cleanup styles`.

---

### Task 2: History store (SQLite) + pipeline wiring + commands

**Files:** Create `src-tauri/src/history.rs`; modify `Cargo.toml` (`rusqlite = { version = "0.32", features = ["bundled"] }`), `lib.rs`, `pipeline.rs`, `commands.rs`

**Interfaces produced:**

```rust
pub struct HistoryStore(Mutex<rusqlite::Connection>); // stored in AppCtx as pub history: HistoryStore
#[derive(Serialize, Clone)]
pub struct Dictation { pub id: i64, pub ts: i64, pub raw: String, pub clean: String,
    pub language: Option<String>, pub duration_ms: i64 }
impl HistoryStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self>   // creates table if absent
    pub fn insert(&self, raw: &str, clean: &str, language: Option<&str>, duration_ms: i64) -> rusqlite::Result<i64>
    pub fn list(&self, search: Option<&str>, limit: u32) -> rusqlite::Result<Vec<Dictation>> // newest first; search = LIKE %q% on raw OR clean
    pub fn delete(&self, id: i64) -> rusqlite::Result<()>
    pub fn clear(&self) -> rusqlite::Result<()>
}
```

- Tests (tempdir DB): insert→list roundtrip; search matches raw and clean, case-insensitive; delete removes one; clear empties; list limit + ordering (newest first).
- pipeline.rs: in `transcribe_and_deliver`, after successful inject (both cleaned and fallback-raw), insert into history (raw = transcript.text, clean = final_text, language from transcript, duration from audio length; compute duration_ms before samples are consumed). Never let a history error break the flow (`let _ =`).
- Commands: `history_list(search: Option<String>, limit: Option<u32>) -> Vec<Dictation>` (default limit 200), `history_delete(id: i64)`, `history_clear()`. Register in lib.rs.

Steps: tests first → implement → wire → full `cargo test` → commit `feat: local sqlite dictation history`.

---

### Task 3: Permission visibility + pipeline-dead signal + deep links

**Files:** Modify `src-tauri/src/hotkey.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `Cargo.toml` (macOS dep `macos-accessibility-client = "0.0.1"`), `capabilities/default.json` (opener permission if needed)

- hotkey.rs `spawn` gains an `on_dead: Box<dyn Fn(String) + Send>` callback param; on `rdev::listen` error call it with Slovak message `"Globálna klávesa nefunguje — chýba povolenie Accessibility. Otvor Nastavenia → Súkromie a bezpečnosť → Prístupnosť."`. lib.rs passes a closure emitting `dictation:pipeline-dead { message }` to all windows AND showing the main window (`get_webview_window("main").show/set_focus`).
- Commands:

```rust
#[tauri::command]
pub fn permissions_status() -> serde_json::Value {
    #[cfg(target_os = "macos")]
    let accessibility = macos_accessibility_client::accessibility::application_is_trusted();
    #[cfg(not(target_os = "macos"))]
    let accessibility = true;
    serde_json::json!({ "accessibility": accessibility })
}

#[tauri::command]
pub fn open_privacy_settings(pane: String) { // pane: "accessibility" | "microphone"
    #[cfg(target_os = "macos")]
    {
        let anchor = if pane == "microphone" { "Privacy_Microphone" } else { "Privacy_Accessibility" };
        let _ = std::process::Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
            .spawn();
    }
    #[cfg(not(target_os = "macos"))] { let _ = pane; }
}
```

- Remove `tauri-plugin-opener` entirely (dep, capability entry, any init) — deep links go through `open` above; one less unused plugin. (Reviewer backlog item.)
- Testable bits are thin; verify compile + existing tests; manual check deferred. Commit `feat: surface accessibility failure, privacy deep-links`.

---

### Task 4: Review-backlog correctness fixes (Rust)

**Files:** Modify `src-tauri/src/pipeline.rs`, `src-tauri/src/stt.rs`, `src-tauri/src/inject.rs`, `src-tauri/src/audio.rs`

1. **apply_for gen check under lock** (final-review minor): fold the gen comparison into the locked section (mirror `advance`, no emit) or reimplement `apply_for` via the same locked pattern. Remove the outside-lock load.
2. **SttError naming**: add `Parse(String)` variant; 2xx JSON decode failures map to it (was mislabeled `Network`). Update the wiremock test expectations if any match on Network for that path; add one test: 200 with invalid JSON → `Parse`.
3. **inject.rs Meta stuck**: if the `'v'` click fails between press/release, still attempt the modifier release (wrap in a closure that always tries release; return first error).
4. **Recording cap**: in the partial loop and via a dedicated check, auto-stop a locked-mode take at 5 minutes (`MAX_TAKE_SECS: f32 = 300.0`): when `recorder.duration_secs() > MAX_TAKE_SECS` send the same path as HotkeySignal::Stop (call `handle_signal(ctx.clone(), HotkeySignal::Stop)` from the partial loop thread guardedly — it's sync; spawn it). Partial uploads: only transcribe the **last 25 s** of the snapshot for partial display (`PARTIAL_WINDOW_SECS: f32 = 25.0`, slice the samples tail) — bounds upload cost on long takes; final pass still uses full audio.
5. **audio**: add `Recorder::tail_samples(secs)` helper? Not needed — slice in pipeline from `snapshot()` output; add a pure helper `fn tail(samples: &[f32], rate: u32, ch: u16, secs: f32) -> &[f32]` in audio.rs WITH unit test (returns whole slice when shorter; correct frame alignment for stereo).

Steps: tests for 2/5 → implement all → full suite → commit `fix: review backlog — parse errors, meta release, take cap, partial window, lock discipline`.

---

### Task 5: Design system + main window shell

**Files:** Create `src/shared/tokens.css`, `src/shared/ipc.ts` (typed invoke wrappers for all commands + Settings/Dictation TS types mirroring Rust), `src/windows/main/App.tsx` (rewrite), `src/windows/main/app.css`, components `src/windows/main/Sidebar.tsx`

**MUST load `frontend-design` skill first.** Deliver: tokens.css (colors light+dark via `prefers-color-scheme`, accent `#6E56CF`, spacing/radius/type scale, system font stack), sidebar layout (app name, nav: Nastavenia / História; footer: version + permission warning badge when `permissions_status().accessibility === false` or `dictation:pipeline-dead` received), content pane switching on local state. Placeholder panes ("…") for Settings/History (Tasks 6–7 fill them). Empty-state and typography polished — this is the face of the app.

ipc.ts exports: `type Settings`, `type Dictation`, `api = { getSettings, setSettings, hasGroqKey, setGroqKey, testGroqKey, meridianStatus, historyList, historyDelete, historyClear, permissionsStatus, openPrivacySettings, hotkeyCaptureStart, finishWizard, cancelDictation, retryTranscription }` — thin `invoke` wrappers, one place for command-name strings.

Verify: `pnpm build`; visual check deferred. Commit `feat: design tokens, typed ipc, main window shell`.

---

### Task 6: Settings page

**Files:** Create `src/windows/main/pages/Settings.tsx` (+ page css or shared classes); modify `src-tauri/src/hotkey.rs`, `commands.rs` (hotkey capture); `Cargo.toml`+`lib.rs` (`tauri-plugin-autostart`)

**MUST load `frontend-design` skill.** Sections (Slovak labels):
- **Klávesa** — current hotkey shown as a key-cap chip; button „Zmeniť" → invokes `hotkey_capture_start` → Rust: one-shot flag consumed by the existing rdev listener thread (add `capture_next: Arc<AtomicBool>` threaded from lib.rs into `hotkey::spawn`; when set, the NEXT KeyPress of ANY key is not interpreted — instead emit `hotkey:captured { key: format!("{:?}", key) }`, clear flag, swallow event). UI listens, shows the captured key, calls `set_settings`. Escape cancels capture (emit nothing, clear flag).
- **Jazyk** — segmented control Auto / SK / CS / EN → `set_settings`.
- **Čistenie textu** — toggle; style radio jemné/silné; model text input (default claude-sonnet-5); Meridian URL input + live status dot (`meridian_status` polled on mount and on URL change, 10s debounce).
- **Groq API kľúč** — password input + „Uložiť" (`set_groq_key`) + „Otestovať" (`test_groq_key` → ✓/✗); status line from `has_groq_key`.
- **Systém** — autostart toggle: register `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None)` in lib.rs, UI calls the plugin's JS API (`@tauri-apps/plugin-autostart` npm pkg: enable/disable/isEnabled) and mirrors into settings.autostart via set_settings.

All controls optimistic-update with rollback on error; saving via one debounced `set_settings`. Verify builds; manual GUI deferred. Commit `feat: settings page with live hotkey capture`.

---

### Task 7: History page

**Files:** Create `src/windows/main/pages/History.tsx`

**MUST load `frontend-design` skill.** Search input (debounced 250 ms → `history_list(search)`), list rows: clean text (2-line clamp), meta line (relative date, language badge, duration), hover actions: Kopírovať (navigator.clipboard.writeText(clean)), rozbaliť (show raw + clean full), Zmazať; header: „Zmazať všetko" with inline confirm. Empty states: no history yet / no search hits. Refresh when window regains focus and after each dictation (listen `dictation:state` for idle+message "✓ vložené"-class transitions or simply refetch on focus + 10s stale). Commit `feat: history page`.

---

### Task 8: First-run wizard

**Files:** Create `src/windows/main/wizard/Wizard.tsx` (+ steps components); modify `App.tsx` (show wizard overlay when `!settings.wizard_done`)

**MUST load `frontend-design` skill.** 5 steps, progress dots, „Ďalej/Preskočiť":
1. **Vitaj** — one-liner čo appka robí; hotkey hint graphic (key-cap „⌥ pravý").
2. **Povolenia** — live `permissions_status` polling (2s while on step); Accessibility row with status + button `open_privacy_settings("accessibility")`; Mikrofón row: status „zistí sa pri prvom diktovaní" + deep-link button; note that dev/terminal owns permissions in dev mode.
3. **Groq kľúč** — explanation (free tier); add command `open_url(url: String)` that spawns the OS opener (`open`/`cmd /c start`) ONLY when url is exactly `https://console.groq.com` (hard allowlist, anything else ignored); input + test + save (reuse Settings widgets/logic via shared component or duplicated minimal form).
4. **Meridian (voliteľné)** — auto `meridian_status`; if down: short note how to start + „Preskočiť".
5. **Skúšobné diktovanie** — instruction to hold the hotkey and dictate into the wizard's textarea (a plain textarea — the paste lands wherever the cursor is, i.e. the focused textarea); success detected via `dictation:state` idle+"✓ vložené" → confetti-free success check ✓; „Dokončiť" → `finish_wizard`.

Commit `feat: first-run wizard`.

---

### Task 9: Tray, bubble position persistence, small UX polish

**Files:** Modify `src-tauri/src/lib.rs` (tray), `pipeline.rs`/`lib.rs` (bubble pos), `src/windows/bubble/Bubble.tsx` (bars reset)

- **Tray** (`tauri` `tray-icon` feature + `image-png`): icon = app icon; menu: „Otvoriť Local Wispr Flow" (show+focus main), separator, Jazyk submenu with radio items Auto/SK/CS/EN (reflect settings.language; on select → same path as set_settings incl. emit), separator, „Ukončiť" (app exit). Main window close → hide instead of quit (`on_window_event` CloseRequested → prevent + hide; macOS: keep Dock icon).
- **Bubble position**: persist on move — listen to bubble window `Moved` event (debounce 500 ms) → save `bubble_pos` via settings::save; on show, use saved pos if Some (validate on-screen: intersects any monitor, else fallback bottom-center via existing `position_bubble`).
- **Bubble polish**: reset `bars` to zeros when a new `recording` phase begins (Bubble.tsx state reset alongside partial/seconds).
- Commit `feat: tray icon, bubble position memory, waveform reset`.

---

### Task 10: Windows compatibility pass (code-level)

**Files:** Audit/modify `src-tauri/src/inject.rs` (already Ctrl+V), `hotkey.rs` (default hotkey on Windows: `"AltGr"` works — right Alt), `lib.rs` (skip nspanel — already cfg'd; ensure bubble `focusable(false)`-equivalent: tauri window config already `focus: false`; add `set_skip_taskbar` no-op check), `tauri.conf.json` (bundle targets `"targets": ["dmg", "nsis"]`), `settings.rs` (no macOS-only paths)

- Grep for every `#[cfg(target_os` and confirm each has a Windows path; document per-item in the report. Add `nsis` bundle config (installer language en + sk if trivial). No Windows machine available: acceptance = `cargo check --target x86_64-pc-windows-msvc` is NOT feasible locally (linker) — instead rely on Task 11 CI windows job compiling. Keep this task to code audit + conf changes + a `docs/WINDOWS.md` note listing untested areas.
- Commit `feat: windows bundle config and platform audit`.

---

### Task 11: CI release workflow + README + v0.1.0

**Files:** Create `.github/workflows/release.yml`, `README.md` (rewrite), bump versions to 0.1.0 (`package.json`, `Cargo.toml`, `tauri.conf.json`)

- Workflow: on tag `v*`: matrix `macos-14` (universal dmg: `pnpm tauri build --target universal-apple-darwin`) + `windows-latest` (nsis exe); steps: checkout, pnpm setup (corepack), node 22, rust stable (rust-toolchain.toml respected), cache (Swatinem/rust-cache), `pnpm install --frozen-lockfile`, build, upload artifacts to a GitHub Release (softprops/action-gh-release, draft=true). Also a `ci.yml`: on PR/push to main — `cargo test`, `cargo clippy -- -D warnings` (fix any clippy fallout), `pnpm build`, plus a `windows-latest` `cargo check` job (catches Windows compile breaks; needs config `pnpm install` + tauri deps).
- README (English, short): what it is, features, install (Releases + Gatekeeper `xattr -d com.apple.quarantine`, SmartScreen note), setup (permissions, Groq key, Meridian optional), dev (`pnpm tauri dev`), architecture one-para, privacy note (what leaves the machine: audio→Groq, text→Meridian/Anthropic), license MIT (add LICENSE).
- Commit `chore: ci release pipeline, readme, v0.1.0`.

---

## Completion checklist

- [ ] `cargo test` full suite green (Plan 1's 30 + new history/settings/stt/audio tests)
- [ ] `cargo clippy -- -D warnings` clean · `pnpm build` clean
- [ ] Final whole-branch review (most capable model) → fix wave → Approved
- [ ] User manual pass: wizard flow, settings hot-apply (change hotkey live), history fills/searches, tray works, bubble remembers position, permission warning shows when revoked
- [ ] Tag v0.1.0 after user validation (not before)

## Out of scope (backlog)

Windows runtime testing on real hardware; code signing/notarization; clipboard image restore; pipeline unit-test harness (AppHandle mock); localization beyond SK strings; theme override toggle (system-follow only for now — spec's manual toggle deferred with user's implicit OK, revisit if requested).
