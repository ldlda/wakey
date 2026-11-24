use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tracing::{Instrument, info, warn};
use wakey_core::protocol::{
    ServerMessage, TerminalAgentHandshake, TerminalControl, TerminalOperatorHandshake,
};

use crate::runtime::terminals::{
    TERMINAL_ATTACH_TIMEOUT, TERMINAL_MAX_FRAME_BYTES, TerminalRelayFrame,
};
use crate::runtime::{AppState, SessionEvent};

use super::{
    TerminalAudit, TerminalCloseReason, append_terminal_audit, close_registered_terminal,
    validate_size,
};

#[derive(Debug, PartialEq, Eq)]
enum TerminalTransportError {
    HandshakeInvalid,
    HandshakeRequired,
    HandshakeTimeout,
    ControlInvalid,
    ControlDirectionInvalid,
    SizeInvalid,
}

impl TerminalTransportError {
    fn code(&self) -> &'static str {
        match self {
            Self::HandshakeInvalid => "invalid handshake",
            Self::HandshakeRequired => "terminal_handshake_required",
            Self::HandshakeTimeout => "terminal_handshake_timeout",
            Self::ControlInvalid => "terminal_control_invalid",
            Self::ControlDirectionInvalid => "terminal_control_direction_invalid",
            Self::SizeInvalid => "terminal_size_invalid",
        }
    }
}

pub async fn agent_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Response {
    ws.max_message_size(TERMINAL_MAX_FRAME_BYTES)
        .on_upgrade(move |socket| handle_agent_terminal_socket(state, terminal_id, socket))
}

pub async fn operator_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Response {
    ws.max_message_size(TERMINAL_MAX_FRAME_BYTES)
        .on_upgrade(move |socket| handle_operator_terminal_socket(state, terminal_id, socket))
}

pub async fn terminal_events_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_terminal_events_socket(state, socket))
}

async fn handle_terminal_events_socket(state: AppState, socket: WebSocket) {
    let mut events = state.terminals.subscribe();
    let (mut write, mut read) = socket.split();
    if write
        .send(Message::Text(r#"{"type":"sessions_changed"}"#.into()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        if write.send(Message::Text(r#"{"type":"sessions_changed"}"#.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = read.next() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        if write.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {}
                }
            }
        }
    }
}

async fn handle_agent_terminal_socket(state: AppState, terminal_id: String, mut socket: WebSocket) {
    let auth = match receive_handshake::<TerminalAgentHandshake>(&mut socket).await {
        Ok(TerminalAgentHandshake::Auth {
            agent_id,
            relay_token,
        }) => (agent_id, relay_token),
        Err(error) => {
            close_socket(&mut socket, error.code()).await;
            return;
        }
    };
    let (mut outbound, pending) = match state
        .terminals
        .attach_agent(&terminal_id, &auth.0, &auth.1)
        .instrument(
            tracing::info_span!("terminal_agent_transport_attach", terminal_id = %terminal_id),
        )
        .await
    {
        Ok(outbound) => outbound,
        Err(error) => {
            let code = error.code();
            warn!(
                terminal_id,
                code, "agent terminal transport attachment rejected"
            );
            close_socket(&mut socket, code).await;
            return;
        }
    };
    info!(terminal_id, agent_id = %auth.0, "agent terminal socket attached");
    append_terminal_audit(
        &state,
        &auth.0,
        &terminal_id,
        TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_agent_attach",
            outcome: "ok",
            message: "agent terminal transport attached",
            metadata: serde_json::json!({}),
        },
    )
    .await;

    let (mut write, mut read) = socket.split();
    for frame in pending {
        let closes = matches!(frame, TerminalRelayFrame::Close);
        if send_relay_frame(&mut write, frame).await.is_err() || closes {
            state.terminals.remove(&terminal_id).await;
            return;
        }
    }
    loop {
        tokio::select! {
            biased;
            outbound_frame = outbound.recv() => {
                let Some(frame) = outbound_frame else { break; };
                if send_relay_frame(&mut write, frame).await.is_err() { break; }
            }
            incoming = read.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match agent_relay_frame(message) {
                    Ok(Some(frame)) => {
                        let closes = matches!(frame, TerminalRelayFrame::Close);
                        audit_agent_control_frame(&state, &auth.0, &terminal_id, &frame).await;
                        if state.terminals.relay_from_agent(&terminal_id, frame).await.is_err() { break; }
                        if closes { break; }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        warn!(terminal_id, code = error.code(), "invalid agent terminal frame");
                        break;
                    }
                }
            }
        }
    }
    if let Some(agent_id) = state.terminals.detach_agent(&terminal_id).await
        && let Some(session) = state.sessions.read().await.get(&agent_id)
    {
        let _ = session
            .tx
            .send(SessionEvent::Message(ServerMessage::SyncTerminalSessions));
    }
    info!(terminal_id, "agent terminal relay detached");
}

async fn handle_operator_terminal_socket(
    state: AppState,
    terminal_id: String,
    mut socket: WebSocket,
) {
    let auth = match receive_handshake::<TerminalOperatorHandshake>(&mut socket).await {
        Ok(TerminalOperatorHandshake::Attach {
            attachment_token,
            operator_id,
        }) => (attachment_token, operator_id),
        Err(error) => {
            close_socket(&mut socket, error.code()).await;
            return;
        }
    };
    let mut outbound = match state
        .terminals
        .attach_operator(&terminal_id, &auth.0, &auth.1)
        .instrument(
            tracing::info_span!("terminal_operator_transport_attach", terminal_id = %terminal_id),
        )
        .await
    {
        Ok(attached) => attached,
        Err(error) => {
            let code = error.code();
            warn!(
                terminal_id,
                code, "operator terminal transport attachment rejected"
            );
            close_socket(&mut socket, code).await;
            return;
        }
    };
    info!(terminal_id, "operator terminal socket attached");
    let snapshot = serde_json::to_string(&TerminalControl::Snapshot)
        .expect("terminal snapshot control serializes");
    if let Err(error) = state
        .terminals
        .relay_to_agent(&terminal_id, TerminalRelayFrame::Text(snapshot))
        .instrument(tracing::info_span!("terminal_snapshot_request", terminal_id = %terminal_id))
        .await
    {
        warn!(
            terminal_id,
            code = error.code(),
            "failed to request terminal snapshot"
        );
    }
    let summary = state.terminals.summary(&terminal_id).await;
    if let Some(summary) = &summary {
        append_terminal_audit(
            &state,
            &summary.agent_id,
            &terminal_id,
            TerminalAudit {
                actor_type: "admin_api",
                event_type: "terminal_operator_attach",
                outcome: "ok",
                message: "operator terminal transport attached",
                metadata: serde_json::json!({}),
            },
        )
        .await;
    }

    let (mut write, mut read) = socket.split();
    if summary.is_some_and(|summary| summary.agent_attached) {
        let ready = serde_json::to_string(&TerminalControl::Ready)
            .expect("terminal ready control serializes");
        if send_relay_frame(&mut write, TerminalRelayFrame::Text(ready))
            .await
            .is_err()
        {
            state.terminals.detach_operator(&terminal_id, &auth.0).await;
            return;
        }
    }
    let mut explicit_close = false;
    loop {
        tokio::select! {
            biased;
            incoming = read.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match operator_relay_frame(message) {
                    Ok(Some(frame)) => {
                        explicit_close = matches!(frame, TerminalRelayFrame::Close);
                        if state.terminals.relay_to_agent(&terminal_id, frame).await.is_err() { break; }
                        if explicit_close { break; }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        warn!(terminal_id, code = error.code(), "invalid operator terminal frame");
                        break;
                    }
                }
            }
            outbound_frame = outbound.recv() => {
                let Some(frame) = outbound_frame else { break; };
                if send_relay_frame(&mut write, frame).await.is_err() { break; }
            }
        }
    }

    if explicit_close {
        close_registered_terminal(&state, &terminal_id, TerminalCloseReason::BrowserClose).await;
    } else {
        state.terminals.detach_operator(&terminal_id, &auth.0).await;
    }
    info!(
        terminal_id,
        explicit_close, "operator terminal socket detached"
    );
}

fn agent_relay_frame(
    message: Message,
) -> Result<Option<TerminalRelayFrame>, TerminalTransportError> {
    match message {
        Message::Binary(bytes) => Ok(Some(TerminalRelayFrame::Binary(bytes.to_vec()))),
        Message::Text(text) => {
            let control: TerminalControl =
                serde_json::from_str(&text).map_err(|_| TerminalTransportError::ControlInvalid)?;
            if !matches!(
                control,
                TerminalControl::Ready
                    | TerminalControl::Exited { .. }
                    | TerminalControl::Error { .. }
                    | TerminalControl::Close
            ) {
                return Err(TerminalTransportError::ControlDirectionInvalid);
            }
            Ok(Some(TerminalRelayFrame::Text(
                serde_json::to_string(&control)
                    .map_err(|_| TerminalTransportError::ControlInvalid)?,
            )))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(Some(TerminalRelayFrame::Close)),
    }
}

fn operator_relay_frame(
    message: Message,
) -> Result<Option<TerminalRelayFrame>, TerminalTransportError> {
    match message {
        Message::Binary(bytes) => Ok(Some(TerminalRelayFrame::Binary(bytes.to_vec()))),
        Message::Text(text) => {
            let control: TerminalControl =
                serde_json::from_str(&text).map_err(|_| TerminalTransportError::ControlInvalid)?;
            if !matches!(
                control,
                TerminalControl::Resize { .. } | TerminalControl::Close
            ) {
                return Err(TerminalTransportError::ControlDirectionInvalid);
            }
            if let TerminalControl::Resize { rows, cols } = control {
                validate_size(rows, cols).map_err(|_| TerminalTransportError::SizeInvalid)?;
            }
            Ok(Some(TerminalRelayFrame::Text(
                serde_json::to_string(&control)
                    .map_err(|_| TerminalTransportError::ControlInvalid)?,
            )))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(None),
    }
}

async fn receive_handshake<T: DeserializeOwned>(
    socket: &mut WebSocket,
) -> Result<T, TerminalTransportError> {
    match tokio::time::timeout(TERMINAL_ATTACH_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            serde_json::from_str(&text).map_err(|_| TerminalTransportError::HandshakeInvalid)
        }
        Ok(_) => Err(TerminalTransportError::HandshakeRequired),
        Err(_) => Err(TerminalTransportError::HandshakeTimeout),
    }
}

async fn close_socket(socket: &mut WebSocket, code: &str) {
    let control = TerminalControl::Error {
        code: code.into(),
        message: code.replace('_', " "),
    };
    if let Ok(json) = serde_json::to_string(&control) {
        let _ = socket.send(Message::Text(json.into())).await;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send_relay_frame<S>(write: &mut S, frame: TerminalRelayFrame) -> Result<(), axum::Error>
where
    S: futures_util::Sink<Message, Error = axum::Error> + Unpin,
{
    let message = match frame {
        TerminalRelayFrame::Binary(bytes) => Message::Binary(bytes.into()),
        TerminalRelayFrame::Text(text) => Message::Text(text.into()),
        TerminalRelayFrame::Close => Message::Close(None),
    };
    write.send(message).await
}

async fn audit_agent_control_frame(
    state: &AppState,
    agent_id: &str,
    terminal_id: &str,
    frame: &TerminalRelayFrame,
) {
    let TerminalRelayFrame::Text(text) = frame else {
        return;
    };
    let Ok(control) = serde_json::from_str::<TerminalControl>(text) else {
        return;
    };
    let audit = match control {
        TerminalControl::Ready => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_ready",
            outcome: "ok",
            message: "terminal PTY ready",
            metadata: serde_json::json!({}),
        },
        TerminalControl::Exited { exit_code } => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_exit",
            outcome: "exited",
            message: "terminal process exited",
            metadata: serde_json::json!({ "exit_code": exit_code }),
        },
        TerminalControl::Error { code, .. } => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_error",
            outcome: "error",
            message: "terminal worker reported an error",
            metadata: serde_json::json!({ "code": code }),
        },
        _ => return,
    };
    append_terminal_audit(state, agent_id, terminal_id, audit).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_control_direction_is_enforced() {
        let resize = serde_json::to_string(&TerminalControl::Resize { rows: 24, cols: 80 })
            .expect("resize json");
        assert!(operator_relay_frame(Message::Text(resize.into())).is_ok());

        let ready = serde_json::to_string(&TerminalControl::Ready).expect("ready json");
        assert_eq!(
            operator_relay_frame(Message::Text(ready.into())).unwrap_err(),
            TerminalTransportError::ControlDirectionInvalid,
        );

        let snapshot = serde_json::to_string(&TerminalControl::Snapshot).expect("snapshot json");
        assert_eq!(
            operator_relay_frame(Message::Text(snapshot.into()))
                .unwrap_err()
                .code(),
            "terminal_control_direction_invalid",
        );
    }

    #[test]
    fn malformed_control_and_invalid_resize_have_distinct_codes() {
        let malformed = agent_relay_frame(Message::Text("{".into())).unwrap_err();
        assert_eq!(malformed.code(), "terminal_control_invalid");

        let resize = serde_json::to_string(&TerminalControl::Resize { rows: 0, cols: 80 })
            .expect("resize json");
        let invalid_size = operator_relay_frame(Message::Text(resize.into())).unwrap_err();
        assert_eq!(invalid_size.code(), "terminal_size_invalid");
    }
}
