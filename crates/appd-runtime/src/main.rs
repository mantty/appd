#![deny(missing_docs)]

//! `appd` native runtime executable.

#[cfg(not(test))]
fn main() {
    appd_runtime::platform::run();
}

#[cfg(test)]
fn main() {}
