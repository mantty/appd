//! Cap'n Proto helpers used at Rust/native boundaries.

use std::io::Cursor;

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize_packed;

use crate::{RuntimeError, RuntimeResult};

/// Encode opaque bytes as a packed Cap'n Proto message.
///
/// # Errors
///
/// Returns an error if Cap'n Proto serialization fails.
pub fn encode_payload(payload: &[u8]) -> RuntimeResult<Vec<u8>> {
    let len = u32::try_from(payload.len())
        .map_err(|_| RuntimeError::CapnpPayloadTooLarge(payload.len()))?;
    let mut message = Builder::new_default();
    {
        let root = message.initn_root::<capnp::data::Builder<'_>>(len);
        root.copy_from_slice(payload);
    }

    let mut encoded = Vec::new();
    serialize_packed::write_message(&mut encoded, &message)?;
    Ok(encoded)
}

/// Decode opaque bytes from a packed Cap'n Proto message.
///
/// # Errors
///
/// Returns an error if the bytes are not a valid packed Cap'n Proto data
/// message.
pub fn decode_payload(encoded: &[u8]) -> RuntimeResult<Vec<u8>> {
    let mut cursor = Cursor::new(encoded);
    let message = serialize_packed::read_message(&mut cursor, ReaderOptions::new())?;
    let root = message.get_root::<capnp::data::Reader<'_>>()?;
    Ok(root.to_vec())
}
