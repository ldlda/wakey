//! Linux/OpenWrt adapters: DHCP leases, interface summaries, neighbor tables, and WoL.

pub mod devices;
pub mod dhcp;
pub mod observations;
#[cfg(target_os = "linux")]
pub mod ping;
#[cfg(all(target_os = "linux", feature = "telemetry"))]
pub mod telemetry;
#[cfg(all(unix, feature = "terminal"))]
pub mod terminal;
pub mod wake;

pub use devices::*;
pub use dhcp::*;
pub use observations::*;
#[cfg(all(unix, feature = "terminal"))]
pub use terminal::*;
pub use wake::*;
