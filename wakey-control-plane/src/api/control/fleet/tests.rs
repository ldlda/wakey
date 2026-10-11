use std::collections::HashMap;
use std::net::IpAddr;

use macaddr::MacAddr;
use wakey_core::{DeviceEndpoint, EndpointKey, EndpointSource, Presence};

use super::build::{
    AgentRuntimeStatus, FleetBuildContext, build_fleet_devices, filter_fleet_devices,
    known_device_summary,
};
use super::normalize_refresh_agent_ids;
use super::types::ListFleetDevicesQuery;
use crate::state::{AgentDeviceRow, AgentDeviceWithChildren, DeviceIdentifier, KnownDevice};

#[test]
fn cache_rebuild_limit_is_retryable_but_db_failures_keep_their_error_code() {
    let busy = super::fleet_error(
        crate::runtime::view_cache::RebuildLimit.into(),
        "list_fleet_devices_failed",
    );
    assert_eq!(busy.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(busy.code, "fleet_busy");
    let failed = super::fleet_error(
        anyhow::anyhow!("database failed"),
        "list_fleet_devices_failed",
    );
    assert_eq!(failed.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(failed.code, "list_fleet_devices_failed");
}

#[tokio::test]
async fn fleet_cache_tracks_store_edits_and_connections_without_caching_filters() {
    use crate::runtime::{AgentSession, AppState};
    use crate::state::{DeviceIdentifierInput, KnownDeviceInput, Store};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock, mpsc};

    let dir = std::env::temp_dir().join(format!("wakey-fleet-cache-{}", uuid::Uuid::new_v4()));
    let state = AppState {
        store: Arc::new(
            Store::load_or_init(&dir.join("state.sqlite3"), vec![], Duration::from_secs(60))
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
        fleet: Arc::new(super::FleetCache::default()),
    };
    let store = &state.store;
    assert_eq!(
        super::sample_fleet_counts(&state).await.unwrap(),
        Default::default()
    );
    let token = store
        .issue_enroll_token(Duration::from_secs(60))
        .await
        .unwrap();
    let agent = store.enroll(&token.enroll_token).await.unwrap();
    let empty = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!(empty.agents[&agent.agent_id].devices, 0);
    assert_eq!(empty.agents[&agent.agent_id].endpoints, 0);
    let observed = wakey_core::Device::from_parts(
        vec![wakey_core::NeighborEntry {
            ip: "192.168.1.10".parse().unwrap(),
            mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
            dev: Some("br-lan".into()),
            state: wakey_core::NeighborState::Reachable,
        }],
        vec![],
    );
    store
        .replace_agent_device_snapshot(&agent.agent_id, &[observed])
        .await
        .unwrap();
    let initial = super::fleet_view(&state).await.unwrap();
    assert_eq!(initial.len(), 1);
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!(counts.agents[&agent.agent_id].devices, 1);
    assert_eq!(counts.agents[&agent.agent_id].endpoints, 1);
    assert_eq!((counts.saved, counts.unsaved), (0, 1));
    assert!(Arc::ptr_eq(
        &initial,
        &super::fleet_view(&state).await.unwrap()
    ));
    assert!(initial[0].recommended_route.is_none());

    let (tx, _rx) = mpsc::unbounded_channel();
    state.sessions.write().await.insert(
        agent.agent_id.clone(),
        AgentSession {
            connection_id: "test".into(),
            tx,
            capabilities: vec![],
            capability_options: Default::default(),
        },
    );
    let connected = super::fleet_view(&state).await.unwrap();
    assert!(connected[0].recommended_route.is_some());
    store
        .set_agent_nickname(&agent.agent_id, Some("router"))
        .await
        .unwrap();
    assert_eq!(
        super::fleet_view(&state).await.unwrap()[0].agents[0]
            .nickname
            .as_deref(),
        Some("router")
    );

    let saved = store
        .create_known_device(KnownDeviceInput {
            display_name: "my pc".into(),
            pinned: true,
            notes: None,
            identifiers: vec![DeviceIdentifierInput {
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:01".into(),
            }],
        })
        .await
        .unwrap();
    assert_eq!(
        super::fleet_view(&state).await.unwrap()[0].display_name,
        "my pc"
    );
    store
        .rename_known_device(&saved.device_id, "renamed pc")
        .await
        .unwrap();
    let before_error = super::fleet_view(&state).await.unwrap();
    assert!(
        store
            .rename_known_device(&saved.device_id, " ")
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &before_error,
        &super::fleet_view(&state).await.unwrap()
    ));
    assert_eq!(
        super::fleet_view(&state).await.unwrap()[0].display_name,
        "renamed pc"
    );
    let query = ListFleetDevicesQuery {
        query: Some("does-not-exist".into()),
        visibility: Some("all".into()),
        ..Default::default()
    };
    assert!(
        super::load_fleet_devices(&state, &query)
            .await
            .unwrap()
            .is_empty()
    );
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!((counts.saved, counts.unsaved), (1, 0));
    let all = ListFleetDevicesQuery {
        visibility: Some("all".into()),
        ..Default::default()
    };
    assert_eq!(
        super::load_fleet_devices(&state, &all).await.unwrap().len(),
        1
    );

    store
        .detach_device_identifier(&saved.device_id, &saved.identifiers[0].identifier_key)
        .await
        .unwrap();
    assert_eq!(super::fleet_view(&state).await.unwrap().len(), 2);
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!((counts.saved, counts.unsaved), (1, 1));
    store
        .attach_device_identifier(
            &saved.device_id,
            DeviceIdentifierInput {
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:01".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(super::fleet_view(&state).await.unwrap().len(), 1);
    let other = store
        .create_known_device(KnownDeviceInput {
            display_name: "other".into(),
            pinned: false,
            notes: None,
            identifiers: vec![],
        })
        .await
        .unwrap();
    assert_eq!(super::fleet_view(&state).await.unwrap().len(), 2);
    let limited = ListFleetDevicesQuery {
        visibility: Some("all".into()),
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(
        super::load_fleet_devices(&state, &limited)
            .await
            .unwrap()
            .len(),
        1
    );
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!((counts.saved, counts.unsaved), (2, 0));
    assert_eq!(
        super::load_fleet_devices(&state, &all).await.unwrap().len(),
        2
    );
    store
        .merge_known_devices(&saved.device_id, &other.device_id)
        .await
        .unwrap();
    assert_eq!(super::fleet_view(&state).await.unwrap().len(), 1);
    store.forget_known_device(&saved.device_id).await.unwrap();
    assert!(
        super::fleet_view(&state).await.unwrap()[0]
            .known_device
            .is_none()
    );

    state.sessions.write().await.clear();
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!(counts.agents[&agent.agent_id].devices, 1);
    assert_eq!((counts.saved, counts.unsaved), (0, 1));
    assert!(
        super::fleet_view(&state).await.unwrap()[0]
            .recommended_route
            .is_none()
    );
    let before_reload = super::fleet_view(&state).await.unwrap();
    store.reload_from_disk().await.unwrap();
    assert!(!Arc::ptr_eq(
        &before_reload,
        &super::fleet_view(&state).await.unwrap()
    ));
    store.revoke_agent(&agent.agent_id).await.unwrap();
    assert!(
        super::fleet_view(&state).await.unwrap()[0].agents[0]
            .nickname
            .is_none()
    );
    store
        .replace_agent_device_snapshot(&agent.agent_id, &[])
        .await
        .unwrap();
    assert!(super::fleet_view(&state).await.unwrap().is_empty());
    assert_eq!(
        super::sample_fleet_counts(&state).await.unwrap(),
        Default::default()
    );
    // Identical devices on two agents count twice as stored rows, once as a fleet identity.
    // Offline endpoints are retained too; these metrics are not reachability counters.
    let offline = wakey_core::Device::from_parts(
        vec![wakey_core::NeighborEntry {
            ip: "192.168.1.10".parse().unwrap(),
            mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
            dev: Some("br-lan".into()),
            state: wakey_core::NeighborState::Failed,
        }],
        vec![],
    );
    for id in ["agent-a", "agent-b"] {
        store
            .replace_agent_device_snapshot(id, std::slice::from_ref(&offline))
            .await
            .unwrap();
    }
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert_eq!(counts.agents.len(), 2);
    for count in counts.agents.values() {
        assert_eq!(count.devices, 1);
        assert_eq!(count.endpoints, 1);
    }
    assert_eq!((counts.saved, counts.unsaved), (0, 1));
    store
        .replace_agent_device_snapshot("agent-a", &[])
        .await
        .unwrap();
    let counts = super::sample_fleet_counts(&state).await.unwrap();
    assert!(!counts.agents.contains_key("agent-a"));
    assert_eq!(counts.agents["agent-b"].devices, 1);
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refresh_agent_selection_keeps_all_agents_beyond_previous_cap() {
    let ids = (0..140)
        .map(|index| format!("agent-{index:03}"))
        .collect::<Vec<_>>();

    let selected = normalize_refresh_agent_ids(ids);

    assert_eq!(selected.len(), 140);
    assert_eq!(selected.first().map(String::as_str), Some("agent-000"));
    assert_eq!(selected.last().map(String::as_str), Some("agent-139"));
}

fn context(connected: &[&str]) -> FleetBuildContext {
    FleetBuildContext {
        agent_status: connected
            .iter()
            .map(|agent| {
                (
                    (*agent).to_string(),
                    AgentRuntimeStatus {
                        nickname: None,
                        connected: true,
                    },
                )
            })
            .collect(),
        identifier_map: HashMap::new(),
    }
}

fn agent_device(
    agent_id: &str,
    device_key: &str,
    mac: Option<&str>,
    ip: Option<&str>,
    last_seen_unix: i64,
) -> AgentDeviceWithChildren {
    AgentDeviceWithChildren {
        device: AgentDeviceRow {
            agent_id: agent_id.into(),
            device_key: device_key.into(),
            presence: "likely_online".into(),
            display_name: Some("lda".into()),
            first_seen_unix: 1,
            last_seen_unix,
        },
        macs: mac
            .map(|m| m.parse::<MacAddr>().unwrap())
            .into_iter()
            .collect(),
        ips: ip
            .map(|i| i.parse::<IpAddr>().unwrap())
            .into_iter()
            .collect(),
        hostnames: vec!["lda".to_string()],
        endpoints: vec![],
        facts: vec![],
    }
}

fn offline_agent_device(
    agent_id: &str,
    device_key: &str,
    mac: Option<&str>,
    ip: Option<&str>,
    last_seen_unix: i64,
) -> AgentDeviceWithChildren {
    AgentDeviceWithChildren {
        device: AgentDeviceRow {
            agent_id: agent_id.into(),
            device_key: device_key.into(),
            presence: "offline".into(),
            display_name: Some("lda".into()),
            first_seen_unix: 1,
            last_seen_unix,
        },
        macs: mac
            .map(|m| m.parse::<MacAddr>().unwrap())
            .into_iter()
            .collect(),
        ips: ip
            .map(|i| i.parse::<IpAddr>().unwrap())
            .into_iter()
            .collect(),
        hostnames: vec!["lda".to_string()],
        endpoints: vec![],
        facts: vec![],
    }
}

fn offline_ip_only_unknown(
    agent_id: &str,
    device_key: &str,
    ip: &str,
    last_seen_unix: i64,
) -> AgentDeviceWithChildren {
    AgentDeviceWithChildren {
        device: AgentDeviceRow {
            agent_id: agent_id.into(),
            device_key: device_key.into(),
            presence: "offline".into(),
            display_name: None,
            first_seen_unix: 1,
            last_seen_unix,
        },
        macs: vec![],
        ips: vec![ip.parse().unwrap()],
        hostnames: vec![],
        endpoints: vec![],
        facts: vec![],
    }
}

fn endpoint(
    source: EndpointSource,
    mac: Option<&str>,
    ip: Option<&str>,
    last_seen_unix: u64,
) -> DeviceEndpoint {
    endpoint_with_presence(source, mac, ip, Presence::LikelyOnline, last_seen_unix)
}

fn endpoint_with_presence(
    source: EndpointSource,
    mac: Option<&str>,
    ip: Option<&str>,
    presence: Presence,
    last_seen_unix: u64,
) -> DeviceEndpoint {
    DeviceEndpoint {
        key: EndpointKey::new(
            source,
            mac.map(|m| m.parse::<MacAddr>().unwrap()),
            ip.map(|i| i.parse::<IpAddr>().unwrap()),
        )
        .expect("endpoint key"),
        hostname: Some("lda".into()),
        interface: Some("br-lan".into()),
        presence,
        first_seen_unix: Some(1),
        last_seen_unix: Some(last_seen_unix),
    }
}

#[test]
fn fleet_grouping_combines_same_mac_across_agents() {
    let devices = build_fleet_devices(
        Vec::new(),
        vec![
            agent_device(
                "agent-a",
                "mac:aa:bb:cc:dd:ee:ff",
                Some("aa:bb:cc:dd:ee:ff"),
                Some("192.168.1.2"),
                10,
            ),
            agent_device(
                "agent-b",
                "mac:aa:bb:cc:dd:ee:ff",
                Some("aa:bb:cc:dd:ee:ff"),
                Some("192.168.2.2"),
                20,
            ),
        ],
        &context(&["agent-a", "agent-b"]),
    );

    let expected_mac: MacAddr = "aa:bb:cc:dd:ee:ff".parse().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].macs, vec![expected_mac]);
    assert_eq!(devices[0].agents.len(), 2);
    assert_eq!(
        devices[0]
            .recommended_route
            .as_ref()
            .map(|route| route.agent_id.as_str()),
        Some("agent-b")
    );
}

#[test]
fn fleet_routes_use_endpoint_mac_ip_pairs() {
    let mac = "aa:bb:cc:dd:ee:ff";
    let mut row = agent_device(
        "agent-a",
        "mac:aa:bb:cc:dd:ee:ff",
        Some(mac),
        Some("192.168.1.2"),
        20,
    );
    row.endpoints = vec![
        endpoint(
            EndpointSource::HookNeighbor,
            Some(mac),
            Some("192.168.1.2"),
            10,
        ),
        endpoint(EndpointSource::Neighbor, Some(mac), Some("192.168.1.3"), 20),
    ];

    let devices = build_fleet_devices(Vec::new(), vec![row], &context(&["agent-a"]));

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].endpoints.len(), 2);
    assert_eq!(devices[0].route_candidates.len(), 2);
    assert_eq!(
        devices[0]
            .recommended_route
            .as_ref()
            .and_then(|route| route.ip),
        Some("192.168.1.3".parse().unwrap())
    );
}

#[test]
fn fleet_summary_keeps_current_dhcp_ip_when_neighbor_endpoint_is_offline() {
    let mac = "aa:bb:cc:dd:ee:ff";
    let mut row = offline_agent_device(
        "agent-a",
        "mac:aa:bb:cc:dd:ee:ff",
        Some(mac),
        Some("192.168.1.99"),
        20,
    );
    row.ips = vec![
        "192.168.1.10".parse().unwrap(),
        "192.168.1.99".parse().unwrap(),
    ];
    row.endpoints = vec![
        endpoint_with_presence(
            EndpointSource::Neighbor,
            Some(mac),
            Some("192.168.1.99"),
            Presence::Offline,
            20,
        ),
        endpoint_with_presence(
            EndpointSource::DhcpLease,
            Some(mac),
            Some("192.168.1.10"),
            Presence::Unknown,
            20,
        ),
    ];

    let devices = build_fleet_devices(Vec::new(), vec![row], &context(&["agent-a"]));

    assert_eq!(devices.len(), 1);
    assert_eq!(
        devices[0].ips,
        vec!["192.168.1.10".parse::<IpAddr>().unwrap()]
    );
}

#[test]
fn known_ip_identifier_matches_endpoint_that_is_not_in_summary_ips() {
    let known = KnownDevice {
        device_id: "dev-1".into(),
        display_name: "lda remembered".into(),
        pinned: true,
        created_at_unix: 1,
        updated_at_unix: 1,
        notes: None,
        identifiers: vec![DeviceIdentifier {
            identifier_key: "ip:192.168.1.99".into(),
            device_id: "dev-1".into(),
            kind: "ip".into(),
            value: "192.168.1.99".into(),
            created_at_unix: 1,
        }],
    };
    let mut ctx = context(&["agent-a"]);
    ctx.identifier_map
        .insert("ip:192.168.1.99".into(), known_device_summary(&known));
    let mut row = agent_device("agent-a", "ip:192.168.1.99", None, None, 20);
    row.endpoints = vec![endpoint_with_presence(
        EndpointSource::HookNeighbor,
        None,
        Some("192.168.1.99"),
        Presence::Offline,
        20,
    )];

    let devices = build_fleet_devices(vec![known], vec![row], &ctx);

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_key, "known:dev-1");
    assert_eq!(devices[0].endpoints.len(), 1);
}

#[test]
fn equivalent_endpoint_routes_collapse_to_best_source() {
    let mac = "aa:bb:cc:dd:ee:ff";
    let ip = "192.168.1.2";
    let mut row = agent_device("agent-a", "mac:aa:bb:cc:dd:ee:ff", Some(mac), Some(ip), 20);
    row.endpoints = vec![
        endpoint_with_presence(
            EndpointSource::HookNeighbor,
            Some(mac),
            Some(ip),
            Presence::LikelyOnline,
            20,
        ),
        endpoint_with_presence(
            EndpointSource::Neighbor,
            Some(mac),
            Some(ip),
            Presence::Online,
            20,
        ),
    ];

    let devices = build_fleet_devices(Vec::new(), vec![row], &context(&["agent-a"]));

    assert_eq!(devices[0].endpoints.len(), 2);
    assert_eq!(devices[0].route_candidates.len(), 1);
    assert_eq!(devices[0].route_candidates[0].source, "neighbor");
    assert_eq!(devices[0].route_candidates[0].presence, Presence::Online);
}

#[test]
fn known_device_with_two_macs_absorbs_both_device_groups() {
    let known = KnownDevice {
        device_id: "dev-1".into(),
        display_name: "lda".into(),
        pinned: true,
        created_at_unix: 1,
        updated_at_unix: 1,
        notes: None,
        identifiers: vec![
            DeviceIdentifier {
                identifier_key: "mac:aa:bb:cc:dd:ee:01".into(),
                device_id: "dev-1".into(),
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:01".into(),
                created_at_unix: 1,
            },
            DeviceIdentifier {
                identifier_key: "mac:aa:bb:cc:dd:ee:02".into(),
                device_id: "dev-1".into(),
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:02".into(),
                created_at_unix: 1,
            },
        ],
    };
    let mut ctx = context(&["agent-a"]);
    for identifier in &known.identifiers {
        ctx.identifier_map.insert(
            identifier.identifier_key.clone(),
            known_device_summary(&known),
        );
    }

    let devices = build_fleet_devices(
        vec![known],
        vec![
            agent_device(
                "agent-a",
                "mac:aa:bb:cc:dd:ee:01",
                Some("aa:bb:cc:dd:ee:01"),
                None,
                10,
            ),
            agent_device(
                "agent-a",
                "mac:aa:bb:cc:dd:ee:02",
                Some("aa:bb:cc:dd:ee:02"),
                None,
                20,
            ),
        ],
        &ctx,
    );

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_key, "known:dev-1");
    assert_eq!(devices[0].macs.len(), 2);
}

#[test]
fn ip_only_unknown_is_visible_but_not_wakeable() {
    let devices = build_fleet_devices(
        Vec::new(),
        vec![agent_device(
            "agent-a",
            "ip:192.168.1.2",
            None,
            Some("192.168.1.2"),
            10,
        )],
        &context(&["agent-a"]),
    );

    let expected_ip: IpAddr = "192.168.1.2".parse().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].ips, vec![expected_ip]);
    assert!(devices[0].recommended_route.is_none());
}

#[test]
fn offline_device_has_offline_presence() {
    let devices = build_fleet_devices(
        Vec::new(),
        vec![offline_agent_device(
            "agent-a",
            "mac:aa:bb:cc:dd:ee:ff",
            Some("aa:bb:cc:dd:ee:ff"),
            Some("192.168.1.2"),
            20,
        )],
        &context(&["agent-a"]),
    );

    assert_eq!(devices.len(), 1);
    assert!(devices[0].ips.is_empty());
    assert_eq!(devices[0].presence, Presence::Offline);
    assert!(devices[0].recommended_route.is_some());
}

#[test]
fn unknown_ip_only_offline_device_is_hidden_by_default() {
    let mut devices = build_fleet_devices(
        Vec::new(),
        vec![offline_ip_only_unknown(
            "agent-a",
            "ip:192.168.1.2",
            "192.168.1.2",
            20,
        )],
        &context(&["agent-a"]),
    );

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].display_name, "Offline device: 192.168.1.2");
    filter_fleet_devices(&mut devices, &ListFleetDevicesQuery::default());
    assert!(devices.is_empty());
}

#[test]
fn endpoint_presence_overrides_conflicting_cached_summary() {
    for connected in [false, true] {
        for (cached, evidence) in [
            (Presence::Unknown, Presence::Offline),
            (Presence::Online, Presence::Unknown),
            (Presence::Offline, Presence::Online),
        ] {
            let mut row = offline_ip_only_unknown("agent-a", "ip:192.168.1.2", "192.168.1.2", 20);
            row.device.presence = cached.as_str().into();
            let mut endpoint = endpoint_with_presence(
                EndpointSource::Neighbor,
                None,
                Some("192.168.1.2"),
                evidence,
                20,
            );
            endpoint.hostname = None;
            row.endpoints = vec![endpoint];
            let ctx = if connected {
                context(&["agent-a"])
            } else {
                context(&[])
            };
            let devices = build_fleet_devices(Vec::new(), vec![row], &ctx);
            assert_eq!(devices[0].presence, evidence);
            assert_eq!(devices[0].endpoints[0].presence, evidence);
            assert_eq!(devices[0].agents[0].connected, connected);
            if evidence == Presence::Offline {
                assert_eq!(devices[0].display_name, "Offline device: 192.168.1.2");
                assert!(devices[0].ips.is_empty());
            }
        }
    }
}

#[test]
fn unnamed_endpoint_labels_preserve_presence_and_inactive_ips() {
    for (presence, mac, ip, expected) in [
        (
            Presence::Offline,
            None,
            Some("192.168.1.2"),
            "Offline device: 192.168.1.2",
        ),
        (
            Presence::Unknown,
            None,
            Some("192.168.1.2"),
            "Device: 192.168.1.2",
        ),
        (
            Presence::Offline,
            Some("aa:bb:cc:dd:ee:01"),
            None,
            "Offline device: AA:BB:CC:DD:EE:01",
        ),
    ] {
        let mut row = offline_ip_only_unknown("agent-a", "test", "192.168.1.2", 20);
        row.ips.clear();
        row.device.presence = presence.as_str().into();
        let mut evidence = endpoint_with_presence(EndpointSource::Neighbor, mac, ip, presence, 20);
        evidence.hostname = None;
        row.endpoints = vec![evidence];
        let devices = build_fleet_devices(Vec::new(), vec![row], &context(&["agent-a"]));
        assert_eq!(devices[0].display_name, expected);
        assert_eq!(devices[0].presence, presence);
        if presence == Presence::Offline {
            assert!(devices[0].ips.is_empty());
        }
    }
}

#[test]
fn visibility_all_keeps_unknown_ip_only_offline_device() {
    let mut devices = build_fleet_devices(
        Vec::new(),
        vec![offline_ip_only_unknown(
            "agent-a",
            "ip:192.168.1.2",
            "192.168.1.2",
            20,
        )],
        &context(&["agent-a"]),
    );

    filter_fleet_devices(
        &mut devices,
        &ListFleetDevicesQuery {
            visibility: Some("all".into()),
            ..Default::default()
        },
    );
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].display_name, "Offline device: 192.168.1.2");
}

#[test]
fn known_ip_only_offline_device_is_kept() {
    let known = KnownDevice {
        device_id: "dev-1".into(),
        display_name: "lda".into(),
        pinned: true,
        created_at_unix: 1,
        updated_at_unix: 1,
        notes: None,
        identifiers: vec![DeviceIdentifier {
            identifier_key: "ip:192.168.1.2".into(),
            device_id: "dev-1".into(),
            kind: "ip".into(),
            value: "192.168.1.2".into(),
            created_at_unix: 1,
        }],
    };
    let mut ctx = context(&["agent-a"]);
    ctx.identifier_map
        .insert("ip:192.168.1.2".into(), known_device_summary(&known));

    let devices = build_fleet_devices(
        vec![known],
        vec![offline_agent_device(
            "agent-a",
            "ip:192.168.1.2",
            None,
            Some("192.168.1.2"),
            20,
        )],
        &ctx,
    );

    let expected_ip: IpAddr = "192.168.1.2".parse().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_key, "known:dev-1");
    assert_eq!(devices[0].display_name, "lda");
    assert!(devices[0].ips.contains(&expected_ip));
}
