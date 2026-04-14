use anyhow::Result;
use wakey_core::{InventoryQuery, Query, QueryInput};

/// Resolve free-form user input into an `InventoryQuery` filter shape.
pub async fn resolve_query(input: impl Into<String>) -> Result<InventoryQuery> {
    Ok(vec![resolve_selector(input).await?])
}

/// Classify one piece of free-form user input into a typed selector.
///
/// The Linux adapter decides whether the input looks like an IP address, MAC,
/// interface name, neighbor state, or plain text.
pub async fn resolve_selector(input: impl Into<String>) -> Result<Query> {
    Ok(
        match wakey_linux::devices::classify_query(input.into()).await {
            QueryInput::Ip(ip_addr) => Query::Ip(ip_addr),
            QueryInput::Mac(mac_addr) => Query::Mac(mac_addr),
            QueryInput::Dev(dev) => Query::Interface(dev),
            QueryInput::Nud(state) => Query::NeighborState(state),
            QueryInput::Name(name) => Query::Text(name),
        },
    )
}
