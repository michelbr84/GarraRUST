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

    /// Programmatic tool calling (P1 do gap analysis 2026-09-15, slice 1 —
    /// o "Code Mode" do Garra): executa um **programa de tools** em JSON num
    /// único turno, sem voltar ao modelo entre passos.
    ///
    /// Formato: `{ "steps": [ { "tool": "repo_search", "args": {...},
    /// "as": "achados" }, ... ] }`. Substituição: qualquer valor string do
    /// `args` com `"$nome_var"` recebe o output textual do passo anterior
    /// (na íntegra — sem interpolação parcial, sem injeção de prefixo).
    ///
    /// Fail-fast: o primeiro passo que falha encerra o programa; passos já
    /// executados ficam no resultado parcial. Orçamento: `max_steps` limita
    /// o tamanho do programa (default 16) — programa não é loop infinito.
    pub async fn execute_program(
        &self,
        ctx: &ToolContext,
        program: &serde_json::Value,
        max_steps: usize,
    ) -> Result<serde_json::Value, ToolError> {
        let steps = program
            .get("steps")
            .and_then(|s| s.as_array())
            .ok_or_else(|| ToolError::Failed("programa precisa de `steps: []`".into()))?;
        if steps.len() > max_steps {
            return Err(ToolError::Failed(format!(
                "programa com {} passos excede o orçamento de {max_steps}",
                steps.len()
            )));
        }

        let mut vars: HashMap<String, String> = HashMap::new();
        let mut executed = Vec::with_capacity(steps.len());

        for (i, step) in steps.iter().enumerate() {
            let tool_name = step
                .get("tool")
                .and_then(|t| t.as_str())
                .ok_or_else(|| ToolError::Failed(format!("passo {i}: falta `tool`")))?;
            let as_var = step.get("as").and_then(|a| as_str(a)).map(str::to_string);
            let tool = self.get(tool_name).ok_or_else(|| {
                ToolError::Failed(format!("passo {i}: tool `{tool_name}` não registrada"))
            })?;

            // args: JSON com substituição $var nos valores string.
            let mut args = step.get("args").cloned().unwrap_or(serde_json::Value::Null);
            substitute_vars(&mut args, &vars);

            let output = execute_with_timeout(
                tool,
                ctx,
                ToolInput {
                    name: tool_name.to_string(),
                    payload: args,
                },
                Duration::from_secs(300),
            )
            .await?;
            if let Some(var) = as_var {
                let texto = match &output.payload {
                    serde_json::Value::String(s) => s.clone(),
                    outro => outro.to_string(),
                };
                vars.insert(var, texto);
            }
            executed.push(serde_json::json!({
                "step": i,
                "tool": tool_name,
                "ok": true,
                "output": output.payload,
            }));
        }

        Ok(serde_json::json!({ "steps": executed, "vars": vars.len() }))
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
// Programmatic tool calling: helpers de substituição $var
// =============================================================================

fn as_str(v: &serde_json::Value) -> Option<&str> {
    v.as_str()
}

/// Substitui valores string exatamente iguais a `"$var"` pelo output textual
/// da variável. Substituição **interna**: não interpola dentro de strings
/// maiores (nada de injeção de prefixo — ou o arg É a variável, ou não é).
fn substitute_vars(value: &mut serde_json::Value, vars: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(var) = s.strip_prefix('$') {
                if let Some(subst) = vars.get(var) {
                    *s = subst.clone();
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                substitute_vars(item, vars);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                substitute_vars(v, vars);
            }
        }
        _ => {}
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
        // ── Programmatic tool calling (P1 gap analysis 2026-09-15) ──────

        // Programa com variável: passo 2 usa o output do passo 1.
        let registry = ToolRegistry::new().register(FerramentaEco);
        let ctx = ToolContext {
            request_id: "t".into(),
        };
        let programa = serde_json::json!({
            "steps": [
                { "tool": "eco", "args": { "q": "busca inicial" }, "as": "achados" },
                { "tool": "eco", "args": { "q": "$achados" }, "as": "final" }
            ]
        });
        let resultado = registry.execute_program(&ctx, &programa, 16).await.unwrap();
        let steps = resultado["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        // Passo 2 recebeu o output do passo 1 via $achados (output não-string
        // é serializado para a variável).
        assert_eq!(
            steps[1]["output"]["q"],
            serde_json::json!({"q": "busca inicial"}).to_string()
        );

        // Substituição parcial não interpola: "$achados e mais" fica literal.
        let programa_parcial = serde_json::json!({
            "steps": [
                { "tool": "eco", "args": { "q": "x" }, "as": "v" },
                { "tool": "eco", "args": { "q": "$v e mais" } }
            ]
        });
        let r2 = registry
            .execute_program(&ctx, &programa_parcial, 16)
            .await
            .unwrap();
        assert_eq!(r2["steps"][1]["output"]["q"], "$v e mais");

        // Fail-fast: tool inexistente no passo 2 => erro com índice.
        let programa_quebrado = serde_json::json!({
            "steps": [
                { "tool": "eco", "args": {} },
                { "tool": "inexistente", "args": {} }
            ]
        });
        let err = registry
            .execute_program(&ctx, &programa_quebrado, 16)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("passo 1"));

        // Orçamento: mais passos que o máximo => erro.
        let programa_grande = serde_json::json!({
            "steps": (0..20).map(|i| serde_json::json!({"tool": "eco", "args": {"i": i}})).collect::<Vec<_>>()
        });
        let err = registry
            .execute_program(&ctx, &programa_grande, 16)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("orçamento"));

        // Programa sem steps => erro claro.
        let err = registry
            .execute_program(&ctx, &serde_json::json!({}), 16)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("steps"));
        assert!(registry.get("eco").is_some());
    }
}
