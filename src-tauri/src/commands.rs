use crate::history::Dictation;
use crate::pipeline::{self, AppCtx};
use crate::settings::{self, Settings};
use crate::state::Event;
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
    let mut s = ctx.settings.write().unwrap();
    s.wizard_done = true;
    settings::save(&ctx.settings_path, &s).map_err(|e| e.to_string())
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
