//! # Run Tests Tool (Phase 5.3)
//!
//! Executes test suites and reports results.
//! Detects test framework automatically: cargo test, flutter test, npm test, pytest.

use async_trait::async_trait;
use garraia_common::Result;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

use super::approval::ApprovalFingerprint;
use super::tool_context::{process_home_dir, resolve_tool_path};
use super::{Tool, ToolContext, ToolOutput};

/// Default timeout for test execution
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum output size
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Detected test framework
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFramework {
    Cargo,
    Flutter,
    Npm,
    Pytest,
    Unknown,
}

/// Executes test suites and reports results.
/// Auto-detects the test framework based on project files.
pub struct RunTestsTool {
    timeout: Duration,
    /// Roda `cargo test` / `npm test` / ... — e `npm test` executa o que o
    /// `package.json` do diretorio mandar. E execucao arbitraria com outro
    /// nome, entao segue a mesma regra do `bash` (GAR-187): com
    /// `agent.tool_confirmation_enabled`, pede confirmacao antes de rodar.
    confirmation_enabled: bool,
}

impl RunTestsTool {
    /// Create a new RunTestsTool
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            confirmation_enabled: false,
        }
    }

    /// Como [`Self::new`], mas exige confirmacao humana antes de executar.
    pub fn new_with_confirmation(timeout_secs: Option<u64>) -> Self {
        Self {
            confirmation_enabled: true,
            ..Self::new(timeout_secs)
        }
    }

    /// Rejects a `test_name` that the runner would read as an **option**
    /// instead of as a filter.
    ///
    /// The value comes from the model and lands as an argument: no shell is
    /// involved, so this is not about metacharacters. It is about `cargo test
    /// --config 'target.<cfg>.runner="/bin/sh -c ..."'`, which is arbitrary
    /// execution, and about the equivalent option surface of `pytest` and
    /// `npm`. The one documented exception is the `-p <crate>` form the schema
    /// advertises, which [`Self::build_command`] splits itself.
    fn validate_test_name(name: &str) -> std::result::Result<(), String> {
        const MAX: usize = 200;
        if name.is_empty() {
            return Err("test_name vazio".to_string());
        }
        if name.len() > MAX {
            return Err(format!("test_name maior que {MAX} caracteres"));
        }
        if name.chars().any(|c| c.is_control()) {
            return Err("test_name contem caractere de controle".to_string());
        }
        if let Some(crate_name) = name.strip_prefix("-p ") {
            let ok = !crate_name.is_empty()
                && crate_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            return if ok {
                Ok(())
            } else {
                Err("nome de crate invalido depois de `-p`".to_string())
            };
        }
        if name.starts_with('-') {
            return Err(
                "test_name nao pode comecar com `-`: seria lido como opcao do runner".to_string(),
            );
        }
        Ok(())
    }

    /// The command line as it will actually run, for the gate to judge.
    ///
    /// Rebuilt from the `Command` rather than from the framework label, so the
    /// gate can never be shown something shorter than what executes.
    fn command_line(cmd: &Command) -> String {
        let std_cmd = cmd.as_std();
        let mut line = std_cmd.get_program().to_string_lossy().into_owned();
        for arg in std_cmd.get_args() {
            line.push(' ');
            line.push_str(&arg.to_string_lossy());
        }
        line
    }

    /// Detect the test framework in the given directory
    fn detect_framework(working_dir: &Path) -> TestFramework {
        if working_dir.join("Cargo.toml").exists() {
            TestFramework::Cargo
        } else if working_dir.join("pubspec.yaml").exists() {
            TestFramework::Flutter
        } else if working_dir.join("package.json").exists() {
            TestFramework::Npm
        } else if working_dir.join("pytest.ini").exists()
            || working_dir.join("setup.py").exists()
            || working_dir.join("pyproject.toml").exists()
        {
            TestFramework::Pytest
        } else {
            TestFramework::Unknown
        }
    }

    /// Build the test command for the detected framework
    fn build_command(
        framework: TestFramework,
        test_name: Option<&str>,
        working_dir: &Path,
    ) -> (Command, String) {
        match framework {
            TestFramework::Cargo => {
                let mut cmd = Command::new("cargo");
                cmd.arg("test");
                if let Some(name) = test_name {
                    // Check if it looks like a crate name (e.g., "-p garraia-agents")
                    if name.starts_with("-p ") {
                        let parts: Vec<&str> = name.splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            cmd.arg("-p").arg(parts[1]);
                        }
                    } else {
                        cmd.arg(name);
                    }
                }
                cmd.arg("--").arg("--color=never");
                cmd.current_dir(working_dir);
                (cmd, "cargo test".to_string())
            }
            TestFramework::Flutter => {
                let mut cmd = Command::new("flutter");
                cmd.arg("test");
                if let Some(name) = test_name {
                    cmd.arg(name);
                }
                cmd.current_dir(working_dir);
                (cmd, "flutter test".to_string())
            }
            TestFramework::Npm => {
                let mut cmd = Command::new("npm");
                cmd.arg("test");
                if let Some(name) = test_name {
                    cmd.arg("--").arg(name);
                }
                cmd.current_dir(working_dir);
                (cmd, "npm test".to_string())
            }
            TestFramework::Pytest => {
                let mut cmd = Command::new("python");
                cmd.arg("-m").arg("pytest");
                if let Some(name) = test_name {
                    cmd.arg(name);
                }
                cmd.arg("-v");
                cmd.current_dir(working_dir);
                (cmd, "pytest".to_string())
            }
            TestFramework::Unknown => {
                // Try cargo test as default for Rust projects
                let mut cmd = Command::new("cargo");
                cmd.arg("test");
                cmd.arg("--").arg("--color=never");
                cmd.current_dir(working_dir);
                (cmd, "cargo test (default)".to_string())
            }
        }
    }

    /// Parse test output for summary
    fn parse_summary(framework: TestFramework, output: &str) -> String {
        match framework {
            TestFramework::Cargo => {
                // Look for "test result:" line
                let mut summary = String::new();
                for line in output.lines() {
                    if line.contains("test result:")
                        || line.contains("FAILED")
                        || line.contains("failures:")
                    {
                        summary.push_str(line.trim());
                        summary.push('\n');
                    }
                }
                if summary.is_empty() {
                    "No test summary found in output.".to_string()
                } else {
                    summary
                }
            }
            _ => {
                // Generic: look for pass/fail patterns
                let lines: Vec<&str> = output.lines().collect();
                let last_lines: Vec<&str> = lines.iter().rev().take(10).copied().collect();
                last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
            }
        }
    }

    /// Truncate output if needed
    fn truncate(output: &str) -> String {
        if output.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while end > 0 && !output.is_char_boundary(end) {
                end -= 1;
            }
            let mut truncated = output[..end].to_string();
            truncated.push_str("\n... (output truncated)");
            truncated
        } else {
            output.to_string()
        }
    }
}

#[async_trait]
impl Tool for RunTestsTool {
    fn name(&self) -> &str {
        "run_tests"
    }

    fn description(&self) -> &str {
        "Executes the project test suite and reports results.\n\
         Auto-detects test framework: cargo test, flutter test, npm test, pytest.\n\
         Returns pass/fail counts and failed test details."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "test_name": {
                    "type": "string",
                    "description": "Specific test name or filter (optional). For cargo: test function name or '-p crate_name'"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for test execution (default: current directory)"
                },
                "framework": {
                    "type": "string",
                    "enum": ["cargo", "flutter", "npm", "pytest", "auto"],
                    "description": "Test framework to use (default: auto-detect)"
                }
            }
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let test_name = input.get("test_name").and_then(|v| v.as_str());
        // #1084: o filtro vem do modelo e vira argumento do runner. Um valor
        // comecando com `-` e lido como opcao — e `cargo test --config` chega
        // a execucao arbitraria pelo `runner` do target.
        if let Some(name) = test_name
            && let Err(e) = Self::validate_test_name(name)
        {
            tracing::warn!(session = %context.session_id, "run_tests: test_name recusado");
            return Ok(ToolOutput::error(format!("test_name invalido: {e}")));
        }
        // Sem `working_dir`, o diretorio da sessao; com, o mesmo resolvedor
        // do `file_read` (relativo a sessao, `..` recusado). Auditoria do
        // #1039: o valor vinha cru do modelo e ia direto para `current_dir`.
        let working_dir_str = input
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let resolved = match resolve_tool_path(
            working_dir_str,
            context.working_dir.as_deref(),
            process_home_dir().as_deref(),
        ) {
            Ok(r) => r,
            Err(e) => return Ok(ToolOutput::error(e.to_string())),
        };
        let working_dir = resolved.path.clone();

        if !working_dir.exists() {
            return Ok(ToolOutput::error(format!(
                "Working directory not found: {}",
                resolved.describe()
            )));
        }

        // #1078 item 2: o assunto da aprovacao e o diretorio que a suite vai
        // rodar. Um "ok" dado para rodar os testes de um projeto nao
        // autoriza rodar os de outro, nem um `bash` qualquer.
        let assunto = working_dir.display().to_string();
        if self.confirmation_enabled && !context.approval.covers(self.name(), &assunto) {
            tracing::warn!(
                dir = %working_dir.display(),
                session = %context.session_id,
                "run_tests: requires user confirmation"
            );
            let marcador = ApprovalFingerprint::of(self.name(), &assunto).marker();
            return Ok(ToolOutput::confirmation_request(format!(
                "{marcador} run_tests vai executar a suite de testes em:\n\
                 ```\n{assunto}\n```\n\
                 Responda **sim** para executar ou **nao** para cancelar."
            )));
        }

        // Detect or use specified framework
        let framework_str = input.get("framework").and_then(|v| v.as_str());
        let framework = match framework_str {
            Some("cargo") => TestFramework::Cargo,
            Some("flutter") => TestFramework::Flutter,
            Some("npm") => TestFramework::Npm,
            Some("pytest") => TestFramework::Pytest,
            _ => Self::detect_framework(&working_dir),
        };

        let (mut cmd, framework_name) = Self::build_command(framework, test_name, &working_dir);

        // #1084 item 4: sem canal de confirmacao, a regra passa a ser a MESMA
        // do `bash`, aplicada ao comando que vai rodar de verdade.
        //
        // O bloqueio anterior era incondicional, e nao protegia nada: no mesmo
        // runtime sem canal, `bash("cargo test")` roda — `cargo test` nao e
        // comando sensivel no gate. Era a mesma capacidade por outra porta,
        // com o custo de deixar `run_tests` inutil no caminho full-auto. Agora
        // quem decide e o gate: uma suite cujo comando o gate considera
        // sensivel continua bloqueada, e uma que ele deixaria passar pelo
        // `bash` passa aqui tambem. Nenhuma capacidade nova e concedida.
        //
        // Com canal de confirmacao nada disso se aplica: a aprovacao humana
        // abaixo continua sendo pedida para toda suite, sensivel ou nao.
        if !self.confirmation_enabled {
            let linha = Self::command_line(&cmd);
            if garraia_common::safety_gate::is_risky(&linha).is_err() {
                tracing::warn!(
                    dir = %working_dir.display(),
                    session = %context.session_id,
                    "run_tests: BLOCKED (fail-closed: sensitive command, no confirmation channel)"
                );
                return Ok(ToolOutput::error(
                    "run_tests bloqueado por seguranca: o comando desta suite e sensivel e \
                     este runtime nao possui canal de confirmacao (fail-closed)."
                        .to_string(),
                ));
            }
        }

        // #1075 R3 (parity — auditoria do hardening): o filho de run_tests
        // executa o que o projeto mandar e, como o bash, herda SOMENTE a
        // allowlist de env do pai — nunca segredos do processo gateway/MCP.
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in garraia_common::safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }

        // Execute with timeout
        let result = tokio::time::timeout(self.timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combined = String::new();
                combined.push_str(&format!("Framework: {}\n", framework_name));
                combined.push_str(&format!(
                    "Exit code: {}\n\n",
                    output.status.code().unwrap_or(-1)
                ));

                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                // Add summary at the end
                let summary = Self::parse_summary(framework, &combined);
                combined.push_str("\n--- Summary ---\n");
                combined.push_str(&summary);

                let is_success = output.status.success();
                let content = Self::truncate(&combined);

                if is_success {
                    Ok(ToolOutput::success(content))
                } else {
                    Ok(ToolOutput::error(content))
                }
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!(
                "Failed to execute {}: {}",
                framework_name, e
            ))),
            Err(_) => Ok(ToolOutput::error(format!(
                "Tests timed out after {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_framework() {
        // Current dir should have Cargo.toml
        let framework = RunTestsTool::detect_framework(Path::new("."));
        assert_eq!(framework, TestFramework::Cargo);
    }

    #[test]
    fn test_parse_cargo_summary() {
        let output = r#"
running 3 tests
test test_one ... ok
test test_two ... ok
test test_three ... FAILED

failures:

---- test_three stdout ----
assertion failed

failures:
    test_three

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let summary = RunTestsTool::parse_summary(TestFramework::Cargo, output);
        assert!(summary.contains("FAILED"));
        assert!(summary.contains("test result:"));
    }

    #[test]
    fn test_run_tests_schema() {
        let tool = RunTestsTool::new(None);
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    /// `approved` liga a aprovacao PARA O DIRETORIO que a tool vai resolver
    /// — que e o `working_dir` quando ele existe, e o diretorio do processo
    /// quando nao. Antes era um booleano solto, e era esse o defeito do
    /// #1078 item 2.
    fn ctx(working_dir: Option<&str>, approved: bool) -> ToolContext {
        // O assunto tem de ser o diretorio RESOLVIDO, que e o que a tool
        // usa para montar a impressao digital — mesma chamada, para o teste
        // nao adivinhar a resolucao e sair dessincronizado dela.
        let assunto = resolve_tool_path(".", working_dir, process_home_dir().as_deref())
            .map(|r| r.path.display().to_string())
            .unwrap_or_default();
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: if approved {
                crate::tools::approval::ToolApproval::granted("run_tests", &assunto)
            } else {
                crate::tools::approval::ToolApproval::None
            },
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    /// Com confirmacao ligada e sem aprovacao, nada roda: a tool devolve o
    /// pedido de confirmacao apontando o diretorio da sessao (o default).
    #[tokio::test]
    async fn asks_confirmation_before_running_in_the_session_dir() {
        let dir = env!("CARGO_MANIFEST_DIR");
        let tool = RunTestsTool::new_with_confirmation(Some(1));
        let out = tool
            .execute(&ctx(Some(dir), false), serde_json::json!({}))
            .await
            .expect("executa");
        assert!(out.requires_confirmation, "{}", out.content);
        assert!(out.content.contains(dir), "{}", out.content);
    }

    /// `..` e recusado antes de qualquer execucao; relativo sem sessao
    /// resolve contra o CWD do processo (como o `file_read`) e, quando nao
    /// existe, o erro explica de onde veio.
    #[tokio::test]
    async fn refuses_traversal_and_explains_relative_without_session_dir() {
        let tool = RunTestsTool::new(Some(1));
        let out = tool
            .execute(
                &ctx(Some(env!("CARGO_MANIFEST_DIR")), true),
                serde_json::json!({"working_dir": "../.."}),
            )
            .await
            .expect("executa");
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("traversal"), "{}", out.content);

        let out = tool
            .execute(
                &ctx(None, true),
                serde_json::json!({"working_dir": "nao_existe_xyz"}),
            )
            .await
            .expect("executa");
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("não tem working_dir"),
            "{}",
            out.content
        );
    }

    /// Diretorio inexistente: erro com o caminho resolvido, sem rodar nada.
    #[tokio::test]
    async fn missing_dir_is_an_error_with_the_resolved_path() {
        let tool = RunTestsTool::new(Some(1));
        let out = tool
            .execute(
                &ctx(Some(env!("CARGO_MANIFEST_DIR")), true),
                serde_json::json!({"working_dir": "nao_existe_xyz"}),
            )
            .await
            .expect("executa");
        assert!(out.is_error);
        assert!(out.content.contains("nao_existe_xyz"), "{}", out.content);
    }

    /// #1084 item 4: sem canal de confirmacao, o que decide e o gate — o mesmo
    /// do `bash`, aplicado ao comando que vai rodar de verdade.
    ///
    /// Aqui o filtro toca o canal de procfs (`environ`, no CONFIRM_LIST), a
    /// linha montada e sensivel, e a suite e bloqueada antes de qualquer
    /// processo nascer. A aprovacao vem ligada de proposito: sem canal ela
    /// pode ter sido plantada no historico, e nao deve valer nada.
    #[tokio::test]
    async fn sensitive_suite_is_blocked_without_confirmation_channel() {
        let tool = RunTestsTool::new(Some(1));
        let out = tool
            .execute(
                &ctx(Some(env!("CARGO_MANIFEST_DIR")), true),
                serde_json::json!({ "framework": "cargo", "test_name": "environ" }),
            )
            .await
            .expect("executa");
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("bloqueado"), "{}", out.content);
        assert!(!out.requires_confirmation);
    }

    /// A parte da paridade que nao da para provar executando: rodar a suite de
    /// verdade dentro do `cargo test` seria recursivo.
    ///
    /// O que esta afirmado aqui e a premissa da mudanca — uma suite comum ja
    /// era permitida pelo mesmo gate atraves do `bash`, entao liberar
    /// `run_tests` no full-auto nao concede capacidade nova.
    #[test]
    fn a_plain_suite_is_what_bash_would_already_allow() {
        use garraia_common::safety_gate::is_risky;

        // Permitidas — e ja eram, via `bash("cargo test")` no mesmo runtime.
        assert!(is_risky("cargo test -- --color=never").is_ok());
        assert!(is_risky("npm test").is_ok());

        // Sensiveis, e continuam sensiveis. `pytest` roda por um interpretador
        // Python, que o gate trata como codigo arbitrario: no full-auto a
        // suite de pytest segue bloqueada, exatamente como
        // `bash("python -m pytest")` estaria. Isto e a paridade funcionando,
        // nao uma excecao a ela.
        assert!(is_risky("python -m pytest -v").is_err());

        // E um filtro que toca procfs torna sensivel ate a linha do cargo.
        assert!(is_risky("cargo test environ -- --color=never").is_err());
    }

    // ── test_name como argumento do runner (#1084) ────────────────────────

    #[test]
    fn test_name_rejects_option_lookalikes() {
        // O payload que motivou a checagem: `--config` do cargo aponta um
        // `runner` de target, que e execucao arbitraria.
        let payload = "--config=target.x.runner='/bin/sh -c curl|sh'";
        assert!(RunTestsTool::validate_test_name(payload).is_err());
        assert!(RunTestsTool::validate_test_name("--offline").is_err());
        assert!(RunTestsTool::validate_test_name("-Zunstable-options").is_err());
        assert!(RunTestsTool::validate_test_name("").is_err());
        assert!(RunTestsTool::validate_test_name("a\nb").is_err());
        assert!(RunTestsTool::validate_test_name(&"a".repeat(201)).is_err());
    }

    #[test]
    fn test_name_accepts_filters_and_the_documented_p_form() {
        assert!(RunTestsTool::validate_test_name("meu_teste").is_ok());
        assert!(RunTestsTool::validate_test_name("tests::modulo::caso").is_ok());
        assert!(RunTestsTool::validate_test_name("-p garraia-agents").is_ok());
        // `-p` so vale com um nome de crate de verdade depois dele.
        assert!(RunTestsTool::validate_test_name("-p ").is_err());
        assert!(RunTestsTool::validate_test_name("-p a b").is_err());
    }

    #[tokio::test]
    async fn option_shaped_test_name_never_reaches_the_runner() {
        let tool = RunTestsTool::new(Some(1));
        let out = tool
            .execute(
                &ctx(Some(env!("CARGO_MANIFEST_DIR")), true),
                serde_json::json!({ "test_name": "--config=target.x.runner='/bin/sh'" }),
            )
            .await
            .expect("executa");
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("test_name invalido"),
            "{}",
            out.content
        );
    }

    #[test]
    fn command_line_shows_what_actually_runs() {
        let dir = std::path::Path::new(".");
        let (cmd, _) = RunTestsTool::build_command(TestFramework::Cargo, Some("meu_teste"), dir);
        let linha = RunTestsTool::command_line(&cmd);
        assert!(linha.starts_with("cargo test"), "{linha}");
        assert!(linha.contains("meu_teste"), "{linha}");
    }

    /// #1075 R3 (auditoria do hardening): o filho de run_tests NAO herda o
    /// env do pai — projeto cargo com teste canario que falha se a variavel
    /// existir no ambiente do filho.
    #[cfg(unix)]
    #[tokio::test]
    async fn r3_run_tests_child_env_is_scrubbed() {
        unsafe {
            std::env::set_var("GARRAIA_R3_RUNTESTS_CANARY", "leak-canary");
        }
        let dir = std::env::temp_dir().join(format!("garraia-r3-runtests-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"canario-r3\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src").join("lib.rs"),
            "#[test]\nfn canario_r3() {\n    let v = std::env::var(\"GARRAIA_R3_RUNTESTS_CANARY\");\n    assert!(v.is_err(), \"canary leaked: {:?}\", v);\n}\n",
        )
        .unwrap();

        let tool = RunTestsTool::new_with_confirmation(Some(300));
        let out = tool
            .execute(
                &ctx(Some(dir.to_string_lossy().as_ref()), true),
                serde_json::json!({}),
            )
            .await
            .expect("executa");

        assert!(
            !out.is_error,
            "teste canario falhou (env vazou?): {}",
            out.content
        );
        assert!(!out.content.contains("leak-canary"), "{}", out.content);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
