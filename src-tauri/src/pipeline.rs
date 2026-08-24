use crate::audio::{self, Recorder};
use crate::cleanup::CleanupClient;
use crate::hotkey::HotkeySignal;
use crate::ratelimit::{Limiter, Priority};
use crate::recordings::RecordingStore;
use crate::settings::{self, LanguageMode, Settings};
use crate::state::{transition, Event, Phase};
use crate::stt::{RetryPolicy, SttClient};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, Wry};

/// A take whose audio is already on disk and whose history row is claimed.
/// Everything downstream only updates that row, so no failure past this point
/// can lose the recording.
#[derive(Debug, Clone)]
pub struct TakeRecord {
    pub row_id: i64,
    pub audio_name: Option<String>,
}

/// How long after the user stops speaking an automatic retry may still paste
/// at the cursor. Past this they've almost certainly moved on, and the text
/// would land in the wrong window — history plus clipboard instead.
pub const INJECT_GRACE: Duration = Duration::from_secs(15);

/// The tray's four language `CheckMenuItem`s, keyed by the mode each one
/// represents — `apply_settings` uses this to keep the tray's checkmarks in
/// sync whenever settings change from any source (Settings page, tray itself).
pub type TrayLangItems = Vec<(LanguageMode, CheckMenuItem<Wry>)>;

pub struct AppCtx {
    pub phase: Mutex<Phase>,
    pub recorder: Recorder,
    pub settings: RwLock<Settings>,
    /// The last take that failed, kept so the bubble's "skúsiť znova" knows
    /// which history row and which WAV on disk to re-run.
    pub pending_take: Mutex<Option<TakeRecord>>,
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
    /// WAV files behind the history rows.
    pub recordings: RecordingStore,
    /// Shared throttle in front of Groq — keeps the live preview from spending
    /// the quota the final transcription needs.
    pub limiter: Limiter,
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

/// Stashes `take` as the one the retry button targets, iff `gen` is still the
/// current take — checked and written atomically under `ctx.phase` (the gen
/// authority) so a stale failure landing late can't clobber a newer take's.
fn store_pending_if_current(ctx: &AppCtx, gen: u64, take: TakeRecord) -> bool {
    let _phase_guard = ctx.phase.lock().unwrap();
    if ctx.take_gen.load(Ordering::SeqCst) != gen {
        return false;
    }
    *ctx.pending_take.lock().unwrap() = Some(take);
    true
}

/// Re-emits the current phase with a new message, without moving the FSM —
/// used to narrate a rate-limit wait while still Transcribing. Gen-guarded so
/// a stale take can't overwrite what the user is looking at.
fn emit_message(ctx: &AppCtx, gen: u64, message: &str) {
    let phase = {
        let guard = ctx.phase.lock().unwrap();
        if ctx.take_gen.load(Ordering::SeqCst) != gen {
            return;
        }
        *guard
    };
    let _ = ctx.app.emit(
        "dictation:state",
        serde_json::json!({ "phase": phase, "message": message }),
    );
}

pub(crate) fn emit_history_changed(ctx: &AppCtx) {
    let _ = ctx.app.emit("history:changed", serde_json::json!({}));
}

/// Writes the take's audio to disk and claims a history row for it, before a
/// single byte goes to Groq. Returns None only if both the file write and the
/// row insert failed, in which case dictation still proceeds — just without
/// the safety net.
fn persist_take(ctx: &AppCtx, gen: u64, wav: &[u8], duration_ms: i64) -> Option<TakeRecord> {
    let audio_name = match ctx.recordings.save(wav, gen) {
        Ok(name) => Some(name),
        Err(e) => {
            eprintln!("could not save recording audio: {e}");
            None
        }
    };
    match ctx.history.insert_pending(audio_name.as_deref(), duration_ms) {
        Ok(row_id) => {
            emit_history_changed(ctx);
            Some(TakeRecord { row_id, audio_name })
        }
        Err(e) => {
            eprintln!("could not claim history row: {e}");
            if let Some(name) = &audio_name {
                ctx.recordings.remove(name);
            }
            None
        }
    }
}

fn show_bubble(ctx: &AppCtx) {
    if let Some(w) = ctx.app.get_webview_window("bubble") {
        let saved = ctx.settings.read().unwrap().bubble_pos;
        crate::position_bubble(&w, saved);
        let _ = w.show();
    }
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
    // The previous take's audio and row live on disk now, so dropping the
    // retry target here costs nothing — history still has it.
    ctx.pending_take.lock().unwrap().take();
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
    if begin(ctx, Event::Cancel, true, None).is_some() {
        let _ = ctx.recorder.stop();
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
        advance(&ctx, gen, Event::Cancel, Some("nič som nepočul"));
        return;
    }
    let stopped_at = Instant::now();
    let duration_ms = (samples.len() as i64 * 1000) / (rate as i64 * ch.max(1) as i64);
    let wav = audio::prepare_wav(&samples, rate, ch);
    // Audio and history row first, network second — this is the whole point.
    let take = persist_take(&ctx, gen, &wav, duration_ms);
    transcribe_and_deliver(ctx, wav, gen, take, Some(stopped_at + INJECT_GRACE)).await;
}

/// Transcribes, cleans, and delivers a take.
///
/// `take` is the already-claimed history row; every outcome updates it, and
/// the DB writes are deliberately *not* gen-guarded — if the user cancels or
/// starts a new take mid-flight, the transcript still lands in history rather
/// than evaporating. Only the FSM and the paste are gen-guarded.
///
/// `inject_deadline` is when pasting at the cursor stops being safe. `None`
/// means always paste (a retry the user asked for explicitly).
pub async fn transcribe_and_deliver(
    ctx: Arc<AppCtx>,
    wav: Vec<u8>,
    gen: u64,
    take: Option<TakeRecord>,
    inject_deadline: Option<Instant>,
) {
    let (groq_url, lang, cleanup_enabled) = {
        let s = ctx.settings.read().unwrap();
        (s.groq_url.clone(), s.language.code(), s.cleanup_enabled)
    };
    let fail = |message: String| {
        if let Some(t) = &take {
            let _ = ctx.history.mark_failed(t.row_id, &message);
            emit_history_changed(&ctx);
            store_pending_if_current(&ctx, gen, t.clone());
        }
        advance(&ctx, gen, Event::Failed, Some(&message));
    };

    let Some(api_key) = settings::groq_api_key(&ctx.settings.read().unwrap()) else {
        fail("chýba Groq API kľúč".into());
        return;
    };

    // 1. STT — retries rate limits and server errors instead of giving up.
    ctx.limiter.try_acquire(Priority::Final);
    let stt = SttClient::new(groq_url, api_key);
    let ctx_for_retry = ctx.clone();
    let transcript = match stt
        .transcribe_with(wav, lang, RetryPolicy::default(), move |attempt, delay| {
            emit_message(
                &ctx_for_retry,
                gen,
                &format!(
                    "limit Groq — skúšam znova o {} s ({attempt}/4)",
                    delay.as_secs().max(1)
                ),
            );
        })
        .await
    {
        Ok(t) => t,
        Err(e) => {
            if e.is_rate_limit() {
                ctx.limiter.note_rate_limited(e.retry_after());
            }
            fail(format!("prepis zlyhal: {e}"));
            return;
        }
    };
    if transcript.text.is_empty() {
        if let Some(t) = &take {
            let _ = ctx.history.mark_failed(t.row_id, "prázdny prepis — ticho?");
            emit_history_changed(&ctx);
        }
        advance(&ctx, gen, Event::Cancel, Some("nič som nepočul"));
        return;
    }

    // 2. Cleanup (best-effort — never lose text)
    let mut note: Option<&str> = None;
    let cleanup_client =
        cleanup_enabled.then(|| CleanupClient::for_settings(&ctx.settings.read().unwrap()));
    let final_text = match cleanup_client {
        Some(client) => {
            if !advance(&ctx, gen, Event::TranscriptReady, Some("✨ upravujem text…")) {
                commit(&ctx, &take, &transcript, &transcript.text);
                return; // cancelled meanwhile, or a stale take
            }
            match client.clean(&transcript.text).await {
                Ok(cleaned) => cleaned,
                Err(_) => {
                    note = Some("vložené bez úprav");
                    transcript.text.clone()
                }
            }
        }
        None => {
            if apply_for(&ctx, gen, Event::TranscriptReady).is_none() {
                commit(&ctx, &take, &transcript, &transcript.text);
                return; // cancelled meanwhile, or a stale take
            }
            transcript.text.clone()
        }
    };
    // Commit before delivering: from here the text is safe in history no
    // matter what the paste does.
    commit(&ctx, &take, &transcript, &final_text);

    if !advance(&ctx, gen, Event::CleanupDone, None) {
        return; // cancelled meanwhile, or a stale take
    }

    // 3. Deliver
    if inject_deadline.is_some_and(|deadline| Instant::now() > deadline) {
        // The retry outlived the user's attention; pasting now would fire into
        // whatever they switched to.
        let _ = crate::inject::copy_only(&final_text);
        advance(
            &ctx,
            gen,
            Event::Failed,
            Some("hotové neskoro — text je v schránke a v histórii"),
        );
        return;
    }
    let text_for_inject = final_text.clone();
    let inject_result =
        tauri::async_runtime::spawn_blocking(move || crate::inject::inject_text(&text_for_inject))
            .await
            .unwrap_or_else(|e| Err(crate::inject::InjectError::Keystroke(e.to_string())));
    match inject_result {
        Ok(()) => {
            advance(
                &ctx,
                gen,
                Event::Injected,
                Some(note.unwrap_or("✓ vložené — text je aj v schránke")),
            );
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

/// Writes the finished transcript to the take's history row. Falls back to a
/// fresh row when the take was never claimed (disk or DB trouble at capture
/// time) so the text still survives.
fn commit(
    ctx: &AppCtx,
    take: &Option<TakeRecord>,
    transcript: &crate::stt::Transcript,
    final_text: &str,
) {
    let language = transcript.language.as_deref();
    match take {
        Some(t) => {
            let _ = ctx
                .history
                .mark_done(t.row_id, &transcript.text, final_text, language);
            // Only clear the retry target if it's still this take's — a stale
            // take finishing late must not disarm a newer take's retry button.
            let mut pending = ctx.pending_take.lock().unwrap();
            if pending.as_ref().is_some_and(|p| p.row_id == t.row_id) {
                pending.take();
            }
        }
        None => {
            let _ = ctx.history.insert(&transcript.text, final_text, language, 0);
        }
    }
    emit_history_changed(ctx);
}

/// Every preview is a full Groq request against the same per-minute quota as
/// the transcription the user is waiting for. At 2.5 s this alone could push a
/// single long take past the free tier's limit; 8 s keeps the preview useful
/// while leaving the quota to the work that matters.
const PARTIAL_INTERVAL_MS: u64 = 8000;
/// Don't bother transcribing less than 1 s of audio.
const PARTIAL_MIN_SECS: f32 = 1.0;
/// Locked-mode takes auto-stop at this length so a forgotten hotkey doesn't
/// record (and eventually upload) indefinitely.
const MAX_TAKE_SECS: f32 = 300.0;
/// Partial uploads only cover the tail of a long take — bounds upload cost
/// as the take grows; the final pass still transcribes the full audio.
const PARTIAL_WINDOW_SECS: f32 = 15.0;

fn spawn_partial_loop(ctx: Arc<AppCtx>, gen: u64) {
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(PARTIAL_INTERVAL_MS));
        interval.tick().await; // first tick fires immediately; skip it
        let mut auto_stop_fired = false;
        // Once Groq has pushed back during this take, the preview stays off
        // for good — the limiter's cooldown expiring must not restart the very
        // traffic that caused the 429 while the user is still speaking.
        let partials_off = Arc::new(AtomicBool::new(false));
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
            if elapsed < PARTIAL_MIN_SECS || partials_off.load(Ordering::SeqCst) {
                continue;
            }
            // Expendable by design: if the quota is running low, the preview
            // is what gives way, never the final transcription.
            if !ctx.limiter.try_acquire(Priority::Partial) {
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
            let Some(api_key) = settings::groq_api_key(&ctx.settings.read().unwrap()) else {
                ctx.partial_inflight.store(false, Ordering::SeqCst);
                continue;
            };
            let ctx2 = ctx.clone();
            let partials_off = partials_off.clone();
            tauri::async_runtime::spawn(async move {
                let wav = audio::prepare_wav(&windowed, rate, ch);
                let stt = SttClient::new(groq_url, api_key);
                // No retries here: a preview that missed its slot is worthless
                // by the time a backoff would have finished.
                match stt
                    .transcribe_with(wav, lang, RetryPolicy::none(), |_, _| {})
                    .await
                {
                    Ok(t) => {
                        // Only show it if we're still recording the same take.
                        let still_current = *ctx2.phase.lock().unwrap() == Phase::Recording
                            && ctx2.take_gen.load(Ordering::SeqCst) == gen;
                        if still_current && !t.text.is_empty() {
                            let _ = ctx2
                                .app
                                .emit("dictation:partial", serde_json::json!({ "text": t.text }));
                        }
                    }
                    Err(e) if e.is_rate_limit() => {
                        ctx2.limiter.note_rate_limited(e.retry_after());
                        partials_off.store(true, Ordering::SeqCst);
                    }
                    Err(_) => {}
                }
                ctx2.partial_inflight.store(false, Ordering::SeqCst);
            });
        }
    });
}
