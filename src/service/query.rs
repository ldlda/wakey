use anyhow::Result;
use wakey_core::{DeviceFilters, DeviceQuery, Query, QueryInput};

pub async fn resolve_query(input: impl Into<String>) -> Result<DeviceQuery> {
    query_to_device_query(resolve_selector(input).await?)
}

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

pub fn query_to_device_query(query: Query) -> Result<DeviceQuery> {
    Ok(match query {
        Query::Ip(ip_addr) => DeviceQuery {
            filter: DeviceFilters {
                ips: vec![ip_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Mac(mac_addr) => DeviceQuery {
            filter: DeviceFilters {
                macs: vec![mac_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Interface(dev) => DeviceQuery {
            filter: DeviceFilters {
                devs: vec![dev],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::NeighborState(state) => DeviceQuery {
            filter: DeviceFilters {
                nuds: vec![state],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Text(name) => DeviceQuery {
            name: Some(name),
            ..Default::default()
        },
    })
}
