//! # LAN Audio Streamer
//!
//! Professional-grade low-latency multi-track audio streaming over LAN.
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                              SENDER PC                                       │
//! │  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐     │
//! │  │ Microphone  │   │Desktop Audio│   │  DAW Out    │   │ Game Audio  │     │
//! │  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └──────┬──────┘     │
//! │         │                 │                 │                 │            │
//! │         ▼                 ▼                 ▼                 ▼            │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                    Track Manager (tracks::manager)                   │   │
//! │  │  ┌──────────────────────────────────────────────────────────────┐  │   │
//! │  │  │  Track 0        Track 1        Track 2        Track 3        │  │   │
//! │  │  │  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │  │   │
//! │  │  │  │ Capture │   │ Capture │   │ Capture │   │ Capture │      │  │   │
//! │  │  │  │ Thread  │   │ Thread  │   │ Thread  │   │ Thread  │      │  │   │
//! │  │  │  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘      │  │   │
//! │  │  │       │             │             │             │            │  │   │
//! │  │  │       ▼             ▼             ▼             ▼            │  │   │
//! │  │  │  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │  │   │
//! │  │  │  │  Opus   │   │  Opus   │   │  Opus   │   │  Opus   │      │  │   │
//! │  │  │  │ Encoder │   │ Encoder │   │ Encoder │   │ Encoder │      │  │   │
//! │  │  │  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘      │  │   │
//! │  │  └───────┼─────────────┼─────────────┼─────────────┼────────────┘  │   │
//! │  └──────────┼─────────────┼─────────────┼─────────────┼───────────────┘   │
//! │             │             │             │             │                    │
//! │             ▼             ▼             ▼             ▼                    │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │              UDP Sender (network::udp) - Single Socket              │   │
//! │  │      Packets: [TrackID|Seq|Timestamp|Opus Data]                     │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! │                                     │                                       │
//! └─────────────────────────────────────┼───────────────────────────────────────┘
//!                                       │ UDP over LAN
//!                                       ▼
//! ┌─────────────────────────────────────┼───────────────────────────────────────┐
//! │                              RECEIVER PC                                     │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │              UDP Receiver (network::udp) - Single Socket            │   │
//! │  │      Demux packets by TrackID                                       │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! │             │             │             │             │                    │
//! │             ▼             ▼             ▼             ▼                    │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                    Track Manager (tracks::manager)                   │   │
//! │  │  ┌──────────────────────────────────────────────────────────────┐  │   │
//! │  │  │  Track 0        Track 1        Track 2        Track 3        │  │   │
//! │  │  │  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │  │   │
//! │  │  │  │ Jitter  │   │ Jitter  │   │ Jitter  │   │ Jitter  │      │  │   │
//! │  │  │  │ Buffer  │   │ Buffer  │   │ Buffer  │   │ Buffer  │      │  │   │
//! │  │  │  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘      │  │   │
//! │  │  │       │             │             │             │            │  │   │
//! │  │  │       ▼             ▼             ▼             ▼            │  │   │
//! │  │  │  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │  │   │
//! │  │  │  │  Opus   │   │  Opus   │   │  Opus   │   │  Opus   │      │  │   │
//! │  │  │  │ Decoder │   │ Decoder │   │ Decoder │   │ Decoder │      │  │   │
//! │  │  │  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘      │  │   │
//! │  │  │       │             │             │             │            │  │   │
//! │  │  │       ▼             ▼             ▼             ▼            │  │   │
//! │  │  │  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │  │   │
//! │  │  │  │Playback │   │Playback │   │Playback │   │Playback │      │  │   │
//! │  │  │  │ Thread  │   │ Thread  │   │ Thread  │   │ Thread  │      │  │   │
//! │  │  │  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘      │  │   │
//! │  │  └───────┼─────────────┼─────────────┼─────────────┼────────────┘  │   │
//! │  └──────────┼─────────────┼─────────────┼─────────────┼───────────────┘   │
//! │             ▼             ▼             ▼             ▼                    │
//! │  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐    │
//! │  │Virtual Dev 0│   │Virtual Dev 1│   │Virtual Dev 2│   │Virtual Dev 3│    │
//! │  │ (OBS Input) │   │ (OBS Input) │   │ (OBS Input) │   │ (OBS Input) │    │
//! │  └─────────────┘   └─────────────┘   └─────────────┘   └─────────────┘    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

pub mod audio;
pub mod codec;
pub mod config;
pub mod error;
pub mod network;
pub mod protocol;
pub mod tracks;
pub mod ui;

pub use error::{Error, Result};

/// Application-wide constants
pub mod constants {
    /// Default sample rate for audio processing
    pub const DEFAULT_SAMPLE_RATE: u32 = 48000;
    
    /// Default channel count (stereo)
    pub const DEFAULT_CHANNELS: u16 = 2;
    
    /// Default Opus bitrate in bits per second
    pub const DEFAULT_BITRATE: u32 = 128_000;
    
    /// Default frame size in milliseconds
    pub const DEFAULT_FRAME_SIZE_MS: f32 = 10.0;
    
    /// Maximum number of concurrent tracks
    pub const MAX_TRACKS: usize = 16;
    
    /// Default UDP port for audio streaming
    pub const DEFAULT_UDP_PORT: u16 = 5000;
    
    /// Default WebSocket port for control
    pub const DEFAULT_WS_PORT: u16 = 8080;
    
    /// Default jitter buffer size in milliseconds
    pub const DEFAULT_JITTER_BUFFER_MS: u32 = 20;
    
    /// Maximum packet size for UDP
    pub const MAX_PACKET_SIZE: usize = 1472; // MTU - IP/UDP headers
    
    /// Lock-free ring buffer capacity (in frames)
    pub const RING_BUFFER_CAPACITY: usize = 256;
}
