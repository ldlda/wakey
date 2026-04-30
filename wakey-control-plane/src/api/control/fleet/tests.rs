use std::collections::HashMap;

use super::build::{
    AgentRuntimeStatus, FleetBuildContext, build_fleet_devices, filter_fleet_devices,
    known_device_summary,
};
use super::inventory::inventory_result_to_observations;
use super::types::ListFleetDevicesQuery;
use crate::state::{AgentDeviceObservation, DeviceIdentifier, KnownDevice};

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

fn observation(
    agent_id: &str,
    mac: Option<&str>,
    ip: Option<&str>,
    last_seen_unix: u64,
) -> AgentDeviceObservation {
    AgentDeviceObservation {
        observation_key: format!(
            "agent:{agent_id}:dhcp:{}",
            mac.map(|mac| format!("mac:{mac}"))
                .or_else(|| ip.map(|ip| format!("ip:{ip}")))
                .unwrap_or_default()
        ),
        agent_id: agent_id.into(),
        kind: "dhcp".into(),
        mac: mac.map(str::to_string),
        ip: ip.map(str::to_string),
        hostname: Some("lda".into()),
        first_seen_unix: 1,
        last_seen_unix,
        last_action: "update".into(),
    }
}

#[test]
fn fleet_grouping_combines_same_mac_across_agents() {
    let devices = build_fleet_devices(
        Vec::new(),
        vec![
            observation(
                "agent-a",
                Some("aa:bb:cc:dd:ee:ff"),
                Some("192.168.1.2"),
                10,
            ),
            observation(
                "agent-b",
                Some("aa:bb:cc:dd:ee:ff"),
                Some("192.168.2.2"),
                20,
            ),
        ],
        &context(&["agent-a", "agent-b"]),
    );

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].macs, vec!["aa:bb:cc:dd:ee:ff"]);
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
fn known_device_with_two_macs_absorbs_both_observation_groups() {
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
            observation("agent-a", Some("aa:bb:cc:dd:ee:01"), None, 10),
            observation("agent-a", Some("aa:bb:cc:dd:ee:02"), None, 20),
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
        vec![observation("agent-a", None, Some("192.168.1.2"), 10)],
        &context(&["agent-a"]),
    );

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].ips, vec!["192.168.1.2"]);
    assert!(devices[0].recommended_route.is_none());
    assert!(!devices[0].route_candidates[0].wakeable);
}

#[test]
fn offline_observation_does_not_advertise_current_ip_or_wake_route() {
    let mut offline = observation(
        "agent-a",
        Some("aa:bb:cc:dd:ee:ff"),
        Some("192.168.1.2"),
        20,
    );
    offline.kind = "neigh".into();
    offline.last_action = "remove".into();

    let devices = build_fleet_devices(Vec::new(), vec![offline], &context(&["agent-a"]));

    assert_eq!(devices.len(), 1);
    assert!(devices[0].ips.is_empty());
    assert_eq!(devices[0].presence, "offline");
    assert!(devices[0].recommended_route.is_none());
    assert!(!devices[0].route_candidates[0].wakeable);
}

#[test]
fn unknown_ip_only_remove_observation_is_hidden() {
    let mut offline = observation("agent-a", None, Some("192.168.1.2"), 20);
    offline.kind = "neigh".into();
    offline.hostname = None;
    offline.last_action = "remove".into();

    let mut devices = build_fleet_devices(Vec::new(), vec![offline], &context(&["agent-a"]));

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].display_name, "(unknown device)");
    filter_fleet_devices(&mut devices, &ListFleetDevicesQuery::default());
    assert!(devices.is_empty());
}

#[test]
fn visibility_all_keeps_unknown_ip_only_remove_observation() {
    let mut offline = observation("agent-a", None, Some("192.168.1.2"), 20);
    offline.kind = "neigh".into();
    offline.hostname = None;
    offline.last_action = "remove".into();

    let mut devices = build_fleet_devices(Vec::new(), vec![offline], &context(&["agent-a"]));

    filter_fleet_devices(
        &mut devices,
        &ListFleetDevicesQuery {
            visibility: Some("all".into()),
            ..Default::default()
        },
    );
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].display_name, "(unknown device)");
}

#[test]
fn known_ip_only_remove_observation_is_kept() {
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
    let mut offline = observation("agent-a", None, Some("192.168.1.2"), 20);
    offline.kind = "neigh".into();
    offline.hostname = None;
    offline.last_action = "remove".into();

    let devices = build_fleet_devices(vec![known], vec![offline], &ctx);

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_key, "known:dev-1");
    assert_eq!(devices[0].display_name, "lda");
    assert!(devices[0].ips.contains(&"192.168.1.2".to_string()));
}

#[test]
fn inventory_result_maps_to_stored_observations() {
    let observations = inventory_result_to_observations(serde_json::json!({
        "kind": "inventory",
        "devices": [{
            "names": ["lda"],
            "ips": ["192.168.1.2"],
            "macs": ["aa:bb:cc:dd:ee:ff"],
            "presence": "likely_online"
        }]
    }))
    .expect("inventory should map");

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].kind, "inventory");
    assert_eq!(observations[0].action, "update");
    assert_eq!(observations[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
}

#[test]
fn inventory_result_preserves_neighbor_failed_ip_as_remove() {
    let observations = inventory_result_to_observations(serde_json::json!({
        "kind": "inventory",
        "devices": [{
            "names": ["lda"],
            "ips": ["192.168.1.2", "192.168.1.3"],
            "macs": ["aa:bb:cc:dd:ee:ff"],
            "neighbors": [
                {
                    "ip": "192.168.1.2",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "state": "FAILED"
                },
                {
                    "ip": "192.168.1.3",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "state": "REACHABLE"
                }
            ],
            "presence": "online"
        }]
    }))
    .expect("inventory should map");

    assert_eq!(observations.len(), 2);
    let removed = observations
        .iter()
        .find(|observation| observation.ip.as_deref() == Some("192.168.1.2"))
        .expect("failed neighbor observation should exist");
    assert_eq!(removed.action, "remove");
    assert_eq!(removed.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));

    let current = observations
        .iter()
        .find(|observation| observation.ip.as_deref() == Some("192.168.1.3"))
        .expect("reachable neighbor observation should exist");
    assert_eq!(current.action, "update");
    assert_eq!(current.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
}

#[test]
fn inventory_result_preserves_hook_observations_and_leases() {
    let observations = inventory_result_to_observations(serde_json::json!({
        "kind": "inventory",
        "devices": [{
            "names": ["lda"],
            "ips": ["192.168.1.2", "192.168.1.3"],
            "macs": ["aa:bb:cc:dd:ee:ff"],
            "observations": [{
                "kind": "neigh",
                "action": "remove",
                "mac": "aa:bb:cc:dd:ee:ff",
                "ip": "192.168.1.2"
            }],
            "leases": [{
                "expires_epoch": 1893456000_u64,
                "ip": "192.168.1.3",
                "mac": "aa:bb:cc:dd:ee:ff",
                "name": "lda"
            }],
            "presence": "likely_online"
        }]
    }))
    .expect("inventory should map");

    assert_eq!(observations.len(), 2);
    assert!(observations.iter().any(|observation| {
        observation.action == "remove"
            && observation.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff")
            && observation.ip.as_deref() == Some("192.168.1.2")
    }));
    assert!(observations.iter().any(|observation| {
        observation.action == "update"
            && observation.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff")
            && observation.ip.as_deref() == Some("192.168.1.3")
            && observation.hostname.as_deref() == Some("lda")
    }));
}
