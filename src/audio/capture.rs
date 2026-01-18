//! Audio capture from input devices
//!
//! Handles capturing audio from multiple devices simultaneously,
//! each running in its own dedicated thread for low latency.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::StreamConfig;
use crossbeam_channel::{bounded, Receiver};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::audio::buffer::{AudioFrame, SharedRingBuffer};
use crate::audio::device::get_device_by_id;
use crate::constants::DEFAULT_SAMPLE_RATE;
use crate::error::AudioError;

/// Audio capture instance for a single device
pub struct AudioCapture {
    /// Track ID this capture belongs to
    track_id: u8,
    
    /// Device identifier
    device_id: String,
    
    /// Whether capture is running
    running: Arc<AtomicBool>,
    
    /// Output buffer for captured frames
    output_buffer: SharedRingBuffer,
    
    /// Stream thread handle
    thread_handle: Option<JoinHandle<()>>,
    
    /// Channel for stream errors
    error_rx: Option<Receiver<AudioError>>,
    
    /// Current sequence number
    sequence: Arc<AtomicU32>,
    
    /// Total samples captured
    samples_captured: Arc<AtomicU64>,
    
    /// Stream configuration
    config: StreamConfig,
    
    /// Start time for timestamps
    start_time: Instant,
}

impl AudioCapture {
    /// Create a new audio capture for the specified device
    pub fn new(
        track_id: u8,
        device_id: &str,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        buffer_size: Option<u32>,
        output_buffer: SharedRingBuffer,
    ) -> Result<Self, AudioError> {
        let device = get_device_by_id(device_id)?;
        
        // Get default config and override with requested settings
        let default_config = device.default_input_config()?;
        
        let config = StreamConfig {
            channels: channels.unwrap_or(default_config.channels()),
            sample_rate: cpal::SampleRate(sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE)),
            buffer_size: match buffer_size {
                Some(size) => cpal::BufferSize::Fixed(size),
                None => cpal::BufferSize::Default,
            },
        };
        
        Ok(Self {
            track_id,
            device_id: device_id.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            output_buffer,
            thread_handle: None,
            error_rx: None,
            sequence: Arc::new(AtomicU32::new(0)),
            samples_captured: Arc::new(AtomicU64::new(0)),
            config,
            start_time: Instant::now(),
        })
    }
    
    /// Start capturing audio
    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        let device = get_device_by_id(&self.device_id)?;
        let (error_tx, error_rx) = bounded::<AudioError>(16);
        self.error_rx = Some(error_rx);
        
        let running = self.running.clone();
        let running_for_loop = self.running.clone();
        let output_buffer = self.output_buffer.clone();
        let sequence = self.sequence.clone();
        let samples_captured = self.samples_captured.clone();
        let config = self.config.clone();
        let channels = self.config.channels;
        let _sample_rate = self.config.sample_rate.0;
        
        // Reset counters
        self.sequence.store(0, Ordering::SeqCst);
        self.samples_captured.store(0, Ordering::SeqCst);
        self.start_time = Instant::now();
        let start_time = self.start_time;
        
        running.store(true, Ordering::SeqCst);
        
        let handle = thread::Builder::new()
            .name(format!("capture-track-{}", self.track_id))
            .spawn(move || {
                let cpal_device = device.into_inner();
                
                let stream = cpal_device.build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if !running.load(Ordering::Relaxed) {
                            return;
                        }
                        
                        // Calculate timestamp
                        let elapsed = start_time.elapsed();
                        let timestamp = elapsed.as_micros() as u64;
                        
                        // Get sequence number
                        let seq = sequence.fetch_add(1, Ordering::Relaxed);
                        
                        // Update sample count
                        samples_captured.fetch_add(data.len() as u64, Ordering::Relaxed);
                        
                        // Create frame and push to buffer
                        let frame = AudioFrame::new(
                            data.to_vec(),
                            channels,
                            timestamp,
                            seq,
                        );
                        
                        // Push to ring buffer (may fail on overflow)
                        let _ = output_buffer.push(frame);
                    },
                    move |err| {
                        let _ = error_tx.try_send(AudioError::StreamError(err.to_string()));
                    },
                    None,
                );
                
                match stream {
                    Ok(stream) => {
                        if let Err(e) = stream.play() {
                            tracing::error!("Failed to start stream: {}", e);
                            return;
                        }
                        
                        // Keep thread alive while running
                        while running_for_loop.load(Ordering::Relaxed) {
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        
                        // Stream is dropped here, stopping capture
                    }
                    Err(e) => {
                        tracing::error!("Failed to build stream: {}", e);
                    }
                }
            })
            .map_err(|e| AudioError::StreamError(e.to_string()))?;
        
        self.thread_handle = Some(handle);
        Ok(())
    }
    
    /// Stop capturing audio
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
    
    /// Check if capture is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    
    /// Get current sequence number
    pub fn current_sequence(&self) -> u32 {
        self.sequence.load(Ordering::Relaxed)
    }
    
    /// Get total samples captured
    pub fn samples_captured(&self) -> u64 {
        self.samples_captured.load(Ordering::Relaxed)
    }
    
    /// Get the stream configuration
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }
    
    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }
    
    /// Get channel count
    pub fn channels(&self) -> u16 {
        self.config.channels
    }
    
    /// Check for errors
    pub fn check_errors(&self) -> Option<AudioError> {
        self.error_rx.as_ref().and_then(|rx| rx.try_recv().ok())
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Multi-device capture manager
pub struct MultiCapture {
    captures: Vec<AudioCapture>,
}

impl MultiCapture {
    pub fn new() -> Self {
        Self {
            captures: Vec::new(),
        }
    }
    
    /// Add a capture for a device
    pub fn add_capture(&mut self, capture: AudioCapture) {
        self.captures.push(capture);
    }
    
    /// Remove a capture by track ID
    pub fn remove_capture(&mut self, track_id: u8) -> Option<AudioCapture> {
        if let Some(pos) = self.captures.iter().position(|c| c.track_id == track_id) {
            Some(self.captures.remove(pos))
        } else {
            None
        }
    }
    
    /// Start all captures
    pub fn start_all(&mut self) -> Result<(), AudioError> {
        for capture in &mut self.captures {
            capture.start()?;
        }
        Ok(())
    }
    
    /// Stop all captures
    pub fn stop_all(&mut self) {
        for capture in &mut self.captures {
            capture.stop();
        }
    }
    
    /// Get capture by track ID
    pub fn get_capture(&self, track_id: u8) -> Option<&AudioCapture> {
        self.captures.iter().find(|c| c.track_id == track_id)
    }
    
    /// Get mutable capture by track ID
    pub fn get_capture_mut(&mut self, track_id: u8) -> Option<&mut AudioCapture> {
        self.captures.iter_mut().find(|c| c.track_id == track_id)
    }
}

impl Default for MultiCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::buffer::create_shared_buffer;
    
    #[test]
    fn test_capture_creation() {
        // This test will only pass if there's an audio device available
        let buffer = create_shared_buffer(64);
        
        // Try to create capture with default device
        // This may fail on CI/systems without audio devices
        let devices = crate::audio::device::list_devices();
        if let Some(device) = devices.iter().find(|d| d.is_input) {
            let capture = AudioCapture::new(
                0,
                &device.id,
                Some(48000),
                Some(2),
                None,
                buffer,
            );
            
            // Just check creation succeeds
            assert!(capture.is_ok() || devices.is_empty());
        }
    }
}
