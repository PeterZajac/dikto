// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--selftest") {
        let Some(wav_path) = args.get(2) else {
            eprintln!("usage: dikto --selftest <wav-path>");
            std::process::exit(2);
        };
        std::process::exit(dikto_lib::run_selftest(wav_path));
    }
    dikto_lib::run()
}
