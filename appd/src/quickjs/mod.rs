#![deny(missing_docs)]

//! `QuickJS` runtime integration for appd.

mod api;
#[cfg(feature = "native")]
mod gateway;
#[cfg(all(test, feature = "native"))]
mod tests;
#[cfg(feature = "native")]
mod transport;
mod worker;

pub use api::{Error, compile_module, compile_worker};

#[cfg(feature = "native")]
pub use api::{Assets, Certificates, QuickJsRuntime, RuntimeConfig, WorkerBundle};
