//! Attach decoded children to their parent using the full agent/device identity.

use std::collections::BTreeMap;

use crate::state::types::*;

#[derive(Default)]
pub(super) struct DeviceRows {
    pub(super) devices: Vec<AgentDeviceRow>,
    pub(super) macs: Vec<AgentDeviceMacRow>,
    pub(super) ips: Vec<AgentDeviceIpRow>,
    pub(super) hostnames: Vec<AgentDeviceHostnameRow>,
    pub(super) endpoints: Vec<AgentDeviceEndpointRow>,
    pub(super) facts: Vec<AgentDeviceFactRow>,
}

type DeviceIndex = BTreeMap<(String, String), AgentDeviceWithChildren>;

impl DeviceRows {
    pub(super) fn into_devices(self) -> Vec<AgentDeviceWithChildren> {
        let mut index = self
            .devices
            .into_iter()
            .map(|device| {
                let key = (device.agent_id.clone(), device.device_key.clone());
                (
                    key,
                    AgentDeviceWithChildren {
                        device,
                        macs: Vec::new(),
                        ips: Vec::new(),
                        hostnames: Vec::new(),
                        endpoints: Vec::new(),
                        facts: Vec::new(),
                    },
                )
            })
            .collect::<DeviceIndex>();
        add_macs(&mut index, self.macs);
        add_ips(&mut index, self.ips);
        add_hostnames(&mut index, self.hostnames);
        add_endpoints(&mut index, self.endpoints);
        add_facts(&mut index, self.facts);
        // Matches the SQL order: agent_id/device_key for fleet reads, and
        // device_key within one agent. Child row order is left unchanged.
        index.into_values().collect()
    }
}

fn add_macs(index: &mut DeviceIndex, rows: Vec<AgentDeviceMacRow>) {
    for row in rows {
        let mac = match macaddr::MacAddr::try_from(&row) {
            Ok(mac) => mac,
            Err(error) => {
                tracing::warn!(%error, agent_id = %row.agent_id, device_key = %row.device_key,
                    raw_mac = %row.mac, "failed to parse mac from agent_device_macs row");
                continue;
            }
        };
        if let Some(device) = index.get_mut(&(row.agent_id, row.device_key)) {
            device.macs.push(mac);
        }
    }
}

fn add_ips(index: &mut DeviceIndex, rows: Vec<AgentDeviceIpRow>) {
    for row in rows {
        let ip = match std::net::IpAddr::try_from(&row) {
            Ok(ip) => ip,
            Err(error) => {
                tracing::warn!(%error, agent_id = %row.agent_id, device_key = %row.device_key,
                    raw_ip = %row.ip, "failed to parse ip from agent_device_ips row");
                continue;
            }
        };
        if let Some(device) = index.get_mut(&(row.agent_id, row.device_key)) {
            device.ips.push(ip);
        }
    }
}

fn add_hostnames(index: &mut DeviceIndex, rows: Vec<AgentDeviceHostnameRow>) {
    for row in rows {
        if let Some(device) = index.get_mut(&(row.agent_id, row.device_key)) {
            device.hostnames.push(row.hostname);
        }
    }
}

fn add_endpoints(index: &mut DeviceIndex, rows: Vec<AgentDeviceEndpointRow>) {
    for row in rows {
        if let Some(endpoint) = row.to_endpoint()
            && let Some(device) = index.get_mut(&(row.agent_id, row.device_key))
        {
            device.endpoints.push(endpoint);
        }
    }
}

fn add_facts(index: &mut DeviceIndex, rows: Vec<AgentDeviceFactRow>) {
    for row in rows {
        if let Some(device) = index.get_mut(&(row.agent_id, row.device_key)) {
            device.facts.push(row.fact_json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent(agent: &str, key: &str) -> AgentDeviceRow {
        AgentDeviceRow {
            agent_id: agent.into(),
            device_key: key.into(),
            presence: "unknown".into(),
            display_name: None,
            first_seen_unix: 1,
            last_seen_unix: 2,
        }
    }

    #[test]
    fn assembly_keeps_agent_boundaries_order_and_empty_parents() {
        let rows = DeviceRows {
            devices: vec![
                parent("b", "shared"),
                parent("a", "shared"),
                parent("a", "empty"),
            ],
            hostnames: vec![
                AgentDeviceHostnameRow {
                    agent_id: "b".into(),
                    device_key: "shared".into(),
                    hostname: "second".into(),
                },
                AgentDeviceHostnameRow {
                    agent_id: "a".into(),
                    device_key: "shared".into(),
                    hostname: "first".into(),
                },
                AgentDeviceHostnameRow {
                    agent_id: "missing".into(),
                    device_key: "shared".into(),
                    hostname: "orphan".into(),
                },
            ],
            facts: vec![AgentDeviceFactRow {
                agent_id: "a".into(),
                device_key: "shared".into(),
                fact_json: "{\"action\":\"remove\"}".into(),
            }],
            ..Default::default()
        }
        .into_devices();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].device.device_key, "empty");
        assert!(rows[0].hostnames.is_empty());
        assert_eq!(rows[1].hostnames, ["first"]);
        assert_eq!(rows[1].facts, ["{\"action\":\"remove\"}"]);
        assert_eq!(rows[2].hostnames, ["second"]);
        assert!(rows[2].facts.is_empty());
    }

    #[test]
    fn malformed_children_do_not_hide_valid_children_or_the_parent() {
        let rows = DeviceRows {
            devices: vec![parent("a", "pc")],
            macs: ["not-a-mac", "aa:bb:cc:dd:ee:01"]
                .into_iter()
                .map(|mac| AgentDeviceMacRow {
                    agent_id: "a".into(),
                    device_key: "pc".into(),
                    mac: mac.into(),
                })
                .collect(),
            ips: ["not-an-ip", "192.168.1.10"]
                .into_iter()
                .map(|ip| AgentDeviceIpRow {
                    agent_id: "a".into(),
                    device_key: "pc".into(),
                    ip: ip.into(),
                })
                .collect(),
            ..Default::default()
        }
        .into_devices();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].macs,
            ["aa:bb:cc:dd:ee:01".parse::<macaddr::MacAddr>().unwrap()]
        );
        assert_eq!(
            rows[0].ips,
            ["192.168.1.10".parse::<std::net::IpAddr>().unwrap()]
        );
    }
}
