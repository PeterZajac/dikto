use crate::audio::{self, Recorder};
use crate::cleanup::CleanupClient;
use crate::hotkey::HotkeySignal;
use crate::settings::{self, Settings};
use crate::state::{transition, Event, Phase};
use crate::stt::SttClient;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppCtx {
    pub phase: Mutex<Phase>,
    pub recorder: Recorder,
    pub settings: RwLock<Settings>,
    pub pending_wav: Mutex<Option<Vec<u8>>>,
    pub partial_inflight: AtomicBool,
    pub app: AppHandle,
}

pub fn set_phase(ctx: &AppCtx, phase: Phase, message: Option<&str>) {
    *ctx.phase.lock().unwrap() = phase;
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": phase, "message": message }),
    );
}

fn apply(ctx: &AppCtx, ev: Event) -> Option<Phase> {
    let mut guard = ctx.phase.lock().unwrap();
    let next = transition(*guard, ev)?;
    *guard = next;
    Some(next)
}

fn show_bubble(ctx: &AppCtx) {
    if let Some(w) = ctx.app.get_webview_window("bubble") {
        crate::position_bubble(&w);
        let _ = w.show();
    }
}

fn hide_bubble_after(ctx: &Arc<AppCtx>, ms: u64) {
    let ctx = ctx.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        // Only hide when we're back to Idle (a new take may have started).
        if *ctx.phase.lock().unwrap() == Phase::Idle {
            if let Some(w) = ctx.app.get_webview_window("bubble") {
                let _ = w.hide();
            }
        }
    });
}

pub fn handle_signal(ctx: Arc<AppCtx>, sig: HotkeySignal) {
    match sig {
        HotkeySignal::Start => start_recording(&ctx),
        HotkeySignal::Stop => {
            if apply(&ctx, Event::StopRequested).is_some() {
                set_phase(&ctx, Phase::Transcribing, None);
                let ctx2 = ctx.clone();
                tauri::async_runtime::spawn(async move { finish(ctx2).await });
            }
        }
        HotkeySignal::Cancel => cancel(&ctx),
    }
}

fn start_recording(ctx: &Arc<AppCtx>) {
    if apply(ctx, Event::StartRequested).is_none() {
        return;
    }
    ctx.pending_wav.lock().unwrap().take();
    let app = ctx.app.clone();
    let last_emit = Arc::new(Mutex::new(std::time::Instant::now()));
    let on_amp = Box::new(move |rms: f32| {
        let mut last = last_emit.lock().unwrap();
        if last.elapsed().as_millis() >= 100 {
            *last = std::time::Instant::now();
            let _ = app.emit("dictation:amplitude", serde_json::json!({ "value": rms }));
        }
    });
    match ctx.recorder.start(on_amp) {
        Ok(()) => {
            set_phase(ctx, Phase::Recording, None);
            show_bubble(ctx);
        }
        Err(e) => {
            apply(ctx, Event::Failed);
            set_phase(ctx, Phase::Error, Some(&format!("mikrofón: {e}")));
            show_bubble(ctx);
        }
    }
}

pub fn cancel(ctx: &Arc<AppCtx>) {
    let _ = ctx.recorder.stop();
    if apply(ctx, Event::Cancel).is_some() {
        set_phase(ctx, Phase::Idle, None);
        hide_bubble_after(ctx, 0);
    }
}

async fn finish(ctx: Arc<AppCtx>) {
    let Some((samples, rate, ch)) = ctx.recorder.stop() else {
        set_phase(&ctx, Phase::Idle, None);
        return;
    };
    // < 0.4 s of audio → treat as silence.
    if samples.len() < (rate as usize * ch as usize) * 2 / 5 {
        set_phase(&ctx, Phase::Idle, Some("nič som nepočul"));
        hide_bubble_after(&ctx, 1200);
        return;
    }
    let wav = audio::prepare_wav(&samples, rate, ch);
    transcribe_and_deliver(ctx, wav).await;
}

pub async fn transcribe_and_deliver(ctx: Arc<AppCtx>, wav: Vec<u8>) {
    let (groq_url, lang, cleanup_enabled, meridian_url, model) = {
        let s = ctx.settings.read().unwrap();
        (
            s.groq_url.clone(),
            s.language.code(),
            s.cleanup_enabled,
            s.meridian_url.clone(),
            s.cleanup_model.clone(),
        )
    };
    let Some(api_key) = settings::groq_api_key() else {
        *ctx.pending_wav.lock().unwrap() = Some(wav);
        apply(&ctx, Event::Failed);
        set_phase(&ctx, Phase::Error, Some("chýba Groq API kľúč"));
        return;
    };

    // 1. STT
    let stt = SttClient::new(groq_url, api_key);
    let transcript = match stt.transcribe(wav.clone(), lang).await {
        Ok(t) => t,
        Err(e) => {
            *ctx.pending_wav.lock().unwrap() = Some(wav);
            apply(&ctx, Event::Failed);
            set_phase(&ctx, Phase::Error, Some(&format!("prepis zlyhal: {e}")));
            return;
        }
    };
    if transcript.text.is_empty() {
        set_phase(&ctx, Phase::Idle, Some("nič som nepočul"));
        hide_bubble_after(&ctx, 1200);
        return;
    }
    if apply(&ctx, Event::TranscriptReady).is_none() {
        return; // cancelled meanwhile
    }

    // 2. Cleanup (best-effort — spec: never lose text)
    let mut note: Option<&str> = None;
    let final_text = if cleanup_enabled {
        set_phase(&ctx, Phase::Cleaning, Some("✨ upravujem text…"));
        match CleanupClient::new(meridian_url, model).clean(&transcript.text).await {
            Ok(cleaned) => cleaned,
            Err(_) => {
                note = Some("vložené bez úprav");
                transcript.text.clone()
            }
        }
    } else {
        transcript.text.clone()
    };
    if apply(&ctx, Event::CleanupDone).is_none() {
        return; // cancelled meanwhile
    }

    // 3. Inject
    set_phase(&ctx, Phase::Injecting, None);
    let inject_result =
        tauri::async_runtime::spawn_blocking(move || crate::inject::inject_text(&final_text))
            .await
            .unwrap_or_else(|e| Err(crate::inject::InjectError::Keystroke(e.to_string())));
    match inject_result {
        Ok(()) => {
            apply(&ctx, Event::Injected);
            set_phase(&ctx, Phase::Idle, Some(note.unwrap_or("✓ vložené")));
            hide_bubble_after(&ctx, 1200);
        }
        Err(e) => {
            apply(&ctx, Event::Failed);
            set_phase(&ctx, Phase::Error, Some(&format!("vloženie zlyhalo: {e}")));
        }
    }
}
