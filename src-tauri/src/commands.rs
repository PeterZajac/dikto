use crate::pipeline::{self, AppCtx};
use crate::settings;
use crate::state::Event;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn cancel_dictation(ctx: State<'_, Arc<AppCtx>>) {
    pipeline::cancel(ctx.inner());
}

/// Re-runs STT on the audio kept from a failed attempt (spec §6: "skúsiť znova").
#[tauri::command]
pub async fn retry_transcription(ctx: State<'_, Arc<AppCtx>>) -> Result<(), String> {
    // Bump gen before the transition so a concurrent start/cancel racing us
    // bumps it again too — our advance() calls then just no-op instead of
    // clobbering whatever phase the FSM has since moved to.
    let gen = ctx.take_gen.fetch_add(1, Ordering::SeqCst) + 1;
    if !pipeline::advance(&ctx, gen, Event::RetryRequested, None) {
        return Err("retry nie je možný teraz".into());
    }
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
