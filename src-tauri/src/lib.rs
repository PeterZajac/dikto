mod audio;
mod cleanup;
mod commands;
mod history;
mod hotkey;
mod inject;
#[cfg(target_os = "macos")]
mod macos_tap;
mod pipeline;
mod settings;
mod state;
mod stt;

use pipeline::AppCtx;
use settings::LanguageMode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

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
            commands::finish_wizard,
            commands::history_list,
            commands::history_delete,
            commands::history_clear,
            commands::permissions_status,
            commands::open_privacy_settings,
            commands::open_url,
            commands::hotkey_capture_start
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("app config dir");
            #[cfg(target_os = "macos")]
            migrate_from_old_identifier(&config_dir, "settings.json");
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
            #[cfg(target_os = "macos")]
            migrate_from_old_identifier(&data_dir, "history.sqlite");
            std::fs::create_dir_all(&data_dir).expect("create app data dir");
            let history = history::HistoryStore::open_or_recover(&data_dir.join("history.sqlite"));

            let capture_next = Arc::new(AtomicBool::new(false));

            let (tx, rx) = mpsc::channel::<hotkey::HotkeySignal>();
            let dead_app = app.handle().clone();
            let captured_app = app.handle().clone();
            let hotkey_interp = hotkey::spawn(
                hotkey_name.clone(),
                tx,
                capture_next.clone(),
                Box::new(move |message: String| {
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
                pending_wav: Mutex::new(None),
                partial_inflight: AtomicBool::new(false),
                take_gen: AtomicU64::new(0),
                app: app.handle().clone(),
                hotkey_name: hotkey_name.clone(),
                settings_path,
                history,
                capture_next: capture_next.clone(),
                tray_lang_items: Mutex::new(None),
                hotkey_interp,
            });
            app.manage(ctx.clone());

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

/// Copies `filename` from the pre-rename app dir (identifier
/// `com.peterzajac.localwisprflow`) into `new_dir` if the new location
/// doesn't have it yet, so upgrading users keep their settings/history after
/// the `Local Wispr Flow` → `Dikto` identifier change. Best-effort: any
/// failure just leaves the app to start fresh in `new_dir`.
#[cfg(target_os = "macos")]
fn migrate_from_old_identifier(new_dir: &std::path::Path, filename: &str) {
    const OLD_IDENTIFIER: &str = "com.peterzajac.localwisprflow";
    let new_file = new_dir.join(filename);
    if new_file.exists() {
        return;
    }
    let Some(old_file) = new_dir.parent().map(|p| p.join(OLD_IDENTIFIER).join(filename)) else {
        return;
    };
    if !old_file.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(new_dir);
    let _ = std::fs::copy(&old_file, &new_file);
}

/// Positions the bubble at `saved` if it's still on-screen (some monitor
/// intersects where the bubble would land), otherwise falls back to the
/// default bottom-center placement.
pub(crate) fn position_bubble(win: &tauri::WebviewWindow, saved: Option<(i32, i32)>) {
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
fn fits_on_a_monitor(win: &tauri::WebviewWindow, pos: (i32, i32)) -> bool {
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
    let current_lang = ctx.settings.read().unwrap().language;

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

    let lang_menu = SubmenuBuilder::new(app, "Jazyk")
        .items(&[&lang_auto, &lang_sk, &lang_cs, &lang_en])
        .build()?;

    let open_item = MenuItemBuilder::with_id("tray_open", "Otvoriť Dikto").build(app)?;
    let quit_item = MenuItemBuilder::with_id("tray_quit", "Ukončiť").build(app)?;

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
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}
