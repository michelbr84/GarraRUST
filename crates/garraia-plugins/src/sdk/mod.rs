//! WASM Plugin SDK (Phase 3.4).
//!
//! Provides the types and traits that plugin authors use to build
//! GarraIA-compatible WASM plugins in Rust.
//!
//! # Architecture
//!
//! Plugins are compiled to WASM (wasm32-wasip1 target) and run inside
//! a sandboxed Wasmtime runtime. The host exposes a set of functions
//! through the `garraia_host` module that plugins can import.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use garraia_plugins::sdk::plugin_trait::PluginImpl;
//!
//! struct MyPlugin;
//!
//! impl PluginImpl for MyPlugin {
//!     fn name(&self) -> &str { "my-plugin" }
//!     fn version(&self) -> &str { "0.1.0" }
//!     fn description(&self) -> &str { "Example plugin" }
//!     fn execute(&self, input: &str) -> String {
//!         format!("processed: {input}")
//!     }
//! }
//! ```

pub mod host_functions;
pub mod plugin_trait;
