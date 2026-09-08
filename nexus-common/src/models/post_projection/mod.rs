//! Generic source replication, independent of custom content and social caches.
//!
//! Inventory is a sequence of consistent current-state pages, not a historical
//! snapshot. Consumers stage a complete scan and replay its retained interval
//! before promotion. All reads and source writes share the same graph lock.

#[cfg(test)]
mod graph_tests;
pub mod mutation;
mod queries;
#[cfg(test)]
mod retention_tests;
mod service;
mod types;

#[cfg(test)]
use service::InventoryRow;
pub use service::{changes, head, inventory, ChangesRequest, InventoryRequest};
pub use types::*;
