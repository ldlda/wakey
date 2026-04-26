pub(in crate::state::store) struct NormalizedDeviceIdentifier {
    pub(in crate::state::store) identifier_key: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) value: String,
}

pub(in crate::state::store) struct EnrollTokenRow {
    pub(in crate::state::store) token: String,
    pub(in crate::state::store) expires_at_unix: i64,
}

pub(in crate::state::store) struct KnownDeviceRow {
    pub(in crate::state::store) device_id: String,
    pub(in crate::state::store) display_name: String,
    pub(in crate::state::store) pinned: i64,
    pub(in crate::state::store) created_at_unix: i64,
    pub(in crate::state::store) updated_at_unix: i64,
    pub(in crate::state::store) notes: Option<String>,
}

pub(in crate::state::store) struct DeviceIdentifierRow {
    pub(in crate::state::store) identifier_key: String,
    pub(in crate::state::store) device_id: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) value: String,
    pub(in crate::state::store) created_at_unix: i64,
}

pub(in crate::state::store) struct AgentObservationRow {
    pub(in crate::state::store) observation_key: String,
    pub(in crate::state::store) agent_id: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) mac: Option<String>,
    pub(in crate::state::store) ip: Option<String>,
    pub(in crate::state::store) hostname: Option<String>,
    pub(in crate::state::store) first_seen_unix: i64,
    pub(in crate::state::store) last_seen_unix: i64,
    pub(in crate::state::store) last_action: String,
}

pub(in crate::state::store) struct AgentObservationViewRow {
    pub(in crate::state::store) observation_key: String,
    pub(in crate::state::store) agent_id: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) mac: Option<String>,
    pub(in crate::state::store) ip: Option<String>,
    pub(in crate::state::store) hostname: Option<String>,
    pub(in crate::state::store) first_seen_unix: i64,
    pub(in crate::state::store) last_seen_unix: i64,
    pub(in crate::state::store) last_action: String,
    pub(in crate::state::store) device_id: Option<String>,
    pub(in crate::state::store) display_name: Option<String>,
    pub(in crate::state::store) pinned: Option<i64>,
}

pub(in crate::state::store) struct ObservationIdentifierRow {
    pub(in crate::state::store) mac: Option<String>,
    pub(in crate::state::store) ip: Option<String>,
}

pub(in crate::state::store) struct ObservationCurrentRow {
    pub(in crate::state::store) mac: Option<String>,
    pub(in crate::state::store) ip: Option<String>,
    pub(in crate::state::store) hostname: Option<String>,
    pub(in crate::state::store) first_seen_unix: i64,
    pub(in crate::state::store) last_seen_unix: i64,
    pub(in crate::state::store) last_action: String,
}

pub(in crate::state::store) struct AlertStateRow {
    pub(in crate::state::store) alert_id: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) severity: String,
    pub(in crate::state::store) status: String,
    pub(in crate::state::store) agent_id: Option<String>,
    pub(in crate::state::store) message: String,
    pub(in crate::state::store) value: i64,
    pub(in crate::state::store) threshold: i64,
    pub(in crate::state::store) last_seen_unix: i64,
    pub(in crate::state::store) metadata_json: String,
}

pub(in crate::state::store) struct AlertTransitionRow {
    pub(in crate::state::store) transition_id: String,
    pub(in crate::state::store) ts_unix: i64,
    pub(in crate::state::store) alert_id: String,
    pub(in crate::state::store) kind: String,
    pub(in crate::state::store) agent_id: Option<String>,
    pub(in crate::state::store) from_status: Option<String>,
    pub(in crate::state::store) to_status: String,
    pub(in crate::state::store) message: String,
    pub(in crate::state::store) metadata_json: String,
}
