//! #1615 — o núcleo do `garra_agent`: o agente completo, one-shot, com tools.
//!
//! Mora no `garraia-gateway` para ser usado pelos dois lados da mesma ferramenta:
//! o servidor stdio (`garra mcp-server`, na CLI) e a ponte MCP HTTP
//! (`POST /mcp`). A CLI depende do gateway, nunca o contrario; por isso o núcleo
//! desce para cá, do mesmo jeito que o `garraia-ask` (#1612) desceu para o seu
//! crate. Ficou aqui e não em `garraia-ask` porque a decisão de expor o `bash`
//! (`ExposicaoDoBash`) e a policy de sandbox moram em [`crate::bootstrap`], e
//! movê-las antes seria uma refatoração à parte.
//!
//! Este módulo **não** lê config de transporte, env de allowlist nem nada de
//! MCP: quem chama decide o jail, o nome da sessão e a política. O que ele faz é
//! montar o runtime (provider + tools + system prompt), rodar um turno e devolver
//! o [`AgentOutcome`] com o resumo das tools.
//!
//! # Fronteira de segurança
//!
//! O agente executa **como o próprio gateway**. Quem o chama não ganha shell
//! irrestrito: o `bash` só existe com um sandbox docker/podman válido ou no host
//! de um `isolated-pod` explícito (#1272), e a denylist do `BashTool` vale em
//! qualquer caso. As file tools ficam presas ao [`FileJail`] que o chamador
//! passa. Na ponte HTTP, a fronteira é: a lista de orquestradores (quem pode
//! chamar), o teto por minuto próprio (quanto pode gastar) e o sandbox acima.
//!
//! # Opt-in
//!
//! Nada aqui é anunciado sozinho. O stdio só registra `garra_agent` com
//! `GARRAIA_MCP_ENABLE_TOOLS`; a ponte HTTP só com `gateway.mcp_http.allow_agent`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use garraia_agents::exec_context::ExecContext;
use garraia_agents::tools::git_diff_tool::GitDiffTool;
use garraia_agents::{
    AgentRuntime, BashTool, ChatMessage, FileJail, FileReadTool, FileWriteTool, TurnEvent,
    WebFetchTool, WebSearchTool,
};
use garraia_ask::{AskError, sanitize_provider_error};
use garraia_config::AppConfig;
use serde_json::json;

use crate::bootstrap::{ExposicaoDoBash, exposicao_do_bash, sandbox_policy_from};

/// 64 KiB de teto para `message`. O mesmo de `garra_ask`.
pub const AGENT_MESSAGE_MAX_BYTES: usize = 64 * 1024;
/// 8 KiB de teto para o `system_prompt` de quem chama. O mesmo de `garra_ask`.
pub const AGENT_SYSTEM_PROMPT_MAX_BYTES: usize = 8 * 1024;

/// Faixa do timeout do agente. O relógio cobre o laço INTEIRO — cada ida ao LLM
/// e cada execução de tool —, então a faixa é mais larga que a do `garra_ask`:
/// trabalho com várias tools roda em minutos. O teto do operador
/// (`GARRAIA_MCP_MAX_TIMEOUT_SECS` no stdio, `gateway.mcp_http.agent_max_seconds`
/// na ponte) se aplica por cima.
pub const AGENT_TIMEOUT_SECS_MIN: u64 = 5;
pub const AGENT_TIMEOUT_SECS_MAX: u64 = 1800;
pub const AGENT_TIMEOUT_SECS_DEFAULT: u64 = 300;

/// Teto do resumo de cada tool no envelope. O runtime já corta em 72 caracteres
/// na origem (`turn_events::SUMMARY_MAX_CHARS`); isto só guarda a invariante.
const TOOL_CALL_SUMMARY_MAX_CHARS: usize = 160;

/// Quanto tempo drenar o canal de eventos depois que o turno termina (ou estoura
/// o timeout). Normalmente volta na hora; o limite só pesa se alguma tarefa de
/// fundo ainda segurar um clone do emissor.
const EVENT_DRAIN_CAP: Duration = Duration::from_millis(500);

/// Opções já validadas de uma execução: defaults aplicados, política checada.
#[derive(Debug, Clone)]
pub struct AgentOptions {
    pub message: String,
    pub provider: String,
    pub model: String,
    pub timeout_secs: u64,
    pub system_prompt: Option<String>,
    /// Diretório em que as file tools resolvem caminho relativo e o `bash`
    /// executa (#1075 R3). Quem o informa já o confinou ao jail.
    pub working_dir: Option<String>,
}

/// Uma execução de tool, como vai no envelope para o host MCP.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub duration_ms: u64,
    pub success: bool,
    pub summary: String,
}

/// O resultado puro de [`agent_oneshot`]. `tool_calls` vai nos DOIS ramos, para
/// o host manter a visibilidade parcial quando a execução falha ou estoura o
/// tempo no meio do laço.
#[derive(Debug, Clone)]
pub enum AgentOutcome {
    Success {
        answer: String,
        provider: String,
        model: String,
        latency_ms: u128,
        session_id: String,
        tool_calls: Vec<ToolCallSummary>,
    },
    Failure {
        error: AskError,
        provider: String,
        model: String,
        tool_calls: Vec<ToolCallSummary>,
    },
}

impl AgentOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Monta o envelope `garra.agent.v1` deste resultado.
    pub fn to_envelope(&self) -> serde_json::Value {
        match self {
            Self::Success {
                answer,
                provider,
                model,
                latency_ms,
                session_id,
                tool_calls,
            } => {
                agent_success_envelope(answer, provider, model, *latency_ms, session_id, tool_calls)
            }
            Self::Failure {
                error,
                provider,
                model,
                tool_calls,
            } => agent_error_envelope(
                error.kind_str(),
                &error.message(),
                provider,
                model,
                tool_calls,
            ),
        }
    }
}

/// `garra.agent.v1` — envelope de sucesso. Mesmo formato do `garra.ask.v1`, mais
/// `session_id` e o resumo das tools.
pub fn agent_success_envelope(
    answer: &str,
    provider: &str,
    model: &str,
    latency_ms: u128,
    session_id: &str,
    tool_calls: &[ToolCallSummary],
) -> serde_json::Value {
    json!({
        "schema": "garra.agent.v1",
        "ok": true,
        "answer": answer,
        "provider": provider,
        "model": model,
        "latency_ms": latency_ms,
        "session_id": session_id,
        "tool_calls": tool_calls,
    })
}

/// `garra.agent.v1` — envelope de erro. `kind` reusa os rótulos estáveis do
/// `AskError` (usage|no_provider|provider_error|timeout|io).
pub fn agent_error_envelope(
    kind: &str,
    message: &str,
    provider: &str,
    model: &str,
    tool_calls: &[ToolCallSummary],
) -> serde_json::Value {
    json!({
        "schema": "garra.agent.v1",
        "ok": false,
        "provider": provider,
        "model": model,
        "tool_calls": tool_calls,
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}

/// Monta as tools do agente: `bash` só onde #1272 permite, file read/write
/// presas ao `jail`, web fetch, git diff e web search com chave Brave.
///
/// O `jail` chega pronto: quem o constrói é quem também valida o `working_dir`,
/// e são a mesma instância. Duas réguas (uma para validar, outra para operar)
/// deixariam a validação aprovar contra uma e as tools agirem com outra (#1244,
/// rodada 4 I2).
///
/// O `bash` só entra quando `exposicao.registra_bash()`. Em `standard` sem
/// sandbox ele não existe: aqui não há canal de confirmação humana (#1075 R1), e
/// o tier arriscado não pega `cat` nem `>`.
fn build_tools(
    config: &AppConfig,
    jail: &FileJail,
) -> (Vec<Box<dyn garraia_agents::Tool>>, ExposicaoDoBash) {
    let policy = sandbox_policy_from(&config.agent.sandbox);
    let allowlist_ativa = !config.agent.bash_allowlist.is_empty();
    let exposicao = exposicao_do_bash(config.execution.perfil(), &policy, allowlist_ativa);
    let tools = build_tools_com(config, jail, policy, &exposicao);
    (tools, exposicao)
}

/// [`build_tools`] com a decisão do `bash` já tomada. É por aqui que os testes
/// injetam a disponibilidade do backend sem depender do host.
fn build_tools_com(
    config: &AppConfig,
    jail: &FileJail,
    policy: garraia_agents::SandboxPolicy,
    exposicao: &ExposicaoDoBash,
) -> Vec<Box<dyn garraia_agents::Tool>> {
    let mut tools: Vec<Box<dyn garraia_agents::Tool>> = Vec::new();
    if exposicao.registra_bash() {
        let mut bash = BashTool::new(None).with_allowlist(config.agent.bash_allowlist.clone());
        bash.set_sandbox_policy(policy.clone());
        // #1272: em `HostComAllowlist` a allowlist é FRONTEIRA — só padrão
        // declarado executa no host.
        if let ExposicaoDoBash::HostComAllowlist = exposicao {
            bash = bash.with_allowlist_only();
        }
        tools.push(Box::new(bash));
    }
    tools.push(Box::new(FileReadTool::new(jail.clone())));
    tools.push(Box::new(FileWriteTool::new(jail.clone())));
    tools.push(Box::new(WebFetchTool::new(None)));
    // #1225 S2: o git_diff também consulta `agent.sandbox`.
    tools.push(Box::new(GitDiffTool::new(None, None).com_sandbox(policy)));
    let brave_config_key = config.llm.get("brave").and_then(|c| c.api_key.clone());
    let brave = brave_config_key
        .or_else(|| std::env::var("BRAVE_API_KEY").ok())
        .filter(|k| !k.trim().is_empty());
    if let Some(key) = brave {
        tools.push(Box::new(WebSearchTool::new(key)));
    }
    tools
}

/// O system prompt padrão, gerado das tools que de fato foram registradas (nome
/// e descrição de cada uma), do diretório de trabalho efetivo e do aviso sobre o
/// CWD do `bash`. A estrutura espelha a receita do chat da CLI (`chat.rs`): modelo
/// fraco só chama tool quando o prompt a nomeia.
pub fn agent_system_prompt(
    tools: &[(String, String)],
    working_dir: Option<&str>,
    bash: &ExposicaoDoBash,
) -> String {
    let mut prompt = String::from(
        "Voce e o GarraIA, um assistente pessoal de IA criado em Rust. \
         Voce esta rodando como agente COMPLETO via MCP e pode executar \
         as ferramentas listadas abaixo para cumprir a tarefa. \
         Seja prestativo, conciso e amigavel. Responda no idioma do usuario.\n\n\
         ## Ferramentas disponiveis\n\
         Voce tem acesso a estas ferramentas que pode usar quando necessario:\n",
    );
    for (name, description) in tools {
        prompt.push_str(&format!("- **{name}**: {}\n", description.trim()));
    }
    prompt.push_str(
        "\nIMPORTANTE: Quando a tarefa envolver arquivos, comandos ou dados reais, \
         USE as ferramentas para investigar em vez de apenas descrever. \
         Use 'file_read' para ler conteudo. \
         Nao invente resultados — execute e reporte o que aconteceu de verdade. \
         Se uma ferramenta falhar ou for bloqueada, relate isso na resposta \
         final: nunca reporte sucesso sem saida real e nunca contorne \
         silenciosamente um bloqueio de seguranca.\n",
    );
    // #1272: o que o modelo lê sobre o shell tem de ser o que o código faz.
    prompt.push_str(&match bash {
        ExposicaoDoBash::Desligado { .. } => "\n## Shell\n\
             A ferramenta 'bash' NAO esta disponivel neste servidor (nenhum \
             sandbox docker/podman utilizavel). Nao existe outro jeito de executar comandos \
             de shell aqui: se a tarefa precisar de um, diga isso na resposta.\n"
            .to_string(),
        ExposicaoDoBash::Sandbox { .. } => "\n## Shell\n\
             O 'bash' roda dentro de um container descartavel: so o diretorio \
             de trabalho e visivel e gravavel, e o resto do host nao existe \
             para ele.\n"
            .to_string(),
        ExposicaoDoBash::HostDoPod => "\n## Shell\n\
             O 'bash' roda no host deste pod isolado (execution.profile = \
             isolated-pod); a denylist de comandos perigosos continua valendo.\n"
            .to_string(),
        ExposicaoDoBash::HostComAllowlist => "\n## Shell\n\
             O 'bash' roda no host, mas em modo allowlist-only: SO os comandos \
             que casam com os padroes declarados pelo operador executam — \
             comando fora da lista e negado. Em encadeamento (; && || |) TODO \
             segmento tem de estar declarado; substituicao ($(...), crase) e \
             redirecionamento (< >) nunca casam.\n"
            .to_string(),
    });
    if let Some(dir) = working_dir {
        prompt.push_str(&format!(
            "\n## Contexto do diretorio\n\
             O chamador indicou o diretorio de trabalho: {dir}\n\
             As ferramentas de arquivo (file_read/file_write) resolvem caminhos \
             relativos contra este diretorio{}. O diretorio ja foi confinado as raizes de \
             arquivo deste servidor: as file tools nao alcancam nada fora \
             delas, e uma recusa de caminho e um bloqueio de seguranca, nao \
             um erro de digitacao — nao tente outra rota para o mesmo arquivo.\n",
            if bash.registra_bash() {
                " e o 'bash' EXECUTA nele (current_dir, hardening #1075)"
            } else {
                ""
            }
        ));
    }
    prompt
}

/// Redutor puro dos eventos do turno: um resumo por tool terminada, na ordem
/// da emissão. `ToolStarted` e `TextDelta` são ignorados; os resumos são
/// truncados de novo por defesa (o runtime já redige e corta na origem).
pub fn events_to_summaries(events: &[TurnEvent]) -> Vec<ToolCallSummary> {
    events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished {
                name,
                duration,
                success,
                summary,
                ..
            } => Some(ToolCallSummary {
                name: name.clone(),
                duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                success: *success,
                summary: truncate_chars(summary, TOOL_CALL_SUMMARY_MAX_CHARS),
            }),
            _ => None,
        })
        .collect()
}

/// Corta por caracteres, nunca por bytes: caminhos e resumos carregam acentos.
/// Espelha `turn_events::truncar`.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// O runtime do `garra_agent`: provider + tools de [`build_tools`] + system
/// prompt gerado delas. Separado de [`agent_oneshot`] para os testes adversariais
/// da #1272 dirigirem ESTA montagem com um provider de stub. `register_tool`
/// recebe `&self`; os setters `&mut` rodam antes.
fn montar_runtime(
    config: &AppConfig,
    jail: &FileJail,
    provider: Arc<dyn garraia_agents::LlmProvider>,
    working_dir: Option<&str>,
) -> AgentRuntime {
    let (tools, exposicao) = build_tools(config, jail);
    let tool_pairs: Vec<(String, String)> = tools
        .iter()
        .map(|t| (t.name().to_string(), t.description().to_string()))
        .collect();
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);
    for tool in tools {
        runtime.register_tool(tool);
    }
    runtime.set_system_prompt(agent_system_prompt(&tool_pairs, working_dir, &exposicao));
    runtime.set_max_tokens(4096);
    runtime
}

/// Agente completo one-shot: resolve o provider, monta um `AgentRuntime` NOVO
/// com tools, roda um turno sobre histórico vazio e devolve o resultado com o
/// resumo das tools.
///
/// `session_prefix` vira o começo do id da sessão de trabalho:
/// `<prefixo>-<uuid>`. Cada chamada tem uma sessão nova, então duas chamadas —
/// de orquestradores diferentes ou não — nunca compartilham histórico.
///
/// O `jail` é o das file tools; quem chama o escolhe (a CLI com
/// `GARRAIA_MCP_ALLOWED_DIRS`, a ponte HTTP com as raízes da config).
pub async fn agent_oneshot(
    config: &AppConfig,
    opts: &AgentOptions,
    jail: &FileJail,
    session_prefix: &str,
) -> AgentOutcome {
    let start = Instant::now();

    // 1. Resolve o provider (o mesmo pipeline do `garra_ask`).
    let (provider_name, model_name, provider) = match garraia_ask::select_explicit_provider(
        config,
        &opts.provider,
        Some(&opts.model),
        None,
    ) {
        Ok(triple) => triple,
        Err(e) => {
            return AgentOutcome::Failure {
                error: AskError::NoProvider(sanitize_provider_error(&format!("{e:#}"))),
                provider: opts.provider.clone(),
                model: opts.model.clone(),
                tool_calls: Vec::new(),
            };
        }
    };

    // 2. Monta o runtime: provider + tools completas.
    //
    // O runtime busca o provider pelo id com que o REGISTROU, não pelo nome que
    // o chamador digitou. Os dois diferem para `llamacpp` (registrado como
    // `llama-cpp`) e para um alias de tipo que não pode ser renomeado. O
    // `provider_name` continua sendo o que o envelope reporta.
    let registered_id = provider.provider_id().to_string();
    let runtime = montar_runtime(config, jail, provider, opts.working_dir.as_deref());

    // 3. Uma chamada: sessão nova, histórico vazio, timeout sobre o laço inteiro.
    //    O canal de eventos dá o ciclo de vida das tools para o resumo.
    let session_id = format!("{session_prefix}-{}", uuid::Uuid::new_v4());
    let history: Vec<ChatMessage> = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(512);
    let exec = ExecContext::with_working_dir(opts.working_dir.clone());

    let call = runtime.process_message_streaming_with_events(
        &session_id,
        &opts.message,
        &history,
        tx,
        None,
        None,
        Some(&registered_id),
        Some(&model_name),
        opts.system_prompt.as_deref(),
        None,
        &exec,
    );
    let result = tokio::time::timeout(Duration::from_secs(opts.timeout_secs), call).await;
    let latency_ms = start.elapsed().as_millis();

    // Drena o que sobrou. O produtor já fechou o emissor nos dois caminhos (turno
    // terminou, ou o timeout derrubou o future), então `recv` devolve `None` na
    // hora. O limite só pesa se uma tarefa de fundo ainda segurar um clone.
    let mut events: Vec<TurnEvent> = Vec::new();
    let _ = tokio::time::timeout(EVENT_DRAIN_CAP, async {
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
    })
    .await;
    let tool_calls = events_to_summaries(&events);

    match result {
        Ok(Ok(answer)) => AgentOutcome::Success {
            answer,
            provider: provider_name,
            model: model_name,
            latency_ms,
            session_id,
            tool_calls,
        },
        Ok(Err(e)) => AgentOutcome::Failure {
            error: AskError::ProviderError(sanitize_provider_error(&format!("{e:#}"))),
            provider: provider_name,
            model: model_name,
            tool_calls,
        },
        Err(_elapsed) => AgentOutcome::Failure {
            error: AskError::Timeout(opts.timeout_secs),
            provider: provider_name,
            model: model_name,
            tool_calls,
        },
    }
}

#[cfg(test)]
mod tests {
    //! Testes do núcleo: montagem de tools (#1225, #1272), system prompt,
    //! envelopes, redutor de eventos e o turno inteiro com provider de stub.
    //! Zero rede externa; os de roteamento usam o endpoint de loopback.

    use super::*;

    /// O jail do gateway: só as raízes da config (sem o CWD do processo).
    fn jail_de(config: &AppConfig) -> FileJail {
        FileJail::from_config_roots(&config.agent.file_roots)
    }

    // ─── #1225: agent.sandbox chega ao BashTool ────────────────────────

    fn config_isolated_pod() -> AppConfig {
        AppConfig {
            execution: garraia_config::ExecutionConfig::new(
                Some(garraia_config::ExecutionProfile::IsolatedPod),
                None,
            ),
            ..AppConfig::default()
        }
    }

    fn ctx_sem_dir() -> garraia_agents::tools::ToolContext {
        garraia_agents::tools::ToolContext {
            session_id: "teste-1272".into(),
            user_id: None,
            is_heartbeat: false,
            approval: Default::default(),
            working_dir: None,
            project_id: None,
        }
    }

    /// `isolated-pod` + `mode = all` sem `backend`: o sandbox é EXIGIDO e
    /// impossível, então o `bash` NÃO é registrado.
    #[test]
    fn isolated_pod_com_sandbox_exigido_sem_backend_nao_registra_bash() {
        let mut config = config_isolated_pod();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        let (tools, exposicao) = build_tools(&config, &jail_de(&config));
        assert!(!tools.iter().any(|t| t.name() == "bash"));
        assert_eq!(
            exposicao,
            ExposicaoDoBash::Desligado {
                motivo: crate::bootstrap::MotivoDoBashDesligado::SemBackend
            }
        );
    }

    /// Prova de fiação ponta a ponta pela função de PRODUÇÃO (`build_tools_com`):
    /// a policy do config chega ao `BashTool`. Com sandbox docker exigido e sessão
    /// sem `working_dir`, todo comando é recusado fail-closed.
    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_do_config_chega_ao_bash_tool_pelo_build_tools() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        let policy = sandbox_policy_from(&config.agent.sandbox);
        let exposicao = ExposicaoDoBash::Sandbox {
            backend: garraia_agents::SandboxBackend::Docker,
        };
        let tools = build_tools_com(&config, &jail_de(&config), policy, &exposicao);
        let bash = tools
            .iter()
            .find(|t| t.name() == "bash")
            .expect("exposicao Sandbox registra a tool bash");
        let out = bash
            .execute(&ctx_sem_dir(), serde_json::json!({"command": "echo nunca"}))
            .await
            .expect("a tool devolve ToolOutput, nao Err");
        assert!(out.is_error, "sandbox obrigatorio tem de bloquear: {out:?}");
        assert!(out.content.contains("fail-closed"), "{}", out.content);
        assert!(
            !out.content.contains("nunca"),
            "o comando nao pode ter rodado no host: {}",
            out.content
        );
    }

    /// #1225 S2: o `git_diff` do `garra_agent` recebe a policy do config.
    #[tokio::test]
    async fn sandbox_do_config_chega_ao_git_diff_pelo_build_tools() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        let tools = build_tools(&config, &jail_de(&config)).0;
        let git = tools
            .iter()
            .find(|t| t.name() == "git_diff")
            .expect("git_diff registrada");
        let texto = match git
            .execute(&ctx_sem_dir(), serde_json::json!({"operation": "status"}))
            .await
        {
            Ok(o) => o.content,
            Err(e) => e.to_string(),
        };
        assert!(texto.contains("nenhum backend"), "{texto}");
    }

    // ─── #1272: bash fail-closed em standard ───────────────────────────

    /// O default de toda instalação (`standard`, sem `agent.sandbox`): o `bash`
    /// NÃO é registrado; as file tools e o git seguem.
    #[test]
    fn standard_sem_sandbox_nao_registra_bash() {
        let config = AppConfig::default();
        let (tools, exposicao) = build_tools(&config, &jail_de(&config));
        let nomes: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!nomes.contains(&"bash"), "bash em standard: {nomes:?}");
        assert!(!exposicao.registra_bash());
        for esperada in ["file_read", "file_write", "web_fetch", "git_diff"] {
            assert!(nomes.contains(&esperada), "{esperada} sumiu: {nomes:?}");
        }
    }

    /// Cada forma de "sandbox que não isola" em `standard` deixa o bash de fora:
    /// ssh (mesmo com as flags reconhecidas), bash elevado, allowlist sem bash, e
    /// sandbox sem backend.
    #[test]
    fn standard_com_sandbox_que_nao_isola_nao_registra_bash() {
        use garraia_config::{SandboxBackendKind, SandboxMode};
        let mut ssh = AppConfig::default();
        ssh.agent.sandbox.mode = SandboxMode::All;
        ssh.agent.sandbox.backend = Some(SandboxBackendKind::Ssh);
        ssh.agent.sandbox.ssh_host = Some("box".into());
        ssh.agent.sandbox.network_disabled = false;
        ssh.agent.sandbox.mount_workdir = false;

        let mut elevado = AppConfig::default();
        elevado.agent.sandbox.mode = SandboxMode::All;
        elevado.agent.sandbox.backend = Some(SandboxBackendKind::Docker);
        elevado.agent.sandbox.elevated = vec!["bash".into()];

        let mut allowlist = AppConfig::default();
        allowlist.agent.sandbox.mode = SandboxMode::Allowlist;
        allowlist.agent.sandbox.backend = Some(SandboxBackendKind::Docker);
        allowlist.agent.sandbox.sandboxed_tools = vec!["web_fetch".into()];

        let mut sem_backend = AppConfig::default();
        sem_backend.agent.sandbox.mode = SandboxMode::All;

        for (caso, config) in [
            ("ssh", ssh),
            ("elevado", elevado),
            ("allowlist", allowlist),
            ("sem_backend", sem_backend),
        ] {
            let (tools, _) = build_tools(&config, &jail_de(&config));
            assert!(
                !tools.iter().any(|t| t.name() == "bash"),
                "{caso}: bash registrado em standard"
            );
        }
    }

    /// Gêmeo positivo: com sandbox docker e o binário "presente" (decisão
    /// injetada), o bash entra.
    #[test]
    fn standard_com_sandbox_valido_registra_bash() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        let policy = sandbox_policy_from(&config.agent.sandbox);
        let exposicao = crate::bootstrap::decidir_exposicao_do_bash(
            config.execution.perfil(),
            &policy,
            true,
            |_| true,
            false,
        );
        let tools = build_tools_com(&config, &jail_de(&config), policy, &exposicao);
        assert!(tools.iter().any(|t| t.name() == "bash"));
    }

    #[test]
    fn isolated_pod_registra_bash() {
        let config = config_isolated_pod();
        let (tools, exposicao) = build_tools(&config, &jail_de(&config));
        assert!(tools.iter().any(|t| t.name() == "bash"));
        assert_eq!(exposicao, ExposicaoDoBash::HostDoPod);
    }

    /// Provider de stub: pede, uma por rodada, as tool calls do roteiro e guarda
    /// o que voltou de cada uma e quais tools o modelo viu. Sem `async_trait`, o
    /// impl é escrito na forma que a macro gera.
    struct Roteiro {
        chamadas: Vec<(String, serde_json::Value)>,
        volta: std::sync::atomic::AtomicUsize,
        resultados: std::sync::Mutex<Vec<String>>,
        vistas: std::sync::Mutex<Vec<String>>,
    }

    impl Roteiro {
        fn novo(chamadas: Vec<(&str, serde_json::Value)>) -> Arc<Self> {
            Arc::new(Self {
                chamadas: chamadas
                    .into_iter()
                    .map(|(n, v)| (n.to_string(), v))
                    .collect(),
                volta: std::sync::atomic::AtomicUsize::new(0),
                resultados: std::sync::Mutex::new(Vec::new()),
                vistas: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn responder(&self, request: &garraia_agents::LlmRequest) -> garraia_agents::LlmResponse {
            use garraia_agents::{ContentBlock, MessagePart};
            let mut resultados = self.resultados.lock().expect("lock");
            resultados.clear();
            for m in &request.messages {
                if let MessagePart::Parts(blocos) = &m.content {
                    for b in blocos {
                        if let ContentBlock::ToolResult { content, .. } = b {
                            resultados.push(content.clone());
                        }
                    }
                }
            }
            *self.vistas.lock().expect("lock") =
                request.tools.iter().map(|t| t.name.clone()).collect();
            let volta = self.volta.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let content = match self.chamadas.get(volta) {
                Some((nome, input)) => vec![ContentBlock::ToolUse {
                    id: format!("roteiro-{volta}"),
                    name: nome.clone(),
                    input: input.clone(),
                }],
                None => vec![ContentBlock::Text {
                    text: "fim do roteiro".to_string(),
                }],
            };
            garraia_agents::LlmResponse {
                content,
                model: "stub".to_string(),
                stop_reason: None,
                usage: None,
            }
        }
    }

    type Fut<'a, T> =
        std::pin::Pin<Box<dyn std::future::Future<Output = garraia_common::Result<T>> + Send + 'a>>;

    impl garraia_agents::LlmProvider for Roteiro {
        fn provider_id(&self) -> &str {
            "roteiro"
        }
        fn complete<'a, 'b, 'c>(
            &'a self,
            request: &'b garraia_agents::LlmRequest,
        ) -> Fut<'c, garraia_agents::LlmResponse>
        where
            'a: 'c,
            'b: 'c,
            Self: 'c,
        {
            let resposta = self.responder(request);
            Box::pin(async move { Ok(resposta) })
        }
        fn health_check<'a, 'c>(&'a self) -> Fut<'c, bool>
        where
            'a: 'c,
            Self: 'c,
        {
            Box::pin(async { Ok(true) })
        }
    }

    /// Roda um turno do `garra_agent` pela montagem de produção ([`montar_runtime`])
    /// com o provider de stub. Devolve os resultados das tools (na última rodada)
    /// e as tools que o modelo viu.
    async fn turno(
        config: &AppConfig,
        working_dir: Option<&str>,
        chamadas: Vec<(&str, serde_json::Value)>,
    ) -> (Vec<String>, Vec<String>) {
        let n = chamadas.len();
        let provider = Roteiro::novo(chamadas);
        let jail = jail_de(config);
        let runtime = montar_runtime(config, &jail, provider.clone(), working_dir);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(512);
        let drena = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let exec = ExecContext::with_working_dir(working_dir.map(str::to_string));
        let _ = runtime
            .process_message_streaming_with_events(
                "mcp-teste-1272",
                "faz o que o roteiro manda",
                &[],
                tx,
                None,
                None,
                Some("roteiro"),
                Some("stub"),
                None,
                None,
                &exec,
            )
            .await;
        drop(runtime);
        let _ = drena.await;
        let resultados = provider.resultados.lock().expect("lock").clone();
        let vistas = provider.vistas.lock().expect("lock").clone();
        assert!(
            provider.volta.load(std::sync::atomic::Ordering::SeqCst) > n,
            "o roteiro nao foi ate o fim"
        );
        (resultados, vistas)
    }

    /// Critério de aceite da #1272, adversarial, pela montagem REAL: em `standard`
    /// sem sandbox o modelo pede `bash cat /etc/shadow`, uma escrita fora das
    /// raízes por redireção, e um `sh -c tee` — e nada roda. Depois pede o mesmo
    /// pelas file tools, que o jail recusa.
    #[cfg(unix)]
    #[tokio::test]
    async fn standard_nega_leitura_e_escrita_fora_por_bash_e_por_file_tools() {
        let fora = tempfile::tempdir().expect("tempdir fora");
        let dentro = tempfile::tempdir().expect("tempdir dentro");
        let alvo = fora.path().join("pwned");
        let alvo_tee = fora.path().join("tee");
        let alvo_fw = fora.path().join("fw");
        let mut config = AppConfig::default();
        config.agent.file_roots = vec![dentro.path().to_string_lossy().into_owned()];
        let dir = dentro.path().to_string_lossy().into_owned();

        let (resultados, vistas) = turno(
            &config,
            Some(&dir),
            vec![
                ("bash", serde_json::json!({"command": "cat /etc/shadow"})),
                (
                    "bash",
                    serde_json::json!({"command": format!("echo pwned > {}", alvo.display())}),
                ),
                (
                    "bash",
                    serde_json::json!({
                        "command": format!("sh -c 'echo x | tee {}'", alvo_tee.display())
                    }),
                ),
                ("file_read", serde_json::json!({"path": "/etc/shadow"})),
                (
                    "file_write",
                    serde_json::json!({"path": alvo_fw.to_string_lossy(), "content": "x"}),
                ),
            ],
        )
        .await;

        assert!(
            !vistas.iter().any(|t| t == "bash"),
            "o modelo viu bash: {vistas:?}"
        );
        assert_eq!(
            resultados.len(),
            5,
            "cada chamada tem um resultado: {resultados:?}"
        );
        let tudo = resultados.join("\n");
        assert!(
            !tudo.contains("root:"),
            "conteudo de /etc/shadow vazou: {tudo}"
        );
        assert!(!alvo.exists(), "a redirecao escreveu fora das raizes");
        assert!(!alvo_tee.exists(), "o tee escreveu fora das raizes");
        assert!(!alvo_fw.exists(), "file_write escreveu fora das raizes");
    }

    /// Gêmeo positivo: em `isolated-pod` explícito o mesmo turno VÊ o bash, roda
    /// um comando no working_dir, e a denylist continua barrando o `rm -rf` da raiz.
    #[cfg(unix)]
    #[tokio::test]
    async fn isolated_pod_roda_bash_no_working_dir_e_mantem_a_denylist() {
        let dentro = tempfile::tempdir().expect("tempdir");
        let dir = dentro.path().canonicalize().expect("canon");
        let dir_str = dir.to_string_lossy().into_owned();
        let mut config = config_isolated_pod();
        config.agent.file_roots = vec![dir_str.clone()];

        let (resultados, vistas) = turno(
            &config,
            Some(&dir_str),
            vec![
                (
                    "bash",
                    serde_json::json!({"command": "echo ok > marca; pwd"}),
                ),
                (
                    "bash",
                    serde_json::json!({"command": concat!("rm -rf", " /")}),
                ),
            ],
        )
        .await;
        assert!(vistas.iter().any(|t| t == "bash"), "{vistas:?}");
        assert!(
            dir.join("marca").exists(),
            "o bash nao rodou no working_dir: {resultados:?}"
        );
        assert!(resultados[0].contains(&dir_str), "{resultados:?}");
        assert!(
            resultados[1].contains("bloqueado"),
            "a denylist tem de continuar valendo: {resultados:?}"
        );
    }

    // ─── agent_system_prompt ──────────────────────────────────────────

    #[test]
    fn system_prompt_names_every_registered_tool() {
        let pairs = vec![
            ("bash".to_string(), "Executa comandos".to_string()),
            ("file_read".to_string(), "Le arquivos".to_string()),
        ];
        let prompt = agent_system_prompt(&pairs, None, &ExposicaoDoBash::HostDoPod);
        assert!(prompt.contains("bash"));
        assert!(prompt.contains("file_read"));
        assert!(prompt.contains("## Ferramentas disponiveis"));
    }

    #[test]
    fn system_prompt_includes_working_dir_and_bash_caveat() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, Some("/tmp/projeto"), &ExposicaoDoBash::HostDoPod);
        assert!(prompt.contains("/tmp/projeto"));
        // #1075 R3: bash EXECUTA no working_dir (não ignora mais).
        assert!(
            prompt.contains("bash' EXECUTA nele"),
            "deve dizer que bash roda no working_dir"
        );
        assert!(prompt.contains("host deste pod"), "{prompt}");
    }

    /// #1272: sem bash registrado o prompt diz que ele não existe, não manda usar
    /// `bash ls` e nunca promete alcance do host.
    #[test]
    fn system_prompt_sem_bash_diz_que_nao_ha_shell() {
        let pairs = vec![("file_read".to_string(), "Le".to_string())];
        let desligado = ExposicaoDoBash::Desligado {
            motivo: crate::bootstrap::MotivoDoBashDesligado::SandboxDesligado,
        };
        let prompt = agent_system_prompt(&pairs, Some("/x"), &desligado);
        assert!(prompt.contains("'bash' NAO esta disponivel"), "{prompt}");
        assert!(!prompt.contains("EXECUTA nele"), "{prompt}");
        assert!(!prompt.contains("sem sandbox"), "{prompt}");
        assert!(!prompt.contains("Use 'bash'"), "{prompt}");
    }

    #[test]
    fn system_prompt_obriga_relatar_falhas_de_ferramenta() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, None, &ExposicaoDoBash::HostDoPod);
        assert!(
            prompt.contains("nunca reporte sucesso sem saida real"),
            "prompt deve obrigar a relatar falhas de ferramenta"
        );
        assert!(
            prompt.contains("bloqueada"),
            "prompt deve cobrir ferramenta bloqueada (ex.: file_write fora de allowed_dirs)"
        );
    }

    /// Doc e código têm de dizer a mesma coisa. Esta varredura é sobre a string
    /// PRODUZIDA em runtime, que é o texto que o modelo lê.
    #[test]
    fn system_prompt_do_working_dir_nao_promete_fail_open() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, Some("/x"), &ExposicaoDoBash::HostDoPod);
        assert!(
            !prompt.contains("validado apenas quanto a existencia"),
            "system prompt ainda promete fail-open: {prompt}"
        );
        assert!(
            prompt.contains("confinado as raizes"),
            "prompt deve dizer que o working_dir e confinado ao jail: {prompt}"
        );
        // #1272: o prompt não promete mais que o bash alcança o host "sem sandbox".
        assert!(
            !prompt.contains("sem sandbox"),
            "prompt ainda promete bash irrestrito: {prompt}"
        );
    }

    // ─── Envelopes (garra.agent.v1) ───────────────────────────────────

    #[test]
    fn agent_success_envelope_shape() {
        let calls = vec![ToolCallSummary {
            name: "bash".into(),
            duration_ms: 120,
            success: true,
            summary: "3 files".into(),
        }];
        let env = agent_success_envelope(
            "feito",
            "openrouter",
            "openrouter/free",
            1500,
            "mcp-abc",
            &calls,
        );
        assert_eq!(env["schema"], "garra.agent.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "feito");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["session_id"], "mcp-abc");
        assert_eq!(env["tool_calls"][0]["name"], "bash");
        assert_eq!(env["tool_calls"][0]["duration_ms"], 120);
        assert!(env.get("error").is_none());
    }

    #[test]
    fn agent_error_envelope_carries_tool_calls_and_kind() {
        let calls = vec![ToolCallSummary {
            name: "bash".into(),
            duration_ms: 4000,
            success: true,
            summary: "trabalhando".into(),
        }];
        let env = agent_error_envelope(
            "timeout",
            "exceeded 5s timeout",
            "openrouter",
            "openrouter/free",
            &calls,
        );
        assert_eq!(env["schema"], "garra.agent.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "timeout");
        assert_eq!(env["error"]["message"], "exceeded 5s timeout");
        assert_eq!(env["tool_calls"][0]["name"], "bash");
        assert!(env.get("answer").is_none());
        assert!(env.get("session_id").is_none());
    }

    #[test]
    fn agent_error_envelope_redacts_key_fingerprints() {
        let sanitized = sanitize_provider_error(concat!("boom ", "sk-", "abcdefgh12345678"));
        let env = agent_error_envelope(
            AskError::ProviderError(sanitized.clone()).kind_str(),
            &AskError::ProviderError(sanitized).message(),
            "openrouter",
            "openrouter/free",
            &[],
        );
        let msg = env["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("sk-[REDACTED]"), "{msg}");
        assert!(!msg.contains(concat!("sk-", "abcdefgh12345678")), "{msg}");
    }

    // ─── Reducer de eventos ───────────────────────────────────────────

    fn finished(name: &str, ms: u64, success: bool, summary: &str) -> TurnEvent {
        TurnEvent::ToolFinished {
            name: name.to_string(),
            duration: Duration::from_millis(ms),
            success,
            summary: summary.to_string(),
            output: String::new(),
        }
    }

    #[test]
    fn events_to_summaries_keeps_only_tool_finished_in_order() {
        let events = vec![
            TurnEvent::TextDelta("oi ".into()),
            TurnEvent::ToolStarted {
                name: "bash".into(),
                detail: "ls".into(),
            },
            finished("bash", 120, true, "3 files"),
            finished("file_read", 5, true, "conteudo"),
        ];
        let got = events_to_summaries(&events);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "bash");
        assert_eq!(got[0].duration_ms, 120);
        assert!(got[0].success);
        assert_eq!(got[0].summary, "3 files");
        assert_eq!(got[1].name, "file_read");
    }

    #[test]
    fn events_to_summaries_truncates_long_summary() {
        let events = vec![finished("bash", 1, true, &"ç".repeat(200))];
        let got = events_to_summaries(&events);
        assert!(got[0].summary.chars().count() <= TOOL_CALL_SUMMARY_MAX_CHARS);
        assert!(got[0].summary.ends_with('…'));
    }

    #[test]
    fn events_to_summaries_passthrough_failure_flag() {
        let events = vec![finished("bash", 30, false, "exit 1")];
        let got = events_to_summaries(&events);
        assert!(!got[0].success);
    }

    #[test]
    fn events_to_summaries_empty_on_no_events() {
        assert!(events_to_summaries(&[]).is_empty());
    }

    // ─── Roteamento de provider (endpoint de loopback) ────────────────

    mod provider_routing {
        use super::*;
        use garraia_ask::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        use garraia_config::LlmProviderConfig;

        fn opts(provider: &str) -> AgentOptions {
            AgentOptions {
                message: "oi".to_string(),
                provider: provider.to_string(),
                model: "m".to_string(),
                timeout_secs: 30,
                system_prompt: None,
                working_dir: None,
            }
        }

        #[tokio::test]
        async fn agent_with_provider_openai_reaches_llm_openai_base_url() {
            let mock = MockEndpoint::start().await;
            let outra = MockEndpoint::start().await;
            let dentro = tempfile::tempdir().expect("tempdir");
            let mut config = AppConfig::default();
            config.agent.file_roots = vec![dentro.path().to_string_lossy().into_owned()];
            config.llm.insert(
                "openai".to_string(),
                LlmProviderConfig {
                    provider: "openai".to_string(),
                    model: Some("m".to_string()),
                    api_key: Some("k-openai".to_string()),
                    base_url: Some(format!("{}/v1", mock.uri())),
                    extra: Default::default(),
                },
            );
            // Uma segunda entrada do mesmo tipo: a chave dela não pode aparecer.
            config.llm.insert(
                "lmstudio".to_string(),
                LlmProviderConfig {
                    provider: "openai".to_string(),
                    model: Some("m".to_string()),
                    api_key: Some("k-lmstudio".to_string()),
                    base_url: Some(format!("{}/v1", outra.uri())),
                    extra: Default::default(),
                },
            );
            let jail = jail_de(&config);
            match agent_oneshot(&config, &opts("openai"), &jail, "mcp").await {
                AgentOutcome::Success { answer, .. } => assert_eq!(answer, SENTINEL),
                AgentOutcome::Failure { error, .. } => panic!("garra_agent falhou: {error:?}"),
            }
            let creds = mock.credentials().await;
            assert!(
                !creds.is_empty() && creds.iter().all(|c| c == "k-openai"),
                "credenciais recebidas {creds:?}"
            );
            assert!(
                outra.paths().await.is_empty(),
                "a outra entrada nao pode receber nada"
            );
        }

        /// Achado do verificador (LOW): o agente pedia ao runtime o provider pelo
        /// nome DIGITADO. Só os braços openai/openrouter registram o alias com o
        /// nome dele; um alias de anthropic/ollama/llamacpp registra com o tipo, e
        /// o turno falhava com "provider '<nome>' not found". O envelope continua
        /// reportando o nome que o chamador passou.
        #[tokio::test]
        async fn agent_resolves_aliases_and_kinds_whose_registered_id_differs() {
            for (name, kind, key) in [
                ("claude", "anthropic", Some("k-claude")),
                ("ollama-local", "ollama", None),
                ("llamacpp", "llamacpp", None),
                ("llama-local", "llamacpp", None),
            ] {
                let mock = MockEndpoint::start().await;
                let dentro = tempfile::tempdir().expect("tempdir");
                let mut config = AppConfig::default();
                config.agent.file_roots = vec![dentro.path().to_string_lossy().into_owned()];
                config.llm.insert(
                    name.to_string(),
                    LlmProviderConfig {
                        provider: kind.to_string(),
                        model: Some("m".to_string()),
                        api_key: key.map(str::to_string),
                        base_url: Some(mock.uri()),
                        extra: Default::default(),
                    },
                );
                let jail = jail_de(&config);
                let mut o = opts(name);
                o.timeout_secs = 30;
                match agent_oneshot(&config, &o, &jail, "mcp").await {
                    AgentOutcome::Success {
                        answer, provider, ..
                    } => {
                        assert_eq!(answer, SENTINEL, "{name}");
                        assert_eq!(provider, name, "{name}: o envelope reporta o nome pedido");
                    }
                    AgentOutcome::Failure { error, .. } => {
                        panic!("garra_agent provider={name} falhou: {error:?}")
                    }
                }
                assert!(
                    !mock.paths().await.is_empty(),
                    "{name}: o endpoint nao recebeu nada"
                );
                if let Some(k) = key {
                    assert!(
                        mock.credentials().await.iter().all(|c| c == k),
                        "{name}: credenciais {:?}",
                        mock.credentials().await
                    );
                }
            }
        }
    }
}
