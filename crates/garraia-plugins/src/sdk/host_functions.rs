//! Host functions exposed to WASM plugins.
//!
//! These functions are linked into the WASM module at instantiation time
//! and allow plugins to interact with the host (GarraIA gateway).
//!
//! # Available host functions
//!
//! | Function | Description |
//! |----------|-------------|
//! | `send_message` | Send a chat message to a channel/session |
//! | `read_file` | Read a file from the host filesystem (scoped) |
//! | `http_request` | Make an HTTP request (allowlisted domains only) |
//! | `log` | Write a log entry to the host's tracing system |
//! | `get_config` | Read plugin configuration values |
//! | `set_state` | Store plugin state (persisted across invocations) |
//! | `get_state` | Retrieve previously stored plugin state |

use serde::{Deserialize, Serialize};

/// Log levels available to plugins.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

/// HTTP method for `http_request`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// Request structure for the `http_request` host function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    /// Timeout in seconds (default: 30).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

/// Response structure from the `http_request` host function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
}

/// Message structure for the `send_message` host function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    /// Target session or channel ID.
    pub target: String,
    /// Message content (plain text or markdown).
    pub content: String,
    /// Optional message metadata.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// File read request for the `read_file` host function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileRequest {
    /// Path relative to the plugin's allowed filesystem scope.
    pub path: String,
    /// Maximum bytes to read (default: 1MB).
    #[serde(default = "default_max_bytes")]
    pub max_bytes: usize,
}

fn default_max_bytes() -> usize {
    1024 * 1024
}

/// File read response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResponse {
    pub content: String,
    pub size: usize,
    pub truncated: bool,
}

/// Plugin state store operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEntry {
    pub key: String,
    pub value: serde_json::Value,
}

/// Configuration value request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigRequest {
    pub key: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

/// Specification of all host functions available to plugins.
///
/// This struct documents the contract between host and guest. Plugin authors
/// should reference this when building WASM modules.
pub struct HostFunctionSpec;

impl HostFunctionSpec {
    /// List of all available host function names.
    pub fn available_functions() -> &'static [&'static str] {
        &[
            "send_message",
            "read_file",
            "http_request",
            "log",
            "get_config",
            "set_state",
            "get_state",
        ]
    }

    /// Returns a JSON Schema-like description of all host functions.
    pub fn schema() -> serde_json::Value {
        serde_json::json!({
            "functions": {
                "send_message": {
                    "description": "Send a chat message to a channel or session",
                    "input": "SendMessageRequest (JSON)",
                    "output": "void"
                },
                "read_file": {
                    "description": "Read a file from the host filesystem (scoped to plugin permissions)",
                    "input": "ReadFileRequest (JSON)",
                    "output": "ReadFileResponse (JSON)"
                },
                "http_request": {
                    "description": "Make an HTTP request (only to allowlisted domains)",
                    "input": "HttpRequest (JSON)",
                    "output": "HttpResponse (JSON)"
                },
                "log": {
                    "description": "Write a log entry to the host tracing system",
                    "input": "{ level: LogLevel, message: string }",
                    "output": "void"
                },
                "get_config": {
                    "description": "Read a plugin configuration value",
                    "input": "ConfigRequest (JSON)",
                    "output": "JSON value or null"
                },
                "set_state": {
                    "description": "Store persistent plugin state",
                    "input": "StateEntry (JSON)",
                    "output": "void"
                },
                "get_state": {
                    "description": "Retrieve previously stored plugin state",
                    "input": "{ key: string }",
                    "output": "JSON value or null"
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_function_list_is_complete() {
        let fns = HostFunctionSpec::available_functions();
        assert_eq!(fns.len(), 7);
        assert!(fns.contains(&"send_message"));
        assert!(fns.contains(&"http_request"));
        assert!(fns.contains(&"log"));
    }

    #[test]
    fn schema_has_all_functions() {
        let schema = HostFunctionSpec::schema();
        let functions = schema["functions"].as_object().expect("functions object");
        assert_eq!(functions.len(), 7);
    }

    #[test]
    fn http_request_serialization() {
        let req = HttpRequest {
            method: HttpMethod::Post,
            url: "https://example.com/api".into(),
            headers: [("Content-Type".into(), "application/json".into())]
                .into_iter()
                .collect(),
            body: Some(r#"{"key":"value"}"#.into()),
            timeout_secs: 10,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains("POST"));
        assert!(json.contains("example.com"));
    }
}
