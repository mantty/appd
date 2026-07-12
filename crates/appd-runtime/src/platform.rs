//! Native platform shells.

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
pub use ios::run;

#[cfg(target_os = "macos")]
pub use macos::run;

/// Run the native runtime shell for the current target platform.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
pub fn run() -> ! {
    compile_error!("appd-runtime currently supports macOS and physical iOS devices");
}
