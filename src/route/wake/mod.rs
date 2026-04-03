//! impls are at [`utils::wake::impl`](crate::utils::wake::r#impl) for some reason
use std::io;
use std::net::IpAddr;

/* use crate::arpparse::IpNeighLine;
use crate::route::api::Status; */
use crate::utils::wake::wake_one;
use crate::{route::error::ApiError, utils::parse::mac};
use axum::{extract::Json, http::StatusCode, response::IntoResponse};
use futures::TryFutureExt;
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::net::UdpSocket;

#[derive(Debug, Default, Serialize, Clone)]
pub struct WakeResult {
    pub result: Vec<WakeTargetResult>,
}
#[skip_serializing_none]
#[derive(Debug, Serialize, Clone, Copy)]
pub struct WakeTargetResult {
    #[serde(flatten)]
    pub target: WakeTarget,
    pub status: WakeTargetStatus,
}

#[derive(Debug, Serialize, Clone, Copy, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WakeTargetStatus {
    Succeed,
    /// not a real address...
    NonexistentAddress,
    WrongSize,
    /// input is not enough
    Incomplete,
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct WakeTarget {
    #[serde(default)]
    pub ip: Option<IpAddr>,
    #[serde(default, with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
}

pub async fn wake_multi(Json(req): Json<Vec<WakeTarget>>) -> impl IntoResponse {
    match wake_multi_split(req).await {
        Ok(result) => (StatusCode::OK, Json(WakeResult { result })).into_response(),
        Err(error) => {
            let error = format!("Error: {}", error);
            ApiError::ise(error).into_response()
        }
    }
}

/// this is so bad
pub async fn wake_multi_split(
    targets: impl IntoIterator<Item = WakeTarget>,
) -> io::Result<Vec<WakeTargetResult>> {
    let sock = UdpSocket::bind("[::]:0")
        .or_else(|_| UdpSocket::bind(":0"))
        .await?;
    sock.set_broadcast(true)?;

    let iter = targets.into_iter().map(async |c| {
        if c.is_incomplete() {
            c.to_incomplete()
        } else {
            let t = c.try_into().expect("complete struct failed to try_into");
            wake_one(&sock, t).await.into()
        }
    });
    Ok(futures::future::join_all(iter).await)
}
/* #[derive(Debug, Serialize)]
pub struct WakeStatusLine {
    #[serde(flatten)]
    pub status: IpNeighLine, // most powerful find of the century
    pub wake_status: WakeTargetStatus,
}
pub type WakeStatus = Status<WakeStatusLine>; */
// /// return status BUT plus a indicator of i sent a wake.
// pub async fn wake_status(
//     Query(DeviceQuery {
//         name,
//         ip,
//         dev,
//         nud,
//         mac,
//         ..
//     }): Query<DeviceQuery>,
// ) -> impl IntoResponse {
// }
pub mod impls;
