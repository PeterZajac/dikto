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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CleanupStyle {
    #[default]
    Light,
    Strong,
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
    pub cleanup_style: CleanupStyle,
    pub wizard_done: bool,
    pub bubble_pos: Option<(i32, i32)>,
    pub autostart: bool,
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
            cleanup_style: CleanupStyle::Light,
            wizard_done: false,
            bubble_pos: None,
            autostart: false,
        }
    }
}

/// True if `url` is `http://` or `https://` pointing at 127.0.0.1 or
/// localhost (dev/mock servers, no real key ever crosses these).
fn is_local_http(url: &str) -> bool {
    ["http://127.0.0.1", "https://127.0.0.1", "http://localhost", "https://localhost"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

impl Settings {
    /// Resets `groq_url`/`meridian_url` to their defaults if they don't match
    /// an allow-listed shape, so a bad or malicious value from a settings.json
    /// edit (or a compromised renderer) can't redirect the STT/cleanup calls —
    /// notably the Groq call, which carries the API key — to an arbitrary host.
    pub fn sanitized(mut self) -> Self {
        if self.groq_url != "https://api.groq.com" && !is_local_http(&self.groq_url) {
            self.groq_url = Settings::default().groq_url;
        }
        // Meridian never receives the Groq key — only dictated text — so a
        // tunneled https:// host is allowed in addition to localhost.
        if !is_local_http(&self.meridian_url) && !self.meridian_url.starts_with("https://") {
            self.meridian_url = Settings::default().meridian_url;
        }
        self
    }
}

pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
        .sanitized()
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
        assert_eq!(s.cleanup_style, CleanupStyle::Light);
        assert!(!s.wizard_done);
        assert_eq!(s.bubble_pos, None);
        assert!(!s.autostart);
    }

    #[test]
    fn old_settings_file_without_new_fields_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        // Simulates a Plan-1-era settings.json, written before cleanup_style,
        // wizard_done, bubble_pos and autostart existed.
        std::fs::write(
            &p,
            r#"{"hotkey":"AltGr","language":"auto","cleanup_enabled":true,
                "cleanup_model":"claude-sonnet-5","meridian_url":"http://127.0.0.1:3456",
                "groq_url":"https://api.groq.com"}"#,
        )
        .unwrap();
        let s = load(&p);
        assert_eq!(s.cleanup_style, CleanupStyle::Light);
        assert!(!s.wizard_done);
        assert_eq!(s.bubble_pos, None);
        assert!(!s.autostart);
    }

    #[test]
    fn bad_groq_url_resets_to_default() {
        let s = Settings { groq_url: "https://evil.example.com".into(), ..Settings::default() };
        assert_eq!(s.sanitized().groq_url, Settings::default().groq_url);
    }

    #[test]
    fn groq_url_localhost_allowed_for_dev_mock() {
        let s = Settings { groq_url: "http://127.0.0.1:8080".into(), ..Settings::default() };
        assert_eq!(s.clone().sanitized().groq_url, "http://127.0.0.1:8080");
        let s = Settings { groq_url: "http://localhost:8080".into(), ..s };
        assert_eq!(s.sanitized().groq_url, "http://localhost:8080");
    }

    #[test]
    fn default_groq_url_passes_sanitized_unchanged() {
        let s = Settings::default();
        assert_eq!(s.clone().sanitized(), s);
    }

    #[test]
    fn bad_meridian_url_resets_to_default() {
        let s = Settings { meridian_url: "http://evil.example.com".into(), ..Settings::default() };
        assert_eq!(s.sanitized().meridian_url, Settings::default().meridian_url);
    }

    #[test]
    fn meridian_url_allows_localhost_and_any_https() {
        let s = Settings { meridian_url: "http://127.0.0.1:3456".into(), ..Settings::default() };
        assert_eq!(s.clone().sanitized().meridian_url, "http://127.0.0.1:3456");
        let s = Settings { meridian_url: "https://my-tunnel.example.com".into(), ..s };
        assert_eq!(s.sanitized().meridian_url, "https://my-tunnel.example.com");
    }

    #[test]
    fn language_codes() {
        assert_eq!(LanguageMode::Auto.code(), None);
        assert_eq!(LanguageMode::Sk.code(), Some("sk"));
        assert_eq!(LanguageMode::Cs.code(), Some("cs"));
        assert_eq!(LanguageMode::En.code(), Some("en"));
    }
}
