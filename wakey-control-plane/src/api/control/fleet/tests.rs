use std::collections::HashMap;
use std::net::IpAddr;

use macaddr::MacAddr;
use wakey_core::{DeviceEndpoint, EndpointKey, EndpointSource, Presence};

use super::build::{
    AgentRuntimeStatus, FleetBuildContext, build_fleet_devices, filter_fleet_devices,
    known_device_summary,
};
use super::types::ListFleetDevicesQuery;
use crate::state::{AgentDeviceRow, AgentDeviceWithChildren, DeviceIdentifier, KnownDevice};

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
    assert_eq!(devices[0].display_name, "(unknown device)");
    filter_fleet_devices(&mut devices, &ListFleetDevicesQuery::default());
    assert!(devices.is_empty());
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
    assert_eq!(devices[0].display_name, "(unknown device)");
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
