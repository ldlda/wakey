use anyhow::Result;
use wakey_core::{InventoryQuery, Query, QueryInput};

/// Resolve free-form user input into an `InventoryQuery` filter shape.
///
/// This is the compatibility entrypoint used by CLI and HTTP paths that still
/// speak in terms of query/filter payloads.
pub async fn resolve_query(input: impl Into<String>) -> Result<InventoryQuery> {
    query_to_inventory_query(resolve_selector(input).await?)
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

/// Convert the newer selector-oriented `Query` model into an `InventoryQuery`.
///
/// This keeps the old filter-based service and HTTP surfaces working while the
/// internals migrate toward selector- and device-oriented APIs.
pub fn query_to_inventory_query(query: Query) -> Result<InventoryQuery> {
    Ok(vec![query])
}
