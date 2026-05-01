CREATE TABLE agent_observation_snapshots (
    agent_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    last_dump_unix INTEGER NOT NULL,
    PRIMARY KEY(agent_id, kind)
);

CREATE TABLE agent_observation_snapshot_keys (
    agent_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    observation_key TEXT NOT NULL,
    PRIMARY KEY(agent_id, kind, observation_key)
);

CREATE INDEX agent_observation_snapshot_keys_observation_idx
    ON agent_observation_snapshot_keys(observation_key);
