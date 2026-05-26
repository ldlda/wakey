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
