//! Native platform shells.

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod webview2_args;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
pub use android::run;

#[cfg(target_os = "ios")]
pub use ios::run;

#[cfg(target_os = "linux")]
pub use linux::run;

#[cfg(target_os = "macos")]
pub use macos::run;

#[cfg(target_os = "windows")]
pub use windows::run;

/// Run the native runtime shell for the current target platform.
#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
pub fn run() -> ! {
    compile_error!("appd-runtime has no native shell for this target");
}
