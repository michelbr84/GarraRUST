use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, time::Duration};
use thiserror::Error;
use tokio::time;

/// Contexto de execução da ferramenta.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("ferramenta expirou após {0:?}")]
    Timeout(Duration),

    #[error("ferramenta falhou: {0}")]
    Failed(String),
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError>;
}

pub struct ToolRegistry {
    tools: HashMap<&'static str, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.insert(tool.name(), Box::new(tool));
        self
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list_names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn execute_with_timeout(
    tool: &dyn Tool,
    ctx: &ToolContext,
    input: ToolInput,
    timeout_duration: Duration,
) -> Result<ToolOutput, ToolError> {
    match time::timeout(timeout_duration, tool.execute(ctx, input)).await {
        Ok(result) => result,
        Err(_elapsed) => Err(ToolError::Timeout(timeout_duration)),
    }
}

// =============================================================================
// Ferramentas: repo_search e list_dir
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct RepoSearchInput {
    pub query: String,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_context_lines")]
    pub context_lines: usize,
    #[serde(default)]
    pub path: Option<String>,
}

fn default_max_results() -> usize {
    20
}
fn default_context_lines() -> usize {
    3
}

#[derive(Debug, Deserialize)]
pub struct ListDirInput {
    pub path: String,
    #[serde(default)]
    pub include_files: bool,
}

pub struct RepoSearchTool {
    root_path: String,
}

impl RepoSearchTool {
    pub fn new(root_path: &str) -> Self {
        Self {
            root_path: root_path.to_string(),
        }
    }
}

#[async_trait]
impl Tool for RepoSearchTool {
    fn name(&self) -> &'static str {
        "repo_search"
    }

    async fn execute(&self, _ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let input: RepoSearchInput = serde_json::from_value(input.payload)
            .map_err(|e| ToolError::Failed(format!("Invalid input: {}", e)))?;

        let search_path = input.path.as_deref().unwrap_or(&self.root_path);
        let query_lower = input.query.to_lowercase();
        let mut results = Vec::new();

        self.search_recursive(search_path, &query_lower, &input, &mut results);
        results.truncate(input.max_results);

        Ok(ToolOutput {
            name: "repo_search".to_string(),
            payload: serde_json::json!({ "query": input.query, "results": results, "total": results.len() }),
        })
    }
}

impl RepoSearchTool {
    fn search_recursive(
        &self,
        path: &str,
        query: &str,
        input: &RepoSearchInput,
        results: &mut Vec<serde_json::Value>,
    ) {
        if results.len() >= input.max_results {
            return;
        }

        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            if results.len() >= input.max_results {
                break;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') || file_name == "node_modules" || file_name == "target" {
                continue;
            }

            let file_path = entry.path();
            if file_path.is_dir() {
                self.search_recursive(&file_path.to_string_lossy(), query, input, results);
            } else if file_path.is_file() {
                let matches_glob = input.globs.is_empty()
                    || input.globs.iter().any(|g| {
                        if g.starts_with("*.") {
                            file_name.ends_with(&g[1..])
                        } else {
                            file_name.contains(g)
                        }
                    });
                if matches_glob {
                    if let Ok(content) = fs::read_to_string(&file_path) {
                        if content.to_lowercase().contains(query) {
                            let lines: Vec<_> = content
                                .lines()
                                .enumerate()
                                .filter(|(_, line)| line.to_lowercase().contains(query))
                                .collect();
                            let snippets: Vec<_> = lines.iter().take(input.context_lines)
                                .map(|(num, line)| serde_json::json!({ "line": num + 1, "content": line }))
                                .collect();
                            if !snippets.is_empty() {
                                results.push(serde_json::json!({ "file": file_path.to_string_lossy(), "snippets": snippets }));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct ListDirTool {
    root_path: String,
}

impl ListDirTool {
    pub fn new(root_path: &str) -> Self {
        Self {
            root_path: root_path.to_string(),
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    async fn execute(&self, _ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let input: ListDirInput = serde_json::from_value(input.payload)
            .map_err(|e| ToolError::Failed(format!("Invalid input: {}", e)))?;

        let path = if input.path.starts_with('/') || input.path.contains(':') {
            input.path.clone()
        } else {
            format!("{}/{}", self.root_path, input.path)
        };

        let mut entries = Vec::new();
        let dir_entries = fs::read_dir(&path)
            .map_err(|e| ToolError::Failed(format!("Cannot read directory: {}", e)))?;

        for entry in dir_entries.flatten().take(100) {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }
            let file_path = entry.path();
            let is_dir = file_path.is_dir();
            if !input.include_files && !is_dir {
                continue;
            }
            entries.push(serde_json::json!({ "name": file_name, "type": if is_dir { "dir" } else { "file" }, "path": file_path.to_string_lossy() }));
        }

        Ok(ToolOutput {
            name: "list_dir".to_string(),
            payload: serde_json::json!({ "path": path, "entries": entries, "total": entries.len() }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FerramentaEco;
    #[async_trait]
    impl Tool for FerramentaEco {
        fn name(&self) -> &'static str {
            "eco"
        }
        async fn execute(
            &self,
            _ctx: &ToolContext,
            input: ToolInput,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                name: input.name,
                payload: input.payload,
            })
        }
    }

    #[tokio::test]
    async fn registry_registra_e_busca() {
        let registry = ToolRegistry::new().register(FerramentaEco);
        assert!(registry.get("eco").is_some());
    }
}
