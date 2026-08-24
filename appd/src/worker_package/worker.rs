use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

use super::{AppLayout, Result};

const WORKER_BUNDLE_HEADER: &[u8] = b"APPD-QJS-GZIP\x01";

/// The entry module and on-disk module directory for a packaged Worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkerManifest {
    /// Module name used to start the Worker.
    pub entry: String,
}

/// Compress `QuickJS` bytecode for storage in a packaged app.
///
/// # Errors
///
/// Returns an error when the gzip encoder cannot write the bytecode.
pub fn compress_worker_bundle(bytecode: &[u8]) -> Result<Vec<u8>> {
    compress_worker_bytecode(bytecode, WORKER_BUNDLE_HEADER)
}

/// Decode a packaged `QuickJS` bundle.
///
/// Legacy uncompressed bundles are returned unchanged.
///
/// # Errors
///
/// Returns an error when a compressed bundle is invalid or cannot be decoded.
pub fn decompress_worker_bundle(bundle: &[u8]) -> Result<Vec<u8>> {
    if !bundle.starts_with(WORKER_BUNDLE_HEADER) {
        return Ok(bundle.to_vec());
    }
    decompress_gzip(&bundle[WORKER_BUNDLE_HEADER.len()..])
}

/// Compress one split Worker module for storage in a packaged app.
///
/// The result is either a standard gzip stream or the original bytecode when
/// compression would make the module larger. This keeps small modules cheap
/// while allowing the runtime to decode each module independently.
///
/// # Errors
///
/// Returns an error when the gzip encoder cannot write the bytecode.
pub fn compress_worker_module(bytecode: &[u8]) -> Result<Vec<u8>> {
    compress_worker_bytecode(bytecode, &[])
}

fn compress_worker_bytecode(bytecode: &[u8], prefix: &[u8]) -> Result<Vec<u8>> {
    let mut compressor = GzEncoder::new(prefix.to_vec(), Compression::best());
    compressor.write_all(bytecode)?;
    let compressed = compressor.finish()?;
    Ok(if compressed.len() < bytecode.len() {
        compressed
    } else {
        bytecode.to_vec()
    })
}

/// Decode one independently stored Worker module.
///
/// # Errors
///
/// Returns an error when a gzip-compressed module is invalid.
pub fn decompress_worker_module(module: &[u8]) -> Result<Vec<u8>> {
    if !module.starts_with(&[0x1f, 0x8b]) {
        return Ok(module.to_vec());
    }
    decompress_gzip(module)
}

fn decompress_gzip(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(compressed);
    let mut bytecode = Vec::new();
    decoder.read_to_end(&mut bytecode)?;
    Ok(bytecode)
}

/// Write the split Worker manifest.
///
/// # Errors
///
/// Returns an error when the manifest cannot be serialized or written.
pub fn write_worker_manifest(layout: &AppLayout, manifest: &WorkerManifest) -> Result<()> {
    std::fs::write(
        layout.worker_manifest(),
        serde_json::to_vec_pretty(manifest)?,
    )?;
    Ok(())
}

/// Read the split Worker manifest.
///
/// # Errors
///
/// Returns an error when the manifest cannot be read or decoded.
pub fn read_worker_manifest(layout: &AppLayout) -> Result<WorkerManifest> {
    Ok(serde_json::from_slice(&std::fs::read(
        layout.worker_manifest(),
    )?)?)
}

#[cfg(test)]
mod tests {
    use super::super::AppLayout;
    use super::{
        WORKER_BUNDLE_HEADER, WorkerManifest, compress_worker_bundle, compress_worker_module,
        decompress_worker_bundle, decompress_worker_module,
    };

    #[test]
    fn round_trips_compressed_worker_bytecode() -> Result<(), Box<dyn std::error::Error>> {
        let bytecode = b"quickjs bytecode".repeat(128);
        let compressed = compress_worker_bundle(&bytecode)?;

        assert!(compressed.starts_with(WORKER_BUNDLE_HEADER));
        assert!(compressed.len() < bytecode.len());
        assert_eq!(decompress_worker_bundle(&compressed)?, bytecode);
        Ok(())
    }

    #[test]
    fn rejects_corrupt_compressed_worker_bytecode() -> Result<(), Box<dyn std::error::Error>> {
        let mut compressed = compress_worker_bundle(&b"quickjs bytecode".repeat(128))?;
        let last = compressed
            .last_mut()
            .ok_or("compressed worker bundle was empty")?;
        *last ^= 1;

        assert!(decompress_worker_bundle(&compressed).is_err());
        Ok(())
    }

    #[test]
    fn accepts_legacy_uncompressed_worker_bytecode() -> Result<(), Box<dyn std::error::Error>> {
        let bytecode = b"legacy bytecode";

        assert_eq!(decompress_worker_bundle(bytecode)?, bytecode);
        Ok(())
    }

    #[test]
    fn compresses_split_modules_independently() -> Result<(), Box<dyn std::error::Error>> {
        let bytecode = b"quickjs bytecode".repeat(128);
        let compressed = compress_worker_module(&bytecode)?;
        assert_eq!(decompress_worker_module(&compressed)?, bytecode);
        Ok(())
    }

    #[test]
    fn round_trips_worker_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let layout = AppLayout::new(directory.path());
        let manifest = WorkerManifest {
            entry: "entry.js".to_owned(),
        };
        super::write_worker_manifest(&layout, &manifest)?;

        assert_eq!(super::read_worker_manifest(&layout)?, manifest);
        Ok(())
    }
}
