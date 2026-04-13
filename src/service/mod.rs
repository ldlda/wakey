pub mod interfaces;
pub mod inventory;
pub mod leases;
pub mod query;
pub mod status;
pub mod wake;

pub use interfaces::{get_interface_summaries, get_interface_summary, get_ips, list_interfaces};
pub use inventory::{inventory, merge_devices, resolve_devices};
pub use leases::{get_leases, leases_without_state};
pub use query::{query_to_device_query, resolve_query, resolve_selector};
pub use status::{StatusResponse, get_status, get_status_for_input};
pub use wake::{
    broadcast_wake_targets, resolve_wake_targets, wake_explicit, wake_from_query, wake_targets,
};
