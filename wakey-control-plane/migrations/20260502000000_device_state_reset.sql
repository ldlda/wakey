CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value BLOB NOT NULL
);

CREATE TABLE enroll_tokens (
    token TEXT PRIMARY KEY,
    expires_at_unix INTEGER NOT NULL
);

CREATE TABLE agents (
    agent_id TEXT PRIMARY KEY,
    agent_token TEXT NOT NULL
);

CREATE TABLE agent_meta (
    agent_id TEXT PRIMARY KEY REFERENCES agents(agent_id) ON DELETE CASCADE,
    nickname TEXT
);

---------------------------------------------------------------------------
-- Agent-reported complete device state
---------------------------------------------------------------------------

CREATE TABLE agent_devices (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    presence TEXT NOT NULL,
    display_name TEXT,
    first_seen_unix INTEGER NOT NULL,
    last_seen_unix INTEGER NOT NULL,
    PRIMARY KEY (agent_id, device_key)
) WITHOUT ROWID;

CREATE INDEX agent_devices_agent_seen_idx
    ON agent_devices(agent_id, last_seen_unix);

CREATE TABLE agent_device_macs (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    mac TEXT NOT NULL,
    PRIMARY KEY (agent_id, device_key, mac),
    FOREIGN KEY (agent_id, device_key)
        REFERENCES agent_devices(agent_id, device_key) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX agent_device_macs_mac_idx
    ON agent_device_macs(mac);

CREATE TABLE agent_device_ips (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    ip TEXT NOT NULL,
    PRIMARY KEY (agent_id, device_key, ip),
    FOREIGN KEY (agent_id, device_key)
        REFERENCES agent_devices(agent_id, device_key) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX agent_device_ips_ip_idx
    ON agent_device_ips(ip);

CREATE TABLE agent_device_endpoints (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    endpoint_key TEXT NOT NULL,
    source TEXT NOT NULL,
    mac TEXT,
    ip TEXT,
    hostname TEXT,
    interface TEXT,
    presence TEXT NOT NULL,
    first_seen_unix INTEGER NOT NULL,
    last_seen_unix INTEGER NOT NULL,
    PRIMARY KEY (agent_id, device_key, endpoint_key),
    FOREIGN KEY (agent_id, device_key)
        REFERENCES agent_devices(agent_id, device_key) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX agent_device_endpoints_mac_idx
    ON agent_device_endpoints(mac);

CREATE INDEX agent_device_endpoints_ip_idx
    ON agent_device_endpoints(ip);

CREATE TABLE agent_device_hostnames (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    hostname TEXT NOT NULL,
    PRIMARY KEY (agent_id, device_key, hostname),
    FOREIGN KEY (agent_id, device_key)
        REFERENCES agent_devices(agent_id, device_key) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE agent_device_facts (
    agent_id TEXT NOT NULL,
    device_key TEXT NOT NULL,
    fact_json TEXT NOT NULL,
    PRIMARY KEY (agent_id, device_key, fact_json),
    FOREIGN KEY (agent_id, device_key)
        REFERENCES agent_devices(agent_id, device_key) ON DELETE CASCADE
) WITHOUT ROWID;

---------------------------------------------------------------------------
-- Operator-curated known devices
---------------------------------------------------------------------------

CREATE TABLE known_devices (
    device_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    pinned INTEGER NOT NULL,
    created_at_unix INTEGER NOT NULL,
    updated_at_unix INTEGER NOT NULL,
    notes TEXT
);

CREATE TABLE device_identifiers (
    identifier_key TEXT PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES known_devices(device_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    created_at_unix INTEGER NOT NULL,
    UNIQUE(kind, value)
);

CREATE INDEX device_identifiers_device_id_idx
    ON device_identifiers(device_id);

---------------------------------------------------------------------------
-- Audit + alerts
---------------------------------------------------------------------------

CREATE TABLE audit_events (
    event_key TEXT PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    ts_unix INTEGER NOT NULL,
    actor_type TEXT NOT NULL,
    actor_id TEXT,
    agent_id TEXT,
    request_id TEXT,
    event_type TEXT NOT NULL,
    outcome TEXT NOT NULL,
    latency_ms INTEGER,
    message TEXT NOT NULL,
    metadata_json TEXT NOT NULL
);

CREATE INDEX audit_events_ts_unix_idx ON audit_events(ts_unix);
CREATE INDEX audit_events_agent_id_idx ON audit_events(agent_id);
CREATE INDEX audit_events_request_id_idx ON audit_events(request_id);
CREATE INDEX audit_events_event_type_outcome_idx ON audit_events(event_type, outcome);

CREATE TABLE active_alerts (
    alert_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    status TEXT NOT NULL,
    agent_id TEXT,
    message TEXT NOT NULL,
    value INTEGER NOT NULL,
    threshold INTEGER NOT NULL,
    last_seen_unix INTEGER NOT NULL,
    metadata_json TEXT NOT NULL
);

CREATE TABLE alert_transitions (
    transition_key TEXT PRIMARY KEY,
    transition_id TEXT NOT NULL UNIQUE,
    ts_unix INTEGER NOT NULL,
    alert_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    agent_id TEXT,
    from_status TEXT,
    to_status TEXT NOT NULL,
    message TEXT NOT NULL,
    metadata_json TEXT NOT NULL
);

CREATE INDEX alert_transitions_ts_unix_idx ON alert_transitions(ts_unix);
