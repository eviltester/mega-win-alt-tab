#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    windows_app::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mega-win-alt-tab is a Windows desktop app.");
}
