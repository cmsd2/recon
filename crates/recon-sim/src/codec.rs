//! The wire boundary, exercised on demand.
//!
//! The simulator moves typed values between processes, so runs are fast and a codec defect
//! cannot masquerade as a protocol defect. Encoding is still worth checking — just not on every
//! delivery of every run — so it is available as an opt-in mode.

use recon_core::error::CodecError;
use serde::{Serialize, de::DeserializeOwned};

/// Encode a value the way a real driver would: once, at the boundary, with no intermediate
/// representation.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    bincode::serde::encode_to_vec(value, bincode::config::standard()).map_err(|e| {
        CodecError::Encode { type_name: core::any::type_name::<T>(), source: Box::new(e) }
    })
}

/// Decode a value produced by [`encode`].
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .map(|(v, _)| v)
        .map_err(|e| CodecError::Decode {
            type_name: core::any::type_name::<T>(),
            source: Box::new(e),
        })
}

/// Encode then decode, confirming the value survives unchanged.
pub fn round_trip<T>(value: &T) -> Result<T, CodecError>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    let bytes = encode(value)?;
    let back: T = decode(&bytes)?;
    if back != *value {
        return Err(CodecError::RoundTripMismatch { type_name: core::any::type_name::<T>() });
    }
    Ok(back)
}
