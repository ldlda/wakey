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

CREATE TABLE audit_events (
    event_key TEXT PRIMARY KEY,
    event_json TEXT NOT NULL,
    ts_unix INTEGER NOT NULL,
    agent_id TEXT,
    request_id TEXT,
    event_type TEXT NOT NULL,
    outcome TEXT NOT NULL
);

CREATE INDEX audit_events_ts_unix_idx ON audit_events(ts_unix);
CREATE INDEX audit_events_agent_id_idx ON audit_events(agent_id);
CREATE INDEX audit_events_request_id_idx ON audit_events(request_id);
CREATE INDEX audit_events_event_type_outcome_idx ON audit_events(event_type, outcome);

CREATE TABLE active_alerts (
    alert_id TEXT PRIMARY KEY,
    alert_json TEXT NOT NULL
);

CREATE TABLE alert_transitions (
    transition_key TEXT PRIMARY KEY,
    transition_json TEXT NOT NULL,
    ts_unix INTEGER NOT NULL
);

CREATE INDEX alert_transitions_ts_unix_idx ON alert_transitions(ts_unix);
