mod audio;
mod cleanup;
mod hotkey;
mod inject;
mod pipeline;
mod settings;
mod state;
mod stt;

use pipeline::AppCtx;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("app config dir");
            let s = settings::load(&config_dir.join("settings.json"));
            let hotkey_name = Arc::new(RwLock::new(s.hotkey.clone()));

            let ctx = Arc::new(AppCtx {
                phase: Mutex::new(state::Phase::Idle),
                recorder: audio::Recorder::new(),
                settings: RwLock::new(s),
                pending_wav: Mutex::new(None),
                partial_inflight: AtomicBool::new(false),
                app: app.handle().clone(),
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
            hotkey::spawn(hotkey_name, tx);
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
