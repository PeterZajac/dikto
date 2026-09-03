use crate::history::Dictation;
use crate::pipeline::{self, AppCtx};
use crate::settings::{self, Settings};
use crate::state::Event;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, State};

#[tauri::command]
pub fn cancel_dictation(ctx: State<'_, Arc<AppCtx>>) {
    pipeline::cancel(ctx.inner());
}

/// Re-runs STT on the audio kept from a failed attempt (spec §6: "skúsiť znova").
#[tauri::command]
pub async fn retry_transcription(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    // begin() only bumps gen and writes the phase if the transition is legal,
    // so a second fast click (still Transcribing) is a no-op, not a clobber.
    let ui = ctx.settings.read().unwrap().ui_language;
    let Some(gen) = pipeline::begin(&ctx, Event::RetryRequested, true, None) else {
        return Err(ui.pick("retry is not possible right now", "retry nie je možný teraz").into());
    };
    let take = ctx.pending_take.lock().unwrap().clone();
    let Some(wav) = take.as_ref().and_then(|t| load_take_audio(&ctx, t)) else {
        let message = ui.pick("no audio to retry", "žiadne audio na zopakovanie");
        pipeline::advance(&ctx, gen, Event::Failed, Some(message));
        return Err(message.into());
    };
    // No deadline: the user just asked for this, so paste wherever they are.
    pipeline::transcribe_and_deliver(ctx.inner().clone(), wav, gen, take, None).await;
    Ok(())
}

fn load_take_audio(ctx: &AppCtx, take: &pipeline::TakeRecord) -> Option<Vec<u8>> {
    ctx.recordings.read(take.audio_name.as_deref()?).ok()
}

#[tauri::command]
pub fn set_groq_key(ctx: State<'_, Arc<AppCtx>>, key: String) -> Result<(), String> {
    let mut new = ctx.settings.read().unwrap().clone();
    new.groq_api_key = key;
    apply_settings(ctx.inner(), new)
}

#[tauri::command]
pub fn get_settings(ctx: State<'_, Arc<AppCtx>>) -> Settings {
    ctx.settings.read().unwrap().clone()
}

/// Persists `new`, updates the live ctx, and notifies the frontend. Shared by
/// the `set_settings` command, the tray's language submenu, and the bubble's
/// position-save task so all paths stay in sync (hotkey listener, on-disk
/// file, in-memory settings, event, tray checkmarks).
pub(crate) fn apply_settings(ctx: &AppCtx, new: Settings) -> Result<(), String> {
    let new = new.sanitized();
    // Save first: a failed write must leave the live hotkey/settings/tray
    // state untouched instead of drifting ahead of what's on disk.
    settings::save(&ctx.settings_path, &new).map_err(|e| e.to_string())?;
    *ctx.hotkey_name.write().unwrap() = new.hotkey.clone();
    *ctx.ui_lang.write().unwrap() = new.ui_language;
    *ctx.settings.write().unwrap() = new.clone();
    let _ = ctx.app.emit("settings:changed", &new);
    // Keep the tray's language checkmarks in sync regardless of which path
    // changed the language (Settings page, tray itself). No-op until the
    // tray has finished building. Clone the handles out and drop the lock
    // before calling set_checked: it blocks on the main-thread event loop,
    // and the tray's own menu handler re-enters apply_settings while running
    // on that same thread — holding the mutex across the call deadlocks.
    let items = ctx.tray_lang_items.lock().unwrap().clone();
    if let Some(items) = items {
        for (mode, item) in items {
            let _ = item.set_checked(mode == new.language);
        }
    }
    let labels = ctx.tray_labels.lock().unwrap().clone();
    if let Some(labels) = labels {
        let ui = new.ui_language;
        let _ = labels.open.set_text(ui.pick(pipeline::TRAY_OPEN.0, pipeline::TRAY_OPEN.1));
        let _ = labels.quit.set_text(ui.pick(pipeline::TRAY_QUIT.0, pipeline::TRAY_QUIT.1));
        let _ = labels
            .language
            .set_text(ui.pick(pipeline::TRAY_LANGUAGE.0, pipeline::TRAY_LANGUAGE.1));
    }
    Ok(())
}

#[tauri::command]
pub fn set_settings(ctx: State<'_, Arc<AppCtx>>, new: Settings) -> Result<(), String> {
    apply_settings(ctx.inner(), new)
}

/// Arms (or, with `cancel: true`, disarms) the one-shot hotkey-capture flag
/// consumed by the rdev listener thread (see `hotkey::spawn`). Settings calls
/// this with `cancel: true` when its 10s capture-UI timeout elapses, so a
/// keypress after the user has given up isn't still swallowed as a capture.
#[tauri::command]
pub fn hotkey_capture_start(ctx: State<'_, Arc<AppCtx>>, cancel: bool) {
    ctx.capture_next.store(!cancel, Ordering::SeqCst);
}

#[tauri::command]
pub fn has_groq_key(ctx: State<'_, Arc<AppCtx>>) -> bool {
    settings::groq_api_key(&ctx.settings.read().unwrap()).is_some()
}

#[tauri::command]
pub async fn test_groq_key(ctx: State<'_, Arc<AppCtx>>) -> Result<bool, String> {
    let (url, key) = {
        let s = ctx.settings.read().unwrap();
        (
            s.groq_url.clone(),
            settings::groq_api_key(&s).ok_or(s.ui_language.pick("key missing", "chýba kľúč"))?,
        )
    };
    let resp = reqwest::Client::new()
        .get(format!("{}/openai/v1/models", url.trim_end_matches('/')))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.status().is_success())
}

/// Tauri v2 requires async commands that borrow `State` to return a
/// `Result` (see AsyncCommandMustReturnResult); this probe never fails,
/// so the error type is uninhabited.
#[tauri::command]
pub async fn meridian_status(ctx: State<'_, Arc<AppCtx>>) -> Result<bool, ()> {
    let (url, model) = {
        let s = ctx.settings.read().unwrap();
        (s.meridian_url.clone(), s.cleanup_model.clone())
    };
    Ok(crate::cleanup::CleanupClient::new(url, model).is_reachable().await)
}

/// Round-trips a one-token completion through Meridian, so the Settings page
/// can prove the model actually answers instead of just that something is
/// listening on the port.
#[tauri::command]
pub async fn test_cleanup(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    let client = {
        let s = ctx.settings.read().unwrap();
        crate::cleanup::CleanupClient::for_settings(&s)
    };
    client.probe().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn finish_wizard(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    let mut new = ctx.settings.read().unwrap().clone();
    new.wizard_done = true;
    apply_settings(ctx.inner(), new)
}

#[tauri::command]
pub fn history_list(
    ctx: State<'_, Arc<AppCtx>>,
    search: Option<String>,
    limit: Option<u32>,
) -> Vec<Dictation> {
    ctx.history
        .list(search.as_deref(), limit.unwrap_or(200))
        .unwrap_or_default()
}

#[tauri::command]
pub fn history_delete(ctx: State<'_, Arc<AppCtx>>, id: i64) {
    if let Ok(Some(audio)) = ctx.history.delete(id) {
        ctx.recordings.remove(&audio);
    }
}

#[tauri::command]
pub fn history_clear(ctx: State<'_, Arc<AppCtx>>) {
    if let Ok(audio) = ctx.history.clear() {
        ctx.recordings.remove_all(audio);
    }
}

/// Re-transcribes a stored recording straight into its history row. Runs
/// outside the dictation FSM on purpose: it's a background repair of an old
/// row, not a live take, so it works while idle and never pastes anywhere.
#[tauri::command]
pub async fn history_retry(ctx: State<'_, Arc<AppCtx>>, id: i64) -> Result<(), String> {
    let ui = ctx.settings.read().unwrap().ui_language;
    let row = ctx
        .history
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or(ui.pick("entry does not exist", "záznam neexistuje"))?;
    let audio = row
        .audio_path
        .ok_or(ui.pick("no audio kept for this entry", "k tomuto záznamu nemáme audio"))?;
    let wav = ctx
        .recordings
        .read(&audio)
        .map_err(|_| ui.pick("audio could not be read", "audio sa nedá načítať"))?;

    let (url, lang) = {
        let s = ctx.settings.read().unwrap();
        (s.groq_url.clone(), s.language.code())
    };
    let key = settings::groq_api_key(&ctx.settings.read().unwrap())
        .ok_or(ui.pick("Groq API key missing", "chýba Groq API kľúč"))?;

    ctx.limiter.try_acquire(crate::ratelimit::Priority::Final);
    let result = crate::stt::SttClient::new(url, key)
        .transcribe_with(wav, lang, crate::stt::RetryPolicy::default(), |_, _| {})
        .await;
    let outcome = match result {
        Ok(t) if !t.text.is_empty() => {
            let cleaned = clean_or_raw(&ctx, &t.text).await;
            ctx.history
                .mark_done(id, &t.text, &cleaned, t.language.as_deref())
                .map_err(|e| e.to_string())
        }
        Ok(_) => ctx
            .history
            .mark_failed(id, ui.pick("empty transcript — silence?", "prázdny prepis — ticho?"))
            .map_err(|e| e.to_string()),
        Err(e) => {
            if e.is_rate_limit() {
                ctx.limiter.note_rate_limited(e.retry_after());
            }
            let message = format!("{}: {e}", ui.pick("transcription failed", "prepis zlyhal"));
            let _ = ctx.history.mark_failed(id, &message);
            Err(message)
        }
    };
    pipeline::emit_history_changed(&ctx);
    outcome.map(|_| ())
}

/// Cleanup for the history-retry path: best-effort, falling back to the raw
/// transcript exactly like the live pipeline does.
async fn clean_or_raw(ctx: &AppCtx, raw: &str) -> String {
    let client = {
        let s = ctx.settings.read().unwrap();
        if !s.cleanup_enabled {
            return raw.to_string();
        }
        crate::cleanup::CleanupClient::for_settings(&s)
    };
    client.clean(raw).await.unwrap_or_else(|_| raw.to_string())
}

fn audio_path_of(ctx: &AppCtx, id: i64) -> Option<std::path::PathBuf> {
    let audio = ctx.history.get(id).ok()??.audio_path?;
    let path = ctx.recordings.path(&audio)?;
    path.exists().then_some(path)
}

/// Absolute path of a stored recording, for the frontend's "save as" flow.
#[tauri::command]
pub fn history_audio_path(ctx: State<'_, Arc<AppCtx>>, id: i64) -> Option<String> {
    audio_path_of(&ctx, id).map(|p| p.to_string_lossy().into_owned())
}

/// Copies a stored recording into the user's Downloads folder and returns the
/// path it landed on, so a dictation the API never managed to transcribe can
/// still be rescued out of the app.
#[tauri::command]
pub fn history_export_audio(ctx: State<'_, Arc<AppCtx>>, id: i64) -> Result<String, String> {
    let ui = ctx.settings.read().unwrap().ui_language;
    let src = audio_path_of(&ctx, id)
        .ok_or(ui.pick("no audio kept for this entry", "k tomuto záznamu nemáme audio"))?;
    let ts = ctx.history.get(id).ok().flatten().map(|d| d.ts).unwrap_or_default();
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or(ui.pick("could not find the Downloads folder", "neviem nájsť priečinok Stiahnuté"))?;
    let dest = free_path(&dir, &format!("dikto-{ts}"));
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().into_owned())
}

/// `<dir>/<stem>.wav`, with a numeric suffix if that name is taken — an export
/// must never quietly overwrite a file the user already has.
fn free_path(dir: &std::path::Path, stem: &str) -> std::path::PathBuf {
    let first = dir.join(format!("{stem}.wav"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|n| dir.join(format!("{stem}-{n}.wav")))
        .find(|p| !p.exists())
        .expect("an unused suffix always exists")
}

#[tauri::command]
pub fn permissions_status() -> serde_json::Value {
    #[cfg(target_os = "macos")]
    let accessibility = macos_accessibility_client::accessibility::application_is_trusted();
    #[cfg(not(target_os = "macos"))]
    let accessibility = true;
    serde_json::json!({ "accessibility": accessibility })
}

/// Opens the OS's default browser — but only for the one hardcoded Groq
/// signup URL (wizard step 3). Anything else is silently ignored so this
/// can never become a generic "open arbitrary URL" primitive.
#[tauri::command]
pub fn open_url(url: String) {
    if url != "https://console.groq.com" {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
}

/// `pane`: "accessibility" | "microphone".
#[tauri::command]
pub fn open_privacy_settings(pane: String) {
    #[cfg(target_os = "macos")]
    {
        let anchor = if pane == "microphone" { "Privacy_Microphone" } else { "Privacy_Accessibility" };
        let _ = std::process::Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let page = if pane == "microphone" { "privacy-microphone" } else { "privacy-general" };
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", &format!("ms-settings:{page}")])
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = pane;
    }
}
