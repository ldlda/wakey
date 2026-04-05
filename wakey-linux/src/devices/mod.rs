mod interfaces;
mod neighbors;
mod query;

pub use interfaces::{devs_sorted, has_dev, list_devs, list_interface_summaries};
pub use neighbors::{get_ips, get_neighbors, query_status};
pub use query::classify_query;
