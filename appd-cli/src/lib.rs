#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Shared appd CLI library types.

mod target_pack;

pub use target_pack::{
    Artifact, ArtifactKind, ESBUILD_DIRECTORY, ESBUILD_EXECUTABLE, MANIFEST_FILE, Platform,
    RUNTIME_DIRECTORY, RUNTIME_JAVASCRIPT_DIRECTORY, Target, TargetPackError, TargetPackManifest,
    load_manifest, write_manifest,
};
