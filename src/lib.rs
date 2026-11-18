//! Cusco: a per-user service that watches local coding agents and publishes
//! which of them need attention. See `docs/design.md`.

pub mod adapter;
pub mod api;
pub mod hook;
pub mod process;
pub mod record;
pub mod row;
pub mod server;
pub mod tree;
