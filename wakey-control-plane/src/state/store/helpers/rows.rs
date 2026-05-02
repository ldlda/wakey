pub struct NormalizedDeviceIdentifier {
    pub identifier_key: String,
    pub kind: String,
    pub value: String,
}

pub struct EnrollTokenRow {
    pub token: String,
    pub expires_at_unix: i64,
}

pub struct KnownDeviceRow {
    pub device_id: String,
    pub display_name: String,
    pub pinned: i64,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
    pub notes: Option<String>,
}

pub struct DeviceIdentifierRow {
    pub identifier_key: String,
    pub device_id: String,
    pub kind: String,
    pub value: String,
    pub created_at_unix: i64,
}

pub struct AlertStateRow {
    pub alert_id: String,
    pub kind: String,
    pub severity: String,
    pub status: String,
    pub agent_id: Option<String>,
    pub message: String,
    pub value: i64,
    pub threshold: i64,
    pub last_seen_unix: i64,
    pub metadata_json: String,
}

pub struct AlertTransitionRow {
    pub transition_id: String,
    pub ts_unix: i64,
    pub alert_id: String,
    pub kind: String,
    pub agent_id: Option<String>,
    pub from_status: Option<String>,
    pub to_status: String,
    pub message: String,
    pub metadata_json: String,
}
