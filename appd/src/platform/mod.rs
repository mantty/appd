//! Platform-specific native boundaries owned by the appd runtime.

#[cfg(target_os = "android")]
mod android;

#[cfg(target_vendor = "apple")]
mod apple;
