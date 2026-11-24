#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalRegistryError {
    AgentLimitReached,
    InvalidTtl,
    OperatorAlreadyAttached,
    AgentMismatch,
    AgentAlreadyAttached,
    InvalidRelayToken,
    AgentDisconnected,
    NotFound,
    Expired,
    InvalidOperatorId,
    InvalidAttachmentToken,
    OperatorMismatch,
}

impl TerminalRegistryError {
    /// Stable error code for HTTP responses, WebSocket close reasons, and logs.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::AgentLimitReached => "agent_terminal_limit_reached",
            Self::InvalidTtl => "terminal_ttl_invalid",
            Self::OperatorAlreadyAttached => "terminal_operator_already_attached",
            Self::AgentMismatch => "terminal_agent_mismatch",
            Self::AgentAlreadyAttached => "terminal_agent_already_attached",
            Self::InvalidRelayToken => "terminal_relay_token_invalid",
            Self::AgentDisconnected => "terminal_agent_disconnected",
            Self::NotFound => "terminal_not_found",
            Self::Expired => "terminal_expired",
            Self::InvalidOperatorId => "terminal_operator_id_invalid",
            Self::InvalidAttachmentToken => "terminal_attachment_token_invalid",
            Self::OperatorMismatch => "terminal_operator_mismatch",
        }
    }
}
