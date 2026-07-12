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
    let Some(gen) = pipeline::begin(&ctx, Event::RetryRequested, true, None) else {
        return Err("retry nie je možný teraz".into());
    };
    let wav = ctx.pending_wav.lock().unwrap().take();
    let Some(wav) = wav else {
        pipeline::advance(&ctx, gen, Event::Failed, Some("žiadne audio na zopakovanie"));
        return Err("žiadne audio na zopakovanie".into());
    };
    pipeline::transcribe_and_deliver(ctx.inner().clone(), wav, gen, 0).await;
    Ok(())
}

/// Dev/setup helper until Plan 2 ships the settings UI.
#[tauri::command]
pub fn set_groq_key(key: String) -> Result<(), String> {
    settings::set_groq_api_key(&key).map_err(|e| e.to_string())
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
pub fn has_groq_key() -> bool {
    settings::groq_api_key().is_some()
}

#[tauri::command]
pub async fn test_groq_key(ctx: State<'_, Arc<AppCtx>>) -> Result<bool, String> {
    let (url, key) = {
        let s = ctx.settings.read().unwrap();
        (s.groq_url.clone(), settings::groq_api_key().ok_or("chýba kľúč")?)
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
    let _ = ctx.history.delete(id);
}

#[tauri::command]
pub fn history_clear(ctx: State<'_, Arc<AppCtx>>) {
    let _ = ctx.history.clear();
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
    }
}
