//! Transitional compatibility wrappers preserved during the migration.
//!
//! Items in this module exist so the codebase can keep working while older
//! parsing paths and adapter surfaces are being retired or replaced. New logic
//! should prefer the service layer and the dedicated crate boundaries instead.

pub mod arpparse;
pub mod dhcpparse;
