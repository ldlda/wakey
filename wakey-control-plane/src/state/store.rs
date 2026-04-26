use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::types::{
    AgentDeviceObservation, AgentDeviceObservationInput, AgentDeviceObservationView, AlertState,
    AlertTransition, AuditEvent, AuditEventFilter, AuditEventInput, DeviceIdentifier,
    DeviceIdentifierInput, EnrollTokenInfo, IssuedAgent, IssuedEnrollToken, KnownDevice,
    KnownDeviceInput, KnownDeviceSummary, StateStats,
};

pub struct Store {
    db_path: PathBuf,
    pool: SqlitePool,
}

const SCHEMA_VERSION_KEY: &str = "schema_version";
const SEEDED_ENROLL_TOKEN_PREFIX: &str = "seeded_enroll_token:";
const SCHEMA_VERSION: u32 = 1;

mod alerts;
mod audit;
mod db;
mod devices;
mod enrollment;
mod helpers;
mod import_sled;
mod observations;

use helpers::alerts_audit::*;
use helpers::core::*;
use helpers::legacy::*;
use helpers::rows::*;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use crate::state::{DeviceIdentifierInput, KnownDeviceInput};

    use super::Store;

    async fn make_store() -> (Store, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.sqlite3");
        let store = Store::load_or_init(&db_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("store should initialize");
        (store, dir)
    }

    fn cleanup_dir(path: &std::path::Path) {
        let _ = fs::remove_dir_all(path);
    }

    async fn insert_token(store: &Store, token: &str, expires_at_unix: u64) {
        sqlx::query(
            "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
        )
        .bind(token)
        .bind(expires_at_unix as i64)
        .execute(&store.pool)
        .await
        .expect("insert should succeed");
    }

    #[tokio::test]
    async fn rejects_directory_state_path() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir should be created");
        let err = match Store::load_or_init(&dir, Vec::new(), Duration::from_secs(60)).await {
            Ok(_) => panic!("directory path should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("legacy sled store"));
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn gc_removes_expired_tokens() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-expired-gc-test", 1).await;

        let removed = store
            .gc_expired_enroll_tokens()
            .await
            .expect("gc should succeed");

        assert_eq!(removed, 1);
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1")
                .bind("enr-expired-gc-test")
                .fetch_one(&store.pool)
                .await
                .expect("read should succeed");
        assert_eq!(exists, 0);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_expired_token() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-expired-enroll-test", 1).await;

        let err = store
            .enroll("enr-expired-enroll-test")
            .await
            .expect_err("expired token should be rejected");

        assert!(err.to_string().contains("expired"));
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1")
                .bind("enr-expired-enroll-test")
                .fetch_one(&store.pool)
                .await
                .expect("read should succeed");
        assert_eq!(exists, 0);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn stats_counts_agents_and_expired_tokens() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-valid-test", i64::MAX as u64).await;

        let _issued = store
            .issue_enroll_token(Duration::from_secs(60))
            .await
            .expect("issue should succeed");

        insert_token(&store, "enr-expired-stats-test", 1).await;

        let issued_agent = store
            .enroll("enr-valid-test")
            .await
            .expect("enroll should succeed for valid token");
        assert!(!issued_agent.agent_id.is_empty());

        let stats = store.stats().await.expect("stats should succeed");

        assert_eq!(stats.agent_count, 1);
        assert_eq!(stats.enroll_token_count, 2);
        assert_eq!(stats.expired_enroll_token_count, 1);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn revoke_agent_removes_credentials() {
        let (store, dir) = make_store().await;

        insert_token(&store, "enr-revoke-agent-test", i64::MAX as u64).await;

        let issued = store
            .enroll("enr-revoke-agent-test")
            .await
            .expect("enroll should succeed");

        assert!(
            store
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed = store
            .revoke_agent(&issued.agent_id)
            .await
            .expect("revoke should succeed");
        assert!(removed);
        assert!(
            !store
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed_again = store
            .revoke_agent(&issued.agent_id)
            .await
            .expect("second revoke should succeed");
        assert!(!removed_again);

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn nickname_set_and_clear_roundtrip() {
        let (store, dir) = make_store().await;

        insert_token(&store, "enr-nickname-test", i64::MAX as u64).await;

        let issued = store
            .enroll("enr-nickname-test")
            .await
            .expect("enroll should succeed");

        let updated = store
            .set_agent_nickname(&issued.agent_id, Some("kitchen-router"))
            .await
            .expect("nickname set should succeed");
        assert!(updated);

        let listed = store.list_agents_with_nicknames().await;
        assert!(listed.iter().any(|(id, name)| {
            id == &issued.agent_id && name.as_deref() == Some("kitchen-router")
        }));

        let cleared = store
            .set_agent_nickname(&issued.agent_id, None)
            .await
            .expect("nickname clear should succeed");
        assert!(cleared);

        let listed = store.list_agents_with_nicknames().await;
        assert!(
            listed
                .iter()
                .any(|(id, name)| id == &issued.agent_id && name.is_none())
        );

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn known_device_can_hold_multiple_manual_mac_identifiers() {
        let (store, dir) = make_store().await;

        let created = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: Some("windows pc".into()),
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "AA:BB:CC:DD:EE:01".into(),
                }],
            })
            .await
            .expect("known device should create");

        assert_eq!(created.display_name, "lda");
        assert!(created.pinned);
        assert_eq!(created.identifiers.len(), 1);
        assert_eq!(created.identifiers[0].value, "aa:bb:cc:dd:ee:01");

        let updated = store
            .attach_device_identifier(
                &created.device_id,
                DeviceIdentifierInput {
                    kind: "MAC".into(),
                    value: "AA:BB:CC:DD:EE:02".into(),
                },
            )
            .await
            .expect("identifier attach should succeed")
            .expect("device should exist");

        assert_eq!(updated.identifiers.len(), 2);
        assert!(
            updated
                .identifiers
                .iter()
                .any(|identifier| identifier.value == "aa:bb:cc:dd:ee:02")
        );

        let matched = store
            .lookup_known_device_by_identifier(DeviceIdentifierInput {
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:02".into(),
            })
            .await
            .expect("lookup should succeed")
            .expect("identifier should match");
        assert_eq!(matched.device_id, created.device_id);

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn known_device_identifier_is_unique_across_devices() {
        let (store, dir) = make_store().await;
        let first = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                }],
            })
            .await
            .expect("first device should create");
        let second = store
            .create_known_device(KnownDeviceInput {
                display_name: "other".into(),
                pinned: false,
                notes: None,
                identifiers: Vec::new(),
            })
            .await
            .expect("second device should create");

        let err = store
            .attach_device_identifier(
                &second.device_id,
                DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                },
            )
            .await
            .expect_err("duplicate identifier should be rejected");
        assert!(
            err.to_string()
                .contains("failed attaching device identifier")
        );

        let listed = store.list_known_devices().await.expect("list should work");
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .find(|device| device.device_id == first.device_id)
                .expect("first should remain")
                .identifiers
                .len()
                == 1
        );

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn agent_observations_upsert_current_state_and_events() {
        let (store, dir) = make_store().await;

        let accepted = store
            .upsert_agent_observations(
                "agent-a",
                vec![crate::state::AgentDeviceObservationInput {
                    kind: "dhcp".into(),
                    action: "update".into(),
                    mac: Some("AA:BB:CC:DD:EE:FF".into()),
                    ip: Some("192.168.1.10".into()),
                    hostname: Some("lda".into()),
                    first_seen_unix: 10,
                    last_seen_unix: 20,
                }],
            )
            .await
            .expect("observation upsert should succeed");
        assert_eq!(accepted, 1);

        let rows = store
            .list_agent_observations(Some("agent-a"), 10)
            .await
            .expect("observations should list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(rows[0].hostname.as_deref(), Some("lda"));

        let event_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_device_observation_events")
                .fetch_one(&store.pool)
                .await
                .expect("event count should read");
        assert_eq!(event_count, 1);

        let accepted = store
            .upsert_agent_observations(
                "agent-a",
                vec![crate::state::AgentDeviceObservationInput {
                    kind: "dhcp".into(),
                    action: "update".into(),
                    mac: Some("AA:BB:CC:DD:EE:FF".into()),
                    ip: Some("192.168.1.10".into()),
                    hostname: Some("lda".into()),
                    first_seen_unix: 10,
                    last_seen_unix: 20,
                }],
            )
            .await
            .expect("duplicate observation upsert should succeed");
        assert_eq!(accepted, 1);

        let event_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_device_observation_events")
                .fetch_one(&store.pool)
                .await
                .expect("event count should read");
        assert_eq!(event_count, 1);

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn agent_observation_views_include_matching_known_device() {
        let (store, dir) = make_store().await;

        let device = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                }],
            })
            .await
            .expect("known device should create");

        store
            .upsert_agent_observations(
                "agent-a",
                vec![
                    crate::state::AgentDeviceObservationInput {
                        kind: "dhcp".into(),
                        action: "update".into(),
                        mac: Some("AA:BB:CC:DD:EE:FF".into()),
                        ip: Some("192.168.1.10".into()),
                        hostname: Some("lda".into()),
                        first_seen_unix: 10,
                        last_seen_unix: 20,
                    },
                    crate::state::AgentDeviceObservationInput {
                        kind: "dhcp".into(),
                        action: "update".into(),
                        mac: Some("00:11:22:33:44:55".into()),
                        ip: Some("192.168.1.11".into()),
                        hostname: Some("guest".into()),
                        first_seen_unix: 11,
                        last_seen_unix: 21,
                    },
                ],
            )
            .await
            .expect("observation upsert should succeed");

        let rows = store
            .list_agent_observation_views(Some("agent-a"), 10)
            .await
            .expect("observation views should list");
        assert_eq!(rows.len(), 2);

        let known = rows
            .iter()
            .find(|row| row.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff"))
            .expect("known observation should be present");
        let known_device = known
            .known_device
            .as_ref()
            .expect("known observation should join device");
        assert_eq!(known_device.device_id, device.device_id);
        assert_eq!(known_device.display_name, "lda");
        assert!(known_device.pinned);

        let unknown = rows
            .iter()
            .find(|row| row.mac.as_deref() == Some("00:11:22:33:44:55"))
            .expect("unknown observation should be present");
        assert!(unknown.known_device.is_none());

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn observation_identifier_can_be_attached_to_known_device() {
        let (store, dir) = make_store().await;

        let device = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: Vec::new(),
            })
            .await
            .expect("known device should create");

        store
            .upsert_agent_observations(
                "agent-a",
                vec![crate::state::AgentDeviceObservationInput {
                    kind: "dhcp".into(),
                    action: "update".into(),
                    mac: Some("AA:BB:CC:DD:EE:FF".into()),
                    ip: Some("192.168.1.10".into()),
                    hostname: Some("lda".into()),
                    first_seen_unix: 10,
                    last_seen_unix: 20,
                }],
            )
            .await
            .expect("observation upsert should succeed");

        let observation = store
            .list_agent_observations(Some("agent-a"), 10)
            .await
            .expect("observations should list")
            .pop()
            .expect("observation should exist");

        let updated = store
            .attach_observation_identifier(&device.device_id, &observation.observation_key)
            .await
            .expect("observation identifier should attach")
            .expect("device should exist");

        assert_eq!(updated.identifiers.len(), 1);
        assert_eq!(updated.identifiers[0].kind, "mac");
        assert_eq!(updated.identifiers[0].value, "aa:bb:cc:dd:ee:ff");

        let views = store
            .list_agent_observation_views(Some("agent-a"), 10)
            .await
            .expect("observation views should list");
        assert_eq!(
            views[0]
                .known_device
                .as_ref()
                .map(|device| device.device_id.as_str()),
            Some(device.device_id.as_str())
        );

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn audit_events_append_and_filter() {
        let (store, dir) = make_store().await;

        store
            .append_audit_event(crate::state::AuditEventInput {
                actor_type: "admin_api".into(),
                actor_id: None,
                agent_id: Some("agent-1".into()),
                request_id: Some("req-1".into()),
                event_type: "command_result".into(),
                outcome: "ok".into(),
                latency_ms: Some(12),
                message: "command completed".into(),
                metadata: serde_json::json!({"command":"devs"}),
            })
            .await
            .expect("append first event should succeed");

        store
            .append_audit_event(crate::state::AuditEventInput {
                actor_type: "agent".into(),
                actor_id: Some("agent-2".into()),
                agent_id: Some("agent-2".into()),
                request_id: Some("req-2".into()),
                event_type: "agent_ws_auth".into(),
                outcome: "rejected".into(),
                latency_ms: None,
                message: "auth rejected".into(),
                metadata: serde_json::json!({}),
            })
            .await
            .expect("append second event should succeed");

        let all = store
            .list_audit_events(crate::state::AuditEventFilter {
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("list all should succeed");
        assert_eq!(all.len(), 2);

        let filtered = store
            .list_audit_events(crate::state::AuditEventFilter {
                agent_id: Some("agent-1".into()),
                event_type: Some("command_result".into()),
                outcome: Some("ok".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("filtered list should succeed");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].request_id.as_deref(), Some("req-1"));

        let rejected = store
            .list_audit_events(crate::state::AuditEventFilter {
                outcome: Some("rejected".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("rejected list should succeed");
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].latency_ms, None);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn alert_transitions_track_open_and_resolve() {
        let (store, dir) = make_store().await;
        let alert = crate::state::AlertState {
            alert_id: "agent_offline:agent-a".into(),
            kind: "agent_offline".into(),
            severity: "warning".into(),
            status: "active".into(),
            agent_id: Some("agent-a".into()),
            message: "agent agent-a offline".into(),
            value: 1,
            threshold: 1,
            last_seen_unix: 10,
            metadata: serde_json::json!({}),
        };

        let opened = store
            .sync_alert_transitions(std::slice::from_ref(&alert))
            .await
            .expect("open transition should succeed");
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].to_status, "active");

        let resolved = store
            .sync_alert_transitions(&[])
            .await
            .expect("resolve transition should succeed");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].to_status, "resolved");

        let history = store
            .list_alert_transitions(None, 10)
            .await
            .expect("history should load");
        assert!(history.len() >= 2);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn bootstrap_seed_tokens_are_not_reseeded_after_consumption() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.sqlite3");

        let first = Store::load_or_init(
            &db_path,
            vec!["enr-bootstrap-once".to_string()],
            Duration::from_secs(60),
        )
        .await
        .expect("initial store should initialize");

        let issued = first
            .enroll("enr-bootstrap-once")
            .await
            .expect("bootstrap token should enroll once");
        assert!(!issued.agent_id.is_empty());

        drop(first);

        let second = Store::load_or_init(
            &db_path,
            vec!["enr-bootstrap-once".to_string()],
            Duration::from_secs(60),
        )
        .await
        .expect("reloaded store should initialize");

        let err = second
            .enroll("enr-bootstrap-once")
            .await
            .expect_err("bootstrap token should not resurrect after restart");
        assert!(
            err.to_string()
                .contains("invalid or already-used enroll token")
        );

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn import_sled_state_copies_core_records() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let sled_path = dir.join("legacy-state.db");
        let sqlite_path = dir.join("state.sqlite3");
        fs::create_dir_all(&dir).expect("dir should exist");

        let legacy = sled::open(&sled_path).expect("legacy db should open");
        let meta = legacy.open_tree("meta").expect("meta should open");
        meta.insert(
            super::SCHEMA_VERSION_KEY.as_bytes(),
            &super::SCHEMA_VERSION.to_le_bytes(),
        )
        .expect("schema should insert");
        let enroll = legacy
            .open_tree("enroll_tokens")
            .expect("enroll tree should open");
        enroll
            .insert(b"enr-import-test", &(i64::MAX as u64).to_le_bytes())
            .expect("token should insert");
        let agents = legacy.open_tree("agents").expect("agents should open");
        agents
            .insert(b"agent-import", b"tok-import")
            .expect("agent should insert");
        let agent_meta = legacy.open_tree("agent_meta").expect("meta should open");
        agent_meta
            .insert(b"agent-import", b"imported-router")
            .expect("nickname should insert");
        legacy.flush().expect("legacy flush should succeed");
        drop(agent_meta);
        drop(agents);
        drop(enroll);
        drop(meta);
        drop(legacy);

        Store::import_sled_state(&sled_path, &sqlite_path, false)
            .await
            .expect("import should succeed");
        let store = Store::load_or_init(&sqlite_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("sqlite should load");

        assert!(store.verify_agent_token("agent-import", "tok-import").await);
        let agents = store.list_agents_with_nicknames().await;
        assert!(agents.iter().any(|(id, nickname)| {
            id == "agent-import" && nickname.as_deref() == Some("imported-router")
        }));
        let tokens = store
            .list_enroll_tokens()
            .await
            .expect("tokens should list");
        assert!(
            tokens
                .iter()
                .any(|token| token.enroll_token == "enr-import-test")
        );

        cleanup_dir(&dir);
    }
}
