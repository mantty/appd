#![deny(missing_docs)]

//! The on-disk contract for a packaged Worker application.
//!
//! `cli` prepares an app with these types and the runtime reads it back.

pub mod assets;
pub mod environment;
mod layout;
mod package;
mod worker;
pub mod wrangler;

pub use layout::AppLayout;
pub use package::{Error, Result, app_host, is_valid_app_name};
pub use worker::{
    WorkerManifest, compress_worker_bundle, compress_worker_module, decompress_worker_bundle,
    decompress_worker_module, read_worker_manifest, write_worker_manifest,
};
