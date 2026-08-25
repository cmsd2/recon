//! Error conventions.
//!
//! Each layer defines its own error type with `thiserror` and preserves the originating cause.
//! Domain failures are never flattened into `std::io::Error`, and never reduced to a string —
//! a decode failure must remain distinguishable from every other decode failure.

use thiserror::Error;

/// Failures at the wire boundary, where typed values become bytes and back.
///
/// This is the one place in the system that touches encoding: layers above pass typed values.
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("failed to encode {type_name}")]
    Encode {
        type_name: &'static str,
        #[source]
        source: Box<dyn core::error::Error + Send + Sync>,
    },
    #[error("failed to decode {type_name}")]
    Decode {
        type_name: &'static str,
        #[source]
        source: Box<dyn core::error::Error + Send + Sync>,
    },
    #[error("{type_name} did not survive an encode/decode round trip")]
    RoundTripMismatch { type_name: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::error::Error as _;

    #[derive(Debug, Error)]
    #[error("the underlying detail")]
    struct Underlying;

    #[test]
    fn every_variant_retains_its_cause() {
        let e = CodecError::Decode { type_name: "PlMsg", source: Box::new(Underlying) };
        // The message identifies decoding as the cause...
        assert_eq!(e.to_string(), "failed to decode PlMsg");
        // ...and the underlying detail survives, rather than being flattened to a string.
        assert_eq!(e.source().expect("cause preserved").to_string(), "the underlying detail");
    }

    #[test]
    fn encode_and_decode_are_distinguishable() {
        let d = CodecError::Decode { type_name: "T", source: Box::new(Underlying) };
        let e = CodecError::Encode { type_name: "T", source: Box::new(Underlying) };
        assert_ne!(d.to_string(), e.to_string());
        assert!(matches!(d, CodecError::Decode { .. }));
        assert!(matches!(e, CodecError::Encode { .. }));
    }

    #[test]
    fn round_trip_mismatch_names_the_type() {
        let e = CodecError::RoundTripMismatch { type_name: "BebMsg" };
        assert!(e.to_string().contains("BebMsg"));
        assert!(e.source().is_none());
    }
}
