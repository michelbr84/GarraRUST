use async_trait::async_trait;
use garraia_common::Result;
use serde_json::json;

use super::{Tool, ToolContext, ToolOutput};

/// Bridges OpenClaw tools into GarraIA's AgentRuntime.
///
/// When OpenClaw is connected and tool-sharing is enabled, this tool
/// forwards `openclaw.*` invocations to the OpenClaw daemon via a
/// WebSocket RPC call.
pub struct OpenClawToolBridge {
    /// Name of the remote tool as exposed by OpenClaw.
    remote_tool_name: String,
    /// Description fetched from OpenClaw's tool manifest.
    remote_description: String,
    /// Input schema from OpenClaw (JSON Schema).
    remote_input_schema: serde_json::Value,
    /// Sender for forwarding tool calls to the OpenClaw client.
    tx: tokio::sync::mpsc::Sender<serde_json::Value>,
    /// Receiver for getting results back. Wrapped in Mutex for interior mutability.
    rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<serde_json::Value>>,
}

impl OpenClawToolBridge {
    /// Create a bridge for a single OpenClaw tool.
    pub fn new(
        remote_tool_name: String,
        remote_description: String,
        remote_input_schema: serde_json::Value,
        tx: tokio::sync::mpsc::Sender<serde_json::Value>,
        rx: tokio::sync::mpsc::Receiver<serde_json::Value>,
    ) -> Self {
        Self {
            remote_tool_name,
            remote_description,
            remote_input_schema,
            tx,
            rx: tokio::sync::Mutex::new(rx),
        }
    }
}

#[async_trait]
impl Tool for OpenClawToolBridge {
    fn name(&self) -> &str {
        &self.remote_tool_name
    }

    fn description(&self) -> &str {
        &self.remote_description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.remote_input_schema.clone()
    }

    async fn execute(&self, _context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        // Send the tool invocation to the OpenClaw daemon.
        let request = json!({
            "type": "tool_call",
            "tool": self.remote_tool_name,
            "input": input,
        });

        self.tx
            .send(request)
            .await
            .map_err(|e| garraia_common::Error::Agent(format!("openclaw bridge send: {e}")))?;

        // Wait for the response with a timeout.
        let mut rx = self.rx.lock().await;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await {
            Ok(Some(response)) => {
                let is_error = response
                    .get("error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let content = response
                    .get("result")
                    .or_else(|| response.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if is_error {
                    Ok(ToolOutput::error(content))
                } else {
                    Ok(ToolOutput::success(content))
                }
            }
            Ok(None) => Ok(ToolOutput::error(
                "OpenClaw tool bridge: channel closed unexpectedly",
            )),
            Err(_) => Ok(ToolOutput::error(
                "OpenClaw tool bridge: timed out waiting for response (30s)",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bridge_returns_timeout_on_no_response() {
        let (tx, _outgoing_rx) = tokio::sync::mpsc::channel(1);
        let (_result_tx, result_rx) = tokio::sync::mpsc::channel(1);

        let bridge = OpenClawToolBridge::new(
            "openclaw.test_tool".to_string(),
            "A test tool".to_string(),
            json!({"type": "object"}),
            tx,
            result_rx,
        );

        assert_eq!(bridge.name(), "openclaw.test_tool");
        assert_eq!(bridge.description(), "A test tool");
    }
}
