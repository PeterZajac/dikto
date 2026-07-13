//! Headless `--selftest <wav-path>` pipeline check (see main.rs). Exercises
//! the same modules the GUI pipeline uses — settings/key resolution, WAV
//! decoding, Groq STT, Meridian cleanup, clipboard injection, and paste-event
//! construction — without spinning up the tauri event loop, so it can be run
//! from a terminal right after a signed install to confirm TCC grants and
//! the Groq key actually work.

use crate::{audio, cleanup, inject, settings, stt};

const CLIPBOARD_MARKER: &str = "dikto-selftest-marker";

enum Outcome {
    Pass(String),
    Fail(String),
    Skip(String),
}

impl Outcome {
    fn pass(msg: impl Into<String>) -> Self {
        Outcome::Pass(msg.into())
    }
    fn fail(msg: impl Into<String>) -> Self {
        Outcome::Fail(msg.into())
    }
    fn skip(msg: impl Into<String>) -> Self {
        Outcome::Skip(msg.into())
    }
    fn is_fail(&self) -> bool {
        matches!(self, Outcome::Fail(_))
    }
}

fn report(stage: &str, outcome: &Outcome) {
    let (tag, detail) = match outcome {
        Outcome::Pass(d) => ("PASS", d.as_str()),
        Outcome::Fail(d) => ("FAIL", d.as_str()),
        Outcome::Skip(d) => ("SKIP", d.as_str()),
    };
    println!("[{tag}] {stage}: {detail}");
}

/// Runs every stage and returns the process exit code: 0 iff every mandatory
/// stage (settings, wav, stt, clipboard, paste-event) passed. `cleanup` is
/// best-effort and never gates the exit code, matching the pipeline's own
/// "never lose text" fallback behavior.
pub fn run(wav_path: &str) -> i32 {
    println!("Dikto self-test");
    let mut mandatory_ok = true;

    let (settings_outcome, settings_data) = stage_settings();
    report("settings", &settings_outcome);
    mandatory_ok &= !settings_outcome.is_fail();

    let (wav_outcome, wav_bytes) = stage_wav(wav_path);
    report("wav", &wav_outcome);
    mandatory_ok &= !wav_outcome.is_fail();

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            report("stt", &Outcome::fail(format!("could not start async runtime: {e}")));
            report("cleanup", &Outcome::skip("async runtime unavailable"));
            report("clipboard", &stage_clipboard());
            report("paste-event", &stage_paste_event());
            return 1;
        }
    };

    let transcript = match (&settings_data, &wav_bytes) {
        (Some((s, key)), Some(wav)) => {
            let (outcome, transcript) = rt.block_on(stage_stt(s, key, wav));
            report("stt", &outcome);
            mandatory_ok &= !outcome.is_fail();
            transcript
        }
        _ => {
            report("stt", &Outcome::fail("skipped — no Groq key or WAV bytes available"));
            mandatory_ok = false;
            None
        }
    };

    let cleanup_outcome = match (&settings_data, &transcript) {
        (Some((s, _)), Some(t)) if !t.text.is_empty() => rt.block_on(stage_cleanup(s, &t.text)),
        _ => Outcome::skip("no transcript available"),
    };
    report("cleanup", &cleanup_outcome);
    // Non-mandatory: never gates the exit code (Meridian is best-effort).

    let clipboard_outcome = stage_clipboard();
    report("clipboard", &clipboard_outcome);
    mandatory_ok &= !clipboard_outcome.is_fail();

    let paste_outcome = stage_paste_event();
    report("paste-event", &paste_outcome);
    mandatory_ok &= !paste_outcome.is_fail();

    if mandatory_ok {
        println!("selftest: all mandatory stages passed");
        0
    } else {
        println!("selftest: one or more mandatory stages failed");
        1
    }
}

/// Loads settings from the real app config dir — `dirs::config_dir()` joined
/// with `APP_IDENTIFIER`, the same shape tauri's `app.path().app_config_dir()`
/// resolves at runtime — and resolves the Groq key (env or settings.json).
fn stage_settings() -> (Outcome, Option<(settings::Settings, String)>) {
    let Some(config_dir) = dirs::config_dir() else {
        return (Outcome::fail("could not resolve the OS config directory"), None);
    };
    let settings_path = config_dir.join(crate::APP_IDENTIFIER).join("settings.json");
    let s = settings::load(&settings_path);
    match settings::groq_api_key(&s) {
        Some(key) => {
            let outcome = Outcome::pass(format!(
                "loaded {} (Groq key present)",
                settings_path.display()
            ));
            (outcome, Some((s, key)))
        }
        None => (
            Outcome::fail(format!(
                "loaded {} but no Groq API key (set GROQ_API_KEY or add it in Settings)",
                settings_path.display()
            )),
            None,
        ),
    }
}

/// Decodes `path` (i16 or f32 PCM, any rate/channel count) via hound, then
/// runs it through the exact `audio::prepare_wav` path the real pipeline uses.
fn stage_wav(path: &str) -> (Outcome, Option<Vec<u8>>) {
    let reader = match hound::WavReader::open(path) {
        Ok(r) => r,
        Err(e) => return (Outcome::fail(format!("could not open {path}: {e}")), None),
    };
    let spec = reader.spec();
    let samples: Result<Vec<f32>, hound::Error> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect(),
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect(),
    };
    let samples = match samples {
        Ok(s) => s,
        Err(e) => return (Outcome::fail(format!("decode error: {e}")), None),
    };
    if samples.is_empty() {
        return (Outcome::fail("WAV file decoded to zero samples"), None);
    }
    let wav = audio::prepare_wav(&samples, spec.sample_rate, spec.channels);
    let outcome = Outcome::pass(format!(
        "{} samples @ {}Hz/{}ch decoded, prepare_wav -> {} bytes",
        samples.len(),
        spec.sample_rate,
        spec.channels,
        wav.len()
    ));
    (outcome, Some(wav))
}

/// Real Groq call via `stt::SttClient`, always with auto language detection
/// regardless of the configured `LanguageMode`.
async fn stage_stt(
    s: &settings::Settings,
    key: &str,
    wav: &[u8],
) -> (Outcome, Option<stt::Transcript>) {
    let client = stt::SttClient::new(s.groq_url.clone(), key.to_string());
    match client.transcribe(wav.to_vec(), None).await {
        Ok(t) => {
            let outcome = Outcome::pass(format!("transcript: {:?}", t.text));
            (outcome, Some(t))
        }
        Err(e) => (Outcome::fail(format!("Groq call failed: {e}")), None),
    }
}

/// Meridian cleanup against the configured `meridian_url`. Unreachable/timeout
/// is a SKIP (Meridian is optional infra), not a FAIL.
async fn stage_cleanup(s: &settings::Settings, text: &str) -> Outcome {
    let client =
        cleanup::CleanupClient::with_style(s.meridian_url.clone(), s.cleanup_model.clone(), s.cleanup_style);
    match client.clean(text).await {
        Ok(cleaned) => Outcome::pass(format!("cleaned: {cleaned:?}")),
        Err(cleanup::CleanupError::Network(e)) => Outcome::skip(format!("Meridian unreachable: {e}")),
        Err(e) => Outcome::fail(format!("Meridian error: {e}")),
    }
}

/// Save/set/get/restore roundtrip of a marker string through the same
/// `arboard` clipboard the pipeline uses for injection.
fn stage_clipboard() -> Outcome {
    let mut cb = match arboard::Clipboard::new() {
        Ok(cb) => cb,
        Err(e) => return Outcome::fail(format!("could not open clipboard: {e}")),
    };
    let previous = cb.get_text().ok();
    if let Err(e) = cb.set_text(CLIPBOARD_MARKER.to_string()) {
        return Outcome::fail(format!("set_text failed: {e}"));
    }
    let roundtrip = cb.get_text();
    if let Some(prev) = previous {
        let _ = cb.set_text(prev);
    }
    match roundtrip {
        Ok(v) if v == CLIPBOARD_MARKER => {
            Outcome::pass("set/get roundtrip matched, previous clipboard restored")
        }
        Ok(v) => Outcome::fail(format!("roundtrip mismatch, got {v:?}")),
        Err(e) => Outcome::fail(format!("get_text failed: {e}")),
    }
}

/// Constructs the Cmd+V CGEvent pair without posting it (the mandatory
/// `paste-event-construct` check), then best-effort posts it via the same
/// `inject::paste_keystroke` the pipeline uses — landing can't be verified
/// headlessly, so a post failure is noted but doesn't fail the stage.
#[cfg(target_os = "macos")]
fn stage_paste_event() -> Outcome {
    match inject::probe_construct_paste_event() {
        Ok(()) => {
            let post_note = match inject::paste_keystroke() {
                Ok(()) => "posted Cmd+V best-effort (landing not verifiable headlessly)".to_string(),
                Err(e) => format!("best-effort post errored, harmless without focus/permission: {e}"),
            };
            Outcome::pass(format!("constructed CGEventSource + Cmd+V keydown/keyup; {post_note}"))
        }
        Err(e) => Outcome::fail(format!("construction failed: {e}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn stage_paste_event() -> Outcome {
    Outcome::skip("unsupported on this OS")
}
