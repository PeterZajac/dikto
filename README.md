# Dikto

Hold a hotkey, speak, and the transcribed text is typed at your cursor in
whatever app you're in. Speech-to-text runs through a free Groq API key,
optional Claude cleanup through your own [Meridian](https://github.com/rynfar/meridian)
instance, and everything else stays on your machine. Tauri v2 + React + Rust.

See [FEATURES.md](FEATURES.md) for what it does, [CHANGELOG.md](CHANGELOG.md)
for what changed.

## Install

Download from the [Releases page](../../releases/latest).

**macOS**

1. Open the `.dmg`, drag Dikto to Applications.
2. The build isn't notarized, so clear the quarantine flag once:
   ```sh
   xattr -d com.apple.quarantine /Applications/Dikto.app
   ```
3. Open Dikto and grant **Accessibility** and **Microphone** when asked.

**Windows (experimental, untested)** — run the `.exe`; on the SmartScreen
warning click "More info" → "Run anyway". See [docs/WINDOWS.md](docs/WINDOWS.md).

## First run

1. Grant permissions (macOS: Accessibility, Microphone).
2. Paste a free Groq API key from [console.groq.com](https://console.groq.com).
3. Optional: run [Meridian](https://github.com/rynfar/meridian) locally for
   Claude cleanup; otherwise raw transcripts are pasted.
4. Try it: click the test field, hold the hotkey (right Option on macOS,
   right Ctrl on Windows), speak, release.

UI is English by default; Slovak in Settings → Interface language.

## Uninstall

macOS, after quitting Dikto from the tray:

```sh
rm -rf /Applications/Dikto.app ~/Library/Application\ Support/com.peterzajac.dikto
tccutil reset All com.peterzajac.dikto
```

Windows: Settings → Apps, then delete `%APPDATA%\com.peterzajac.dikto`.

## Build from source

Prerequisites: Rust via [rustup](https://rustup.rs) (toolchain pinned in
`rust-toolchain.toml`, auto-installed), Node.js 22 with `corepack enable`.
macOS also needs `xcode-select --install`; Windows needs the
[C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
and WebView2.

```sh
git clone https://github.com/PeterZajac/dikto.git
cd dikto
corepack enable
pnpm install
pnpm tauri dev      # run with hot reload
pnpm tauri build    # bundle: src-tauri/target/release/bundle/
```

In dev mode on macOS the permissions belong to the terminal you launched
from, not to Dikto. More in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## License

MIT — see [LICENSE](LICENSE).
