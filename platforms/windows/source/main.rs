#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod shell;

#[cfg(windows)]
fn main() {
    if let Err(error) = shell::run() {
        show_startup_error(&error);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn show_startup_error(error: &anyhow::Error) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
        core::{HSTRING, w},
    };

    let message = HSTRING::from(format!("The app could not start.\n\n{error:#}"));
    unsafe {
        MessageBoxW(None, &message, w!("tokamak"), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn main() {
    panic!("tokamak-shell-windows must be built on Windows");
}
