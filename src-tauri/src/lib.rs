mod audio;
mod cleanup;
mod commands;
mod history;
mod hotkey;
mod inject;
mod pipeline;
mod settings;
mod state;
mod stt;

use pipeline::AppCtx;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Dev convenience: picks up GROQ_API_KEY from a repo-root .env
    // (dotenv walks up from CWD; silently a no-op in bundled builds).
    let _ = dotenvy::dotenv();
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
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
            commands::finish_wizard,
            commands::history_list,
            commands::history_delete,
            commands::history_clear,
            commands::permissions_status,
            commands::open_privacy_settings
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

            let data_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&data_dir).expect("create app data dir");
            let history = history::HistoryStore::open(&data_dir.join("history.sqlite"))
                .expect("open history db");

            let ctx = Arc::new(AppCtx {
                phase: Mutex::new(state::Phase::Idle),
                recorder: audio::Recorder::new(),
                settings: RwLock::new(s),
                pending_wav: Mutex::new(None),
                partial_inflight: AtomicBool::new(false),
                take_gen: AtomicU64::new(0),
                app: app.handle().clone(),
                hotkey_name: hotkey_name.clone(),
                settings_path,
                history,
            });
            app.manage(ctx.clone());

            if let Some(bubble) = app.get_webview_window("bubble") {
                position_bubble(&bubble);
                #[cfg(target_os = "macos")]
                {
                    use tauri_nspanel::WebviewWindowExt;
                    // NSWindowStyleMaskNonactivatingPanel = 1 << 7,
                    // NSStatusWindowLevel = 25,
                    // collection: canJoinAllSpaces (1<<0) | fullScreenAuxiliary (1<<8)
                    if let Ok(panel) = bubble.to_panel() {
                        panel.set_style_mask(1 << 7);
                        panel.set_level(25);
                        panel.set_collection_behaviour(
                            tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior::from_bits_retain(
                                (1 << 0) | (1 << 8),
                            ),
                        );
                    }
                }
            }

            let (tx, rx) = mpsc::channel::<hotkey::HotkeySignal>();
            let dead_app = app.handle().clone();
            hotkey::spawn(
                hotkey_name,
                tx,
                Box::new(move |message: String| {
                    let _ = dead_app.emit("dictation:pipeline-dead", serde_json::json!({ "message": message }));
                    if let Some(win) = dead_app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }),
            );
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

pub(crate) fn position_bubble(win: &tauri::WebviewWindow) {
    if let (Ok(Some(monitor)), Ok(size)) = (win.primary_monitor(), win.outer_size()) {
        let m = monitor.size();
        let x = (m.width.saturating_sub(size.width)) / 2;
        let y = m.height.saturating_sub(size.height + 120);
        let _ = win.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}
