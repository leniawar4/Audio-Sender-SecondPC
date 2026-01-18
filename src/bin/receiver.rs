//! Audio Receiver Application
//!
//! Receives audio streams from sender and outputs to virtual devices.

use anyhow::Result;
use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use lan_audio_streamer::{
    audio::{
        buffer::{AudioFrame, JitterBuffer},
        device::list_devices,
        playback::NetworkPlayback,
    },
    codec::OpusDecoder,
    config::{AppConfig},
    constants::*,
    network::receiver::{AudioReceiver, ReceivedPacket},
    protocol::TrackConfig,
    tracks::TrackManager,
    ui::WebServer,
};

/// Per-track receiver state
struct TrackState {
    decoder: OpusDecoder,
    jitter_buffer: JitterBuffer,
    playback: Option<NetworkPlayback>,
    packets_received: u64,
    packets_lost: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    tracing::info!("Starting LAN Audio Receiver");
    
    // Load or create config
    let config = AppConfig::default();
    
    // List available output devices
    println!("\n=== Available Output Devices ===");
    let devices = list_devices();
    for device in &devices {
        if device.is_output {
            let default_marker = if device.is_default { " [DEFAULT]" } else { "" };
            println!("  {}{}:", device.name, default_marker);
            println!("    ID: {}", device.id);
            println!("    Sample rates: {:?}", device.sample_rates);
            println!("    Channels: {:?}", device.channels);
        }
    }
    println!();
    
    // Create track manager
    let track_manager = Arc::new(TrackManager::new());
    
    // Start web UI
    let web_server = WebServer::new(
        config.ui.clone(),
        track_manager.clone(),
        false, // is_receiver
    );
    let _web_handle = web_server.start_background();
    
    tracing::info!("Web UI available at http://{}:{}", config.ui.bind_address, config.ui.http_port);
    
    // Create packet receiver channel
    let (packet_tx, packet_rx) = bounded::<ReceivedPacket>(4096);
    
    // Create and start network receiver
    let mut receiver = AudioReceiver::new();
    receiver.set_global_channel(packet_tx);
    receiver.start(config.network.clone())?;
    
    tracing::info!("Network receiver started on port {}", config.network.udp_port);
    
    // Track states
    let mut track_states: HashMap<u8, TrackState> = HashMap::new();
    
    // Get default output device
    let default_output = devices.iter()
        .find(|d| d.is_output && d.is_default)
        .map(|d| d.id.clone())
        .unwrap_or_default();
    
    tracing::info!("Default output device: {}", default_output);
    tracing::info!("Waiting for audio streams...");
    
    // Main receiving loop
    let mut last_stats_time = std::time::Instant::now();
    
    loop {
        // Process received packets
        while let Ok(packet) = packet_rx.try_recv() {
            let track_id = packet.track_id;
            
            // Initialize track state if new
            if !track_states.contains_key(&track_id) {
                tracing::info!("New track {} detected, initializing...", track_id);
                
                // Determine channel count from packet
                let channels = if packet.is_stereo { 2 } else { 1 };
                
                // Create decoder
                let frame_size = (DEFAULT_SAMPLE_RATE as f32 * DEFAULT_FRAME_SIZE_MS / 1000.0) as usize;
                let decoder = match OpusDecoder::new(DEFAULT_SAMPLE_RATE, channels, frame_size) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Failed to create decoder for track {}: {}", track_id, e);
                        continue;
                    }
                };
                
                // Create jitter buffer (32 slots, 2 frame minimum delay)
                let jitter_buffer = JitterBuffer::new(32, 2);
                
                // Create playback (optional - may not have output device)
                let playback = if !default_output.is_empty() {
                    match NetworkPlayback::new(
                        track_id,
                        &default_output,
                        Some(DEFAULT_SAMPLE_RATE),
                        Some(channels),
                        32, // jitter buffer size
                        2,  // min delay
                    ) {
                        Ok(mut p) => {
                            if let Err(e) = p.start() {
                                tracing::warn!("Failed to start playback for track {}: {}", track_id, e);
                                None
                            } else {
                                tracing::info!("Started playback for track {} on {}", track_id, default_output);
                                Some(p)
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to create playback for track {}: {}", track_id, e);
                            None
                        }
                    }
                } else {
                    None
                };
                
                // Create track in manager
                let track_config = TrackConfig {
                    track_id: Some(track_id),
                    name: format!("Track {}", track_id),
                    device_id: default_output.clone(),
                    bitrate: DEFAULT_BITRATE,
                    frame_size_ms: DEFAULT_FRAME_SIZE_MS,
                    channels,
                    ..Default::default()
                };
                let _ = track_manager.create_track(track_config);
                
                track_states.insert(track_id, TrackState {
                    decoder,
                    jitter_buffer,
                    playback,
                    packets_received: 0,
                    packets_lost: 0,
                });
            }
            
            // Process packet
            if let Some(state) = track_states.get_mut(&track_id) {
                state.packets_received += 1;
                
                // Decode audio
                match state.decoder.decode(&packet.payload) {
                    Ok(samples) => {
                        // Create audio frame
                        let frame = AudioFrame::new(
                            samples,
                            state.decoder.channels(),
                            packet.timestamp,
                            packet.sequence,
                        );
                        
                        // Insert into jitter buffer
                        state.jitter_buffer.insert(frame.clone());
                        
                        // Push to playback if available
                        if let Some(ref playback) = state.playback {
                            playback.push_frame(frame);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Decode error on track {}: {}", track_id, e);
                        state.packets_lost += 1;
                    }
                }
            }
        }
        
        // Process jitter buffers and feed playback
        for (_, state) in &mut track_states {
            if let Some(ref playback) = state.playback {
                // Process jitter buffer
                playback.process();
            }
        }
        
        // Periodic stats
        if last_stats_time.elapsed() >= Duration::from_secs(5) {
            last_stats_time = std::time::Instant::now();
            
            let recv_stats = receiver.stats();
            tracing::info!(
                "Receiver stats: {} packets, {} bytes, {} invalid",
                recv_stats.packets_received,
                recv_stats.bytes_received,
                recv_stats.invalid_packets
            );
            
            for (track_id, state) in &track_states {
                let jitter_stats = state.jitter_buffer.stats();
                tracing::info!(
                    "Track {} stats: {} received, {} lost ({:.1}% loss), jitter buffer: {}/{}",
                    track_id,
                    state.packets_received,
                    state.packets_lost,
                    jitter_stats.loss_rate() * 100.0,
                    jitter_stats.level,
                    jitter_stats.capacity
                );
            }
        }
        
        // Small sleep to prevent busy-waiting
        tokio::time::sleep(Duration::from_micros(500)).await;
    }
}
