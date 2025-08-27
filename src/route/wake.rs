//! impls are at [`utils::wake::impl`](crate::utils::wake::r#impl) for some reason
use std::io;
use std::net::IpAddr;

use crate::utils::parse::mac::{des_opm, ser_opm};
use crate::utils::wake::wake_one;
use axum::{extract::Json, http::StatusCode, response::IntoResponse};
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Debug, Default, Serialize, Clone)]
pub struct WakeResult {
    pub success: bool,
    pub result: Option<Vec<WakeTargetResult>>,
    pub error: Option<String>,
}
#[skip_serializing_none]
#[derive(Debug, Serialize, Clone, Copy)]
pub struct WakeTargetResult {
    pub ip: Option<IpAddr>,
    #[serde(serialize_with = "ser_opm")]
    pub mac: Option<MacAddr>,
    pub status: WakeTargetStatus,
}

#[derive(Debug, Serialize, Clone, Copy, Hash)]
#[serde(rename_all="snake_case")]
pub enum WakeTargetStatus {
    Succeed,
    /// not a real address...
    NonexistentAddress,
    WrongSize,
    /// input is not enough
    Incomplete,
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct WakeTarget {
    pub ip: Option<IpAddr>,
    #[serde(deserialize_with = "des_opm")]
    pub mac: Option<MacAddr>,
}

pub async fn wake_multi(Json(req): Json<Vec<WakeTarget>>) -> impl IntoResponse {
    match wake_multi_split(req).await {
        Ok(results) => (
            StatusCode::OK,
            Json(WakeResult {
                success: true,
                result: Some(results),
                ..Default::default()
            }),
        ),
        Err(error) => {
            let error = Some(format!("Error: {}", error));
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WakeResult {
                    success: false,
                    error,
                    ..Default::default()
                }),
            )
        }
    }
}

/// this is so bad
pub async fn wake_multi_split(
    targets: impl IntoIterator<Item = WakeTarget>,
) -> io::Result<Vec<WakeTargetResult>> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;

    Ok(
        futures::future::join_all(targets.into_iter().map(async |c| {
            if c.is_incomplete() {
                c.to_incomplete()
            } else {
                wake_one(
                    &sock,
                    c.try_into().expect("complete struct failed to try_into"),
                )
                .await
                .into()
            }
        }))
        .await,
    )
}
