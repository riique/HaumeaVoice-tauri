#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if sonora_lib::native_messaging::is_native_messaging_invocation() {
        if let Err(error) = sonora_lib::native_messaging::run_native_messaging_host() {
            eprintln!("Sonora native messaging host failed: {error}");
        }
        return;
    }
    sonora_lib::run()
}
