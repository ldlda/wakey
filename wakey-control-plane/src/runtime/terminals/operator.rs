use tokio::sync::mpsc;

use super::{TERMINAL_RELAY_QUEUE, TerminalRelayFrame};

#[derive(Default)]
pub(super) struct OperatorAttachment {
    pending: Option<PendingAttachment>,
    live: Option<LiveAttachment>,
}

struct PendingAttachment {
    token: String,
    operator_id: Option<String>,
}

struct LiveAttachment {
    sender: mpsc::Sender<TerminalRelayFrame>,
    operator_id: String,
    connection_id: String,
}

impl OperatorAttachment {
    pub(super) fn issue_token(&mut self, token: String, operator_id: Option<&str>) {
        self.pending = Some(PendingAttachment {
            token,
            operator_id: operator_id.map(str::to_owned),
        });
    }

    pub(super) fn validate_token(
        &self,
        token: &str,
        operator_id: &str,
    ) -> Result<(), &'static str> {
        if self.is_attached() {
            return Err("terminal_operator_already_attached");
        }
        let pending = self
            .pending
            .as_ref()
            .filter(|pending| pending.token == token)
            .ok_or("terminal_attachment_token_invalid")?;
        if pending
            .operator_id
            .as_deref()
            .is_some_and(|id| id != operator_id)
        {
            return Err("terminal_operator_mismatch");
        }
        Ok(())
    }

    // The registry keeps its lock from validation through handoff and attach.
    pub(super) fn attach(&mut self, operator_id: &str) -> mpsc::Receiver<TerminalRelayFrame> {
        let pending = self.pending.take().expect("validated attachment token");
        let (sender, receiver) = mpsc::channel(TERMINAL_RELAY_QUEUE);
        self.live = Some(LiveAttachment {
            sender,
            operator_id: operator_id.to_owned(),
            // The consumed token identifies this socket during detach.
            connection_id: pending.token,
        });
        receiver
    }

    pub(super) fn is_attached(&self) -> bool {
        self.live.is_some()
    }

    pub(super) fn is_held_by_other(&self, operator_id: &str) -> bool {
        self.live
            .as_ref()
            .is_some_and(|live| live.operator_id != operator_id)
    }

    pub(super) fn sender(&self) -> Option<mpsc::Sender<TerminalRelayFrame>> {
        self.live.as_ref().map(|live| live.sender.clone())
    }

    pub(super) fn into_sender(self) -> Option<mpsc::Sender<TerminalRelayFrame>> {
        self.live.map(|live| live.sender)
    }

    pub(super) fn detach_operator(&mut self, operator_id: &str) -> bool {
        self.detach_if(|live| live.operator_id == operator_id)
    }

    pub(super) fn detach_connection(&mut self, connection_id: &str) -> bool {
        self.detach_if(|live| live.connection_id == connection_id)
    }

    pub(super) fn detach_sender(&mut self, sender: &mpsc::Sender<TerminalRelayFrame>) -> bool {
        self.detach_if(|live| live.sender.same_channel(sender))
    }

    fn detach_if(&mut self, matches: impl FnOnce(&LiveAttachment) -> bool) -> bool {
        if self.live.as_ref().is_some_and(matches) {
            self.live = None;
            true
        } else {
            false
        }
    }
}
