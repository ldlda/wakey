use macaddr::MacAddr;
use serde::Deserialize;
use std::net::IpAddr;

use crate::model::NeighborState;

/// Canonical inventory query represented as an AND of selector terms.
pub type InventoryQuery = Vec<Query>;

#[derive(Debug, Default, Clone)]
pub struct InventoryQueryBuilder {
    terms: InventoryQuery,
}

impl InventoryQueryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn maybe_text(mut self, text: Option<String>) -> Self {
        if let Some(text) = text {
            self.terms.push(Query::Text(text));
        }
        self
    }

    pub fn ips(mut self, values: impl IntoIterator<Item = IpAddr>) -> Self {
        self.terms.extend(values.into_iter().map(Query::Ip));
        self
    }

    pub fn interfaces(mut self, values: impl IntoIterator<Item = String>) -> Self {
        self.terms.extend(values.into_iter().map(Query::Interface));
        self
    }

    pub fn neighbor_states(mut self, values: impl IntoIterator<Item = NeighborState>) -> Self {
        self.terms
            .extend(values.into_iter().map(Query::NeighborState));
        self
    }

    pub fn macs(mut self, values: impl IntoIterator<Item = MacAddr>) -> Self {
        self.terms.extend(values.into_iter().map(Query::Mac));
        self
    }

    pub fn build(self) -> InventoryQuery {
        self.terms
    }
}

/// Path helper for routes that receive a single `{name}` segment.
#[derive(Debug, Default, Clone, Hash, Deserialize)]
pub struct NamePath {
    pub name: String,
}

/// Low-level classified input used by Linux query classification.
#[derive(Debug)]
pub enum QueryInput {
    Ip(IpAddr),
    Mac(MacAddr),
    Dev(String),
    Nud(NeighborState),
    Name(String),
}

/// Higher-level typed selector used by the service layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Query {
    Text(String),
    Ip(IpAddr),
    Mac(MacAddr),
    Interface(String),
    NeighborState(NeighborState),
}
