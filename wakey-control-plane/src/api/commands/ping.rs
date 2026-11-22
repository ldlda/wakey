use axum::http::StatusCode;
use tokio::sync::{Semaphore, SemaphorePermit};
use wakey_agent::protocol::{AgentCapability, AgentCommand};

use crate::api::ApiError;

/// Hold the permit for the lifetime of the relay request, including its reply.
pub(super) fn admit<'a>(
    command: &AgentCommand,
    capabilities: &[AgentCapability],
    slots: &'a Semaphore,
) -> Result<Option<SemaphorePermit<'a>>, ApiError> {
    if !matches!(command, AgentCommand::Ping(_)) {
        return Ok(None);
    }
    if !capabilities.contains(&AgentCapability::Ping) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ping_unsupported",
            "this agent does not advertise ping support",
        ));
    }
    slots.try_acquire().map(Some).map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "ping_busy",
            "too many pings are in progress",
        )
    })
}
