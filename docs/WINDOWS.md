# Windows support

Status: **compiles, never run.** CI runs `cargo check` on `windows-latest`
for every push, and the release workflow builds an NSIS installer, but nobody
has launched Dikto on a real Windows machine yet. Treat Windows as
experimental until someone does and reports back.

## What differs from macOS

| Area | macOS | Windows |
|---|---|---|
| Global hotkey listener (`hotkey.rs`) | Raw `CGEventTap` in `macos_tap.rs` (rdev's macOS backend crashes on macOS 15) | `rdev::listen` (`SetWindowsHookEx`) |
| Default hotkey (`settings.rs`) | `AltGr` = right Option | `ControlRight` = right Ctrl. Right Alt is `AltGr` on SK/CZ layouts and types `@ { } [ ] \`, so it must not be the default there |
| Text delivery (`inject.rs`) | Clipboard + typed directly as CGEvent unicode keystrokes | Clipboard + `Ctrl+V` via `enigo` (`SendInput`) |
| Audio capture (`audio.rs`) | `cpal` / CoreAudio | `cpal` / WASAPI, same code path |
| Groq key storage (`settings.rs`) | Plain text in `settings.json` | Same |
| Accessibility permission | Required; checked with `macos-accessibility-client`, wizard blocks until granted | No equivalent; `permissions_status` reports `true`, wizard hides the row |
| Privacy settings deep link (`commands.rs`) | `x-apple.systempreferences:…` | `ms-settings:privacy-microphone` / `ms-settings:privacy-general` |
| Bubble window | `tauri-nspanel`: non-activating panel, floats over fullscreen apps | Plain Tauri flags from `tauri.conf.json` (`alwaysOnTop`, `transparent`, `skipTaskbar`, `focus: false`) |
| Tray | Left click opens the menu | Same code; Windows users usually expect right click |
| Autostart | LaunchAgent via `tauri-plugin-autostart` | Registry `Run` key via the same plugin |
| Installer | `.dmg`, unsigned | NSIS `.exe`, per-user install, unsigned (SmartScreen warns) |

## Unverified at runtime

- Hotkey capture and push-to-talk / double-tap timing through `rdev`.
- Clipboard save + `Ctrl+V` injection into a real foreground app; the
  `120 ms` / `150 ms` delays in `inject.rs` were tuned on macOS.
- WASAPI device enumeration, default sample rate, mono downmix, resampling.
- Tray icon rendering and the language submenu checkmarks.
- Autostart toggle writing the registry key.
- Bubble transparency and always-on-top behaviour on WebView2.
- The NSIS installer itself (per-user mode, English + Slovak strings).

## If you test on Windows

Run the installer, open Settings, press the hotkey in a text field and
dictate a sentence. Then report which rows above worked, and attach the
output of `Dikto.exe --selftest <some.wav>` from a terminal.
