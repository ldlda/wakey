mod interfaces;
mod neighbors;
mod query;

pub use interfaces::{has_dev, list_interface_summaries};
pub use neighbors::{get_ips, get_neighbors, query_neighbors};
pub use query::classify_query;
