//! Opus codec wrapper
//!
//! Provides per-track Opus encoding and decoding with
//! configuration optimized for different audio types.

pub mod encoder;
pub mod decoder;

pub use encoder::OpusEncoder;
pub use decoder::OpusDecoder;
