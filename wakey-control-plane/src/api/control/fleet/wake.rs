use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use macaddr::MacAddr;
use tracing::{Instrument, info, warn};
use wakey_agent::protocol::{AgentCommand, WakeRequest};

use super::types::{FleetDevice, FleetWakeRoute, WakeFleetDeviceRequest, WakeFleetDeviceResponse};
use super::{fleet_error, fleet_view};
use crate::api::{ApiError, commands::relay_agent_command};
use crate::runtime::AppState;

#[tracing::instrument(name = "fleet_wake", skip_all, fields(explicit_route = req.route_id.is_some(), outcome = tracing::field::Empty))]
pub async fn wake_fleet_device(
    State(state): State<AppState>,
    Json(req): Json<WakeFleetDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Early database/relay errors retain an outcome even without a command result.
    tracing::Span::current().record("outcome", "error");
    let selected = load_selected_route(&state, &req.device_key, req.route_id.as_deref()).await?;
    info!(agent_id = %selected.route.agent_id, has_ip = selected.route.ip.is_some(),
        explicit_route = req.route_id.is_some(), "fleet wake route selected");
    let response = dispatch_wake(&state, selected, req.timeout_ms).await?;
    tracing::Span::current().record("outcome", response.command.status.as_str());
    Ok((StatusCode::OK, Json(response)))
}

/// List visibility and pagination are presentation policy, not wake eligibility.
/// Borrow the complete cached view and clone only the selected route.
async fn load_selected_route(
    state: &AppState,
    device_key: &str,
    route_id: Option<&str>,
) -> Result<SelectedRoute, ApiError> {
    let span = tracing::info_span!("fleet_load", grouped = tracing::field::Empty);
    let devices = fleet_view(state)
        .instrument(span.clone())
        .await
        .map_err(|err| {
            warn!(error = %err, "failed loading fleet devices for wake");
            fleet_error(err, "load_fleet_devices_failed")
        })?;
    span.record("grouped", devices.len());
    select_route(&devices, device_key, route_id).map_err(|error| {
        tracing::Span::current().record("outcome", error.outcome());
        error.into_api_error()
    })
}

/// Selection does not rank routes: the fleet builder already chose the recommendation.
/// An explicit route never falls back, including when it is disconnected or IP-only.
fn select_route(
    devices: &[FleetDevice],
    device_key: &str,
    route_id: Option<&str>,
) -> Result<SelectedRoute, RouteError> {
    let device = devices
        .iter()
        .find(|device| device.device_key == device_key)
        .ok_or(RouteError::DeviceNotFound)?;
    let route = match route_id {
        Some(id) => device
            .route_candidates
            .iter()
            .find(|route| route.route_id == id),
        None => device.recommended_route.as_ref(),
    }
    .ok_or(RouteError::Unavailable)?;
    let mac = route.mac.ok_or(RouteError::MissingMac)?;
    if !route.connected {
        return Err(RouteError::Disconnected);
    }
    if !route.wakeable {
        return Err(RouteError::NotWakeable);
    }
    Ok(SelectedRoute {
        route: route.clone(),
        mac,
    })
}

/// Only the selector constructs this; dispatch cannot accidentally use an IP-only route.
#[derive(Debug)]
struct SelectedRoute {
    route: FleetWakeRoute,
    mac: MacAddr,
}

impl SelectedRoute {
    fn command(&self) -> AgentCommand {
        AgentCommand::Wake(WakeRequest {
            query: None,
            mac: Some(self.mac),
            ip: self.route.ip,
        })
    }
}

async fn dispatch_wake(
    state: &AppState,
    selected: SelectedRoute,
    timeout_ms: Option<u64>,
) -> Result<WakeFleetDeviceResponse, ApiError> {
    // The relay checks the current agent connection again; cached connectivity
    // is only selection evidence, not a guarantee that the agent is still there.
    let command = relay_agent_command(
        state,
        &selected.route.agent_id,
        selected.command(),
        timeout_ms,
    )
    .await?;
    Ok(WakeFleetDeviceResponse {
        route: selected.route,
        command,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum RouteError {
    DeviceNotFound,
    Unavailable,
    MissingMac,
    Disconnected,
    NotWakeable,
}

impl RouteError {
    fn outcome(&self) -> &'static str {
        match self {
            Self::DeviceNotFound => "device_not_found",
            Self::Unavailable => "route_unavailable",
            Self::MissingMac => "route_without_mac",
            Self::Disconnected => "route_disconnected",
            Self::NotWakeable => "route_not_wakeable",
        }
    }

    fn into_api_error(self) -> ApiError {
        let message = match self {
            Self::DeviceNotFound => {
                return ApiError::new(
                    StatusCode::NOT_FOUND,
                    "fleet_device_not_found",
                    "fleet device not found",
                );
            }
            Self::Unavailable => "no wakeable connected MAC-backed route is available",
            Self::MissingMac => "selected route does not include a MAC address",
            Self::Disconnected => "selected route agent is not connected",
            Self::NotWakeable => "selected route is not wakeable",
        };
        ApiError::new(StatusCode::BAD_REQUEST, "wake_route_unavailable", message)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{load_fleet_devices, types::ListFleetDevicesQuery};
    use super::*;
    use wakey_core::Presence;

    #[tokio::test]
    async fn wake_lookup_can_select_a_device_beyond_the_list_limit() {
        use std::collections::{BTreeSet, HashMap};
        use std::sync::Arc;
        use std::time::Duration;
        use tokio::sync::{Mutex, RwLock};

        let dir = std::env::temp_dir().join(format!("wakey-wake-limit-{}", uuid::Uuid::new_v4()));
        let state = AppState {
            store: Arc::new(
                crate::state::Store::load_or_init(
                    &dir.join("state.sqlite3"),
                    vec![],
                    Duration::from_secs(60),
                )
                .await
                .unwrap(),
            ),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            public_url: "http://localhost".into(),
            command_timeout: Duration::from_secs(30),
            enroll_token_ttl: Duration::from_secs(60),
            terminals: crate::runtime::terminals::TerminalRegistry::new(),
            alerts: crate::api::AlertsHub::new(),
            fleet: Arc::new(super::super::FleetCache::default()),
        };
        // Seed the complete cached view, as the builder would. Exercise both
        // production read paths without thousands of unrelated SQL inserts.
        state
            .fleet
            .get_or_try_init(
                || async { (state.store.fleet_revision(), BTreeSet::new()) },
                || async {
                    Ok((0..1001)
                        .map(|i| {
                            let mut row = device(vec![route("valid")], Some(route("valid")));
                            row.device_key = format!("pc-{i}");
                            row
                        })
                        .collect())
                },
            )
            .await
            .unwrap();

        let listed = load_fleet_devices(
            &state,
            &ListFleetDevicesQuery {
                visibility: Some("all".into()),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed.len(), 1000);
        assert!(listed.iter().all(|row| row.device_key != "pc-1000"));
        {
            use axum::extract::Path;
            let details =
                super::super::get_fleet_device(State(state.clone()), Path("pc-1000".into()))
                    .await
                    .unwrap()
                    .into_response();
            assert_eq!(details.status(), StatusCode::OK);
            let body = axum::body::to_bytes(details.into_body(), usize::MAX)
                .await
                .unwrap();
            let row: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(row["device_key"], "pc-1000");
        }
        for route_id in [None, Some("valid")] {
            let selected = load_selected_route(&state, "pc-1000", route_id)
                .await
                .unwrap();
            assert_eq!(selected.route.route_id, "valid");
        }
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn route(id: &str) -> FleetWakeRoute {
        FleetWakeRoute {
            route_id: id.into(),
            agent_id: "agent-a".into(),
            nickname: None,
            connected: true,
            mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
            ip: Some("192.168.1.10".parse().unwrap()),
            hostname: None,
            interface: None,
            source: "neighbor".into(),
            presence: Presence::Offline,
            last_seen_unix: 1,
            wakeable: true,
        }
    }

    fn device(candidates: Vec<FleetWakeRoute>, recommended: Option<FleetWakeRoute>) -> FleetDevice {
        FleetDevice {
            device_key: "pc".into(),
            display_name: "pc".into(),
            known_device: None,
            pinned: false,
            ips: vec![],
            macs: vec![],
            hostnames: vec![],
            agents: vec![],
            sources: vec![],
            endpoints: vec![],
            first_seen_unix: None,
            last_seen_unix: None,
            presence: Presence::Offline,
            route_candidates: candidates,
            recommended_route: recommended,
        }
    }

    #[test]
    fn selection_uses_recommendation_or_exact_override_and_preserves_address_pair() {
        let first = route("first");
        let mut recommended = route("recommended");
        recommended.agent_id = "agent-b".into();
        recommended.ip = Some("192.168.2.20".parse().unwrap());
        let devices = [device(vec![first, recommended.clone()], Some(recommended))];
        for (override_id, route_id, agent, ip) in [
            (None, "recommended", "agent-b", "192.168.2.20"),
            (Some("first"), "first", "agent-a", "192.168.1.10"),
        ] {
            let selected = select_route(&devices, "pc", override_id).unwrap();
            assert_eq!(selected.route.route_id, route_id);
            assert_eq!(selected.route.agent_id, agent);
            let AgentCommand::Wake(command) = selected.command() else {
                panic!("not a wake command")
            };
            assert!(command.query.is_none());
            assert_eq!(command.mac, selected.route.mac);
            assert_eq!(command.ip, Some(ip.parse().unwrap()));
        }
    }

    #[test]
    fn missing_device_and_missing_routes_keep_distinct_errors() {
        let valid = route("valid");
        let devices = [device(vec![valid.clone()], Some(valid))];
        assert_eq!(
            select_route(&devices, "missing", None).unwrap_err(),
            RouteError::DeviceNotFound
        );
        assert_eq!(
            select_route(&devices, "pc", Some("missing")).unwrap_err(),
            RouteError::Unavailable
        );
        let devices = [device(vec![route("valid")], None)];
        assert_eq!(
            select_route(&devices, "pc", None).unwrap_err(),
            RouteError::Unavailable
        );
    }

    #[test]
    fn invalid_explicit_routes_do_not_fall_back_and_validation_order_is_preserved() {
        for (mac, connected, wakeable, expected) in [
            (None, false, false, RouteError::MissingMac),
            (
                Some("aa:bb:cc:dd:ee:01"),
                false,
                false,
                RouteError::Disconnected,
            ),
            (
                Some("aa:bb:cc:dd:ee:01"),
                true,
                false,
                RouteError::NotWakeable,
            ),
        ] {
            let mut invalid = route("invalid");
            invalid.mac = mac.map(|value| value.parse().unwrap());
            invalid.connected = connected;
            invalid.wakeable = wakeable;
            let devices = [device(vec![invalid, route("valid")], Some(route("valid")))];
            assert_eq!(
                select_route(&devices, "pc", Some("invalid")).unwrap_err(),
                expected
            );
        }
    }

    #[test]
    fn mac_only_offline_route_is_still_wakeable() {
        let mut mac_only = route("mac-only");
        mac_only.ip = None;
        let devices = [device(vec![mac_only.clone()], Some(mac_only))];
        let selected = select_route(&devices, "pc", None).unwrap();
        let AgentCommand::Wake(command) = selected.command() else {
            panic!("not a wake command")
        };
        assert!(command.mac.is_some());
        assert!(command.ip.is_none());
    }

    #[test]
    fn route_errors_preserve_http_codes_messages_and_trace_outcomes() {
        for (error, status, code, message, outcome) in [
            (
                RouteError::DeviceNotFound,
                StatusCode::NOT_FOUND,
                "fleet_device_not_found",
                "fleet device not found",
                "device_not_found",
            ),
            (
                RouteError::Unavailable,
                StatusCode::BAD_REQUEST,
                "wake_route_unavailable",
                "no wakeable connected MAC-backed route is available",
                "route_unavailable",
            ),
            (
                RouteError::MissingMac,
                StatusCode::BAD_REQUEST,
                "wake_route_unavailable",
                "selected route does not include a MAC address",
                "route_without_mac",
            ),
            (
                RouteError::Disconnected,
                StatusCode::BAD_REQUEST,
                "wake_route_unavailable",
                "selected route agent is not connected",
                "route_disconnected",
            ),
            (
                RouteError::NotWakeable,
                StatusCode::BAD_REQUEST,
                "wake_route_unavailable",
                "selected route is not wakeable",
                "route_not_wakeable",
            ),
        ] {
            assert_eq!(error.outcome(), outcome);
            let response = error.into_api_error();
            assert_eq!(response.status, status);
            assert_eq!(response.code, code);
            assert_eq!(response.message, message);
        }
    }
}
