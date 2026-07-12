use crate::audio::{self, Recorder};
use crate::cleanup::CleanupClient;
use crate::hotkey::HotkeySignal;
use crate::settings::{self, LanguageMode, Settings};
use crate::state::{transition, Event, Phase};
use crate::stt::SttClient;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, Wry};

/// The tray's four language `CheckMenuItem`s, keyed by the mode each one
/// represents — `apply_settings` uses this to keep the tray's checkmarks in
/// sync whenever settings change from any source (Settings page, tray itself).
pub type TrayLangItems = Vec<(LanguageMode, CheckMenuItem<Wry>)>;

pub struct AppCtx {
    pub phase: Mutex<Phase>,
    pub recorder: Recorder,
    pub settings: RwLock<Settings>,
    pub pending_wav: Mutex<Option<Vec<u8>>>,
    pub partial_inflight: AtomicBool,
    /// Bumped on every start_recording/cancel/retry; async tasks capture the
    /// value at spawn time and drop their FSM event if it no longer matches —
    /// otherwise a stale task from a superseded take could clobber a new one.
    pub take_gen: AtomicU64,
    pub app: AppHandle,
    /// Same Arc handed to hotkey::spawn — set_settings updates it live so a
    /// hotkey change hot-applies without restarting the listener thread.
    pub hotkey_name: Arc<RwLock<String>>,
    pub settings_path: PathBuf,
    pub history: crate::history::HistoryStore,
    /// Shared with hotkey::spawn's listener thread — set true to divert the
    /// next KeyPress into a `hotkey:captured` event instead of interpreting it.
    pub capture_next: Arc<AtomicBool>,
    /// Set once by `build_tray` after the menu is built; `None` until then.
    /// `apply_settings` uses it to refresh the tray's language checkmarks.
    pub tray_lang_items: Mutex<Option<TrayLangItems>>,
    /// The hotkey listener's tap/lock interpreter. The pipeline resets it
    /// whenever it moves to Idle for a reason the interpreter didn't decide
    /// (Esc cancel, 300s auto-stop) so the physical key state it tracks can't
    /// desync from the pipeline and eat the next real press.
    pub hotkey_interp: Arc<Mutex<crate::hotkey::Interpreter>>,
}

/// Applies an event raised by async work belonging to a specific take. If
/// the take has since been cancelled/superseded (its generation no longer
/// matches), the event is dropped instead of applied. Mirrors `advance`'s
/// locked check-then-write, minus the emit.
fn apply_for(ctx: &AppCtx, gen: u64, ev: Event) -> Option<Phase> {
    let mut guard = ctx.phase.lock().unwrap();
    if ctx.take_gen.load(Ordering::SeqCst) != gen {
        return None;
    }
    let next = transition(*guard, ev)?;
    *guard = next;
    Some(next)
}

/// Atomically: check the take is current, apply the transition, write the
/// phase, then emit. Returns false when the event was dropped (stale take
/// or illegal transition).
pub(crate) fn advance(ctx: &AppCtx, gen: u64, ev: Event, message: Option<&str>) -> bool {
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

/// Entry point for synchronous, hotkey/command-driven events (start, stop,
/// cancel, retry) — as opposed to take-scoped async events, which go through
/// `advance`. Under the phase lock: validate the transition; on success
/// optionally start a new take (bump take_gen), write the phase, then emit.
/// Returns the take gen when the transition applied; None means the event
/// was dropped with zero side effects (no gen bump, no phase write, no emit).
pub(crate) fn begin(ctx: &AppCtx, ev: Event, new_take: bool, message: Option<&str>) -> Option<u64> {
    let (next, gen) = {
        let mut guard = ctx.phase.lock().unwrap();
        let next = transition(*guard, ev)?;
        let gen = if new_take {
            ctx.take_gen.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            ctx.take_gen.load(Ordering::SeqCst)
        };
        *guard = next;
        (next, gen)
    };
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": next, "message": message }),
    );
    Some(gen)
}

/// Stashes `wav` in `ctx.pending_wav` iff `gen` is still the current take,
/// checked and written atomically under `ctx.phase` (the gen authority) so a
/// take that starts between a separate check-then-write can't have its
/// pending audio clobbered by a stale failure from the previous one. Returns
/// whether the write happened.
fn store_pending_if_current(ctx: &AppCtx, gen: u64, wav: Vec<u8>) -> bool {
    let _phase_guard = ctx.phase.lock().unwrap();
    if ctx.take_gen.load(Ordering::SeqCst) != gen {
        return false;
    }
    *ctx.pending_wav.lock().unwrap() = Some(wav);
    true
}

fn show_bubble(ctx: &AppCtx) {
    if let Some(w) = ctx.app.get_webview_window("bubble") {
        let saved = ctx.settings.read().unwrap().bubble_pos;
        crate::position_bubble(&w, saved);
        let _ = w.show();
    }
}

/// `gen` is the take that scheduled this hide. A newer take starting (and
/// possibly finishing) during `ms` would flip the phase back to Idle for a
/// different take — checking phase alone isn't enough, so take_gen must also
/// still match under the same phase-lock hold before we hide the bubble.
fn hide_bubble_after(ctx: &Arc<AppCtx>, gen: u64, ms: u64) {
    let ctx = ctx.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        let should_hide = {
            let guard = ctx.phase.lock().unwrap();
            *guard == Phase::Idle && ctx.take_gen.load(Ordering::SeqCst) == gen
        };
        if should_hide {
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
            if let Some(gen) = begin(&ctx, Event::StopRequested, false, None) {
                let ctx2 = ctx.clone();
                tauri::async_runtime::spawn(async move { finish(ctx2, gen).await });
            }
        }
        HotkeySignal::Cancel => cancel(&ctx),
    }
}

fn start_recording(ctx: &Arc<AppCtx>) {
    // New take: any in-flight async work from a previous one is now stale.
    let Some(gen) = begin(ctx, Event::StartRequested, true, None) else {
        return;
    };
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
            show_bubble(ctx);
            spawn_partial_loop(ctx.clone(), gen);
        }
        Err(e) => {
            advance(ctx, gen, Event::Failed, Some(&format!("mikrofón: {e}")));
            show_bubble(ctx);
        }
    }
}

pub fn cancel(ctx: &Arc<AppCtx>) {
    // Gen only bumps (and recorder only stops) on a legal transition —
    // otherwise (e.g. Esc during Injecting, where Cancel is illegal) we'd
    // invalidate the in-flight take's gen while its phase stays put, wedging it.
    if let Some(gen) = begin(ctx, Event::Cancel, true, None) {
        let _ = ctx.recorder.stop();
        hide_bubble_after(ctx, gen, 0);
        // This Cancel didn't come from the hotkey interpreter (Esc, or a
        // command-driven cancel) — resync it to Idle so a stale Locked/
        // TapArmed mode doesn't eat the next real key press.
        ctx.hotkey_interp.lock().unwrap().reset();
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
            hide_bubble_after(&ctx, gen, 1200);
        }
        return;
    }
    let duration_ms = (samples.len() as i64 * 1000) / (rate as i64 * ch.max(1) as i64);
    let wav = audio::prepare_wav(&samples, rate, ch);
    transcribe_and_deliver(ctx, wav, gen, duration_ms).await;
}

pub async fn transcribe_and_deliver(ctx: Arc<AppCtx>, wav: Vec<u8>, gen: u64, duration_ms: i64) {
    let (groq_url, lang, cleanup_enabled, meridian_url, model, cleanup_style) = {
        let s = ctx.settings.read().unwrap();
        (
            s.groq_url.clone(),
            s.language.code(),
            s.cleanup_enabled,
            s.meridian_url.clone(),
            s.cleanup_model.clone(),
            s.cleanup_style,
        )
    };
    let Some(api_key) = settings::groq_api_key() else {
        // Only keep the audio if we're still the take in charge — a stale
        // failure landing late must not clobber a newer take's saved audio.
        store_pending_if_current(&ctx, gen, wav);
        advance(&ctx, gen, Event::Failed, Some("chýba Groq API kľúč"));
        return;
    };

    // 1. STT
    let stt = SttClient::new(groq_url, api_key);
    let transcript = match stt.transcribe(wav.clone(), lang).await {
        Ok(t) => t,
        Err(e) => {
            store_pending_if_current(&ctx, gen, wav);
            advance(&ctx, gen, Event::Failed, Some(&format!("prepis zlyhal: {e}")));
            return;
        }
    };
    if transcript.text.is_empty() {
        if advance(&ctx, gen, Event::Cancel, Some("nič som nepočul")) {
            hide_bubble_after(&ctx, gen, 1200);
        }
        return;
    }

    // 2. Cleanup (best-effort — spec: never lose text)
    let mut note: Option<&str> = None;
    let final_text = if cleanup_enabled {
        if !advance(&ctx, gen, Event::TranscriptReady, Some("✨ upravujem text…")) {
            return; // cancelled meanwhile, or a stale take
        }
        match CleanupClient::with_style(meridian_url, model, cleanup_style)
            .clean(&transcript.text)
            .await
        {
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
            let _ = ctx.history.insert(
                &transcript.text,
                &final_text,
                transcript.language.as_deref(),
                duration_ms,
            );
            if advance(&ctx, gen, Event::Injected, Some(note.unwrap_or("✓ vložené"))) {
                hide_bubble_after(&ctx, gen, 1200);
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
                Some(&format!("vloženie zlyhalo — text je v schránke (Cmd/Ctrl+V). {e}")),
            );
        }
    }
}

const PARTIAL_INTERVAL_MS: u64 = 2500;
/// Don't bother transcribing less than 1 s of audio.
const PARTIAL_MIN_SECS: f32 = 1.0;
/// Locked-mode takes auto-stop at this length so a forgotten hotkey doesn't
/// record (and eventually upload) indefinitely.
const MAX_TAKE_SECS: f32 = 300.0;
/// Partial uploads only cover the tail of a long take — bounds upload cost
/// as the take grows; the final pass still transcribes the full audio.
const PARTIAL_WINDOW_SECS: f32 = 25.0;

fn spawn_partial_loop(ctx: Arc<AppCtx>, gen: u64) {
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(PARTIAL_INTERVAL_MS));
        interval.tick().await; // first tick fires immediately; skip it
        let mut auto_stop_fired = false;
        loop {
            interval.tick().await;
            // Phase check alone isn't enough: a new take could already be
            // Recording again by the time a stale loop's tick fires.
            if *ctx.phase.lock().unwrap() != Phase::Recording
                || ctx.take_gen.load(Ordering::SeqCst) != gen
            {
                return; // recording ended, or this loop belongs to a stale take
            }
            let elapsed = ctx.recorder.duration_secs();
            if !auto_stop_fired && elapsed > MAX_TAKE_SECS {
                auto_stop_fired = true;
                // The synthetic Stop below doesn't come from the interpreter,
                // but in Locked mode no key is physically held — reset now so
                // a later real key-down starts a fresh take instead of being
                // read against a stale Locked/TapArmed mode.
                ctx.hotkey_interp.lock().unwrap().reset();
                let ctx2 = ctx.clone();
                // handle_signal is sync and does its own async spawn; run it
                // off the tokio executor. Guarded by auto_stop_fired so a
                // delayed phase transition can't fire this more than once.
                std::thread::spawn(move || handle_signal(ctx2, HotkeySignal::Stop));
                continue;
            }
            if elapsed < PARTIAL_MIN_SECS {
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
            // Only the last PARTIAL_WINDOW_SECS get uploaded for the partial
            // preview; the final transcription still uses the full take.
            let windowed = audio::tail(&samples, rate, ch, PARTIAL_WINDOW_SECS).to_vec();
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
                let wav = audio::prepare_wav(&windowed, rate, ch);
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
