//! Runs the real dictation pipeline end to end — WAV in, mock Groq, mock
//! Meridian, SQLite history, on-disk recording, clipboard out — on Tauri's
//! `MockRuntime`, so no window, hotkey or microphone is involved. Delivery
//! is always driven past its paste deadline on purpose: typing into whatever
//! app has focus is the one thing a test must never do, and the deadline
//! path copies to the clipboard instead.
use super::*;
use crate::audio::Recorder;
use crate::history::{HistoryStore, STATUS_DONE, STATUS_FAILED};
use crate::hotkey::Interpreter;
use crate::ratelimit::Limiter;
use crate::recordings::RecordingStore;
use crate::settings::{Settings, UiLanguage};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Listener;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Every delivery writes the machine-wide clipboard, so the scenarios must
/// not interleave. Each #[tokio::test] runs on its own runtime thread, so
/// holding a std mutex across the await is harmless here.
static CLIPBOARD: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    CLIPBOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Harness {
    _dir: tempfile::TempDir,
    _app: tauri::App<MockRuntime>,
    ctx: Arc<AppCtx<MockRuntime>>,
    /// Every `dictation:state` payload the pipeline emitted, in order.
    states: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl Harness {
    fn new(settings: Settings) -> Self {
        let app = mock_builder().build(mock_context(noop_assets())).expect("mock app");
        let dir = tempfile::tempdir().unwrap();
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = states.clone();
        app.listen("dictation:state", move |e| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) {
                sink.lock().unwrap().push(v);
            }
        });
        let ctx = Arc::new(AppCtx {
            phase: Mutex::new(Phase::Idle),
            recorder: Recorder::new(),
            settings: RwLock::new(settings),
            pending_take: Mutex::new(None),
            partial_inflight: AtomicBool::new(false),
            take_gen: AtomicU64::new(0),
            app: app.handle().clone(),
            hotkey_name: Arc::new(RwLock::new("AltGr".into())),
            settings_path: dir.path().join("settings.json"),
            history: HistoryStore::open(&dir.path().join("history.sqlite")).unwrap(),
            recordings: RecordingStore::new(dir.path().join("audio")),
            limiter: Limiter::default(),
            capture_next: Arc::new(AtomicBool::new(false)),
            tray_lang_items: Mutex::new(None),
            tray_labels: Mutex::new(None),
            ui_lang: Arc::new(RwLock::new(UiLanguage::En)),
            hotkey_interp: Arc::new(Mutex::new(Interpreter::new())),
        });
        Self { _dir: dir, _app: app, ctx, states }
    }

    /// Hotkey down, hotkey up, audio captured: the state the pipeline is in
    /// when `transcribe_and_deliver` normally runs. Returns the take gen and
    /// the persisted take record.
    fn stop_with_audio(&self, wav: &[u8]) -> (u64, Option<TakeRecord>) {
        let gen = begin(&self.ctx, Event::StartRequested, true, None).expect("start");
        assert_eq!(begin(&self.ctx, Event::StopRequested, false, None), Some(gen));
        let take = persist_take(&self.ctx, gen, wav, 1000);
        (gen, take)
    }

    /// Runs delivery with the paste window already closed, so the text goes
    /// to the clipboard + history instead of being typed anywhere.
    async fn deliver_late(&self, wav: Vec<u8>, gen: u64, take: Option<TakeRecord>) {
        let expired = Instant::now() - Duration::from_secs(1);
        transcribe_and_deliver(self.ctx.clone(), wav, gen, take, Some(expired)).await;
    }

    fn phases(&self) -> Vec<String> {
        self.states
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["phase"].as_str().unwrap_or("?").to_string())
            .collect()
    }

    fn last_state(&self) -> serde_json::Value {
        self.states.lock().unwrap().last().cloned().expect("at least one state event")
    }

    fn messages(&self) -> Vec<String> {
        self.states
            .lock()
            .unwrap()
            .iter()
            .filter_map(|v| v["message"].as_str().map(str::to_string))
            .collect()
    }

    fn only_row(&self) -> crate::history::Dictation {
        let rows = self.ctx.history.list(None, 10).unwrap();
        assert_eq!(rows.len(), 1, "expected exactly one history row, got {rows:?}");
        rows.into_iter().next().unwrap()
    }
}

fn settings_for(groq: &MockServer, meridian: &MockServer, cleanup_enabled: bool) -> Settings {
    Settings {
        groq_url: groq.uri(),
        meridian_url: meridian.uri(),
        cleanup_enabled,
        groq_api_key: "gsk_test".into(),
        ..Settings::default()
    }
}

/// One second of a 440 Hz tone, 16 kHz mono — enough to clear the silence gate.
fn tone_wav() -> Vec<u8> {
    let samples: Vec<f32> = (0..16_000)
        .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16_000.0).sin() * 0.3)
        .collect();
    audio::prepare_wav(&samples, 16_000, 1)
}

async fn groq_ok(server: &MockServer, text: &str) {
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .and(header("authorization", "Bearer gsk_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": text,
            "language": "en"
        })))
        .mount(server)
        .await;
}

async fn meridian_ok(server: &MockServer, cleaned: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{ "type": "text", "text": cleaned }]
        })))
        .mount(server)
        .await;
}

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn dictation_lands_in_history_and_clipboard_with_cleanup() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    groq_ok(&groq, "hello world this is dikto").await;
    meridian_ok(&meridian, "Hello world, this is Dikto.").await;
    let h = Harness::new(settings_for(&groq, &meridian, true));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    let take_name = take.as_ref().and_then(|t| t.audio_name.clone()).expect("audio persisted");
    assert_eq!(h.only_row().status, "pending", "row claimed before any network call");

    h.deliver_late(wav, gen, take).await;

    assert_eq!(h.phases(), ["recording", "transcribing", "cleaning", "injecting", "error"]);
    assert_eq!(
        h.last_state()["message"],
        "finished late — text is in the clipboard and in History"
    );
    let row = h.only_row();
    assert_eq!(row.status, STATUS_DONE);
    assert_eq!(row.raw, "hello world this is dikto");
    assert_eq!(row.clean, "Hello world, this is Dikto.");
    assert_eq!(row.language.as_deref(), Some("en"));
    assert_eq!(row.audio_path.as_deref(), Some(take_name.as_str()));
    assert!(h.ctx.recordings.read(&take_name).is_ok(), "recording kept for retention window");
    assert!(h.ctx.pending_take.lock().unwrap().is_none(), "nothing left to retry");
    assert_eq!(*h.ctx.phase.lock().unwrap(), Phase::Error);
    if let Some(text) = clipboard_text() {
        assert_eq!(text, "Hello world, this is Dikto.");
    } else {
        eprintln!("no clipboard in this environment — skipping clipboard assertion");
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn meridian_failure_falls_back_to_the_raw_transcript() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    groq_ok(&groq, "raw words").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&meridian)
        .await;
    let h = Harness::new(settings_for(&groq, &meridian, true));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    h.deliver_late(wav, gen, take).await;

    let row = h.only_row();
    assert_eq!(row.status, STATUS_DONE);
    assert_eq!(row.clean, "raw words", "cleanup must never lose the text");
    assert_eq!(h.phases(), ["recording", "transcribing", "cleaning", "injecting", "error"]);
    if let Some(text) = clipboard_text() {
        assert_eq!(text, "raw words");
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn cleanup_disabled_skips_meridian_entirely() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    groq_ok(&groq, "verbatim").await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&meridian).await;
    let h = Harness::new(settings_for(&groq, &meridian, false));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    h.deliver_late(wav, gen, take).await;

    assert_eq!(h.only_row().clean, "verbatim");
    assert_eq!(h.phases(), ["recording", "transcribing", "injecting", "error"]);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn groq_rate_limit_is_retried_and_narrated() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "1")
                .set_body_string("rate limit reached"),
        )
        .up_to_n_times(1)
        .mount(&groq)
        .await;
    groq_ok(&groq, "second time lucky").await;
    let h = Harness::new(settings_for(&groq, &meridian, false));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    h.deliver_late(wav, gen, take).await;

    assert!(
        h.messages().iter().any(|m| m == "Groq rate limit — retrying in 1 s (2/4)"),
        "bubble should narrate the wait before attempt 2, got {:?}",
        h.messages()
    );
    assert_eq!(h.only_row().status, STATUS_DONE);
    assert_eq!(h.only_row().raw, "second time lucky");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn groq_rejection_keeps_the_audio_and_offers_a_retry() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid api key"))
        .mount(&groq)
        .await;
    let h = Harness::new(settings_for(&groq, &meridian, false));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    let row_id = take.as_ref().unwrap().row_id;
    h.deliver_late(wav, gen, take).await;

    let row = h.only_row();
    assert_eq!(row.status, STATUS_FAILED);
    let err = row.error.clone().unwrap_or_default();
    assert!(err.starts_with("transcription failed") && err.contains("401"), "got {err}");
    assert!(row.audio_path.is_some(), "audio kept so the take can be retried");
    let last = h.last_state();
    assert_eq!(last["phase"], "error");
    assert_eq!(last["retryable"], true);
    assert_eq!(h.ctx.pending_take.lock().unwrap().as_ref().map(|t| t.row_id), Some(row_id));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn silence_is_reported_and_not_pasted() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    groq_ok(&groq, "   ").await;
    let h = Harness::new(settings_for(&groq, &meridian, false));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    h.deliver_late(wav, gen, take).await;

    let row = h.only_row();
    assert_eq!(row.status, STATUS_FAILED);
    assert_eq!(row.error.as_deref(), Some("empty transcript — silence?"));
    let last = h.last_state();
    assert_eq!(last["phase"], "idle");
    assert_eq!(last["message"], "I didn't hear anything");
    assert_eq!(*h.ctx.phase.lock().unwrap(), Phase::Idle);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn messages_follow_the_slovak_ui_language() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
        .mount(&groq)
        .await;
    let mut settings = settings_for(&groq, &meridian, false);
    settings.ui_language = UiLanguage::Sk;
    let h = Harness::new(settings);

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    h.deliver_late(wav, gen, take).await;

    assert!(h.only_row().error.unwrap_or_default().starts_with("prepis zlyhal"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn a_cancelled_take_still_keeps_its_transcript_in_history() {
    let _serial = serial();
    let (groq, meridian) = (MockServer::start().await, MockServer::start().await);
    groq_ok(&groq, "kept anyway").await;
    let h = Harness::new(settings_for(&groq, &meridian, false));

    let wav = tone_wav();
    let (gen, take) = h.stop_with_audio(&wav);
    // The user hits Esc while Groq is still working: the FSM moves on, but
    // the row must still receive the text when the response lands.
    cancel(&h.ctx);
    h.deliver_late(wav, gen, take).await;

    let row = h.only_row();
    assert_eq!(row.status, STATUS_DONE);
    assert_eq!(row.raw, "kept anyway");
    assert_eq!(*h.ctx.phase.lock().unwrap(), Phase::Idle, "stale take must not move the FSM");
}
