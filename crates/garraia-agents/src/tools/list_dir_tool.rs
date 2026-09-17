//! # List Directory Tool (Phase 5.3)
//!
//! Intelligent directory listing with tree-style output,
//! file sizes, and .gitignore awareness.

use async_trait::async_trait;
use garraia_common::Result;
use std::path::{Path, PathBuf};

use super::file_jail::FileJail;
use super::tool_context::{process_home_dir, resolve_tool_path};
use super::{Tool, ToolContext, ToolOutput};

/// Maximum depth for directory traversal
const MAX_DEPTH: usize = 10;

/// Maximum entries to return
const MAX_ENTRIES: usize = 500;

/// Default depth
const DEFAULT_DEPTH: usize = 2;

/// Patterns to always skip (common non-useful directories)
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".dart_tool",
    ".pub-cache",
    "__pycache__",
    ".next",
    "dist",
    "build",
    ".gradle",
    ".idea",
    ".vs",
    ".vscode",
];

/// Intelligent directory listing with tree-style output.
/// Respects common ignore patterns and provides file sizes.
pub struct ListDirTool {
    jail: FileJail,
    max_entries: usize,
}

impl ListDirTool {
    /// Create a new ListDirTool.
    ///
    /// Issue #1244: `jail` e obrigatorio. `list_dir` e a tool de
    /// *reconhecimento* do par — com ela o modelo acha `~/.ssh` antes de
    /// pedir o `file_read`, entao confinar so a leitura seria meia correcao.
    pub fn new(jail: FileJail, max_entries: Option<usize>) -> Self {
        Self {
            jail,
            max_entries: max_entries.unwrap_or(MAX_ENTRIES),
        }
    }

    /// Format file size in human-readable form
    fn format_size(size: u64) -> String {
        if size < 1024 {
            format!("{}B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1}KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1}GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Check if a directory should be skipped
    fn should_skip(name: &str) -> bool {
        SKIP_DIRS.contains(&name)
    }

    /// Recursively list directory contents
    fn list_recursive(
        &self,
        path: &Path,
        prefix: &str,
        depth: usize,
        max_depth: usize,
        pattern: Option<&str>,
        entries: &mut Vec<String>,
    ) -> std::io::Result<()> {
        if depth > max_depth || entries.len() >= self.max_entries {
            return Ok(());
        }

        let mut items: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();

        // Sort: directories first, then alphabetically
        items.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        let total = items.len();

        for (idx, entry) in items.iter().enumerate() {
            if entries.len() >= self.max_entries {
                entries.push(format!("{}... ({} more entries)", prefix, total - idx));
                break;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let is_last = idx == total - 1;
            let connector = if is_last { "\\-- " } else { "|-- " };
            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}|   ", prefix)
            };

            let file_type = entry.file_type().unwrap_or_else(|_| {
                // Fallback: treat as file
                std::fs::symlink_metadata(entry.path())
                    .map(|m| m.file_type())
                    .unwrap_or_else(|_| entry.file_type().unwrap())
            });

            if file_type.is_dir() {
                if Self::should_skip(&name) {
                    entries.push(format!("{}{}{}/  (skipped)", prefix, connector, name));
                    continue;
                }

                entries.push(format!("{}{}{}/", prefix, connector, name));

                if depth < max_depth {
                    let _ = self.list_recursive(
                        &entry.path(),
                        &child_prefix,
                        depth + 1,
                        max_depth,
                        pattern,
                        entries,
                    );
                }
            } else {
                // Apply pattern filter if specified
                if let Some(pat) = pattern {
                    let pat_lower = pat.to_lowercase();
                    let name_lower = name.to_lowercase();
                    if !name_lower.contains(&pat_lower) && !glob_match(&pat_lower, &name_lower) {
                        continue;
                    }
                }

                let size = entry
                    .metadata()
                    .map(|m| Self::format_size(m.len()))
                    .unwrap_or_else(|_| "?".to_string());

                entries.push(format!("{}{}{}  ({})", prefix, connector, name, size));
            }
        }

        Ok(())
    }
}

/// Simple glob matching (supports * and ?)
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Handle *.ext pattern
    if let Some(ext) = pattern.strip_prefix("*.") {
        return text.ends_with(&format!(".{}", ext));
    }

    // Handle prefix* pattern
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }

    pattern == text
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "Lists directory contents in a tree-style format.\n\
         Shows file sizes, skips common build/cache directories (.git, node_modules, target).\n\
         Supports depth control and file pattern filtering."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (default: current directory)"
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum depth to traverse (default: 2, max: 10)"
                },
                "pattern": {
                    "type": "string",
                    "description": "File pattern filter (e.g., '*.rs', '*.py')"
                }
            }
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        // Sem `path`, lista o diretorio da sessao — nunca o CWD do processo
        // do gateway, que nao significa nada para quem esta no telefone.
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let depth = input
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_DEPTH as u64) as usize;

        let depth = depth.min(MAX_DEPTH);

        let pattern = input.get("pattern").and_then(|v| v.as_str());

        // O mesmo resolvedor do `file_read` (#923): `~` expandido, relativo
        // juntado ao `working_dir` da sessao, `..` recusado. Auditoria do
        // #1039: a tool ficou anos sem ser registrada, e quando entrou no
        // runtime aceitava qualquer caminho cru do modelo.
        let resolved = match resolve_tool_path(
            path_str,
            context.working_dir.as_deref(),
            process_home_dir().as_deref(),
        ) {
            Ok(r) => r,
            Err(e) => return Ok(ToolOutput::error(e.to_string())),
        };
        // #1244: confina antes de tocar o disco. `list_dir` devolve a recusa
        // como `ToolOutput::error` (o modelo continua o turno) em vez de `Err`,
        // que e como o resto desta tool reporta problema de caminho.
        let path: PathBuf = match self
            .jail
            .confine(&resolved.path, context.working_dir.as_deref())
        {
            Ok(p) => p,
            Err(denial) => {
                tracing::warn!(
                    session = %context.session_id,
                    motivo = ?denial,
                    "list_dir: caminho recusado pelo jail"
                );
                // #1039 preservado dentro da #1244: a dica de "a sessao nao
                // tem working_dir" fala da SESSAO, nao do disco, entao ela
                // pode acompanhar a recusa sem virar oraculo de existencia.
                let dica =
                    if resolved.origin == super::tool_context::PathOrigin::RelativeToProcessCwd {
                        " O caminho era relativo e a sessão não tem working_dir: \
                     peça um caminho dentro do diretório da sessão."
                    } else {
                        ""
                    };
                return Ok(ToolOutput::error(format!("{}{dica}", denial.message())));
            }
        };

        // Validate path exists
        if !path.exists() {
            return Ok(ToolOutput::error(format!(
                "Directory not found: {}",
                resolved.describe()
            )));
        }

        if !path.is_dir() {
            return Ok(ToolOutput::error(format!(
                "Not a directory: {}",
                resolved.describe()
            )));
        }

        let mut entries = Vec::new();
        entries.push(format!("{}/", path.display()));

        match self.list_recursive(&path, "", 0, depth, pattern, &mut entries) {
            Ok(()) => {
                if entries.len() >= self.max_entries {
                    entries.push(format!(
                        "\n(listing truncated at {} entries)",
                        self.max_entries
                    ));
                }
                Ok(ToolOutput::success(entries.join("\n")))
            }
            Err(e) => Ok(ToolOutput::error(format!("Error listing directory: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::file_jail::DENIAL_MESSAGE;

    #[test]
    fn test_format_size() {
        assert_eq!(ListDirTool::format_size(100), "100B");
        assert_eq!(ListDirTool::format_size(1500), "1.5KB");
        assert_eq!(ListDirTool::format_size(1_500_000), "1.4MB");
    }

    #[test]
    fn test_should_skip() {
        assert!(ListDirTool::should_skip(".git"));
        assert!(ListDirTool::should_skip("node_modules"));
        assert!(ListDirTool::should_skip("target"));
        assert!(!ListDirTool::should_skip("src"));
        assert!(!ListDirTool::should_skip("crates"));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test*", "test_file"));
    }

    fn ctx(working_dir: Option<&str>) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    /// `.` e o diretorio da sessao, e sem `path` o default e o mesmo.
    #[tokio::test]
    async fn test_list_dir_current() {
        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        let ctx = ctx(Some(env!("CARGO_MANIFEST_DIR")));

        let output = tool
            .execute(&ctx, serde_json::json!({"path": ".", "depth": 1}))
            .await
            .expect("should not error");
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("Cargo.toml"), "{}", output.content);

        let output = tool
            .execute(&ctx, serde_json::json!({"depth": 1}))
            .await
            .expect("should not error");
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("Cargo.toml"), "{}", output.content);
    }

    /// Relativo sem sessao resolve contra o CWD do processo, como o
    /// `file_read` — e o erro diz isso, para o modelo saber pedir um caminho
    /// absoluto em vez de tentar de novo.
    #[tokio::test]
    async fn test_list_dir_relative_without_session_dir_explains_itself() {
        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        let output = tool
            .execute(&ctx(None), serde_json::json!({"path": "nao_existe_xyz"}))
            .await
            .expect("should not error");
        assert!(output.is_error, "{}", output.content);
        assert!(
            output.content.contains("não tem working_dir"),
            "{}",
            output.content
        );
    }

    /// `..` nunca passa, com ou sem sessao.
    #[tokio::test]
    async fn test_list_dir_rejects_parent_dir() {
        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        for wd in [Some(env!("CARGO_MANIFEST_DIR")), None] {
            let output = tool
                .execute(&ctx(wd), serde_json::json!({"path": "../.."}))
                .await
                .expect("should not error");
            assert!(output.is_error, "{}", output.content);
        }
    }

    #[tokio::test]
    async fn test_list_dir_not_found() {
        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"path": "/nonexistent_dir_12345"}))
            .await
            .expect("should not error");

        assert!(output.is_error);
    }

    // ─── issue #1244: o jail ───────────────────────────────────────────────

    fn raiz() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        (tmp, root)
    }

    /// `list_dir` é a tool de reconhecimento do par: sem jail nela, o modelo
    /// encontra `~/.ssh` e só então pede o `file_read`.
    #[tokio::test]
    async fn diretorio_fora_da_raiz_e_recusado() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        std::fs::write(fora.join("segredo.txt"), b"x").expect("write");

        let tool = ListDirTool::new(FileJail::from_roots([&root]), None);
        let out = tool
            .execute(
                &ctx(None),
                serde_json::json!({ "path": fora.to_str().expect("utf8") }),
            )
            .await
            .expect("tool nao deve estourar");

        assert!(out.is_error, "{}", out.content);
        assert!(out.content.starts_with(DENIAL_MESSAGE), "{}", out.content);
        assert!(!out.content.contains("segredo.txt"), "{}", out.content);
    }

    /// Symlink de diretório apontando para fora da raiz.
    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_de_diretorio_para_fora_e_recusado() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        std::fs::write(fora.join("segredo.txt"), b"x").expect("write");
        std::os::unix::fs::symlink(&fora, root.join("atalho")).expect("symlink");

        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        let out = tool
            .execute(
                &ctx(Some(root.to_str().expect("utf8"))),
                serde_json::json!({ "path": "atalho" }),
            )
            .await
            .expect("tool nao deve estourar");

        assert!(out.is_error, "{}", out.content);
        assert!(!out.content.contains("segredo.txt"), "{}", out.content);
    }

    /// Fail-closed: sem raiz nenhuma, nem um diretório que existe é listado.
    #[tokio::test]
    async fn sem_raiz_nenhuma_nao_lista() {
        let (_t, root) = raiz();
        std::fs::write(root.join("a.txt"), b"x").expect("write");

        let tool = ListDirTool::new(FileJail::sessions_only(), None);
        let out = tool
            .execute(
                &ctx(None),
                serde_json::json!({ "path": root.to_str().expect("utf8") }),
            )
            .await
            .expect("tool nao deve estourar");

        assert!(out.is_error, "{}", out.content);
        assert!(!out.content.contains("a.txt"), "{}", out.content);
    }
}
