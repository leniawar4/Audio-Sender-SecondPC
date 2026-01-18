//! Opus decoder wrapper
//!
//! Provides Opus decoding with packet loss concealment.

use opus::{Channels, Decoder};
use crate::error::CodecError;

/// Opus decoder wrapper
pub struct OpusDecoder {
    decoder: Decoder,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    /// Decoding buffer (reused to avoid allocations)
    decode_buffer: Vec<f32>,
    /// Frames decoded
    frames_decoded: u64,
    /// Frames lost (PLC used)
    frames_lost: u64,
    /// Total samples produced
    samples_produced: u64,
}

impl OpusDecoder {
    /// Create a new Opus decoder
    pub fn new(sample_rate: u32, channels: u16, frame_size: usize) -> Result<Self, CodecError> {
        let opus_channels = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(CodecError::DecoderInit(
                format!("Unsupported channel count: {}", channels)
            )),
        };
        
        let decoder = Decoder::new(sample_rate, opus_channels)
            .map_err(|e| CodecError::DecoderInit(e.to_string()))?;
        
        // Pre-allocate decoding buffer for max frame size
        // 120ms at 48kHz stereo = 11520 samples
        let decode_buffer = vec![0.0f32; 48000 * 2 * 120 / 1000];
        
        Ok(Self {
            decoder,
            sample_rate,
            channels,
            frame_size,
            decode_buffer,
            frames_decoded: 0,
            frames_lost: 0,
            samples_produced: 0,
        })
    }
    
    /// Decode Opus packet to audio samples
    /// Returns interleaved f32 samples
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<f32>, CodecError> {
        let samples = self.decoder
            .decode_float(data, &mut self.decode_buffer, false)
            .map_err(|e| CodecError::DecodingFailed(e.to_string()))?;
        
        let total_samples = samples * self.channels as usize;
        self.frames_decoded += 1;
        self.samples_produced += total_samples as u64;
        
        Ok(self.decode_buffer[..total_samples].to_vec())
    }
    
    /// Decode with FEC (Forward Error Correction)
    /// Use when the previous packet was lost
    pub fn decode_fec(&mut self, data: &[u8]) -> Result<Vec<f32>, CodecError> {
        let samples = self.decoder
            .decode_float(data, &mut self.decode_buffer, true)
            .map_err(|e| CodecError::DecodingFailed(e.to_string()))?;
        
        let total_samples = samples * self.channels as usize;
        self.frames_decoded += 1;
        self.samples_produced += total_samples as u64;
        
        Ok(self.decode_buffer[..total_samples].to_vec())
    }
    
    /// Generate packet loss concealment samples
    /// Use when a packet is lost and no FEC is available
    pub fn decode_plc(&mut self) -> Result<Vec<f32>, CodecError> {
        let samples = self.decoder
            .decode_float(&[], &mut self.decode_buffer, false)
            .map_err(|e| CodecError::DecodingFailed(e.to_string()))?;
        
        let total_samples = samples * self.channels as usize;
        self.frames_lost += 1;
        self.samples_produced += total_samples as u64;
        
        Ok(self.decode_buffer[..total_samples].to_vec())
    }
    
    /// Reset decoder state
    pub fn reset(&mut self) -> Result<(), CodecError> {
        self.decoder.reset_state()
            .map_err(|e| CodecError::DecoderInit(e.to_string()))
    }
    
    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    
    /// Get channel count
    pub fn channels(&self) -> u16 {
        self.channels
    }
    
    /// Get frame size in samples (per channel)
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }
    
    /// Get statistics
    pub fn stats(&self) -> DecoderStats {
        DecoderStats {
            frames_decoded: self.frames_decoded,
            frames_lost: self.frames_lost,
            samples_produced: self.samples_produced,
            loss_rate: if self.frames_decoded + self.frames_lost > 0 {
                self.frames_lost as f32 / (self.frames_decoded + self.frames_lost) as f32
            } else {
                0.0
            },
        }
    }
    
    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.frames_decoded = 0;
        self.frames_lost = 0;
        self.samples_produced = 0;
    }
}

/// Decoder statistics
#[derive(Debug, Clone)]
pub struct DecoderStats {
    pub frames_decoded: u64,
    pub frames_lost: u64,
    pub samples_produced: u64,
    pub loss_rate: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::OpusEncoder;
    
    #[test]
    fn test_decoder_creation() {
        let decoder = OpusDecoder::new(48000, 2, 480);
        assert!(decoder.is_ok());
    }
    
    #[test]
    fn test_encode_decode_roundtrip() {
        let mut encoder = OpusEncoder::music(48000, 2).unwrap();
        let mut decoder = OpusDecoder::new(48000, 2, encoder.frame_size()).unwrap();
        
        // Create test samples (sine wave)
        let frame_size = encoder.samples_per_frame();
        let mut samples: Vec<f32> = Vec::with_capacity(frame_size);
        for i in 0..frame_size / 2 {
            let t = i as f32 / 48000.0;
            let val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
            samples.push(val); // Left
            samples.push(val); // Right
        }
        
        // Encode
        let encoded = encoder.encode(&samples).unwrap();
        
        // Decode
        let decoded = decoder.decode(&encoded).unwrap();
        
        // Verify we got the right number of samples back
        assert_eq!(decoded.len(), frame_size);
    }
    
    #[test]
    fn test_plc() {
        let mut decoder = OpusDecoder::new(48000, 2, 480).unwrap();
        
        // Generate PLC samples
        let plc_samples = decoder.decode_plc();
        assert!(plc_samples.is_ok());
        
        let stats = decoder.stats();
        assert_eq!(stats.frames_lost, 1);
    }
}
