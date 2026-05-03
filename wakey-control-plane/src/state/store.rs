use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::types::{
    AlertState, AlertTransition, AuditEvent, AuditEventFilter, AuditEventInput, DeviceIdentifier,
    DeviceIdentifierInput, EnrollTokenInfo, IssuedAgent, IssuedEnrollToken, KnownDevice,
    KnownDeviceInput, StateStats,
};

pub struct Store {
    db_path: PathBuf,
    pool: SqlitePool,
}

const SCHEMA_VERSION_KEY: &str = "schema_version";
const SEEDED_ENROLL_TOKEN_PREFIX: &str = "seeded_enroll_token:";
const SCHEMA_VERSION: u32 = 2;

pub(crate) mod agent_devices;
mod alerts;
mod audit;
mod db;
mod devices;
mod enrollment;
mod helpers;
mod migrate_sqlite;

use helpers::alerts_audit::*;
use helpers::core::*;
use helpers::rows::*;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use crate::state::{DeviceIdentifierInput, KnownDeviceInput};

    use super::Store;
    use super::helpers::test_helpers::TestStore;

    async fn insert_token(store: &Store, token: &str, expires_at_unix: u64) {
        let expires = expires_at_unix as i64;
        sqlx::query!(
            "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
            token,
            expires
        )
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn gc_removes_expired_tokens() {
        let ts = TestStore::new().await;
        insert_token(ts.store(), "enr-expired-gc-test", 1).await;

        let removed = ts
            .store()
            .gc_expired_enroll_tokens()
            .await
            .expect("gc should succeed");

        assert_eq!(removed, 1);
        let exists = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1",
            "enr-expired-gc-test"
        )
        .fetch_one(&ts.store().pool)
        .await
        .expect("read should succeed");
        assert_eq!(exists, 0);
    }

    #[tokio::test]
    async fn enroll_rejects_expired_token() {
        let ts = TestStore::new().await;
        insert_token(ts.store(), "enr-expired-enroll-test", 1).await;

        let err = ts
            .store()
            .enroll("enr-expired-enroll-test")
            .await
            .expect_err("expired token should be rejected");

        assert!(err.to_string().contains("expired"));
        let exists = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1",
            "enr-expired-enroll-test"
        )
        .fetch_one(&ts.store().pool)
        .await
        .expect("read should succeed");
        assert_eq!(exists, 0);
    }

    #[tokio::test]
    async fn stats_counts_agents_and_expired_tokens() {
        let ts = TestStore::new().await;
        insert_token(ts.store(), "enr-valid-test", i64::MAX as u64).await;

        let _issued = ts
            .store()
            .issue_enroll_token(Duration::from_secs(60))
            .await
            .expect("issue should succeed");

        insert_token(ts.store(), "enr-expired-stats-test", 1).await;

        let issued_agent = ts
            .store()
            .enroll("enr-valid-test")
            .await
            .expect("enroll should succeed for valid token");
        assert!(!issued_agent.agent_id.is_empty());

        let stats = ts.store().stats().await.expect("stats should succeed");

        assert_eq!(stats.agent_count, 1);
        assert_eq!(stats.enroll_token_count, 2);
        assert_eq!(stats.expired_enroll_token_count, 1);
    }

    #[tokio::test]
    async fn revoke_agent_removes_credentials() {
        let ts = TestStore::new().await;

        insert_token(ts.store(), "enr-revoke-agent-test", i64::MAX as u64).await;

        let issued = ts
            .store()
            .enroll("enr-revoke-agent-test")
            .await
            .expect("enroll should succeed");

        assert!(
            ts.store()
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed = ts
            .store()
            .revoke_agent(&issued.agent_id)
            .await
            .expect("revoke should succeed");
        assert!(removed);
        assert!(
            !ts.store()
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed_again = ts
            .store()
            .revoke_agent(&issued.agent_id)
            .await
            .expect("second revoke should succeed");
        assert!(!removed_again);
    }

    #[tokio::test]
    async fn nickname_set_and_clear_roundtrip() {
        let ts = TestStore::new().await;

        insert_token(ts.store(), "enr-nickname-test", i64::MAX as u64).await;

        let issued = ts
            .store()
            .enroll("enr-nickname-test")
            .await
            .expect("enroll should succeed");

        let updated = ts
            .store()
            .set_agent_nickname(&issued.agent_id, Some("kitchen-router"))
            .await
            .expect("nickname set should succeed");
        assert!(updated);

        let listed = ts.store().list_agents_with_nicknames().await;
        assert!(listed.iter().any(|(id, name)| {
            id == &issued.agent_id && name.as_deref() == Some("kitchen-router")
        }));

        let cleared = ts
            .store()
            .set_agent_nickname(&issued.agent_id, None)
            .await
            .expect("nickname clear should succeed");
        assert!(cleared);

        let listed = ts.store().list_agents_with_nicknames().await;
        assert!(
            listed
                .iter()
                .any(|(id, name)| id == &issued.agent_id && name.is_none())
        );
    }

    #[tokio::test]
    async fn known_device_can_hold_multiple_manual_mac_identifiers() {
        let ts = TestStore::new().await;

        let created = ts
            .store()
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

        let updated = ts
            .store()
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

        let matched = ts
            .store()
            .lookup_known_device_by_identifier(DeviceIdentifierInput {
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:02".into(),
            })
            .await
            .expect("lookup should succeed")
            .expect("identifier should match");
        assert_eq!(matched.device_id, created.device_id);
    }

    #[tokio::test]
    async fn known_device_identifier_is_unique_across_devices() {
        let ts = TestStore::new().await;
        let first = ts
            .store()
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
        let second = ts
            .store()
            .create_known_device(KnownDeviceInput {
                display_name: "other".into(),
                pinned: false,
                notes: None,
                identifiers: Vec::new(),
            })
            .await
            .expect("second device should create");

        let err = ts
            .store()
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

        let listed = ts
            .store()
            .list_known_devices()
            .await
            .expect("list should work");
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
    }

    #[tokio::test]
    async fn device_identifier_can_be_detached_manually() {
        let ts = TestStore::new().await;
        let created = ts
            .store()
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![
                    DeviceIdentifierInput {
                        kind: "mac".into(),
                        value: "aa:bb:cc:dd:ee:ff".into(),
                    },
                    DeviceIdentifierInput {
                        kind: "ip".into(),
                        value: "192.168.1.2".into(),
                    },
                ],
            })
            .await
            .expect("known device should create");

        let updated = ts
            .store()
            .detach_device_identifier(&created.device_id, "ip:192.168.1.2")
            .await
            .expect("identifier detach should succeed")
            .expect("device should exist");

        assert_eq!(updated.identifiers.len(), 1);
        assert_eq!(
            updated.identifiers[0].identifier_key,
            "mac:aa:bb:cc:dd:ee:ff"
        );

        let unmatched = ts
            .store()
            .lookup_known_device_by_identifier(DeviceIdentifierInput {
                kind: "ip".into(),
                value: "192.168.1.2".into(),
            })
            .await
            .expect("lookup should succeed");
        assert!(unmatched.is_none());
    }

    #[tokio::test]
    async fn merge_known_devices_moves_identifiers_and_deletes_source() {
        let ts = TestStore::new().await;
        let target = ts
            .store()
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:01".into(),
                }],
            })
            .await
            .expect("target should create");
        let source = ts
            .store()
            .create_known_device(KnownDeviceInput {
                display_name: "lda duplicate".into(),
                pinned: false,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:02".into(),
                }],
            })
            .await
            .expect("source should create");

        let merged = ts
            .store()
            .merge_known_devices(&target.device_id, &source.device_id)
            .await
            .expect("merge should succeed")
            .expect("target should remain");

        assert_eq!(merged.device_id, target.device_id);
        assert_eq!(merged.identifiers.len(), 2);
        assert!(
            merged
                .identifiers
                .iter()
                .any(|identifier| identifier.value == "aa:bb:cc:dd:ee:02")
        );
        assert!(
            ts.store()
                .get_known_device(&source.device_id)
                .await
                .expect("source lookup should work")
                .is_none()
        );
    }

    #[tokio::test]
    async fn audit_events_append_and_filter() {
        let ts = TestStore::new().await;

        ts.store()
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

        ts.store()
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

        let all = ts
            .store()
            .list_audit_events(crate::state::AuditEventFilter {
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("list all should succeed");
        assert_eq!(all.len(), 2);

        let filtered = ts
            .store()
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

        let rejected = ts
            .store()
            .list_audit_events(crate::state::AuditEventFilter {
                outcome: Some("rejected".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("rejected list should succeed");
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].latency_ms, None);
    }

    #[tokio::test]
    async fn alert_transitions_track_open_and_resolve() {
        let ts = TestStore::new().await;
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

        let opened = ts
            .store()
            .sync_alert_transitions(std::slice::from_ref(&alert))
            .await
            .expect("open transition should succeed");
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].to_status, "active");

        let resolved = ts
            .store()
            .sync_alert_transitions(&[])
            .await
            .expect("resolve transition should succeed");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].to_status, "resolved");

        let history = ts
            .store()
            .list_alert_transitions(None, 10)
            .await
            .expect("history should load");
        assert!(history.len() >= 2);
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

        let _ = fs::remove_dir_all(&dir);
    }
}
