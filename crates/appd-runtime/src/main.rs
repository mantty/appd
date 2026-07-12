#![deny(missing_docs)]

//! `appd` native runtime executable.

#[cfg(all(not(test), feature = "native-shell"))]
fn main() {
    appd_runtime::platform::run();
}

#[cfg(any(test, not(feature = "native-shell")))]
fn main() {}
