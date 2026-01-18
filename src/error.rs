//! Error types for the audio streaming application

use thiserror::Error;

/// Main error type for the application
#[derive(Error, Debug)]
pub enum Error {
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),
    
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),
    
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    
    #[error("Track error: {0}")]
    Track(#[from] TrackError),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Audio subsystem errors
#[derive(Error, Debug)]
pub enum AudioError {
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Failed to open stream: {0}")]
    StreamError(String),
    
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    
    #[error("Buffer overflow")]
    BufferOverflow,
    
    #[error("Buffer underrun")]
    BufferUnderrun,
    
    #[error("WASAPI error: {0}")]
    WasapiError(String),
    
    #[error("cpal error: {0}")]
    CpalError(String),
}

/// Codec errors
#[derive(Error, Debug)]
pub enum CodecError {
    #[error("Encoder initialization failed: {0}")]
    EncoderInit(String),
    
    #[error("Decoder initialization failed: {0}")]
    DecoderInit(String),
    
    #[error("Encoding failed: {0}")]
    EncodingFailed(String),
    
    #[error("Decoding failed: {0}")]
    DecodingFailed(String),
    
    #[error("Invalid frame size: {0}")]
    InvalidFrameSize(usize),
}

/// Network errors
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Socket bind failed: {0}")]
    BindFailed(String),
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Send failed: {0}")]
    SendFailed(String),
    
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),
    
    #[error("Packet too large: {0} bytes")]
    PacketTooLarge(usize),
    
    #[error("Invalid packet format")]
    InvalidPacket,
    
    #[error("Timeout")]
    Timeout,
}

/// Track management errors
#[derive(Error, Debug)]
pub enum TrackError {
    #[error("Track not found: {0}")]
    NotFound(u8),
    
    #[error("Track already exists: {0}")]
    AlreadyExists(u8),
    
    #[error("Maximum tracks reached: {0}")]
    MaxTracksReached(usize),
    
    #[error("Invalid track configuration: {0}")]
    InvalidConfig(String),
    
    #[error("Track is not active")]
    NotActive,
}

/// Result type alias for the application
pub type Result<T> = std::result::Result<T, Error>;
