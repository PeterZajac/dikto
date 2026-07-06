use crate::pipeline::{self, AppCtx};
use crate::settings;
use crate::state::Phase;
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
    let wav = ctx.pending_wav.lock().unwrap().take();
    let Some(wav) = wav else {
        return Err("žiadne audio na zopakovanie".into());
    };
    // Treat the retry as a fresh take so a stale FSM event from the
    // original failed attempt can no longer touch it.
    let gen = ctx.take_gen.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut phase = ctx.phase.lock().unwrap();
        *phase = Phase::Transcribing;
    }
    pipeline::set_phase(&ctx, Phase::Transcribing, None);
    pipeline::transcribe_and_deliver(ctx.inner().clone(), wav, gen).await;
    Ok(())
}

/// Dev/setup helper until Plan 2 ships the settings UI.
#[tauri::command]
pub fn set_groq_key(key: String) -> Result<(), String> {
    settings::set_groq_api_key(&key).map_err(|e| e.to_string())
}
