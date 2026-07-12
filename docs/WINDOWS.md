# Windows compatibility — code audit

Status: code-level audit + bundle config only. **No Windows machine or CI
Windows job has run this app yet.** Everything marked "tested? no" below is
verified only by reading the code and by `cargo build`/`cargo test` on macOS
(which compiles the non-`cfg(target_os = "macos")` branches too, since they're
plain `#[cfg(not(target_os = "macos"))]` / `#[cfg(target_os = "windows")]`
arms reachable from the same source tree). Real Windows verification is
Task 11's job (CI `windows-latest` build).

## Audit table

| Area | macOS impl | Windows impl | Tested on Windows? |
|---|---|---|---|
| Global hotkey listener (`src-tauri/src/hotkey.rs`) | `rdev::listen` global hook; default hotkey `"AltGr"` (right Option) | Same `rdev::listen` call, no `#[cfg]` needed — `rdev` 0.5.3 maps `Key::AltGr` on Windows to `VK_RMENU` (165, right Alt), so the default hotkey string round-trips identically | No — rely on rdev's own Windows backend (SetWindowsHookEx) |
| Paste injection (`src-tauri/src/inject.rs:32-35`) | `enigo` press `Key::Meta` (Cmd) + `V` | `#[cfg(not(target_os = "macos"))]` presses `Key::Control` + `V` | No — `enigo` 0.2's Windows backend uses `SendInput` |
| Audio capture (`src-tauri/src/audio.rs`) | `cpal` default host/device, dedicated thread because `cpal::Stream` is `!Send` on macOS | Same code path, no cfg — `cpal`'s WASAPI backend on Windows is also driven from the same dedicated thread (harmless there too, just not required) | No |
| Keyring / API key storage (`src-tauri/src/settings.rs`, `Cargo.toml`) | `keyring` crate `apple-native` feature (Keychain) | `keyring` crate `windows-native` feature (Credential Manager) — both features are already enabled together in `Cargo.toml` `keyring = { features = ["apple-native", "windows-native"] }` | No |
| Tray icon (`src-tauri/src/lib.rs` `build_tray`) | `TrayIconBuilder` (tauri-plugin `tray-icon`), left-click opens menu | Same code, no `#[cfg]` — cross-platform Tauri API | No — Windows convention is usually right-click for tray menus; left-click-opens is a UX choice, not a compile issue |
| Autostart / launch-on-login (`src-tauri/src/lib.rs:28-31`, `tauri-plugin-autostart` 2.5.1) | Plugin registers a macOS LaunchAgent (`MacosLauncher::LaunchAgent`, only used behind the plugin's own `#[cfg(target_os = "macos")]`) | Plugin writes a `Run` registry key via the `auto-launch` crate (`#[cfg(windows)]` inside the plugin) | No — `MacosLauncher` argument compiles fine on Windows because the plugin marks the parameter `#[allow(unused)]` there |
| `open_url` command (`src-tauri/src/commands.rs:149-166`) | `open <url>` | `#[cfg(target_os = "windows")]` → `cmd /c start "" <url>` | No |
| `open_privacy_settings` command (`src-tauri/src/commands.rs:169-182`) | Opens `x-apple.systempreferences:...` deep link | `#[cfg(not(target_os = "macos"))]` → no-op (`let _ = pane;`) | No — Windows has no equivalent flow implemented yet; permissions page will just not offer a "open settings" shortcut on Windows |
| `permissions_status` command (`src-tauri/src/commands.rs:137-144`) | Calls `macos_accessibility_client::accessibility::application_is_trusted()` | `#[cfg(not(target_os = "macos"))]` → hardcoded `true` (Windows has no Accessibility-permission concept blocking global input hooks) | No |
| NSPanel bubble styling (`src-tauri/src/lib.rs:26-27`, `:102-117`) | `tauri-nspanel` plugin init + panel style mask / level / collection behavior, so the bubble floats above fullscreen apps without stealing focus | Entirely skipped — both the plugin `.plugin(...)` call and the panel-styling block are behind `#[cfg(target_os = "macos")]`, and the `tauri-nspanel` / `macos-accessibility-client` crates are declared under `[target.'cfg(target_os = "macos")'.dependencies]` in `Cargo.toml` so they aren't even fetched/compiled for a Windows target | No — the bubble window will use plain Tauri window flags only (see next row) |
| Bubble focus/taskbar/transparency flags (`src-tauri/tauri.conf.json` bubble window) | `focus: false`, `skipTaskbar: true`, `alwaysOnTop: true`, `transparent: true`, `decorations: false`, `shadow: false` — all declarative, applied by Tauri itself | Same declarative config, no code path difference — Tauri translates these to the equivalent Win32 window styles/`SetWindowPos` calls | No — transparency + no-shadow combinations on WebView2 are a known area to double check visually |
| Bundle targets (`src-tauri/tauri.conf.json` `bundle.targets`) | `"dmg"` | `"nsis"`, with `bundle.windows.nsis.installMode: "currentUser"` and `languages: ["English", "Slovak"]` | No — cross-compiling an NSIS installer from macOS is not attempted here; Task 11 CI builds it on `windows-latest` |

## Explicitly untested-at-runtime list

Nothing in this list has ever executed on a real Windows machine or a
Windows CI runner. All of it is "should work" based on reading the
dependency source and Tauri/rdev/enigo/cpal/keyring's own Windows support:

- Global hotkey capture and push-to-talk/double-tap timing via `rdev` on
  Windows (including whether the listener needs to run elevated for any
  target application, which macOS's Accessibility-permission model doesn't
  have an equivalent gate for).
- Clipboard save/restore + `Ctrl+V` injection via `enigo`/`arboard` into a
  real Windows foreground app (timing constants `80ms`/`150ms` in
  `inject.rs` were tuned on macOS only).
- `cpal` audio capture via WASAPI (device enumeration, default sample
  rate/channels, mono-downmix, resampling).
- `keyring`'s Windows Credential Manager backend for reading/writing the
  Groq API key.
- Tray icon rendering/behavior and the language submenu's checkmarks.
- Autostart via the Windows registry `Run` key (enable/disable toggle from
  Settings).
- `cmd /c start` opening the Groq signup URL in the default browser.
- Bubble window visuals (transparency, no shadow, always-on-top, no
  taskbar entry, no focus steal) without the NSPanel-equivalent styling
  that macOS gets — Windows relies entirely on the declarative Tauri window
  config since there is no NSPanel counterpart implemented.
- The dead-hotkey-listener error message (Slovak text pointing at
  "Nastavenia → Súkromie a bezpečnosť → Prístupnosť") is macOS-worded; if the
  listener ever fails to start on Windows, the message shown would be
  inaccurate. Left as-is (UX copy, not a compile/functional gap) — flagged
  here as a known risk, not fixed by this task.
- NSIS installer produced by `tauri.conf.json`'s `bundle.windows.nsis`
  config (per-user install mode, English + Slovak installer language) has
  never actually been built or run.

## Known risks

- No Windows machine or CI job has run this app end-to-end; every "Windows
  path" in the table above is inferred from reading `rdev`/`enigo`/`cpal`/
  `keyring`/`tauri-plugin-autostart` source, not from execution.
- `open_privacy_settings` is a no-op on Windows — if a future permissions
  flow needs to deep-link into Windows privacy settings (e.g.
  `ms-settings:privacy-microphone`), that's unimplemented.
- The dead-listener message shown to the user is macOS-specific wording
  (see above) and would mislead a Windows user if the hotkey listener ever
  failed to start.
- Cross-compiling `x86_64-pc-windows-msvc` from macOS is not possible here
  (no linker) — this task could not run even `cargo check` for the Windows
  target locally. Task 11's CI `windows-latest` job is the first real
  compile-time signal for Windows.
