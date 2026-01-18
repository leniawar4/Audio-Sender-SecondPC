//! HTTP API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use crate::audio::device::list_devices;
use crate::protocol::{
    AudioDeviceInfo, ControlMessage, TrackConfig, TrackConfigUpdate, TrackStatus,
};
use crate::ui::server::AppState;

/// API response wrapper
#[derive(serde::Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
    
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// System status
#[derive(serde::Serialize)]
pub struct SystemStatus {
    pub mode: String,
    pub track_count: usize,
    pub uptime_seconds: u64,
}

/// Get system status
pub async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<SystemStatus>> {
    let status = SystemStatus {
        mode: if state.is_sender { "sender" } else { "receiver" }.to_string(),
        track_count: state.track_manager.track_count(),
        uptime_seconds: 0, // TODO: Track uptime
    };
    
    Json(ApiResponse::ok(status))
}

/// Get available audio devices
pub async fn get_devices() -> Json<ApiResponse<Vec<AudioDeviceInfo>>> {
    let devices = list_devices();
    Json(ApiResponse::ok(devices))
}

/// Get all tracks
pub async fn get_tracks(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<TrackStatus>>> {
    let tracks = state.track_manager.get_all_statuses();
    Json(ApiResponse::ok(tracks))
}

/// Create a new track
pub async fn create_track(
    State(state): State<Arc<AppState>>,
    Json(config): Json<TrackConfig>,
) -> (StatusCode, Json<ApiResponse<u8>>) {
    match state.track_manager.create_track(config) {
        Ok(id) => {
            // Broadcast creation
            let _ = state.control_tx.send(ControlMessage::CreateTrack(
                state.track_manager.get_track(id)
                    .map(|t| t.config.clone())
                    .unwrap_or_default()
            ));
            
            (StatusCode::CREATED, Json(ApiResponse::ok(id)))
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Delete a track
pub async fn delete_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.remove_track(id) {
        Ok(_) => {
            let _ = state.control_tx.send(ControlMessage::RemoveTrack { track_id: id });
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::NOT_FOUND, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Update a track
pub async fn update_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
    Json(update): Json<TrackConfigUpdate>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.update_track(id, update.clone()) {
        Ok(_) => {
            let _ = state.control_tx.send(ControlMessage::UpdateTrack {
                track_id: id,
                config: update,
            });
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Set track mute state
#[derive(serde::Deserialize)]
pub struct MuteRequest {
    pub muted: bool,
}

pub async fn set_mute(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
    Json(req): Json<MuteRequest>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.set_muted(id, req.muted) {
        Ok(_) => {
            let _ = state.control_tx.send(ControlMessage::SetMute {
                track_id: id,
                muted: req.muted,
            });
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::NOT_FOUND, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Set track solo state
#[derive(serde::Deserialize)]
pub struct SoloRequest {
    pub solo: bool,
}

pub async fn set_solo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
    Json(req): Json<SoloRequest>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.set_solo(id, req.solo) {
        Ok(_) => {
            let _ = state.control_tx.send(ControlMessage::SetSolo {
                track_id: id,
                solo: req.solo,
            });
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::NOT_FOUND, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Start a track
pub async fn start_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.start_track(id) {
        Ok(_) => {
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Stop a track
pub async fn stop_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match state.track_manager.stop_track(id) {
        Ok(_) => {
            (StatusCode::OK, Json(ApiResponse::ok(())))
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e.to_string())))
        }
    }
}
