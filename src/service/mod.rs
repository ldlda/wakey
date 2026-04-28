pub mod interfaces;
pub mod inventory;
pub mod leases;
pub mod query;
pub mod wake;

pub use interfaces::{get_interface_summaries, get_interface_summary, get_ips};
pub use inventory::{
    inventory, local_observation_to_fact, merge_devices, merge_devices_with_observations,
    resolve_devices,
};
pub use leases::{get_leases, leases_without_state};
pub use query::{resolve_query, resolve_selector};
pub use wake::{
    broadcast_wake_targets, resolve_wake_targets, wake_explicit, wake_from_query, wake_targets,
};
