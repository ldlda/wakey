use chrono::{DateTime, Local, Utc};
use comfy_table::{Cell, ContentArrangement, Table, presets};
use wakey_core::{DhcpLeaseWithState, InterfaceSummary, WakeResult};

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

pub fn render_status_table(status: &wakey::StatusResponse) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "State", "IF"]);
    for row in &status.table {
        table.add_row([
            Cell::new(row.ip),
            Cell::new(row.mac.map(|m| m.to_string()).unwrap_or_default()),
            Cell::new(format!("{:?}", row.state).to_lowercase()),
            Cell::new(row.dev.clone().unwrap_or_default()),
        ]);
    }
    table
}

pub fn render_leases_table(leases: &[DhcpLeaseWithState]) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "Name", "Expires", "NUD"]);
    for lease in leases {
        let expires = format_epoch_local(lease.lease_line.expires_epoch);
        table.add_row([
            Cell::new(lease.lease_line.ip),
            Cell::new(lease.lease_line.mac),
            Cell::new(lease.lease_line.name.clone().unwrap_or_default()),
            Cell::new(expires),
            Cell::new(
                lease
                    .nud_state
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_default(),
            ),
        ]);
    }
    table
}

pub fn render_wake_table(result: &WakeResult) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "Status"]);
    for row in &result.result {
        table.add_row([
            Cell::new(row.target.ip.map(|ip| ip.to_string()).unwrap_or_default()),
            Cell::new(row.target.mac.map(|m| m.to_string()).unwrap_or_default()),
            Cell::new(format!("{:?}", row.status).to_lowercase()),
        ]);
    }
    table
}

pub fn render_devs_table(devs: &[InterfaceSummary]) -> Table {
    let mut table = base_table();
    table.set_header([
        "Interface",
        "State",
        "MAC",
        "Family",
        "Address",
        "Broadcast",
        "Scope/Label",
    ]);

    for dev in devs {
        if dev.addrs.is_empty() {
            table.add_row([
                Cell::new(&dev.ifname),
                Cell::new(&dev.operstate),
                Cell::new(dev.mac.map(|m| m.to_string()).unwrap_or_default()),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
            ]);
            continue;
        }

        for (idx, addr) in dev.addrs.iter().enumerate() {
            let scope_label = match (&addr.scope, &addr.label) {
                (Some(scope), Some(label)) => format!("{scope} ({label})"),
                (Some(scope), None) => scope.clone(),
                (None, Some(label)) => label.clone(),
                (None, None) => String::new(),
            };

            let lead = idx == 0;
            table.add_row([
                Cell::new(if lead { dev.ifname.as_str() } else { "" }),
                Cell::new(if lead { dev.operstate.as_str() } else { "" }),
                Cell::new(if lead {
                    dev.mac.map(|m| m.to_string()).unwrap_or_default()
                } else {
                    String::new()
                }),
                Cell::new(addr.family.clone().unwrap_or_default()),
                Cell::new(addr.cidr.clone().unwrap_or_default()),
                Cell::new(addr.broadcast.map(|ip| ip.to_string()).unwrap_or_default()),
                Cell::new(scope_label),
            ]);
        }
    }

    table
}

fn format_epoch_local(epoch: u64) -> String {
    match DateTime::<Utc>::from_timestamp(epoch as i64, 0) {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => epoch.to_string(),
    }
}
