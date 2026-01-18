//! Audio Sender Application
//!
//! Captures audio from multiple devices and streams to receiver over UDP.

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use lan_audio_streamer::{
    audio::{
        buffer::{create_shared_buffer},
        capture::AudioCapture,
        device::list_devices,
    },
    codec::OpusEncoder,
    config::{AppConfig, OpusConfig},
    constants::*,
    network::sender::{MultiTrackSender},
    protocol::{TrackConfig, TrackType},
    tracks::TrackManager,
    ui::WebServer,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    tracing::info!("Starting LAN Audio Sender");
    
    // Load or create config
    let config = AppConfig::default();
    
    // List available devices
    println!("\n=== Available Audio Devices ===");
    let devices = list_devices();
    for device in &devices {
        let device_type = match (device.is_input, device.is_output) {
            (true, true) => "Input/Output",
            (true, false) => "Input",
            (false, true) => "Output",
            _ => "Unknown",
        };
        let default_marker = if device.is_default { " [DEFAULT]" } else { "" };
        println!("  {} ({}){}:", device.name, device_type, default_marker);
        println!("    ID: {}", device.id);
        println!("    Sample rates: {:?}", device.sample_rates);
        println!("    Channels: {:?}", device.channels);
    }
    println!();
    
    // Create track manager
    let track_manager = Arc::new(TrackManager::new());
    
    // Start web UI
    let web_server = WebServer::new(
        config.ui.clone(),
        track_manager.clone(),
        true, // is_sender
    );
    let _web_handle = web_server.start_background();
    
    tracing::info!("Web UI available at http://{}:{}", config.ui.bind_address, config.ui.http_port);
    
    // Get target address from args or use default
    let target_addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:5000".to_string())
        .parse()
        .expect("Invalid target address");
    
    tracing::info!("Target receiver: {}", target_addr);
    
    // Create network sender
    let mut network_sender = MultiTrackSender::new(&config.network, target_addr)?;
    network_sender.start(config.network.clone())?;
    
    tracing::info!("Network sender started");
    
    // Example: Create a track from the default input device
    if let Some(input_device) = devices.iter().find(|d| d.is_input && d.is_default) {
        let track_config = TrackConfig {
            track_id: Some(0),
            name: format!("Default Input - {}", input_device.name),
            device_id: input_device.id.clone(),
            bitrate: 128_000,
            frame_size_ms: 10.0,
            channels: 2,
            track_type: TrackType::Music,
            fec_enabled: false,
        };
        
        let track_id = track_manager.create_track(track_config)?;
        tracing::info!("Created track {} for device {}", track_id, input_device.name);
        
        // Create capture buffer
        let capture_buffer = create_shared_buffer(RING_BUFFER_CAPACITY);
        
        // Create and start audio capture
        let mut capture = AudioCapture::new(
            track_id,
            &input_device.id,
            Some(DEFAULT_SAMPLE_RATE),
            Some(DEFAULT_CHANNELS),
            None,
            capture_buffer.clone(),
        )?;
        
        capture.start()?;
        tracing::info!("Audio capture started");
        
        // Create Opus encoder for this track
        let opus_config = OpusConfig::music();
        let mut encoder = OpusEncoder::new(opus_config)?;
        let frame_size = encoder.samples_per_frame();
        
        tracing::info!(
            "Opus encoder initialized: {}Hz, {} channels, {} samples/frame ({:.1}ms)",
            DEFAULT_SAMPLE_RATE,
            DEFAULT_CHANNELS,
            frame_size,
            encoder.frame_duration_ms()
        );
        
        // Main encoding/sending loop
        let mut sample_buffer: Vec<f32> = Vec::with_capacity(frame_size * 2);
        let mut sequence: u32 = 0;
        let start_time = Instant::now();
        
        tracing::info!("Starting main loop - press Ctrl+C to stop");
        
        loop {
            // Check for captured audio
            while let Some(frame) = capture_buffer.try_pop() {
                // Accumulate samples
                sample_buffer.extend_from_slice(&frame.samples);
                
                // Process complete frames
                while sample_buffer.len() >= frame_size {
                    let samples: Vec<f32> = sample_buffer.drain(..frame_size).collect();
                    
                    // Encode
                    match encoder.encode(&samples) {
                        Ok(encoded) => {
                            // Calculate timestamp
                            let timestamp = start_time.elapsed().as_micros() as u64;
                            
                            // Send over network
                            if let Err(e) = network_sender.send_audio(
                                track_id,
                                encoded,
                                timestamp,
                                DEFAULT_CHANNELS == 2,
                            ) {
                                tracing::warn!("Failed to send packet: {}", e);
                            }
                            
                            sequence = sequence.wrapping_add(1);
                        }
                        Err(e) => {
                            tracing::warn!("Encoding failed: {}", e);
                        }
                    }
                }
            }
            
            // Small sleep to prevent busy-waiting
            tokio::time::sleep(Duration::from_micros(500)).await;
            
            // Periodic stats logging
            if sequence > 0 && sequence % 1000 == 0 {
                let stats = encoder.stats();
                let sender_stats = network_sender.stats();
                tracing::info!(
                    "Stats: {} frames encoded, {} packets sent, {:.1} KB sent, avg frame {:.0} bytes",
                    stats.frames_encoded,
                    sender_stats.packets_sent,
                    sender_stats.bytes_sent as f64 / 1024.0,
                    stats.average_frame_size
                );
            }
        }
    } else {
        tracing::warn!("No input device found!");
        
        // Keep running for web UI
        tracing::info!("Running in UI-only mode. Configure tracks via web interface.");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
