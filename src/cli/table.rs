use chrono::{DateTime, Local};
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL};
use wakey_core::{DeviceInventory, DhcpLeaseWithState, InterfaceSummary, WakeResult};

pub fn render_status_table(status: &DeviceInventory) -> Table {
    let mut table = base_table();
    table.set_header(vec!["Name", "IP", "MAC", "Presence", "Interfaces"]);
    for row in &status.devices {
        table.add_row(vec![
            Cell::new(
                row.names
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "(unnamed)".into()),
            ),
            Cell::new(
                row.ips
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Cell::new(
                row.macs
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Cell::new(format!("{:?}", row.presence)),
            Cell::new(row.interfaces.join(", ")),
        ]);
    }
    table
}

pub fn render_leases_table(leases: &[DhcpLeaseWithState]) -> Table {
    let mut table = base_table();
    table.set_header(vec!["Expires", "IP", "MAC", "Name", "State"]);
    for lease in leases {
        table.add_row(vec![
            Cell::new(format_epoch(lease.lease_line.expires_epoch)),
            Cell::new(lease.lease_line.ip.to_string()),
            Cell::new(lease.lease_line.mac.to_string()),
            Cell::new(lease.lease_line.name.clone().unwrap_or_default()),
            Cell::new(lease.nud_state.map(|v| v.to_string()).unwrap_or_default()),
        ]);
    }
    table
}

pub fn render_wake_table(result: &WakeResult) -> Table {
    let mut table = base_table();
    table.set_header(vec!["IP", "MAC", "Status"]);
    for row in &result.result {
        table.add_row(vec![
            Cell::new(row.target.ip.map(|v| v.to_string()).unwrap_or_default()),
            Cell::new(row.target.mac.map(|v| v.to_string()).unwrap_or_default()),
            Cell::new(format!("{:?}", row.status)),
        ]);
    }
    table
}

pub fn render_devs_table(devs: &[InterfaceSummary]) -> Table {
    let mut table = base_table();
    table.set_header(vec![
        "Ifname",
        "State",
        "MAC",
        "Family",
        "CIDR",
        "Broadcast",
        "Scope/Label",
    ]);

    for dev in devs {
        if dev.addrs.is_empty() {
            table.add_row(vec![
                Cell::new(&dev.ifname),
                Cell::new(&dev.operstate),
                Cell::new(dev.mac.map(|v| v.to_string()).unwrap_or_default()),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
            ]);
            continue;
        }

        for (idx, addr) in dev.addrs.iter().enumerate() {
            let lead = idx == 0;
            table.add_row(vec![
                Cell::new(if lead { dev.ifname.as_str() } else { "" }),
                Cell::new(if lead { dev.operstate.as_str() } else { "" }),
                Cell::new(if lead {
                    dev.mac.map(|v| v.to_string()).unwrap_or_default()
                } else {
                    String::new()
                }),
                Cell::new(addr.family.clone().unwrap_or_default()),
                Cell::new(addr.cidr.clone().unwrap_or_default()),
                Cell::new(addr.broadcast.map(|v| v.to_string()).unwrap_or_default()),
                Cell::new(match (&addr.scope, &addr.label) {
                    (Some(scope), Some(label)) => format!("{scope} / {label}"),
                    (Some(scope), None) => scope.clone(),
                    (None, Some(label)) => label.clone(),
                    (None, None) => String::new(),
                }),
            ]);
        }
    }

    table
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

fn format_epoch(epoch: u64) -> String {
    DateTime::from_timestamp(epoch as i64, 0)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| epoch.to_string())
}
