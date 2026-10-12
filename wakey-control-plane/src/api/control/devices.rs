use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::api::ApiError;
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

#[derive(Debug, Deserialize)]
pub struct RenameKnownDeviceRequest {
    pub display_name: String,
}

pub async fn rename_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<RenameKnownDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // The store validates too, but checking here distinguishes bad input from
    // storage failures without interpreting anyhow error messages.
    if req.display_name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "rename_known_device_failed",
            "display_name must not be empty",
        ));
    }
    match state
        .store
        .rename_known_device(&device_id, &req.display_name)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to rename known device");
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "rename_known_device_failed",
                err.to_string(),
            ))
        }
    }
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
) -> Result<impl IntoResponse, ApiError> {
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
            Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "create_known_device_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn list_known_devices(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
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
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_known_devices_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn get_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    match state.store.get_known_device(&device_id).await {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to get known device");
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "get_known_device_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn forget_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<impl IntoResponse, ApiError> {
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
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "forget_known_device_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn attach_device_identifier(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<DeviceIdentifierRequest>,
) -> Result<impl IntoResponse, ApiError> {
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
        Ok(None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to attach device identifier");
            Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "attach_device_identifier_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn detach_device_identifier(
    State(state): State<AppState>,
    AxumPath((device_id, identifier_key)): AxumPath<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    match state
        .store
        .detach_device_identifier(&device_id, &identifier_key)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to detach device identifier");
            Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "detach_device_identifier_failed",
                err.to_string(),
            ))
        }
    }
}

pub async fn merge_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<MergeKnownDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    match state
        .store
        .merge_known_devices(&device_id, &req.source_device_id)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "target or source known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to merge known devices");
            Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "merge_known_device_failed",
                err.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock};

    #[tokio::test]
    async fn rename_distinguishes_invalid_names_missing_devices_and_storage_failures() {
        let dir = std::env::temp_dir().join(format!("wakey-rename-api-{}", uuid::Uuid::new_v4()));
        let store = Arc::new(
            crate::state::Store::load_or_init(
                &dir.join("state.sqlite3"),
                vec![],
                Duration::from_secs(60),
            )
            .await
            .unwrap(),
        );
        let state = AppState {
            store: store.clone(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            public_url: "http://localhost".into(),
            command_timeout: Duration::from_secs(30),
            enroll_token_ttl: Duration::from_secs(60),
            terminals: crate::runtime::terminals::TerminalRegistry::new(),
            alerts: crate::api::AlertsHub::new(),
            fleet: Arc::new(crate::api::FleetCache::default()),
        };
        let device = store
            .create_known_device(KnownDeviceInput {
                display_name: "before".into(),
                pinned: false,
                notes: None,
                identifiers: vec![],
            })
            .await
            .unwrap();
        let rename = |id: String, name: &str| {
            rename_known_device(
                State(state.clone()),
                AxumPath(id),
                Json(RenameKnownDeviceRequest {
                    display_name: name.into(),
                }),
            )
        };
        let response = rename(device.device_id.clone(), " after ")
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store
                .get_known_device(&device.device_id)
                .await
                .unwrap()
                .unwrap()
                .display_name,
            "after"
        );
        assert_eq!(
            rename("missing".into(), "valid")
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            rename(device.device_id.clone(), " \t\n ")
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::BAD_REQUEST
        );
        // A deterministic SQLite failure, not a clock-sensitive lock timeout.
        let mut tx = store.begin_write().await.unwrap();
        sqlx::query("CREATE TRIGGER reject_rename BEFORE UPDATE ON known_devices BEGIN SELECT RAISE(ABORT, 'test rename failure'); END")
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let failure = rename(device.device_id.clone(), "valid")
            .await
            .err()
            .unwrap();
        assert_eq!(failure.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(failure.code, "rename_known_device_failed");
        assert_eq!(
            store
                .get_known_device(&device.device_id)
                .await
                .unwrap()
                .unwrap()
                .display_name,
            "after"
        );
        assert_eq!(
            rename(device.device_id, " ").await.err().unwrap().status,
            StatusCode::BAD_REQUEST
        );
        drop(state);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
