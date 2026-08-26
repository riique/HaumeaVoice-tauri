#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if haumea_voice_lib::native_messaging::is_native_messaging_invocation() {
        if let Err(error) = haumea_voice_lib::native_messaging::run_native_messaging_host() {
            eprintln!("Haumea Voice native messaging host failed: {error}");
        }
        return;
    }
    haumea_voice_lib::run()
}
