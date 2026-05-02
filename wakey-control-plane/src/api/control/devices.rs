use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::api::json_error;
use crate::runtime::AppState;
use crate::state::{DeviceIdentifierInput, KnownDeviceInput};

#[derive(Debug, Deserialize)]
pub struct CreateKnownDeviceRequest {
    pub display_name: String,
    #[serde(default)]
    pub pinned: bool,
    pub notes: Option<String>,
    #[serde(default)]
    pub identifiers: Vec<DeviceIdentifierRequest>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceIdentifierRequest {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnownDeviceResponse {
    pub device_id: String,
    pub display_name: String,
    pub pinned: bool,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub notes: Option<String>,
    pub identifiers: Vec<DeviceIdentifierResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceIdentifierResponse {
    pub identifier_key: String,
    pub device_id: String,
    pub kind: String,
    pub value: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForgetKnownDeviceResponse {
    pub device_id: String,
    pub forgotten: bool,
}

#[derive(Debug, Deserialize)]
pub struct MergeKnownDeviceRequest {
    pub source_device_id: String,
}

pub async fn create_known_device(
    State(state): State<AppState>,
    Json(req): Json<CreateKnownDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let input = KnownDeviceInput {
        display_name: req.display_name,
        pinned: req.pinned,
        notes: req.notes,
        identifiers: req
            .identifiers
            .into_iter()
            .map(|identifier| DeviceIdentifierInput {
                kind: identifier.kind,
                value: identifier.value,
            })
            .collect(),
    };

    match state.store.create_known_device(input).await {
        Ok(device) => Ok((StatusCode::CREATED, Json(known_device_response(device)))),
        Err(err) => {
            warn!(error = %err, "failed to create known device");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "create_known_device_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn list_known_devices(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.list_known_devices().await {
        Ok(devices) => Ok((
            StatusCode::OK,
            Json(
                devices
                    .into_iter()
                    .map(known_device_response)
                    .collect::<Vec<_>>(),
            ),
        )),
        Err(err) => {
            warn!(error = %err, "failed to list known devices");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_known_devices_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn forget_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.forget_known_device(&device_id).await {
        Ok(forgotten) => Ok((
            StatusCode::OK,
            Json(ForgetKnownDeviceResponse {
                device_id,
                forgotten,
            }),
        )),
        Err(err) => {
            warn!(error = %err, "failed to forget known device");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "forget_known_device_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn attach_device_identifier(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<DeviceIdentifierRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let input = DeviceIdentifierInput {
        kind: req.kind,
        value: req.value,
    };

    match state
        .store
        .attach_device_identifier(&device_id, input)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to attach device identifier");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "attach_device_identifier_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn detach_device_identifier(
    State(state): State<AppState>,
    AxumPath((device_id, identifier_key)): AxumPath<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state
        .store
        .detach_device_identifier(&device_id, &identifier_key)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to detach device identifier");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "detach_device_identifier_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn merge_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<MergeKnownDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state
        .store
        .merge_known_devices(&device_id, &req.source_device_id)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "target or source known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to merge known devices");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "merge_known_device_failed",
                &err.to_string(),
            ))
        }
    }
}

fn known_device_response(device: crate::state::KnownDevice) -> KnownDeviceResponse {
    KnownDeviceResponse {
        device_id: device.device_id,
        display_name: device.display_name,
        pinned: device.pinned,
        created_at_unix: device.created_at_unix,
        updated_at_unix: device.updated_at_unix,
        notes: device.notes,
        identifiers: device
            .identifiers
            .into_iter()
            .map(|identifier| DeviceIdentifierResponse {
                identifier_key: identifier.identifier_key,
                device_id: identifier.device_id,
                kind: identifier.kind,
                value: identifier.value,
                created_at_unix: identifier.created_at_unix,
            })
            .collect(),
    }
}
