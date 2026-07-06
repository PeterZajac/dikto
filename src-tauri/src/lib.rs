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
    tauri::Builder::default()
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
