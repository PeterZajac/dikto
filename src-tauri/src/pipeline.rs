use crate::audio::{self, Recorder};
use crate::cleanup::CleanupClient;
use crate::hotkey::HotkeySignal;
use crate::settings::{self, Settings};
use crate::state::{transition, Event, Phase};
use crate::stt::SttClient;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppCtx {
    pub phase: Mutex<Phase>,
    pub recorder: Recorder,
    pub settings: RwLock<Settings>,
    pub pending_wav: Mutex<Option<Vec<u8>>>,
    pub partial_inflight: AtomicBool,
    /// Bumped on every start_recording/cancel; async tasks capture the value
    /// at spawn time and drop their FSM event if it no longer matches —
    /// otherwise a stale task from a cancelled take could clobber a new one.
    pub take_gen: AtomicU64,
    pub app: AppHandle,
}

pub fn set_phase(ctx: &AppCtx, phase: Phase, message: Option<&str>) {
    *ctx.phase.lock().unwrap() = phase;
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": phase, "message": message }),
    );
}

pub(crate) fn apply(ctx: &AppCtx, ev: Event) -> Option<Phase> {
    let mut guard = ctx.phase.lock().unwrap();
    let next = transition(*guard, ev)?;
    *guard = next;
    Some(next)
}

/// Same as `apply`, but for events raised by async work belonging to a
/// specific take. If the take has since been cancelled/superseded (its
/// generation no longer matches), the event is dropped instead of applied.
fn apply_for(ctx: &AppCtx, gen: u64, ev: Event) -> Option<Phase> {
    if ctx.take_gen.load(Ordering::SeqCst) != gen {
        return None;
    }
    apply(ctx, ev)
}

/// Atomically: check the take is current, apply the transition, write the
/// phase, then emit. Returns false when the event was dropped (stale take
/// or illegal transition).
fn advance(ctx: &AppCtx, gen: u64, ev: Event, message: Option<&str>) -> bool {
    let next = {
        let mut guard = ctx.phase.lock().unwrap();
        if ctx.take_gen.load(Ordering::SeqCst) != gen {
            return false;
        }
        let Some(next) = transition(*guard, ev) else { return false };
        *guard = next;
        next
    };
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": next, "message": message }),
    );
    true
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
                // take_gen is atomic and re-checked under the phase lock by
                // advance() before every take-scoped transition, so a
                // concurrent cross-thread cancel (cancel_dictation runs on
                // its own thread) can't corrupt this read — at worst the
                // gen is stale and finish() sees recorder.stop() return
                // None, which self-heals via the Cancel event.
                let gen = ctx.take_gen.load(Ordering::SeqCst);
                let ctx2 = ctx.clone();
                tauri::async_runtime::spawn(async move { finish(ctx2, gen).await });
            }
        }
        HotkeySignal::Cancel => cancel(&ctx),
    }
}

fn start_recording(ctx: &Arc<AppCtx>) {
    if apply(ctx, Event::StartRequested).is_none() {
        return;
    }
    // New take: any in-flight async work from a previous one is now stale.
    let gen = ctx.take_gen.fetch_add(1, Ordering::SeqCst) + 1;
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
            spawn_partial_loop(ctx.clone(), gen);
        }
        Err(e) => {
            // Synchronous, hotkey-driven — no stale-task risk here.
            apply(ctx, Event::Failed);
            set_phase(ctx, Phase::Error, Some(&format!("mikrofón: {e}")));
            show_bubble(ctx);
        }
    }
}

pub fn cancel(ctx: &Arc<AppCtx>) {
    let _ = ctx.recorder.stop();
    // Invalidate any async work in flight for the take being cancelled.
    ctx.take_gen.fetch_add(1, Ordering::SeqCst);
    if apply(ctx, Event::Cancel).is_some() {
        set_phase(ctx, Phase::Idle, None);
        hide_bubble_after(ctx, 0);
    }
}

async fn finish(ctx: Arc<AppCtx>, gen: u64) {
    let Some((samples, rate, ch)) = ctx.recorder.stop() else {
        // Recorder was already stopped (e.g. a concurrent cancel) — only
        // move to Idle if we're still the take that owns the phase.
        advance(&ctx, gen, Event::Cancel, None);
        return;
    };
    // < 0.4 s of audio → treat as silence.
    if samples.len() < (rate as usize * ch as usize) * 2 / 5 {
        if advance(&ctx, gen, Event::Cancel, Some("nič som nepočul")) {
            hide_bubble_after(&ctx, 1200);
        }
        return;
    }
    let wav = audio::prepare_wav(&samples, rate, ch);
    transcribe_and_deliver(ctx, wav, gen).await;
}

pub async fn transcribe_and_deliver(ctx: Arc<AppCtx>, wav: Vec<u8>, gen: u64) {
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
        // Only keep the audio if we're still the take in charge — a stale
        // failure landing late must not clobber a newer take's saved audio.
        if ctx.take_gen.load(Ordering::SeqCst) == gen {
            *ctx.pending_wav.lock().unwrap() = Some(wav);
        }
        advance(&ctx, gen, Event::Failed, Some("chýba Groq API kľúč"));
        return;
    };

    // 1. STT
    let stt = SttClient::new(groq_url, api_key);
    let transcript = match stt.transcribe(wav.clone(), lang).await {
        Ok(t) => t,
        Err(e) => {
            if ctx.take_gen.load(Ordering::SeqCst) == gen {
                *ctx.pending_wav.lock().unwrap() = Some(wav);
            }
            advance(&ctx, gen, Event::Failed, Some(&format!("prepis zlyhal: {e}")));
            return;
        }
    };
    if transcript.text.is_empty() {
        if advance(&ctx, gen, Event::Cancel, Some("nič som nepočul")) {
            hide_bubble_after(&ctx, 1200);
        }
        return;
    }

    // 2. Cleanup (best-effort — spec: never lose text)
    let mut note: Option<&str> = None;
    let final_text = if cleanup_enabled {
        if !advance(&ctx, gen, Event::TranscriptReady, Some("✨ upravujem text…")) {
            return; // cancelled meanwhile, or a stale take
        }
        match CleanupClient::new(meridian_url, model).clean(&transcript.text).await {
            Ok(cleaned) => cleaned,
            Err(_) => {
                note = Some("vložené bez úprav");
                transcript.text.clone()
            }
        }
    } else {
        if apply_for(&ctx, gen, Event::TranscriptReady).is_none() {
            return; // cancelled meanwhile, or a stale take
        }
        transcript.text.clone()
    };
    if !advance(&ctx, gen, Event::CleanupDone, None) {
        return; // cancelled meanwhile, or a stale take
    }

    // 3. Inject
    let text_for_inject = final_text.clone();
    let inject_result =
        tauri::async_runtime::spawn_blocking(move || crate::inject::inject_text(&text_for_inject))
            .await
            .unwrap_or_else(|e| Err(crate::inject::InjectError::Keystroke(e.to_string())));
    match inject_result {
        Ok(()) => {
            if advance(&ctx, gen, Event::Injected, Some(note.unwrap_or("✓ vložené"))) {
                hide_bubble_after(&ctx, 1200);
            }
        }
        Err(e) => {
            // Never lose text: leave it in the clipboard at minimum,
            // regardless of whether this take is still the active one.
            let _ = crate::inject::copy_only(&final_text);
            advance(
                &ctx,
                gen,
                Event::Failed,
                Some(&format!("vloženie zlyhalo — text je v schránke (Cmd+V). {e}")),
            );
        }
    }
}

const PARTIAL_INTERVAL_MS: u64 = 2500;
/// Don't bother transcribing less than 1 s of audio.
const PARTIAL_MIN_SECS: f32 = 1.0;

fn spawn_partial_loop(ctx: Arc<AppCtx>, gen: u64) {
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(PARTIAL_INTERVAL_MS));
        interval.tick().await; // first tick fires immediately; skip it
        loop {
            interval.tick().await;
            // Phase check alone isn't enough: a new take could already be
            // Recording again by the time a stale loop's tick fires.
            if *ctx.phase.lock().unwrap() != Phase::Recording
                || ctx.take_gen.load(Ordering::SeqCst) != gen
            {
                return; // recording ended, or this loop belongs to a stale take
            }
            if ctx.recorder.duration_secs() < PARTIAL_MIN_SECS {
                continue;
            }
            // One partial request at a time; skip a beat when Groq is slow.
            if ctx
                .partial_inflight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }
            let Some((samples, rate, ch)) = ctx.recorder.snapshot() else {
                ctx.partial_inflight.store(false, Ordering::SeqCst);
                continue;
            };
            let (groq_url, lang) = {
                let s = ctx.settings.read().unwrap();
                (s.groq_url.clone(), s.language.code())
            };
            let Some(api_key) = settings::groq_api_key() else {
                ctx.partial_inflight.store(false, Ordering::SeqCst);
                continue;
            };
            let ctx2 = ctx.clone();
            tauri::async_runtime::spawn(async move {
                let wav = audio::prepare_wav(&samples, rate, ch);
                let stt = SttClient::new(groq_url, api_key);
                if let Ok(t) = stt.transcribe(wav, lang).await {
                    // Only show it if we're still recording the same take.
                    let still_current = *ctx2.phase.lock().unwrap() == Phase::Recording
                        && ctx2.take_gen.load(Ordering::SeqCst) == gen;
                    if still_current && !t.text.is_empty() {
                        let _ = ctx2
                            .app
                            .emit("dictation:partial", serde_json::json!({ "text": t.text }));
                    }
                }
                ctx2.partial_inflight.store(false, Ordering::SeqCst);
            });
        }
    });
}
