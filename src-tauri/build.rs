fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "cancel_dictation",
            "retry_transcription",
            "set_groq_key",
        ])),
    )
    .expect("failed to run tauri-build");
}
