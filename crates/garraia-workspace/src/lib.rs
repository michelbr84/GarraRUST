//! `garraia-workspace` — Postgres-backed multi-tenant workspace for GarraIA.
//!
//! Scope (GAR-407 bootstrap): connection, migration 001 (users + groups schema),
//! smoke test. No CRUD yet — that lives in downstream issues (GAR-393 API, etc.).

pub mod config;
pub mod error;
pub mod store;

pub use config::WorkspaceConfig;
pub use error::{Result, WorkspaceError};
pub use store::Workspace;
