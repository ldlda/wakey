use super::*;
use wakey_core::protocol::DEFAULT_TERMINAL_SESSION_TTL_SECONDS;

const OPERATOR_A: &str = "browser-tab-a";
const OPERATOR_B: &str = "browser-tab-b";

#[tokio::test]
async fn credentials_are_scoped_and_single_use() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");

    assert!(
        registry
            .attach_agent(&created.terminal_id, "other", &created.relay_token)
            .await
            .is_err()
    );
    let _ = registry
        .attach_agent(&created.terminal_id, "router", &created.relay_token)
        .await
        .expect("attach agent");
    assert_eq!(
        registry
            .attach_agent(&created.terminal_id, "router", &created.relay_token)
            .await
            .expect_err("relay token is single use"),
        "terminal_agent_already_attached"
    );

    registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach operator");
    assert!(
        registry
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A,)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn operator_token_is_bound_to_its_tab_and_stays_consumed_after_detach() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    let id = &created.terminal_id;
    let token = registry
        .issue_attachment_token_for_operator(id, OPERATOR_A)
        .await
        .expect("issue attachment token");

    assert_eq!(
        registry
            .attach_operator(id, &token, OPERATOR_B)
            .await
            .unwrap_err(),
        "terminal_operator_mismatch",
    );
    let _receiver = registry
        .attach_operator(id, &token, OPERATOR_A)
        .await
        .unwrap();
    assert!(registry.detach_operator(id, &token).await.is_some());
    assert_eq!(
        registry
            .attach_operator(id, &token, OPERATOR_A)
            .await
            .unwrap_err(),
        "terminal_attachment_token_invalid",
    );
}

#[tokio::test]
async fn detached_output_is_not_transcribed_by_control_plane() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    registry
        .relay_from_agent(
            &created.terminal_id,
            TerminalRelayFrame::Binary(b"not retained".to_vec()),
        )
        .await
        .expect("ignore detached output");

    let mut outbound = registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach operator");
    assert!(outbound.try_recv().is_err());
}

#[tokio::test]
async fn attached_output_is_delivered_live_only() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    let mut outbound = registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach operator");
    let frame = TerminalRelayFrame::Binary(b"recent prompt".to_vec());
    registry
        .relay_from_agent(&created.terminal_id, frame)
        .await
        .expect("relay output");
    outbound.recv().await.expect("live output");
    registry
        .detach_operator(&created.terminal_id, &created.attachment_token)
        .await;

    let token = registry
        .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
        .await
        .expect("reattach token");
    let mut remounted = registry
        .attach_operator(&created.terminal_id, &token, OPERATOR_A)
        .await
        .expect("reattach operator");

    assert!(remounted.try_recv().is_err());
}

#[tokio::test]
async fn operator_input_waits_for_agent_attachment() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    let resize = TerminalRelayFrame::Text(r#"{"type":"resize","rows":30,"cols":120}"#.into());
    registry
        .relay_to_agent(&created.terminal_id, resize)
        .await
        .expect("queue resize before agent attachment");

    let (_, pending) = registry
        .attach_agent(&created.terminal_id, "router", &created.relay_token)
        .await
        .expect("attach agent");
    assert_eq!(pending.len(), 1);
    assert!(matches!(pending[0], TerminalRelayFrame::Text(_)));
}

#[tokio::test]
async fn agent_can_hold_two_terminal_sessions() {
    let registry = TerminalRegistry::new();
    registry.create("router".into()).await.expect("first");
    registry.create("router".into()).await.expect("second");
    let third = registry.create("router".into()).await;
    assert_eq!(third.err(), Some("agent_terminal_limit_reached"));
}

#[tokio::test]
async fn advertised_limit_controls_session_capacity() {
    let registry = TerminalRegistry::new();
    for _ in 0..3 {
        registry
            .create_with_limits("router".into(), 3, DEFAULT_TERMINAL_SESSION_TTL_SECONDS)
            .await
            .expect("within advertised limit");
    }
    let fourth = registry
        .create_with_limits("router".into(), 3, DEFAULT_TERMINAL_SESSION_TTL_SECONDS)
        .await;
    assert_eq!(fourth.err(), Some("agent_terminal_limit_reached"));
}

#[tokio::test]
async fn session_ttl_is_captured_and_zero_is_unlimited() {
    let registry = TerminalRegistry::new();
    let finite = registry
        .create_with_limits("router".into(), 2, 67)
        .await
        .expect("finite session");
    assert_eq!(
        finite.expires_at_unix,
        finite.created_at_unix.checked_add(67)
    );

    registry.remove(&finite.terminal_id).await;
    let unlimited = registry
        .create_with_limits("router".into(), 2, 0)
        .await
        .expect("unlimited session");
    assert_eq!(unlimited.expires_at_unix, None);
    assert_eq!(
        registry
            .summary(&unlimited.terminal_id)
            .await
            .expect("unlimited summary")
            .expires_at_unix,
        None
    );
}

#[tokio::test]
async fn unlimited_inventory_can_adopt_an_old_session() {
    let registry = TerminalRegistry::new();
    let terminal_id = TerminalId::new("old-unlimited").expect("terminal id");
    let reported = AgentTerminalSession {
        terminal_id: terminal_id.clone(),
        created_at_unix: 0,
        session_ttl_seconds: 0,
    };

    let credentials = registry
        .reconcile_agent_sessions("router", &[reported])
        .await;
    assert_eq!(credentials.len(), 1);
    assert_eq!(
        registry
            .summary(terminal_id.as_str())
            .await
            .expect("adopted unlimited session")
            .expires_at_unix,
        None
    );
}

#[tokio::test]
async fn removed_session_is_distinct_from_unknown_session() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");

    assert!(!registry.was_closed(&created.terminal_id).await);
    assert!(!registry.was_closed("never-existed").await);
    registry.remove(&created.terminal_id).await.expect("remove");
    assert!(registry.was_closed(&created.terminal_id).await);
    assert!(!registry.was_closed("never-existed").await);
}

#[tokio::test]
async fn agent_inventory_adopts_and_reconnects_live_session() {
    let registry = TerminalRegistry::new();
    let terminal_id = TerminalId::new("survived-cc").expect("terminal id");
    let created_at_unix = u64::MAX;
    let reported = AgentTerminalSession {
        terminal_id: terminal_id.clone(),
        created_at_unix,
        session_ttl_seconds: 0,
    };

    let credentials = registry
        .reconcile_agent_sessions("router", &[reported])
        .await;
    assert_eq!(credentials.len(), 1);
    let (_, relay_token) = &credentials[0];
    registry
        .attach_agent(terminal_id.as_str(), "router", relay_token)
        .await
        .expect("attach adopted agent session");

    let summary = registry
        .summary(terminal_id.as_str())
        .await
        .expect("adopted summary");
    assert_eq!(summary.agent_id, "router");
    assert_eq!(summary.created_at_unix, created_at_unix);
    assert!(summary.agent_attached);
}

#[tokio::test]
async fn expired_sessions_release_agent_quota() {
    let registry = TerminalRegistry::new();
    let first = registry.create("router".into()).await.expect("first");
    registry.create("router".into()).await.expect("second");
    registry
        .inner
        .lock()
        .await
        .get_mut(&first.terminal_id)
        .expect("first session")
        .expiry = TerminalExpiry::Finite {
        deadline: Instant::now(),
        unix: first.created_at_unix,
    };

    registry
        .create("router".into())
        .await
        .expect("expired session no longer consumes quota");
    assert!(registry.summary(&first.terminal_id).await.is_none());
}

#[tokio::test]
async fn reconciliation_does_not_revive_expired_agent_session() {
    let registry = TerminalRegistry::new();
    let terminal_id = TerminalId::new("expired-agent-session").expect("terminal id");
    let reported = AgentTerminalSession {
        terminal_id: terminal_id.clone(),
        created_at_unix: 0,
        session_ttl_seconds: DEFAULT_TERMINAL_SESSION_TTL_SECONDS,
    };

    assert!(
        registry
            .reconcile_agent_sessions("router", &[reported])
            .await
            .is_empty()
    );
    assert!(registry.summary(terminal_id.as_str()).await.is_none());
}

#[tokio::test]
async fn operator_detach_does_not_remove_session() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach operator");

    registry
        .detach_operator(&created.terminal_id, &created.attachment_token)
        .await;

    let summary = registry
        .summary(&created.terminal_id)
        .await
        .expect("session remains after detach");
    assert!(!summary.operator_attached);
    registry
        .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
        .await
        .expect("detached session can be reattached");
}

#[tokio::test]
async fn attachment_handoff_releases_previous_operator_atomically() {
    let registry = TerminalRegistry::new();
    let previous = registry.create("router".into()).await.expect("previous");
    let next = registry.create("router".into()).await.expect("next");
    registry
        .attach_operator(
            &previous.terminal_id,
            &previous.attachment_token,
            OPERATOR_A,
        )
        .await
        .expect("attach previous");

    let next_token = registry
        .issue_attachment_token_for_operator(&next.terminal_id, OPERATOR_A)
        .await
        .expect("handoff token");

    assert!(
        !registry
            .summary(&previous.terminal_id)
            .await
            .unwrap()
            .operator_attached
    );
    registry
        .attach_operator(&next.terminal_id, &next_token, OPERATOR_A)
        .await
        .expect("attach next");
}

#[tokio::test]
async fn attachment_handoff_does_not_evict_unrelated_operator() {
    let registry = TerminalRegistry::new();
    let previous = registry.create("router".into()).await.expect("previous");
    let occupied = registry.create("router".into()).await.expect("occupied");
    registry
        .attach_operator(
            &previous.terminal_id,
            &previous.attachment_token,
            OPERATOR_A,
        )
        .await
        .expect("attach previous");
    registry
        .attach_operator(
            &occupied.terminal_id,
            &occupied.attachment_token,
            OPERATOR_B,
        )
        .await
        .expect("attach occupied");

    let error = registry
        .issue_attachment_token_for_operator(&occupied.terminal_id, OPERATOR_A)
        .await
        .expect_err("occupied target remains protected");

    assert_eq!(error, "terminal_operator_already_attached");
    assert!(
        registry
            .summary(&previous.terminal_id)
            .await
            .unwrap()
            .operator_attached
    );
    assert!(
        registry
            .summary(&occupied.terminal_id)
            .await
            .unwrap()
            .operator_attached
    );
}

#[tokio::test]
async fn stale_socket_cleanup_does_not_detach_replacement() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");
    let old_connection_id = created.attachment_token.clone();
    let _old_outbound = registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach old socket");

    let replacement_token = registry
        .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
        .await
        .expect("replace own socket");
    let _replacement_outbound = registry
        .attach_operator(&created.terminal_id, &replacement_token, OPERATOR_A)
        .await
        .expect("attach replacement socket");

    assert!(
        registry
            .detach_operator(&created.terminal_id, &old_connection_id)
            .await
            .is_none()
    );
    assert!(
        registry
            .summary(&created.terminal_id)
            .await
            .unwrap()
            .operator_attached
    );
}

#[tokio::test]
async fn reconciliation_does_not_remove_open_request_still_in_flight() {
    let registry = TerminalRegistry::new();
    let created = registry.create("router".into()).await.expect("create");

    registry.reconcile_agent_sessions("router", &[]).await;

    assert!(registry.summary(&created.terminal_id).await.is_some());
}

#[tokio::test]
async fn session_events_invalidate_mutations_but_not_reads_or_terminal_bytes() {
    let registry = TerminalRegistry::new();
    let mut events = registry.subscribe();
    let created = registry.create("router".into()).await.expect("create");
    assert!(events.try_recv().is_ok());

    registry.summaries().await;
    registry.summary(&created.terminal_id).await;
    assert!(events.try_recv().is_err(), "ordinary reads emit no event");

    let mut operator = registry
        .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
        .await
        .expect("attach operator");
    assert!(events.try_recv().is_ok());
    registry
        .relay_from_agent(
            &created.terminal_id,
            TerminalRelayFrame::Binary(b"terminal output".to_vec()),
        )
        .await
        .expect("relay terminal bytes");
    operator.recv().await.expect("terminal bytes delivered");
    assert!(events.try_recv().is_err(), "terminal bytes emit no event");

    registry
        .detach_operator(&created.terminal_id, &created.attachment_token)
        .await
        .expect("detach operator");
    assert!(events.try_recv().is_ok());
    registry.remove(&created.terminal_id).await.expect("remove");
    assert!(events.try_recv().is_ok());
}

#[tokio::test]
async fn lagged_session_receiver_recovers_with_latest_invalidation() {
    let registry = TerminalRegistry::new();
    let mut events = registry.subscribe();
    for _ in 0..(TERMINAL_EVENTS_CAPACITY + 3) {
        registry
            .create_with_limits("router".into(), TERMINAL_EVENTS_CAPACITY + 3, 0)
            .await
            .expect("create");
    }

    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Lagged(_))
    ));
    assert!(events.try_recv().is_ok());
    assert_eq!(
        registry.summaries().await.len(),
        TERMINAL_EVENTS_CAPACITY + 3
    );
}

#[tokio::test]
async fn agent_attach_detach_and_reconciliation_invalidate_catalog_changes() {
    let registry = TerminalRegistry::new();
    let mut events = registry.subscribe();
    let terminal_id = TerminalId::new("reconciled-session").expect("terminal id");
    let reported = AgentTerminalSession {
        terminal_id: terminal_id.clone(),
        created_at_unix: 0,
        session_ttl_seconds: 0,
    };

    let credentials = registry
        .reconcile_agent_sessions("router", std::slice::from_ref(&reported))
        .await;
    assert_eq!(credentials.len(), 1);
    assert!(events.try_recv().is_ok(), "adoption changes the catalog");

    let (_, relay_token) = &credentials[0];
    registry
        .attach_agent(terminal_id.as_str(), "router", relay_token)
        .await
        .expect("attach agent");
    assert!(events.try_recv().is_ok());
    registry
        .detach_agent(terminal_id.as_str())
        .await
        .expect("detach agent");
    assert!(events.try_recv().is_ok());

    registry.reconcile_agent_sessions("router", &[]).await;
    assert!(
        events.try_recv().is_ok(),
        "reconciliation removes stale session"
    );
    assert!(registry.summary(terminal_id.as_str()).await.is_none());
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn pruning_sibling_keeps_requested_summary_and_notifies_once() {
    let registry = TerminalRegistry::new();
    let mut events = registry.subscribe();
    let expired = registry.create("router".into()).await.expect("expired");
    let live = registry.create("router".into()).await.expect("live");
    events.try_recv().expect("expired create event");
    events.try_recv().expect("live create event");
    registry
        .inner
        .lock()
        .await
        .get_mut(&expired.terminal_id)
        .expect("expired session")
        .expiry = TerminalExpiry::Finite {
        deadline: Instant::now(),
        unix: expired.created_at_unix,
    };

    let summary = registry
        .summary(&live.terminal_id)
        .await
        .expect("live sibling remains available");
    assert_eq!(summary.terminal_id, live.terminal_id);
    assert!(matches!(events.try_recv(), Ok(())));
    assert!(events.try_recv().is_err());
}
