use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::Result;

const WORKER_BUNDLE_HEADER: &[u8] = b"APPD-QJS-GZIP\x01";

/// Compress `QuickJS` bytecode for storage in a packaged app.
///
/// # Errors
///
/// Returns an error when the gzip encoder cannot write the bytecode.
pub fn compress_worker_bundle(bytecode: &[u8]) -> Result<Vec<u8>> {
    let mut compressor = GzEncoder::new(WORKER_BUNDLE_HEADER.to_vec(), Compression::best());
    compressor.write_all(bytecode)?;
    let compressed = compressor.finish()?;
    if compressed.len() < bytecode.len() {
        Ok(compressed)
    } else {
        Ok(bytecode.to_vec())
    }
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
    let mut decoder = GzDecoder::new(&bundle[WORKER_BUNDLE_HEADER.len()..]);
    let mut bytecode = Vec::new();
    decoder.read_to_end(&mut bytecode)?;
    Ok(bytecode)
}

#[cfg(test)]
mod tests {
    use super::{WORKER_BUNDLE_HEADER, compress_worker_bundle, decompress_worker_bundle};

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
}
