#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = margin_lib::cli::run(std::env::args_os()) {
        std::process::exit(code);
    }
    margin_lib::run();
}
