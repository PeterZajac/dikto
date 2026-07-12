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
    fn language_codes() {
        assert_eq!(LanguageMode::Auto.code(), None);
        assert_eq!(LanguageMode::Sk.code(), Some("sk"));
        assert_eq!(LanguageMode::Cs.code(), Some("cs"));
        assert_eq!(LanguageMode::En.code(), Some("en"));
    }
}
