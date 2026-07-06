fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "cancel_dictation",
            "retry_transcription",
            "set_groq_key",
            "get_settings",
            "set_settings",
            "has_groq_key",
            "test_groq_key",
            "meridian_status",
            "finish_wizard",
            "history_list",
            "history_delete",
            "history_clear",
            "permissions_status",
            "open_privacy_settings",
        ])),
    )
    .expect("failed to run tauri-build");
}
