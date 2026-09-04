mod audio;
mod cleanup;
mod commands;
mod history;
mod hotkey;
mod inject;
#[cfg(target_os = "macos")]
mod macos_tap;
mod pipeline;
mod ratelimit;
mod recordings;
mod selftest;
mod settings;
mod state;
mod stt;

use pipeline::AppCtx;
use settings::{LanguageMode, UiLanguage};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

/// Must match `identifier` in tauri.conf.json — `--selftest` resolves the
/// real app config dir itself (headless, no `AppHandle`) and needs to agree
/// with what `app.path().app_config_dir()` resolves to at runtime.
pub(crate) const APP_IDENTIFIER: &str = "com.peterzajac.dikto";

/// Runs the headless `--selftest <wav-path>` pipeline check (see
/// `selftest.rs`) and returns the process exit code.
pub fn run_selftest(wav_path: &str) -> i32 {
    selftest::run(wav_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Dev convenience: picks up GROQ_API_KEY from a repo-root .env
    // (dotenv walks up from CWD; silently a no-op in bundled builds).
    let _ = dotenvy::dotenv();
    let builder = tauri::Builder::default();
    // Must be registered first (per tauri-plugin-single-instance's docs) so
    // it can intercept a second launch before any other plugin's setup runs.
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }));
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    let builder = builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None,
    ));
    builder
        .invoke_handler(tauri::generate_handler![
            commands::cancel_dictation,
            commands::retry_transcription,
            commands::set_groq_key,
            commands::get_settings,
            commands::set_settings,
            commands::has_groq_key,
            commands::test_groq_key,
            commands::meridian_status,
            commands::meridian_models,
            commands::finish_wizard,
            commands::history_list,
            commands::history_delete,
            commands::history_clear,
            commands::history_retry,
            commands::history_audio_path,
            commands::history_export_audio,
            commands::test_cleanup,
            commands::permissions_status,
            commands::open_privacy_settings,
            commands::open_url,
            commands::hotkey_capture_start
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("app config dir");
            let settings_path = config_dir.join("settings.json");
            if !settings_path.exists() {
                // First run: write the defaults template so the file always
                // exists once the app has started.
                let _ = settings::save(&settings_path, &settings::Settings::default());
            }
            let s = settings::load(&settings_path);
            let hotkey_name = Arc::new(RwLock::new(s.hotkey.clone()));
            let bubble_pos = s.bubble_pos;

            let data_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&data_dir).expect("create app data dir");
            let history = history::HistoryStore::open_or_recover(&data_dir.join("history.sqlite"));
            let recordings = recordings::RecordingStore::new(data_dir.join("audio"));
            startup_maintenance(&history, &recordings, s.history_retention_days, s.ui_language);

            let capture_next = Arc::new(AtomicBool::new(false));

            let (tx, rx) = mpsc::channel::<hotkey::HotkeySignal>();
            let ui_lang = Arc::new(RwLock::new(s.ui_language));
            let dead_app = app.handle().clone();
            let captured_app = app.handle().clone();
            let ui_lang_for_dead = ui_lang.clone();
            let hotkey_interp = hotkey::spawn(
                hotkey_name.clone(),
                tx,
                capture_next.clone(),
                Box::new(move |death: hotkey::ListenerDeath| {
                    let message = listener_death_message(*ui_lang_for_dead.read().unwrap(), &death);
                    let _ = dead_app.emit("dictation:pipeline-dead", serde_json::json!({ "message": message }));
                    if let Some(win) = dead_app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }),
                Box::new(move |key: String| {
                    let _ = captured_app.emit("hotkey:captured", serde_json::json!({ "key": key }));
                }),
            );

            let ctx = Arc::new(AppCtx {
                phase: Mutex::new(state::Phase::Idle),
                recorder: audio::Recorder::new(),
                settings: RwLock::new(s),
                pending_take: Mutex::new(None),
                partial_inflight: AtomicBool::new(false),
                take_gen: AtomicU64::new(0),
                app: app.handle().clone(),
                hotkey_name: hotkey_name.clone(),
                settings_path,
                history,
                recordings,
                limiter: ratelimit::Limiter::default(),
                capture_next: capture_next.clone(),
                tray_lang_items: Mutex::new(None),
                tray_labels: Mutex::new(None),
                ui_lang,
                hotkey_interp,
            });
            app.manage(ctx.clone());
            spawn_retention_ticker(ctx.clone());

            build_tray(app.handle(), &ctx)?;

            if let Some(main) = app.get_webview_window("main") {
                let main_for_close = main.clone();
                main.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        // The app lives on in the tray — closing the window
                        // just hides it instead of quitting the process.
                        api.prevent_close();
                        let _ = main_for_close.hide();
                    }
                });
            }

            if let Some(bubble) = app.get_webview_window("bubble") {
                position_bubble(&bubble, bubble_pos);
                // Always visible: idle renders as a mini-dot (click-through),
                // so the user sees at a glance that Dikto is alive.
                let _ = bubble.show();
                #[cfg(target_os = "macos")]
                {
                    use tauri_nspanel::WebviewWindowExt;
                    // NSWindowStyleMaskNonactivatingPanel = 1 << 7,
                    // NSStatusWindowLevel = 25,
                    // collection: canJoinAllSpaces (1<<0) | fullScreenAuxiliary (1<<8)
                    if let Ok(panel) = bubble.to_panel() {
                        panel.set_style_mask(1 << 7);
                        panel.set_level(25);
                        // tauri-nspanel re-exports the deprecated `cocoa` crate; no
                        // objc2-app-kit equivalent is wired through its API yet.
                        #[allow(deprecated)]
                        panel.set_collection_behaviour(
                            tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior::from_bits_retain(
                                (1 << 0) | (1 << 8),
                            ),
                        );
                    }
                }

                // Persist the bubble's position whenever the user drags it,
                // debounced so a drag doesn't spam disk writes: each Moved
                // event bumps a generation counter and schedules a save that
                // only actually writes if no later move has superseded it.
                let move_gen = Arc::new(AtomicU64::new(0));
                let ctx_for_move = ctx.clone();
                bubble.on_window_event(move |event| {
                    if let WindowEvent::Moved(pos) = event {
                        let gen = move_gen.fetch_add(1, Ordering::SeqCst) + 1;
                        let move_gen = move_gen.clone();
                        let ctx = ctx_for_move.clone();
                        let pos = (pos.x, pos.y);
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            if move_gen.load(Ordering::SeqCst) != gen {
                                return; // superseded by a later move
                            }
                            let mut new = ctx.settings.read().unwrap().clone();
                            new.bubble_pos = Some(pos);
                            // Routed through apply_settings (not a direct write+save) so
                            // `settings:changed` fires — otherwise Settings.tsx's stale
                            // in-memory copy round-trips through set_settings on any
                            // unrelated change and silently reverts this.
                            let _ = commands::apply_settings(&ctx, new);
                        });
                    }
                });
            }

            std::thread::spawn(move || {
                while let Ok(sig) = rx.recv() {
                    pipeline::handle_signal(ctx.clone(), sig);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Startup housekeeping for the recordings store, in the order that keeps the
/// DB and the disk agreeing: rescue takes the last run died on, delete
/// dictations past their retention, then delete WAVs nothing points at any more.
fn startup_maintenance(
    history: &history::HistoryStore,
    recordings: &recordings::RecordingStore,
    retention_days: u32,
    ui: UiLanguage,
) {
    let stale_error = ui.pick(
        "transcription did not run — the app was closed",
        "prepis neprebehol — aplikácia sa ukončila",
    );
    match history.fail_stale_pending(stale_error) {
        Ok(n) if n > 0 => eprintln!("recovered {n} unfinished dictation(s) from the last run"),
        Ok(_) => {}
        Err(e) => eprintln!("could not recover unfinished dictations: {e}"),
    }
    apply_retention(history, recordings, retention_days);
    match history.all_audio_paths() {
        Ok(keep) => recordings.sweep_orphans(&keep.into_iter().collect()),
        // Without a reliable keep-set a sweep would delete real recordings.
        Err(e) => eprintln!("skipping orphan sweep, could not read history: {e}"),
    }
}

/// Deletes completed dictations (text and audio) older than `retention_days`.
/// 0 keeps everything. Returns how many rows went.
pub(crate) fn apply_retention(
    history: &history::HistoryStore,
    recordings: &recordings::RecordingStore,
    retention_days: u32,
) -> usize {
    if retention_days == 0 {
        return 0;
    }
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
        - (retention_days as i64 * 24 * 60 * 60 * 1000);
    match history.delete_done_before(cutoff) {
        Ok((deleted, freed)) => {
            recordings.remove_all(freed);
            deleted
        }
        Err(e) => {
            eprintln!("could not apply history retention: {e}");
            0
        }
    }
}

/// The app lives in the tray for weeks, so retention can't rely on startup
/// alone — re-check every hour.
fn spawn_retention_ticker(ctx: Arc<AppCtx>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        tick.tick().await; // the first tick fires immediately; startup already ran
        loop {
            tick.tick().await;
            let days = ctx.settings.read().unwrap().history_retention_days;
            if apply_retention(&ctx.history, &ctx.recordings, days) > 0 {
                pipeline::emit_history_changed(&ctx);
            }
        }
    });
}

/// Wording for the "hotkey is dead" banner, in the current UI language.
fn listener_death_message(ui: UiLanguage, death: &hotkey::ListenerDeath) -> String {
    match death {
        hotkey::ListenerDeath::MissingAccessibility => ui
            .pick(
                "Hotkey not working — Accessibility permission missing. Open System Settings → \
                 Privacy & Security → Accessibility and enable Dikto; the app picks it up on its own, \
                 no restart needed.",
                "Globálna klávesa nefunguje — chýba povolenie Prístupnosť. Otvor Nastavenia → Súkromie \
                 a bezpečnosť → Prístupnosť a povoľ Dikto; appka to zachytí sama, netreba ju reštartovať.",
            )
            .to_string(),
        hotkey::ListenerDeath::Failed(detail) => {
            if cfg!(target_os = "macos") {
                ui.pick(
                    "Hotkey not working — Accessibility permission missing. Open System Settings → \
                     Privacy & Security → Accessibility.",
                    "Globálna klávesa nefunguje — chýba povolenie Prístupnosť. Otvor Nastavenia → \
                     Súkromie a bezpečnosť → Prístupnosť.",
                )
                .to_string()
            } else {
                format!(
                    "{} ({detail}). {}",
                    ui.pick(
                        "Hotkey not working — the keyboard listener failed to start",
                        "Globálna klávesa nefunguje — sledovanie klávesnice sa nepodarilo spustiť"
                    ),
                    ui.pick("Try restarting Dikto.", "Skús Dikto reštartovať.")
                )
            }
        }
    }
}

/// Positions the bubble at `saved` if it's still on-screen (some monitor
/// intersects where the bubble would land), otherwise falls back to the
/// default bottom-center placement.
pub(crate) fn position_bubble<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, saved: Option<(i32, i32)>) {
    if let Some(pos) = saved {
        if fits_on_a_monitor(win, pos) {
            let _ = win.set_position(tauri::PhysicalPosition::new(pos.0, pos.1));
            return;
        }
    }
    if let (Ok(Some(monitor)), Ok(size)) = (win.primary_monitor(), win.outer_size()) {
        let m = monitor.size();
        let x = (m.width.saturating_sub(size.width)) / 2;
        let y = m.height.saturating_sub(size.height + 120);
        let _ = win.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}

/// True if placing `win` at `pos` (top-left, physical pixels) would overlap
/// at least one currently available monitor.
fn fits_on_a_monitor<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, pos: (i32, i32)) -> bool {
    let Ok(size) = win.outer_size() else { return false };
    let Ok(monitors) = win.available_monitors() else { return false };
    let (x, y) = (pos.0 as i64, pos.1 as i64);
    let (right, bottom) = (x + size.width as i64, y + size.height as i64);
    monitors.iter().any(|m| {
        let mp = m.position();
        let ms = m.size();
        let (mx, my) = (mp.x as i64, mp.y as i64);
        let (mright, mbottom) = (mx + ms.width as i64, my + ms.height as i64);
        x < mright && right > mx && y < mbottom && bottom > my
    })
}

/// Builds the tray icon: an "open" item, a language submenu that mirrors
/// `settings.language` and updates it via the same path as `set_settings`,
/// and a quit item. The returned `TrayIcon` is kept alive internally by
/// Tauri's resource table, so it doesn't need to be stored by the caller.
fn build_tray(app: &tauri::AppHandle, ctx: &Arc<AppCtx>) -> tauri::Result<()> {
    let (current_lang, ui) = {
        let s = ctx.settings.read().unwrap();
        (s.language, s.ui_language)
    };

    let lang_auto = CheckMenuItemBuilder::with_id("lang_auto", "Auto")
        .checked(current_lang == LanguageMode::Auto)
        .build(app)?;
    let lang_sk = CheckMenuItemBuilder::with_id("lang_sk", "SK")
        .checked(current_lang == LanguageMode::Sk)
        .build(app)?;
    let lang_cs = CheckMenuItemBuilder::with_id("lang_cs", "CS")
        .checked(current_lang == LanguageMode::Cs)
        .build(app)?;
    let lang_en = CheckMenuItemBuilder::with_id("lang_en", "EN")
        .checked(current_lang == LanguageMode::En)
        .build(app)?;

    let lang_menu = SubmenuBuilder::new(app, ui.pick(pipeline::TRAY_LANGUAGE.0, pipeline::TRAY_LANGUAGE.1))
        .items(&[&lang_auto, &lang_sk, &lang_cs, &lang_en])
        .build()?;

    let open_item =
        MenuItemBuilder::with_id("tray_open", ui.pick(pipeline::TRAY_OPEN.0, pipeline::TRAY_OPEN.1)).build(app)?;
    let quit_item =
        MenuItemBuilder::with_id("tray_quit", ui.pick(pipeline::TRAY_QUIT.0, pipeline::TRAY_QUIT.1)).build(app)?;
    *ctx.tray_labels.lock().unwrap() = Some(pipeline::TrayLabels {
        open: open_item.clone(),
        quit: quit_item.clone(),
        language: lang_menu.clone(),
    });

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .separator()
        .item(&lang_menu)
        .separator()
        .item(&quit_item)
        .build()?;

    // (mode, item) pairs so a selection can flip its own check on and every
    // other language's check off — Tauri doesn't do radio-group behavior for
    // plain CheckMenuItems.
    let lang_items: Vec<(LanguageMode, CheckMenuItem<tauri::Wry>)> = vec![
        (LanguageMode::Auto, lang_auto),
        (LanguageMode::Sk, lang_sk),
        (LanguageMode::Cs, lang_cs),
        (LanguageMode::En, lang_en),
    ];
    // Handed to apply_settings so it can refresh these checkmarks regardless
    // of which path (tray or Settings page) changed the language.
    *ctx.tray_lang_items.lock().unwrap() = Some(lang_items);

    let ctx_for_menu = ctx.clone();
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().0.as_str() {
            "tray_open" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "tray_quit" => app.exit(0),
            id @ ("lang_auto" | "lang_sk" | "lang_cs" | "lang_en") => {
                let mode = match id {
                    "lang_auto" => LanguageMode::Auto,
                    "lang_sk" => LanguageMode::Sk,
                    "lang_cs" => LanguageMode::Cs,
                    _ => LanguageMode::En,
                };
                let mut new = ctx_for_menu.settings.read().unwrap().clone();
                new.language = mode;
                // apply_settings refreshes all four checkmarks itself via
                // ctx.tray_lang_items, so no need to touch them here.
                let _ = commands::apply_settings(&ctx_for_menu, new);
            }
            _ => {}
        });
    // macOS menu bar: a monochrome template glyph so the system tints it for
    // light and dark bars. Elsewhere the coloured app icon reads better.
    #[cfg(target_os = "macos")]
    {
        if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")) {
            builder = builder.icon(icon).icon_as_template(true);
        }
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod maintenance_tests {
    use super::*;
    use crate::history::{HistoryStore, STATUS_DONE, STATUS_FAILED};
    use crate::recordings::RecordingStore;

    struct Fixture {
        _dir: tempfile::TempDir,
        history: HistoryStore,
        recordings: RecordingStore,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let history = HistoryStore::open(&dir.path().join("history.sqlite")).unwrap();
        let recordings = RecordingStore::new(dir.path().join("audio"));
        Fixture { _dir: dir, history, recordings }
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    fn backdate(history: &HistoryStore, id: i64, days: i64) {
        history
            .set_ts_for_test(id, now_ms() - days * 24 * 60 * 60 * 1000)
            .unwrap();
    }

    #[test]
    fn a_take_interrupted_by_a_crash_becomes_retryable_and_keeps_its_audio() {
        let f = fixture();
        let name = f.recordings.save(b"RIFF-interrupted", 1).unwrap();
        let id = f.history.insert_pending(Some(&name), 5000).unwrap();

        startup_maintenance(&f.history, &f.recordings, 7, UiLanguage::En);

        let row = f.history.get(id).unwrap().unwrap();
        assert_eq!(row.status, STATUS_FAILED);
        assert!(row.error.is_some());
        assert_eq!(row.audio_path.as_deref(), Some(name.as_str()));
        assert_eq!(f.recordings.read(&name).unwrap(), b"RIFF-interrupted");
    }

    #[test]
    fn retention_deletes_old_completed_dictations_with_their_audio() {
        let f = fixture();
        let name = f.recordings.save(b"old", 1).unwrap();
        let id = f.history.insert_pending(Some(&name), 5000).unwrap();
        f.history.mark_done(id, "surovy", "Cisty.", Some("sk")).unwrap();
        backdate(&f.history, id, 30);
        let fresh = f.recordings.save(b"fresh", 2).unwrap();
        let fresh_id = f.history.insert_pending(Some(&fresh), 5000).unwrap();
        f.history.mark_done(fresh_id, "n", "N.", None).unwrap();
        backdate(&f.history, fresh_id, 6);

        startup_maintenance(&f.history, &f.recordings, 7, UiLanguage::En);

        assert!(f.history.get(id).unwrap().is_none(), "row past retention must go");
        assert!(f.recordings.read(&name).is_err(), "the WAV should be gone");
        let kept = f.history.get(fresh_id).unwrap().unwrap();
        assert_eq!(kept.status, STATUS_DONE);
        assert_eq!(kept.audio_path.as_deref(), Some(fresh.as_str()));
        assert_eq!(f.recordings.read(&fresh).unwrap(), b"fresh");
    }

    #[test]
    fn apply_retention_reports_how_many_rows_went() {
        let f = fixture();
        let id = f.history.insert_pending(None, 5000).unwrap();
        f.history.mark_done(id, "a", "A.", None).unwrap();
        backdate(&f.history, id, 8);

        assert_eq!(apply_retention(&f.history, &f.recordings, 7), 1);
        assert_eq!(apply_retention(&f.history, &f.recordings, 7), 0);
    }

    #[test]
    fn retention_zero_keeps_history_forever() {
        let f = fixture();
        let name = f.recordings.save(b"ancient", 1).unwrap();
        let id = f.history.insert_pending(Some(&name), 5000).unwrap();
        f.history.mark_done(id, "a", "A.", None).unwrap();
        backdate(&f.history, id, 3650);

        startup_maintenance(&f.history, &f.recordings, 0, UiLanguage::En);

        assert_eq!(f.history.get(id).unwrap().unwrap().audio_path.as_deref(), Some(name.as_str()));
        assert_eq!(f.recordings.read(&name).unwrap(), b"ancient");
    }

    #[test]
    fn a_failed_take_stays_in_history_no_matter_how_old() {
        let f = fixture();
        let name = f.recordings.save(b"rate-limited", 1).unwrap();
        let id = f.history.insert_pending(Some(&name), 5000).unwrap();
        f.history.mark_failed(id, "groq api 429").unwrap();
        backdate(&f.history, id, 3650);

        startup_maintenance(&f.history, &f.recordings, 7, UiLanguage::En);

        let row = f.history.get(id).unwrap().expect("failed rows are never pruned");
        assert_eq!(row.audio_path.as_deref(), Some(name.as_str()));
        assert_eq!(f.recordings.read(&name).unwrap(), b"rate-limited");
    }

    #[test]
    fn a_wav_no_row_points_at_is_swept_while_referenced_ones_survive() {
        let f = fixture();
        let orphan = f.recordings.save(b"orphan", 1).unwrap();
        let kept = f.recordings.save(b"kept", 2).unwrap();
        let id = f.history.insert_pending(Some(&kept), 5000).unwrap();
        f.history.mark_done(id, "a", "A.", None).unwrap();

        startup_maintenance(&f.history, &f.recordings, 7, UiLanguage::En);

        assert!(f.recordings.read(&orphan).is_err());
        assert_eq!(f.recordings.read(&kept).unwrap(), b"kept");
    }

    #[test]
    fn maintenance_on_an_empty_store_is_a_no_op() {
        let f = fixture();
        startup_maintenance(&f.history, &f.recordings, 7, UiLanguage::En);
        assert!(f.history.list(None, 10).unwrap().is_empty());
    }
}
