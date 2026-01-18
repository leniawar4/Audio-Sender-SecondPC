//! Opus encoder wrapper
//!
//! Provides low-latency Opus encoding with per-track configuration.

use bytes::Bytes;
use opus::{Application, Channels, Encoder};
use crate::config::{OpusConfig, OpusBandwidth, OpusSignal};
use crate::error::CodecError;
use crate::protocol::TrackType;

/// Opus encoder wrapper with optimized settings
pub struct OpusEncoder {
    encoder: Encoder,
    config: OpusConfig,
    /// Encoding buffer (reused to avoid allocations)
    encode_buffer: Vec<u8>,
    /// Frame counter for statistics
    frames_encoded: u64,
    /// Total bytes produced
    bytes_produced: u64,
}

impl OpusEncoder {
    /// Create a new Opus encoder with the specified configuration
    pub fn new(config: OpusConfig) -> Result<Self, CodecError> {
        let channels = match config.channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(CodecError::EncoderInit(
                format!("Unsupported channel count: {}", config.channels)
            )),
        };
        
        let application = match config.application {
            TrackType::Voice => Application::Voip,
            TrackType::Music => Application::Audio,
            TrackType::LowLatency => Application::LowDelay,
        };
        
        let mut encoder = Encoder::new(config.sample_rate, channels, application)
            .map_err(|e| CodecError::EncoderInit(e.to_string()))?;
        
        // Configure encoder
        Self::configure_encoder(&mut encoder, &config)?;
        
        // Pre-allocate encoding buffer (max Opus frame is about 1275 bytes)
        let encode_buffer = vec![0u8; 4000];
        
        Ok(Self {
            encoder,
            config,
            encode_buffer,
            frames_encoded: 0,
            bytes_produced: 0,
        })
    }
    
    /// Create encoder optimized for voice
    pub fn voice(sample_rate: u32, channels: u16) -> Result<Self, CodecError> {
        let mut config = OpusConfig::voice();
        config.sample_rate = sample_rate;
        config.channels = channels;
        config.frame_size = OpusConfig::frame_size_from_ms(sample_rate, 10.0);
        Self::new(config)
    }
    
    /// Create encoder optimized for music
    pub fn music(sample_rate: u32, channels: u16) -> Result<Self, CodecError> {
        let mut config = OpusConfig::music();
        config.sample_rate = sample_rate;
        config.channels = channels;
        config.frame_size = OpusConfig::frame_size_from_ms(sample_rate, 10.0);
        Self::new(config)
    }
    
    /// Create encoder optimized for low latency
    pub fn low_latency(sample_rate: u32, channels: u16) -> Result<Self, CodecError> {
        let mut config = OpusConfig::low_latency();
        config.sample_rate = sample_rate;
        config.channels = channels;
        config.frame_size = OpusConfig::frame_size_from_ms(sample_rate, 2.5);
        Self::new(config)
    }
    
    /// Configure the encoder with all settings
    fn configure_encoder(encoder: &mut Encoder, config: &OpusConfig) -> Result<(), CodecError> {
        // Bitrate
        encoder.set_bitrate(opus::Bitrate::Bits(config.bitrate as i32))
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set bitrate: {}", e)))?;
        
        // VBR settings
        encoder.set_vbr(config.vbr)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set VBR: {}", e)))?;
        
        if config.vbr && config.cvbr {
            encoder.set_vbr_constraint(true)
                .map_err(|e| CodecError::EncoderInit(format!("Failed to set CVBR: {}", e)))?;
        }
        
        // Complexity (0-10)
        encoder.set_complexity(config.complexity as i32)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set complexity: {}", e)))?;
        
        // FEC
        encoder.set_inband_fec(config.fec)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set FEC: {}", e)))?;
        
        if config.fec {
            encoder.set_packet_loss_perc(config.packet_loss_perc as i32)
                .map_err(|e| CodecError::EncoderInit(format!("Failed to set packet loss: {}", e)))?;
        }
        
        // DTX
        encoder.set_dtx(config.dtx)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set DTX: {}", e)))?;
        
        // Signal type
        let signal = match config.signal {
            OpusSignal::Auto => opus::Signal::Auto,
            OpusSignal::Voice => opus::Signal::Voice,
            OpusSignal::Music => opus::Signal::Music,
        };
        encoder.set_signal(signal)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set signal type: {}", e)))?;
        
        // Bandwidth
        let bandwidth = match config.max_bandwidth {
            OpusBandwidth::Narrowband => opus::Bandwidth::Narrowband,
            OpusBandwidth::Mediumband => opus::Bandwidth::Mediumband,
            OpusBandwidth::Wideband => opus::Bandwidth::Wideband,
            OpusBandwidth::Superwideband => opus::Bandwidth::Superwideband,
            OpusBandwidth::Fullband => opus::Bandwidth::Fullband,
        };
        encoder.set_bandwidth(bandwidth)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set bandwidth: {}", e)))?;
        
        Ok(())
    }
    
    /// Encode audio samples to Opus
    /// 
    /// Input must be interleaved f32 samples with length = frame_size * channels
    pub fn encode(&mut self, samples: &[f32]) -> Result<Bytes, CodecError> {
        let expected_len = self.config.frame_size * self.config.channels as usize;
        if samples.len() != expected_len {
            return Err(CodecError::InvalidFrameSize(samples.len()));
        }
        
        let size = self.encoder
            .encode_float(samples, &mut self.encode_buffer)
            .map_err(|e| CodecError::EncodingFailed(e.to_string()))?;
        
        self.frames_encoded += 1;
        self.bytes_produced += size as u64;
        
        Ok(Bytes::copy_from_slice(&self.encode_buffer[..size]))
    }
    
    /// Update bitrate dynamically
    pub fn set_bitrate(&mut self, bitrate: u32) -> Result<(), CodecError> {
        self.encoder.set_bitrate(opus::Bitrate::Bits(bitrate as i32))
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set bitrate: {}", e)))?;
        self.config.bitrate = bitrate;
        Ok(())
    }
    
    /// Update FEC setting dynamically
    pub fn set_fec(&mut self, enabled: bool, packet_loss_perc: u8) -> Result<(), CodecError> {
        self.encoder.set_inband_fec(enabled)
            .map_err(|e| CodecError::EncoderInit(format!("Failed to set FEC: {}", e)))?;
        
        if enabled {
            self.encoder.set_packet_loss_perc(packet_loss_perc as i32)
                .map_err(|e| CodecError::EncoderInit(format!("Failed to set packet loss: {}", e)))?;
        }
        
        self.config.fec = enabled;
        self.config.packet_loss_perc = packet_loss_perc;
        Ok(())
    }
    
    /// Get current configuration
    pub fn config(&self) -> &OpusConfig {
        &self.config
    }
    
    /// Get expected frame size in samples (per channel)
    pub fn frame_size(&self) -> usize {
        self.config.frame_size
    }
    
    /// Get expected total samples per frame (including all channels)
    pub fn samples_per_frame(&self) -> usize {
        self.config.frame_size * self.config.channels as usize
    }
    
    /// Get frame duration in milliseconds
    pub fn frame_duration_ms(&self) -> f32 {
        self.config.frame_duration_ms()
    }
    
    /// Get statistics
    pub fn stats(&self) -> EncoderStats {
        EncoderStats {
            frames_encoded: self.frames_encoded,
            bytes_produced: self.bytes_produced,
            average_frame_size: if self.frames_encoded > 0 {
                self.bytes_produced as f32 / self.frames_encoded as f32
            } else {
                0.0
            },
        }
    }
    
    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.frames_encoded = 0;
        self.bytes_produced = 0;
    }
}

/// Encoder statistics
#[derive(Debug, Clone)]
pub struct EncoderStats {
    pub frames_encoded: u64,
    pub bytes_produced: u64,
    pub average_frame_size: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_encoder_creation() {
        let encoder = OpusEncoder::music(48000, 2);
        assert!(encoder.is_ok());
        
        let encoder = encoder.unwrap();
        assert_eq!(encoder.config().sample_rate, 48000);
        assert_eq!(encoder.config().channels, 2);
    }
    
    #[test]
    fn test_encoding() {
        let mut encoder = OpusEncoder::music(48000, 2).unwrap();
        let frame_size = encoder.samples_per_frame();
        
        // Create a test frame (silence)
        let samples = vec![0.0f32; frame_size];
        
        let result = encoder.encode(&samples);
        assert!(result.is_ok());
        
        let encoded = result.unwrap();
        assert!(!encoded.is_empty());
        assert!(encoded.len() < frame_size * 4); // Should be compressed
    }
    
    #[test]
    fn test_voice_encoder() {
        let mut encoder = OpusEncoder::voice(48000, 1).unwrap();
        let frame_size = encoder.samples_per_frame();
        
        let samples = vec![0.0f32; frame_size];
        let result = encoder.encode(&samples);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_low_latency_encoder() {
        let encoder = OpusEncoder::low_latency(48000, 2).unwrap();
        
        // Low latency should use 2.5ms frames = 120 samples at 48kHz
        assert_eq!(encoder.frame_size(), 120);
        assert!((encoder.frame_duration_ms() - 2.5).abs() < 0.1);
    }
}
