//! Native platform shells.

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "ios", target_os = "macos"))]
mod proxy;

#[cfg(target_os = "ios")]
pub use ios::run;

#[cfg(target_os = "macos")]
pub use macos::run;

#[cfg(target_os = "android")]
pub use android::run;

/// Run the native runtime shell for the current target platform.
#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
pub fn run() -> ! {
    compile_error!("appd-runtime currently supports Apple and Android targets");
}
