//! Plugin trait that WASM plugins must implement.
//!
//! This defines the interface contract between GarraIA and plugin modules.
//! Plugins compiled to WASM must export functions matching this trait.
//!
//! # WASM exports
//!
//! The compiled WASM module must export the following functions:
//!
//! | Export | Signature | Description |
//! |--------|-----------|-------------|
//! | `_start` | `() -> ()` | Entry point (WASI convention) |
//! | `plugin_name` | `() -> *const u8` | Returns plugin name |
//! | `plugin_version` | `() -> *const u8` | Returns plugin version |
//! | `plugin_describe` | `() -> *const u8` | Returns plugin description |
//! | `plugin_execute` | `(*const u8, usize) -> (*const u8, usize)` | Process input, return output |
//!
//! # Example
//!
//! ```rust,ignore
//! use garraia_plugins::sdk::plugin_trait::PluginImpl;
//!
//! struct TranslatorPlugin;
//!
//! impl PluginImpl for TranslatorPlugin {
//!     fn name(&self) -> &str { "translator" }
//!     fn version(&self) -> &str { "0.1.0" }
//!     fn description(&self) -> &str { "Translates text between languages" }
//!
//!     fn execute(&self, input: &str) -> String {
//!         // Parse input JSON, do translation, return result
//!         format!("translated: {input}")
//!     }
//!
//!     fn capabilities(&self) -> Vec<String> {
//!         vec!["translate".into(), "detect-language".into()]
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

/// Trait that all GarraIA WASM plugins must implement.
///
/// Plugin authors implement this trait in Rust, compile to
/// `wasm32-wasip1`, and place the resulting `.wasm` file alongside
/// a `plugin.toml` manifest in the plugins directory.
pub trait PluginImpl {
    /// Human-readable plugin name (must match manifest).
    fn name(&self) -> &str;

    /// Plugin version in semver format.
    fn version(&self) -> &str;

    /// Short description of what the plugin does.
    fn description(&self) -> &str;

    /// Process input and return output.
    ///
    /// Input and output are JSON strings. The plugin is responsible for
    /// parsing the input and serializing the output.
    fn execute(&self, input: &str) -> String;

    /// List of capabilities/tools this plugin provides.
    fn capabilities(&self) -> Vec<String> {
        Vec::new()
    }

    /// Called once when the plugin is loaded. Use for initialization.
    fn on_load(&self) {}

    /// Called when the plugin is about to be unloaded. Use for cleanup.
    fn on_unload(&self) {}
}

/// Plugin metadata that can be queried without executing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

/// Input format for plugin execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExecuteInput {
    /// The action/tool to invoke.
    pub action: String,
    /// Parameters for the action.
    #[serde(default)]
    pub params: serde_json::Value,
    /// Optional context (session ID, user info, etc.).
    #[serde(default)]
    pub context: Option<PluginContext>,
}

/// Execution context passed to plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

/// Output format from plugin execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExecuteOutput {
    /// Whether the execution succeeded.
    pub success: bool,
    /// Result data (action-specific).
    #[serde(default)]
    pub data: serde_json::Value,
    /// Error message if execution failed.
    #[serde(default)]
    pub error: Option<String>,
}

impl PluginExecuteOutput {
    /// Create a successful output.
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    /// Create an error output.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: serde_json::Value::Null,
            error: Some(message.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_ok() {
        let output = PluginExecuteOutput::ok(serde_json::json!({"result": 42}));
        assert!(output.success);
        assert_eq!(output.data["result"], 42);
        assert!(output.error.is_none());
    }

    #[test]
    fn output_err() {
        let output = PluginExecuteOutput::err("something went wrong");
        assert!(!output.success);
        assert_eq!(output.error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn execute_input_serialization() {
        let input = PluginExecuteInput {
            action: "translate".into(),
            params: serde_json::json!({"text": "hello", "target": "es"}),
            context: Some(PluginContext {
                session_id: Some("sess-123".into()),
                user_id: None,
                channel: Some("telegram".into()),
            }),
        };
        let json = serde_json::to_string(&input).expect("serialize");
        let parsed: PluginExecuteInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.action, "translate");
        assert_eq!(
            parsed.context.as_ref().and_then(|c| c.channel.as_deref()),
            Some("telegram")
        );
    }
}
