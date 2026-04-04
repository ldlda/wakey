pub mod dev {
    pub use wakey_linux::devices::devs_sorted;
}
pub mod leases {
    pub use wakey_linux::dhcp::enrich_leases_with_nud_state;
}
pub mod macs;
pub mod parser {
    pub use wakey_core::QueryInput as QueryType;

    pub async fn parse_query(q: String) -> QueryType {
        wakey_linux::devices::classify_query(q).await
    }
}

pub use leases::*;
pub use macs::*;
