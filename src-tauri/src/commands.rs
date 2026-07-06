use crate::pipeline::{self, AppCtx};
use crate::settings;
use crate::state::Event;
use std::sync::Arc;
use tauri::State;

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
    pipeline::transcribe_and_deliver(ctx.inner().clone(), wav, gen).await;
    Ok(())
}

/// Dev/setup helper until Plan 2 ships the settings UI.
#[tauri::command]
pub fn set_groq_key(key: String) -> Result<(), String> {
    settings::set_groq_api_key(&key).map_err(|e| e.to_string())
}
