//! GarraIA interactive chat REPL.
//!
//! `garraia chat` or just `garra` opens a local-first AI assistant
//! that streams responses from Ollama (offline) or cloud providers (online).

use garraia_agents::exec_context::ExecContext;
use std::future::Future;
use std::io::{self, Write as _};
use std::sync::Arc;

use anyhow::{Context, Result};
use garraia_agents::{
    AgentRuntime, BashTool, ChatMessage, ChatRole, CodeReviewTool, DeviceExecuteTool,
    DeviceListTool, DeviceReadTool, DeviceToolsConfig, FileJail, FileReadTool, FileWriteTool,
    ListDirTool, LlmProvider, MessagePart, OllamaProvider, OpenAiProvider, RepoSearchTool,
    RunTestsTool, ValidacaoDeModelo, WebFetchTool, WebSearchTool, normalize_ollama_tag,
    tools::git_diff_tool::GitDiffTool,
};
use garraia_config::AppConfig;
use garraia_db::SessionStore;
use garraia_gateway::bootstrap::{
    avisa_cobertura_do_sandbox, sandbox_policy_from, spawn_hardware_adapters,
};
use garraia_hardware::DeviceRegistry;
use tokio::sync::mpsc;

use crate::defaults::{DEFAULT_CLOUD_PROVIDER, DEFAULT_LOCAL_PROVIDER};
use crate::provider_binding;
use crate::ui::error_card::ErrorCard;
use crate::ui::panel;
use crate::ui::tool_log::{Busca, ToolLog};
use crate::ui::{TerminalRenderer, UiEvent};
use garraia_agents::TurnEvent;

use std::path::Path;

// ── #1298: /model transacional ──────────────────────────────────────────────

/// O que o REPL faz com o resultado de `validar_modelo` antes de tocar o
/// estado do `/model`. A troca só acontece nos efeitos `Aplicar*` — `Ausente`
/// recusa com o estado anterior intacto, e a política inteira fica numa
/// função pura para o teste fixar sem precisar de provider de verdade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EfeitoTrocaDeModelo {
    /// Listado na curada: aplica como troca validada.
    Aplicar,
    /// Fora da curada mas no catálogo completo: aplica, e a confirmação diz
    /// de onde o nome veio — o `/models` não vai listá-lo, e isso não é bug.
    AplicarComNotaDeCatalogo,
    /// O catálogo real não tem o modelo: recusa e mantém provider/model.
    RecusarEManterEstado,
    /// Provider sem catálogo (ex.: Anthropic): aplica, mas nunca como
    /// sucesso validado — a confirmação carrega a ressalva.
    AplicarSemValidacao,
}

/// Política pura do `/model` (#1298): cada `ValidacaoDeModelo` tem um único
/// efeito. Falha de validação (o `Err` do provider) não passa por aqui — o
/// handler a trata como recusa fail-closed antes de chegar à política.
fn efeito_da_validacao(v: &ValidacaoDeModelo) -> EfeitoTrocaDeModelo {
    match v {
        ValidacaoDeModelo::Listado => EfeitoTrocaDeModelo::Aplicar,
        ValidacaoDeModelo::ListadoForaDaCurada => EfeitoTrocaDeModelo::AplicarComNotaDeCatalogo,
        ValidacaoDeModelo::Ausente => EfeitoTrocaDeModelo::RecusarEManterEstado,
        ValidacaoDeModelo::SemListagem => EfeitoTrocaDeModelo::AplicarSemValidacao,
    }
}

/// ANSI color helpers
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
/// Os comandos do `garra chat`, e a fonte do `/help`.
///
/// Uma tabela, e nao dez `println!`: o `/help` de antes era uma lista escrita
/// a mao ao lado do `match`, entao ele ja mentia — nao citava `/models` nem
/// `/tools`, e prometia um `/provider <nome>` que so responde "reinicie".
/// Ha um teste que confere a tabela contra o `match` do REPL, para a proxima
/// divergencia aparecer no CI e nao no terminal de quem usa.
const COMANDOS: &[(&str, &str)] = &[
    ("/status", "Provider, modelo, ferramentas e o ultimo turno"),
    ("/context", "Diretorio, ramo e projeto detectados"),
    ("/tools", "Ferramentas que o agente tem"),
    ("/tool", "Saidas de ferramenta guardadas nesta sessao"),
    ("/tool <n>", "A saida inteira de uma chamada"),
    ("/history", "Historico da conversa"),
    (
        "/resume [id]",
        "Retomar a sessao mais recente (ou um id) — o alvo e sempre a ultima atividade",
    ),
    ("/logs", "Onde fica o log, e como segui-lo"),
    ("/models", "Modelos que este provider lista"),
    ("/model <nome>", "Trocar de modelo sem reiniciar"),
    (
        "/provider <nome>",
        "Como trocar de provider (pede reinicio)",
    ),
    ("/clear", "Limpar o historico"),
    ("/help", "Mostrar isto"),
    ("/exit", "Sair"),
];

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// So os marcadores de projeto, sem a listagem de arquivos (#940).
///
/// O `/context` mostra isto e o prompt do sistema mostra
/// [`scan_directory_context`], que acrescenta os arquivos do topo. Sao
/// publicos diferentes: o modelo se beneficia de saber que existe um
/// `docker-compose.yml`; a pessoa que digitou `/context` queria uma linha, e
/// recebia quinze nomes de arquivo embrulhados no painel.
///
/// Nao ha varredura nova aqui — sao `exists()` de caminho conhecido. A propria
/// issue pede para nao varrer o diretorio so para desenhar status.
fn project_summary(cwd: &str) -> String {
    let p = Path::new(cwd);
    // Mesma tabela que o cabecalho usa (#935) — duas copias divergiriam.
    let mut markers = crate::ui::project_markers(p);
    if p.join(".git").exists() {
        markers.push("Git repo");
    }
    markers.join(", ")
}

/// Scan the current directory for project markers and build a context summary.
///
/// Correção do **contexto de projeto** (feedback "contexto de projeto"):
/// o resumo antigo listava qualquer entrada do topo — incluindo diretórios
/// de build/dependência (`target/`, `node_modules/`, `dist/`) — e não dizia
/// **que projeto é este** nem **em que ramo** o agente está. Agora:
///
/// - diretórios de build/dependência são filtrados da listagem;
/// - o nome do projeto vem do primeiro heading do `README.md` (quando há);
/// - o ramo git atual entra no contexto (mesma leitura de `HEAD` usada no
///   painel `/contexto` — sem spawnar `git`, sem varrer a árvore).
fn scan_directory_context(cwd: &str) -> String {
    let p = Path::new(cwd);
    let markers = project_summary(cwd);

    // List top-level files (up to 15) for context, skipping noise dirs.
    const NOISE_DIRS: &[&str] = &[
        "target",
        "node_modules",
        "dist",
        "build",
        "out",
        ".venv",
        "venv",
        "__pycache__",
        ".next",
        ".turbo",
        "vendor",
        "coverage",
    ];
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(p) {
        for entry in entries.flatten().take(30) {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_noise_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && NOISE_DIRS.contains(&name.as_str());
            if !name.starts_with('.') && !is_noise_dir {
                files.push(name);
            }
            if files.len() >= 15 {
                break;
            }
        }
    }

    if markers.is_empty() && files.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();
    if !markers.is_empty() {
        parts.push(markers);
    }
    // Project identity: first heading of the README, when present.
    if let Ok(readme) = std::fs::read_to_string(p.join("README.md")) {
        let name = readme.lines().map(str::trim).find_map(|l| {
            l.strip_prefix("# ")
                .map(str::trim_start)
                .map(str::to_string)
        });
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            parts.push(format!("Projeto: {name}"));
        }
    }
    // Current git branch (same HEAD read as the /contexto panel).
    if let Some(ramo) = crate::ui::git_branch(p) {
        parts.push(format!("Ramo: {ramo}"));
    }
    if !files.is_empty() {
        parts.push(format!("Arquivos: {}", files.join(", ")));
    }
    parts.join(" | ")
}

/// Helper to resolve a NON-LLM API key (today only Brave's, for `web_search`)
/// checking env var, explicit config, and "main" config.
///
/// LLM providers do not come through here any more: their endpoint and
/// credential are bound together by [`provider_binding`], because this
/// helper returns a key with no idea of which host it belongs to — which is
/// how `-p openai` ended up sending a custom endpoint's key to api.openai.com.
fn get_api_key(config: &AppConfig, provider_name: &str, env_var: &str) -> Option<String> {
    if !env_var.is_empty()
        && let Ok(key) = std::env::var(env_var)
        && !key.is_empty()
    {
        return Some(key);
    }
    if let Some(cfg) = config.llm.get(provider_name)
        && let Some(ref k) = cfg.api_key
        && !k.is_empty()
    {
        return Some(k.clone());
    }
    if let Some(cfg) = config.llm.get("main")
        && cfg.provider == provider_name
        && let Some(ref k) = cfg.api_key
        && !k.is_empty()
    {
        return Some(k.clone());
    }
    None
}

/// Registers the CLI agent's tools: the gateway's set (#1036) plus
/// `git_diff`, which only the CLI has.
///
/// `review` is the provider + model `code_review` runs its second LLM call
/// on; `None` skips it, which is what the tests do because building a real
/// provider needs a backend. `brave_key` gates `web_search` exactly like the
/// gateway does. `config` feeds `spawn_hardware_adapters`: com
/// `hardware.mqtt` ou `hardware.home_assistant`
/// configurado o CLI abre o mesmo transporte e o mesmo store de presenca do
/// gateway (`AppConfig::hardware_db_path`). `schedule_heartbeat` /
/// `schedule_recurring` need a
/// `SessionStore` the chat does not open — they stay gateway-only, on
/// purpose and said here rather than silently.
///
/// #1088 mudou **metade** dessa frase: com `--persist`/`--resume` o chat
/// passa a abrir um `SessionStore`, entao "o chat nao abre store" deixou de
/// ser verdade no modo persistente. O registro das duas tools continua
/// gateway-only mesmo assim — a loja existe so quando a pessoa pediu, e
/// registrar a tool condicionalmente daria ao agente um conjunto de
/// ferramentas que depende de uma flag. Fica para um slice proprio.
fn register_cli_tools(
    runtime: &AgentRuntime,
    config: &AppConfig,
    review: Option<(Arc<dyn LlmProvider>, String)>,
    brave_key: Option<String>,
    bash_allowlist: Vec<String>,
) {
    // #1244: na CLI quem roda o binario e o dono da maquina, e o diretorio
    // corrente e a escolha explicita dele — dai o CWD ser raiz aqui e nao no
    // gateway, que atende pedido de terceiro. `agent.file_roots` soma.
    let file_jail = FileJail::from_config_roots_plus_cwd(&config.agent.file_roots);
    runtime.register_tool(Box::new(FileReadTool::new(file_jail.clone())));
    runtime.register_tool(Box::new(FileWriteTool::new(file_jail.clone())));
    // #1225: mesma policy de sandbox do gateway, pela MESMA funcao
    // (`sandbox_policy_from`) — o `garra chat` nao pode divergir do servidor
    // sobre onde um comando roda. Sem `agent.sandbox` no config a policy e
    // `Off` e nada muda.
    //
    // As duas correcoes sao ortogonais e ficam as duas: o jail decide ONDE o
    // arquivo pode estar, a policy decide ONDE o comando roda. O `bash`
    // continua fora do jail de proposito — ver #1272.
    let mut bash_tool = BashTool::new_with_confirmation(Some(30)).with_allowlist(bash_allowlist);
    // #1225 S2: a mesma policy vale para as tools que spawnam programa.
    let politica = sandbox_policy_from(&config.agent.sandbox);
    bash_tool.set_sandbox_policy(politica.clone());
    // #1225 S2: uma vez por processo — `register_cli_tools` roda uma vez na
    // subida do `garra chat`. Fora de `sandbox_policy_from` porque no MCP a
    // policy e reconstruida por chamada.
    avisa_cobertura_do_sandbox(&config.agent.sandbox);
    runtime.register_tool(Box::new(bash_tool));
    runtime.register_tool(Box::new(
        GitDiffTool::new(None, None).com_sandbox(politica.clone()),
    ));
    runtime.register_tool(Box::new(ListDirTool::new(file_jail.clone(), None)));
    runtime.register_tool(Box::new(
        RepoSearchTool::new(None, None).com_sandbox(politica.clone()),
    ));
    // Runs whatever the project's test script says; confirmed like `bash`.
    // O `working_dir` do modelo fica no mesmo jail das file tools.
    runtime.register_tool(Box::new(
        RunTestsTool::new_with_confirmation(None)
            .com_sandbox(politica.clone())
            .com_jail(file_jail),
    ));
    runtime.register_tool(Box::new(WebFetchTool::new(None)));
    // ADR 0020 / epic #1124: tools de hardware. O CLI é interativo — sempre
    // com canal de confirmação (R3/R4 pedem "sim" na própria conversa). O
    // registry nasce vazio; com `hardware.mqtt` configurado, o adapter sobe
    // pela MESMA função do gateway (fonte única do wiring — resolução de
    // senha, client id, store de presença e catálogo de skills (#1250)
    // idênticos, inclusive os warns).
    // Sem adapter, `device_list` lista nada (#1128 liga o boot via config).
    let device_registry = Arc::new(DeviceRegistry::new());
    let boot = spawn_hardware_adapters(config, device_registry.clone());
    let device_config = Arc::new({
        let mut cfg = DeviceToolsConfig::new(device_registry);
        if let Some(state) = boot.state {
            cfg = cfg.com_estado(state);
        }
        if let Some(fonte) = boot.sinonimos {
            cfg = cfg.com_sinonimos(fonte);
        }
        cfg
    });
    runtime.register_tool(Box::new(DeviceListTool::new(device_config.clone())));
    runtime.register_tool(Box::new(DeviceReadTool::new(device_config.clone())));
    runtime.register_tool(Box::new(DeviceExecuteTool::new(device_config)));
    if let Some((provider, model)) = review {
        runtime.register_tool(Box::new(
            CodeReviewTool::new(provider, model, None).com_sandbox(politica),
        ));
    }
    if let Some(key) = brave_key {
        runtime.register_tool(Box::new(WebSearchTool::new(key)));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistencia do `garra chat` (#1088)
// ─────────────────────────────────────────────────────────────────────────────

/// Quantas mensagens o `--resume` traz de volta.
///
/// E o mesmo numero da hidratacao do gateway (`state.rs`), e pela mesma
/// razao: acima disso o historico inteiro da sessao iria para o prompt de
/// cada turno, e um prompt gigante custa mais caro do que as mensagens
/// antigas valem.
const RESUME_LIMIT: usize = 100;

/// Nome do arquivo do `SessionStore` dentro do diretorio de dados.
///
/// O mesmo `sessions.db` que o gateway abre (`server.rs`): quem retoma no
/// chat uma conversa que comecou num canal encontra as mensagens no lugar
/// onde elas ja estavam.
///
/// `pub(crate)` porque o `runs_cmd` (#1227 slice 4) le o ledger do **mesmo**
/// arquivo: duplicar o nome la abriria espaco para os dois divergirem.
pub(crate) const SESSIONS_DB: &str = "sessions.db";

/// Abre o `SessionStore` **apenas** quando a sessao vai usa-lo.
///
/// `None` — o padrao, sem `--persist` nem `--resume` — significa nenhum
/// arquivo criado e nada gravado em disco, que e exatamente o que o chat
/// sempre fez. A decisao de nao gravar era implicita num comentario; agora
/// ela e uma flag, e este `None` e a prova de que o caminho antigo continua
/// intacto (ha teste).
fn open_chat_store(
    config: &AppConfig,
    persist: bool,
    resume: Option<&str>,
) -> Result<Option<SessionStore>> {
    if !persist && resume.is_none() {
        return Ok(None);
    }
    let data_dir = config.resolved_data_dir();
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("nao foi possivel criar {}", data_dir.display()))?;
    let path = data_dir.join(SESSIONS_DB);
    let store = SessionStore::open(&path)
        .with_context(|| format!("nao foi possivel abrir {}", path.display()))?;
    // #1227 (slice 1): abrir o banco tambem e uma subida — runs `running`
    // de uma queda anterior viram `interrupted` com ids no log (nunca
    // `goal`), igual ao gateway.
    garraia_db::agent_runs::log_interrupted_runs(&store);
    Ok(Some(store))
}

/// Quem aprova, no CLI, um pedido de confirmacao pausado (#1343).
///
/// O processo e a fronteira de usuario: quem digita no terminal desta
/// sessao e o mesmo humano que leu o pedido. O remetente e uma constante, e
/// o escopo fica preso a `session_id` — `/resume` para outra sessao troca o
/// escopo, e um restart zera o registro (ele vive em memoria no runtime).
pub(crate) const REMETENTE_CLI: &str = "local-tty";

/// O canal do CLI no registro de aprovacoes pendentes.
pub(crate) const CANAL_CLI: &str = "cli";

/// O `ExecContext` de um turno do `garraia chat`.
///
/// #980: o diretorio do projeto resolve caminho relativo das ferramentas de
/// arquivo (**nao e sandbox** — ver `ExecContext::working_dir`). #1343: o
/// escopo de aprovacao faz o "sim" do turno seguinte rodar o pedido pausado,
/// uma vez.
fn exec_do_turno(cwd: &str, session_id: &str) -> ExecContext {
    let mut exec = ExecContext::with_working_dir(Some(cwd.to_string()));
    exec.approval_scope = garraia_agents::ApprovalScope::new(CANAL_CLI, session_id, REMETENTE_CLI);
    exec
}

/// Grava um turno (pergunta + resposta) no store.
/// `direction` segue o vocabulario que o gateway ja usa em `persist_turn` —
/// `"user"` e `"assistant"` —, porque e o que `load_history` e a hidratacao
/// do gateway leem de volta. Nada de schema novo: sao as duas mesmas
/// chamadas, so que feitas pelo CLI.
///
/// #1300: a gravacao e fracionada, nao atomica de proposito. A pergunta
/// grava ANTES do turno rodar (`persist_user_turn`) e a resposta/marcador
/// grava no fim (`persist_assistant_turn`). Um crash duro no meio deixa a
/// pergunta sola no banco — e exatamente isso que o `--resume latest`
/// seguinte recupera. O `append_turn` de antes gravava o par so DEPOIS do
/// turno completo, entao timeout, Ctrl+C, provider caindo ou kill -9
/// apagavam o turno do ponto de vista do resume: a sessao restaurada
/// terminava na pergunta anterior, e o trabalho recente sumia.
fn persist_user_turn(store: &SessionStore, session_id: &str, user_text: &str) -> Result<()> {
    store.upsert_session(
        session_id,
        "cli",
        "local",
        &serde_json::json!({ "origem": "garra chat" }),
    )?;
    let meta = serde_json::json!({ "channel_id": "cli", "user_id": "local" });
    store.append_message(session_id, "user", user_text, chrono::Utc::now(), &meta)?;
    Ok(())
}

/// Grava o lado do assistant: resposta completa ou o marcador de
/// interrupcao de `marcador_de_interrupcao`. A sessao ja existe — o
/// `persist_user_turn` do mesmo turno a criou.
fn persist_assistant_turn(
    store: &SessionStore,
    session_id: &str,
    assistant_text: &str,
) -> Result<()> {
    let meta = serde_json::json!({ "channel_id": "cli", "user_id": "local" });
    store.append_message(
        session_id,
        "assistant",
        assistant_text,
        chrono::Utc::now(),
        &meta,
    )?;
    Ok(())
}

/// O que o fim de um turno diz de si, para o historico hidratado contar a
/// verdade. Timeout, Ctrl+C e erro gravam um marcador como mensagem do
/// assistant; so o turno completo fica sem nada. O `--resume` seguinte
/// hidrata o marcador como texto normal — o modelo retomado ve que a ultima
/// resposta NAO aconteceu, em vez de ler uma conversa que termina em
/// resposta inventada pela ausencia.
fn marcador_de_interrupcao<T, E>(outcome: &TurnOutcome<T, E>) -> Option<&'static str> {
    match outcome {
        TurnOutcome::TimedOut => Some("[turno interrompido: timeout]"),
        TurnOutcome::Cancelled => Some("[turno interrompido: cancelado]"),
        TurnOutcome::Done(Err(_)) => Some("[turno interrompido: erro]"),
        TurnOutcome::Done(Ok(_)) => None,
    }
}

/// Decide o id inicial da sessao. Id explicito do `--resume` vence;
/// `latest` (o valor quando a flag vem sem argumento) cai para o alvo do
/// banco (`latest_session_id`); sem alvo, o id novo entra com a marca de
/// "comecou nova" para o chamador avisar — primeiro uso com `--resume` e
/// boot legitimo, nao erro.
fn id_inicial_da_sessao(
    resume: Option<&str>,
    latest: Option<String>,
    novo: String,
) -> (String, bool) {
    match resume {
        Some("latest") => match latest {
            Some(id) => (id, false),
            None => (novo, true),
        },
        Some(id) => (id.to_string(), false),
        None => (novo, true),
    }
}

/// Le o historico gravado de uma sessao, em ordem cronologica.
///
/// Direcoes que nao sao `"user"` nem `"assistant"` (resumo de sistema, lixo
/// de uma versao antiga) sao descartadas em vez de virarem mensagem com
/// papel inventado.
fn load_history(store: &SessionStore, session_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
    let gravadas = store.load_recent_messages(session_id, limit)?;
    Ok(gravadas
        .into_iter()
        .filter_map(|m| {
            let role = match m.direction.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                _ => return None,
            };
            Some(ChatMessage {
                role,
                content: MessagePart::Text(m.content),
            })
        })
        .collect())
}

/// One line of the system prompt per registered tool.
///
/// Built from `tool_names()` so the prompt can never list a tool the
/// runtime lacks — the drift #1036 found was a hand-written list that
/// mentioned four tools registered nowhere and never mentioned the ones
/// that were.
fn tool_docs(names: &[String]) -> String {
    names
        .iter()
        .filter_map(|n| tool_help(n).map(|d| format!("- **{n}**: {d}\n")))
        .collect()
}

fn tool_help(name: &str) -> Option<&'static str> {
    Some(match name {
        "file_read" => "Le o conteudo de um arquivo. Use para ver codigo, configs, READMEs.",
        "file_write" => "Escreve/cria arquivos. Use para editar codigo ou criar novos arquivos.",
        "bash" => "Executa comandos no terminal (ls, dir, cargo, git, etc.).",
        "git_diff" => "Executa comandos git seguros (diff, status, log, branch).",
        "list_dir" => {
            "Lista um diretorio em arvore, com tamanhos; pula .git, target, node_modules."
        }
        "repo_search" => "Busca texto ou regex nos arquivos do projeto (rg, ou grep).",
        "run_tests" => {
            "Roda a suite de testes do projeto (cargo, flutter, npm, pytest) e resume o resultado; pede confirmacao."
        }
        "web_fetch" => "Baixa o conteudo de uma URL publica (enderecos internos sao recusados).",
        "web_search" => "Busca na web (Brave) e devolve titulos, links e trechos.",
        "code_review" => "Revisa um diff ou arquivo e aponta problemas e melhorias.",
        // ADR 0020 / epic #1124: os tres entram no prompt com as tools —
        // sem linha aqui, o prompt lista quem nao descreve e o teste que
        // confere a tabela contra o registro falha.
        "device_list" => {
            "Lista os dispositivos fisicos registrados e o estado online/offline de cada um."
        }
        "device_read" => {
            "Le uma capability de um dispositivo fisico (ex.: a temperatura do sensor da sala)."
        }
        "device_execute" => {
            "Executa uma capability em um dispositivo fisico (ex.: ligar a luz); o risco (R0-R5) decide se pede confirmacao."
        }
        _ => return None,
    })
}

/// GAR-576 — Resolve the model name for a given provider kind.
///
/// Lookup order:
///   1. `model_override` (the CLI `--model` flag, absolute precedence).
///   2. `config.llm[provider_kind].model` (key-match).
///   3. The first `config.llm[*]` entry whose `provider` field equals
///      `provider_kind` and whose `model` is `Some(non-empty)`.
///
/// Returns `None` only when no source supplies a usable model name; the
/// caller is then responsible for picking a hardcoded fallback.
fn resolve_provider_model(
    config: &AppConfig,
    provider_kind: &str,
    model_override: Option<&str>,
) -> Option<String> {
    if let Some(m) = model_override
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    if let Some(cfg) = config.llm.get(provider_kind)
        && let Some(m) = cfg.model.as_deref()
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    for cfg in config.llm.values() {
        if cfg.provider == provider_kind
            && let Some(m) = cfg.model.as_deref()
            && !m.is_empty()
        {
            return Some(m.to_string());
        }
    }
    None
}

/// GAR-576 — Decision returned by [`decide_default_provider`].
///
/// `UseDefault` says "the operator configured `agent.default_provider`,
/// the matching `llm[<key>]` block is present, and a credential is
/// reachable — go build the provider". `FallThroughToChain` says
/// "either no default is configured, the lookup failed, or there is no
/// usable credential — fall back to the legacy autodetect heuristic".
#[derive(Debug, Clone, PartialEq, Eq)]
enum DefaultProviderDecision {
    UseDefault {
        config_key: String,
        provider_kind: String,
        model: String,
    },
    FallThroughToChain {
        reason: &'static str,
    },
}

/// GAR-576 — Decide whether to honor `config.agent.default_provider`
/// before the legacy autodetect chain.
///
/// Pure function: takes presence-bool flags for the relevant env vars
/// instead of reading `std::env` directly, so unit tests can assert
/// regression scenarios (e.g. `OPENAI_API_KEY` in `.env` no longer
/// hijacks the provider when `agent.default_provider = "openrouter"`)
/// without mutating process-global env state.
fn decide_default_provider(
    config: &AppConfig,
    env_has_openai_key: bool,
    env_has_openrouter_key: bool,
    env_has_anthropic_key: bool,
) -> DefaultProviderDecision {
    let Some(default_key) = config.agent.default_provider.as_deref() else {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "no agent.default_provider configured",
        };
    };
    let Some(cfg) = config.llm.get(default_key) else {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "agent.default_provider key not present in llm map",
        };
    };
    let provider_kind = cfg.provider.trim();

    // Same emptiness rule as `provider_binding::bind_entry`, so the decision
    // and the construction cannot disagree about whether a key exists.
    let cfg_has_key = cfg.api_key.as_deref().is_some_and(|k| !k.trim().is_empty());
    // The kind's env var only counts when the entry talks to the kind's
    // default host — `bind_entry` never sends it to the entry's own
    // `base_url`, and the decision must not count a key the build will not
    // use.
    let env_reaches_entry =
        provider_binding::env_credential_allowed(provider_kind, cfg.base_url.as_deref());
    let credential_ok = match provider_kind {
        // Local — health-checked by the caller. `llamacpp` talks to a local
        // llama-server (default http://localhost:8080), keyless like ollama.
        "ollama" | "llamacpp" => true,
        "anthropic" => cfg_has_key || (env_has_anthropic_key && env_reaches_entry),
        // OpenAI-compatible local backends (e.g. LM Studio) commonly omit
        // the api_key and rely on `base_url` reachability. Treat an entry
        // pointing at its own endpoint as credential-ok for routing (it gets
        // the keyless placeholder); the OpenAI API itself needs a key.
        "openai" => cfg_has_key || !env_reaches_entry || env_has_openai_key,
        "openrouter" => cfg_has_key || (env_has_openrouter_key && env_reaches_entry),
        _ => {
            return DefaultProviderDecision::FallThroughToChain {
                reason: "unknown provider kind in agent.default_provider",
            };
        }
    };

    if !credential_ok {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "no credential available for agent.default_provider",
        };
    }

    // The default entry's OWN model first: `resolve_provider_model` looks up
    // `llm.<kind>` before anything else, so with `default_provider: lmstudio`
    // (kind `openai`) next to an `llm.openai` it used to send llm.openai's
    // model name to the LM Studio endpoint.
    let model = cfg
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .or_else(|| resolve_provider_model(config, provider_kind, None))
        .unwrap_or_else(|| hardcoded_default_model(provider_kind));

    DefaultProviderDecision::UseDefault {
        config_key: default_key.to_string(),
        provider_kind: provider_kind.to_string(),
        model,
    }
}

/// #1180 — one stop of the legacy autodetect chain, the heuristic that runs
/// when `agent.default_provider` is absent or unusable (a fresh clone, an
/// `install.sh --skip-setup`, a hand-written config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutodetectCandidate {
    Anthropic,
    OpenAi,
    OpenRouter,
    Ollama,
}

/// #1180 decision 2 — "the cloud is always the first option, unless the user
/// asks otherwise". This function is where that sentence is true in code.
///
/// The chain used to try Ollama **first**, unconditionally: a box with
/// `OPENROUTER_API_KEY` exported and a stray `ollama serve` running would
/// silently answer from the local model, contradicting both the issue and
/// `README.md`. Now the providers whose credentials actually exist come
/// first, in the order they already had among themselves
/// (Anthropic → OpenAI → OpenRouter), and Ollama is what is left when none
/// of them does — which is exactly the local fallback #1180 wants to keep.
///
/// Ollama is always in the returned list, always last: it is the only
/// provider that needs no credential, so it is the only one that can be the
/// unconditional end of the chain. Its health check still has to pass; the
/// caller falls through to an offline Ollama handle if it does not.
///
/// Pure on purpose — `detect_provider` itself needs a daemon and a network,
/// so the ordering is tested here instead of there.
fn autodetect_order(
    has_anthropic_credential: bool,
    has_openai_credential: bool,
    has_openrouter_credential: bool,
) -> Vec<AutodetectCandidate> {
    let mut order = Vec::with_capacity(4);
    if has_anthropic_credential {
        order.push(AutodetectCandidate::Anthropic);
    }
    if has_openai_credential {
        order.push(AutodetectCandidate::OpenAi);
    }
    if has_openrouter_credential {
        order.push(AutodetectCandidate::OpenRouter);
    }
    order.push(AutodetectCandidate::Ollama);
    order
}

/// GAR-576 — Last-resort fallback model name per provider kind, used
/// only when neither the CLI flag nor `config.llm` supplies one.
///
/// Single source of truth: `select_explicit_provider` and `detect_provider`
/// both route through here rather than repeating the literals inline, so the
/// two paths cannot disagree about what "the default" means.
pub(crate) fn hardcoded_default_model(provider_kind: &str) -> String {
    // The two project defaults (issue #1180) are keyed by the shared
    // constants rather than by a literal, so a rename in `crate::defaults`
    // cannot leave this table pointing at a provider kind that no longer
    // exists.
    match provider_kind {
        // The local *second* option — see `crate::defaults`.
        crate::defaults::DEFAULT_LOCAL_PROVIDER => crate::defaults::DEFAULT_LOCAL_MODEL,
        // llama-server serves whatever model it was started with; the
        // OpenAI-compatible API accepts any string here — `default` matches
        // `garraia_agents::llama_cpp::DEFAULT_MODEL` byte-for-byte.
        "llamacpp" => "default",
        "anthropic" => "claude-sonnet-4-5-20250929",
        "openai" => "gpt-4o",
        // The official project default. Never `openrouter/auto`: `auto`
        // stays reachable only when the user passes it explicitly.
        crate::defaults::DEFAULT_CLOUD_PROVIDER => crate::defaults::DEFAULT_CLOUD_MODEL,
        "echo" => "echo-stub",
        _ => "auto",
    }
    .to_string()
}

/// GAR-576 — Construct an [`LlmProvider`] from a config-resolved default.
///
/// Returns `None` when construction is infeasible (e.g. Ollama daemon
/// unreachable, or required api_key absent at build time); the caller
/// then falls through to the legacy autodetect chain.
///
/// Endpoint AND credential come from the `llm.<config_key>` entry itself
/// ([`provider_binding::bind_entry`]). This used to read the key from
/// `llm.<kind>` instead, so `default_provider: lmstudio` (kind `openai`)
/// next to an `llm.openai` sent llm.openai's key to the LM Studio endpoint,
/// and an `anthropic` default dropped its `base_url` entirely.
async fn try_build_default_provider(
    config_key: &str,
    cfg: &garraia_config::LlmProviderConfig,
    model: &str,
    env: provider_binding::Env<'_>,
) -> Option<Arc<dyn LlmProvider>> {
    // GAR-576: return ONLY the trait object — the display strings
    // (config_key, model) are formed at the call site from inputs that
    // never pass through this function. That keeps CodeQL's cleartext-
    // logging dataflow analysis from conservatively tainting the model
    // name through this scope, which also resolves the api_key.
    let binding = provider_binding::bind_entry(config_key, cfg, env);
    let provider = provider_binding::build_provider(&binding, model, None, None).ok()?;
    // Local daemons are health-checked: a dead one must not win over the
    // autodetect chain.
    if matches!(binding.kind(), "ollama" | "llamacpp")
        && !provider.health_check().await.unwrap_or(false)
    {
        return None;
    }
    Some(provider)
}

/// Model for an explicitly named provider: `--model` > the bound entry's own
/// model > `resolve_provider_model` (legacy scan by kind) > the per-kind
/// hardcoded default.
fn explicit_model(
    config: &AppConfig,
    binding: &provider_binding::ProviderBinding,
    model_override: Option<&str>,
) -> String {
    model_override
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .or_else(|| binding.model().map(str::to_string))
        .or_else(|| resolve_provider_model(config, binding.kind(), None))
        .unwrap_or_else(|| hardcoded_default_model(binding.kind()))
}

/// GAR-579 — Build a provider from an explicit `--provider <name>` flag.
///
/// Returns the same `(display_name, model, Arc<dyn LlmProvider>)` triple
/// that `detect_provider` returns. Honors `model_override` first, then the
/// bound entry's model, then `resolve_provider_model`, then a hardcoded
/// per-kind fallback. An unknown name is an error; a missing api_key for a
/// cloud provider is an error.
///
/// `name` is a provider kind (`openai`, `anthropic`, …) or an alias defined
/// under `llm:` (`lmstudio` with `provider: openai`) — the MCP policy already
/// accepted aliases, this is where they now resolve. Endpoint and
/// credential come from the same entry ([`provider_binding::bind_named`]):
/// `-p openai` used to read `llm.openai.api_key` and drop
/// `llm.openai.base_url`, sending the key of a custom OpenAI-compatible
/// endpoint to https://api.openai.com (v0.4.4 clean-install smoke).
///
/// `url_override` is the CLI `--url` flag. Only the keyless `llamacpp`
/// consumes it; on a keyed provider it would ship the entry's key to an
/// ad-hoc address.
///
/// Shared by `chat::run_chat`, `ask::run_ask` and the MCP tools so the
/// explicit-provider path lives in exactly one place.
pub(crate) fn select_explicit_provider(
    config: &AppConfig,
    name: &str,
    model_override: Option<&str>,
    url_override: Option<&str>,
) -> Result<(String, String, Arc<dyn LlmProvider>)> {
    select_explicit_provider_with_env(
        config,
        name,
        model_override,
        url_override,
        &provider_binding::process_env,
    )
}

/// [`select_explicit_provider`] with the environment injected, so tests pin
/// the env-var fallback without touching the process environment.
fn select_explicit_provider_with_env(
    config: &AppConfig,
    name: &str,
    model_override: Option<&str>,
    url_override: Option<&str>,
    env: provider_binding::Env<'_>,
) -> Result<(String, String, Arc<dyn LlmProvider>)> {
    let Some(binding) = provider_binding::bind_named(config, name, env) else {
        anyhow::bail!(
            "Provider desconhecido: {name}. Use: ollama, llamacpp, anthropic, openai, openrouter \
             (ou o nome de uma entrada em llm: no config.yml)"
        );
    };
    let model = explicit_model(config, &binding, model_override);
    // An OpenAI-compatible alias registers under its own name (GAR-582), so
    // a lookup by that name resolves. The other kinds cannot be renamed and
    // register as their kind; callers that look the provider up by name
    // (the MCP agent) therefore ask for `provider.provider_id()`, never for
    // the name that was typed.
    let provider_id = (!provider_binding::is_buildable_kind(name)).then_some(name);
    let provider = provider_binding::build_provider(&binding, &model, provider_id, url_override)?;
    Ok((name.to_string(), model, provider))
}

/// Base URL of the local Ollama daemon. Extracted so the autodetect chain
/// and [`try_local_ollama_model`] cannot drift apart.
fn ollama_base_url() -> String {
    std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string())
}

/// How long the "is this tag installed?" probe may take. Local HTTP against
/// `/api/tags`; 2s is generous and keeps `garra --model …` snappy when the
/// daemon is down.
const OLLAMA_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// True when `model` is the model of a configured *non-Ollama* provider.
///
/// Without this, `--model gpt-4o` on a box that also runs Ollama would be
/// probed against `/api/tags`, come back missing, and offer to *pull gpt-4o
/// from Ollama* — a download that cannot succeed and a question the user
/// should never be asked. When config already says `gpt-4o` belongs to
/// `openai`, the Ollama path is skipped entirely and the regular chain
/// routes it.
fn model_belongs_to_configured_cloud_provider(config: &AppConfig, model: &str) -> bool {
    config
        .llm
        .values()
        .any(|cfg| cfg.provider != "ollama" && cfg.model.as_deref() == Some(model))
}

/// Outcome of probing the local Ollama daemon for an explicit `--model` tag.
enum LocalOllamaProbe {
    /// The tag is installed; the provider is ready to use.
    Installed(Box<(String, String, Arc<dyn LlmProvider>)>),
    /// The daemon answered but does not have this tag. Carries the
    /// normalized tag so the caller can offer to pull it.
    Missing { tag: String },
    /// Not an Ollama-shaped reference, or the daemon is unreachable.
    NotApplicable,
}

/// Probe the local Ollama daemon for an explicit `--model <tag>`.
///
/// This is what makes `garraia --model qwen3.8` open the local model
/// directly. `qwen3.8` is normalized to `qwen3.8:latest` first, because
/// `/api/tags` only ever reports explicit tags.
///
/// Deliberately conservative: it can only ever win on an exact hit in
/// `/api/tags`, so `--model gpt-4o` is never hijacked to a local provider.
async fn try_local_ollama_model(config: &AppConfig, model_override: &str) -> LocalOllamaProbe {
    // `openrouter/auto` and friends belong to another provider — bail before
    // spending a round-trip.
    let Some(tag) = normalize_ollama_tag(model_override) else {
        return LocalOllamaProbe::NotApplicable;
    };
    // Config already claims this name for a cloud provider — not a tag to pull.
    if model_belongs_to_configured_cloud_provider(config, model_override) {
        return LocalOllamaProbe::NotApplicable;
    }

    let base = ollama_base_url();
    let Ok(probe_client) = reqwest::Client::builder()
        .timeout(OLLAMA_PROBE_TIMEOUT)
        .build()
    else {
        return LocalOllamaProbe::NotApplicable;
    };
    let probe = OllamaProvider::new(None, Some(base.clone())).with_client(probe_client);

    match probe.resolve_installed_model(&tag).await {
        Ok(Some(found)) => {
            // Rebuild with the default (untimed) client — the 2s probe budget
            // must not cap inference.
            let provider = OllamaProvider::new(Some(found.clone()), Some(base));
            LocalOllamaProbe::Installed(Box::new((
                "ollama".to_string(),
                found,
                Arc::new(provider) as Arc<dyn LlmProvider>,
            )))
        }
        Ok(None) => LocalOllamaProbe::Missing { tag },
        // Daemon down: stay quiet and let the regular chain report whatever
        // it finds. `--model` is still honored by every branch below.
        Err(_) => LocalOllamaProbe::NotApplicable,
    }
}

/// Offer to pull a missing Ollama tag, then return the ready provider.
///
/// * `assume_yes` (the `-y` flag) pulls without asking.
/// * An interactive terminal gets a confirmation prompt.
/// * Anything else — a pipe, CI, `ask --json` — never prompts and returns
///   `None` so the caller can fall through with a visible hint.
///
/// Progress goes to stderr so `ask --json` keeps a single clean JSON line on
/// stdout.
async fn offer_pull_ollama_model(
    tag: &str,
    assume_yes: bool,
) -> Option<(String, String, Arc<dyn LlmProvider>)> {
    use std::io::IsTerminal as _;

    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    if !assume_yes {
        if !interactive {
            eprintln!(
                "{YELLOW}Modelo '{tag}' nao esta baixado no Ollama.{RESET} \
                 Rode `ollama pull {tag}` (ou use -y para baixar agora)."
            );
            return None;
        }
        eprint!("{YELLOW}Modelo '{tag}' nao esta baixado. Baixar agora? [S/n] {RESET}");
        let _ = io::stderr().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() {
            return None;
        }
        let answer = answer.trim().to_lowercase();
        if !(answer.is_empty()
            || answer == "s"
            || answer == "sim"
            || answer == "y"
            || answer == "yes")
        {
            eprintln!("{DIM}  Download cancelado. Rode `ollama pull {tag}` quando quiser.{RESET}");
            return None;
        }
    }

    let base = ollama_base_url();
    // Default client: no timeout — a cold pull can run for many minutes.
    let puller = OllamaProvider::new(Some(tag.to_string()), Some(base.clone()));

    eprintln!("{DIM}Baixando {tag}…{RESET}");
    let mut last_status = String::new();
    let result = puller
        .pull_model(tag, |progress| {
            match progress.percent() {
                Some(pct) => eprint!("\r{DIM}  {} {pct:.0}%          {RESET}", progress.status),
                None if progress.status != last_status => {
                    eprint!("\r{DIM}  {}          {RESET}", progress.status);
                }
                None => return,
            }
            last_status = progress.status.clone();
            let _ = io::stderr().flush();
        })
        .await;
    eprintln!();

    match result {
        Ok(()) => {
            eprintln!("{GREEN}  {tag} pronto.{RESET}");
            let provider = OllamaProvider::new(Some(tag.to_string()), Some(base));
            Some((
                "ollama".to_string(),
                tag.to_string(),
                Arc::new(provider) as Arc<dyn LlmProvider>,
            ))
        }
        Err(e) => {
            eprintln!("{YELLOW}  Falha ao baixar {tag}: {e}{RESET}");
            None
        }
    }
}

/// Detect which provider to use based on config and availability.
///
/// `model_override` is the CLI `--model` flag. It has absolute precedence
/// over every configured model, and — when it names a tag the local Ollama
/// daemon has installed — it also selects the provider (see
/// [`try_local_ollama_model`]).
/// A chave que o [`detect_provider`] devolve quando nada respondeu: nenhum
/// provider configurado, nenhuma chave no ambiente e nenhum Ollama local com
/// saude. Quem precisa distinguir "achei um provider" de "sobrou o palpite"
/// (o `max-power`, que tem caminho offline) compara com ela.
pub(crate) const OLLAMA_ULTIMO_RECURSO: &str = "ollama (offline)";

pub async fn detect_provider(
    config: &AppConfig,
    url_override: Option<&str>,
    model_override: Option<&str>,
    assume_yes: bool,
) -> (String, String, Arc<dyn LlmProvider>) {
    detect_provider_with_env(
        config,
        url_override,
        model_override,
        assume_yes,
        &provider_binding::process_env,
    )
    .await
}

/// [`detect_provider`] with the environment injected, so tests pin every
/// env-var fallback without touching the process environment.
async fn detect_provider_with_env(
    config: &AppConfig,
    url_override: Option<&str>,
    model_override: Option<&str>,
    assume_yes: bool,
    env: provider_binding::Env<'_>,
) -> (String, String, Arc<dyn LlmProvider>) {
    // 0. If a custom URL is provided, use OpenAI-compatible provider (LM Studio, vLLM, etc.)
    if let Some(url) = url_override {
        let base = url.trim_end_matches('/').to_string();
        // An ad-hoc address gets `LLM_API_KEY` or the key of the `llm:` entry
        // that describes that same address — never `OPENAI_API_KEY` or
        // `GARRAIA_EMBEDDING_API_KEY`, which belong to other endpoints.
        let key = provider_binding::credential_for_ad_hoc_url(config, &base, env);
        let provider = OpenAiProvider::new(
            &key,
            None, // model will be set from --model flag or default
            Some(base.clone()),
        )
        .with_name("lmstudio");

        // An explicit --model wins over whatever the endpoint advertises.
        let model = match model_override {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => match provider.available_models().await {
                Ok(models) if !models.is_empty() => models[0].clone(),
                _ => "default".to_string(),
            },
        };
        return (
            format!("lmstudio ({})", base),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 0.5 — an explicitly named, locally-installed Ollama tag beats
    // `agent.default_provider`: `garraia --model qwen3.8` must open qwen3.8
    // even on a box configured for OpenRouter. Placed after `--url`, which is
    // a more explicit routing instruction than a bare model name.
    if let Some(m) = model_override.filter(|m| !m.is_empty()) {
        match try_local_ollama_model(config, m).await {
            LocalOllamaProbe::Installed(hit) => return *hit,
            LocalOllamaProbe::Missing { tag } => {
                if let Some(hit) = offer_pull_ollama_model(&tag, assume_yes).await {
                    return hit;
                }
                // Declined / failed / non-interactive: fall through. Every
                // branch below still honors `--model`.
            }
            LocalOllamaProbe::NotApplicable => {}
        }
    }

    // GAR-576 — honor `config.agent.default_provider` BEFORE the env-based
    // autodetect chain below. This prevents a stale `OPENAI_API_KEY` loaded
    // from cwd `.env` (via `dotenvy::dotenv()` in main.rs) from hijacking the
    // provider when the operator explicitly configured a different default.
    let env_has = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    let decision = decide_default_provider(
        config,
        env_has("OPENAI_API_KEY"),
        env_has("OPENROUTER_API_KEY"),
        env_has("ANTHROPIC_API_KEY"),
    );
    if let DefaultProviderDecision::UseDefault {
        config_key,
        provider_kind: _,
        model,
    } = decision
        // `--model` outranks the configured default.
        && let model = model_override
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or(model)
        && let Some(cfg) = config.llm.get(&config_key)
        && let Some(provider) = try_build_default_provider(&config_key, cfg, &model, env).await
    {
        // GAR-576: form the display tuple here from the (untainted)
        // strings returned by `decide_default_provider` — they never
        // pass through the function that resolves the api_key.
        return (config_key, model, provider);
        // If construction fails (e.g. Ollama health-check fails) the
        // outer `if-let` chain shorts out and we fall through to the
        // legacy autodetect chain below.
    }

    // Every branch below resolves its model through `explicit_model`, so
    // `--model` is honored whichever provider wins — and the returned
    // provider object always carries the model it will actually be asked for.
    let ollama_url = ollama_base_url();
    let model = resolve_provider_model(config, DEFAULT_LOCAL_PROVIDER, model_override)
        .unwrap_or_else(|| hardcoded_default_model(DEFAULT_LOCAL_PROVIDER));

    // #1180 decision 2 — the cloud is the first option unless the user asked
    // for something else. The credentials are resolved up front (cheap: env
    // var + `config.llm` lookup, no network, no vault) because the ORDER of
    // the chain depends on which of them exist, and that ordering decision
    // lives in a pure function so it can be tested without a daemon.
    //
    // Each candidate is bound WHOLE — endpoint and credential from the same
    // `llm:` entry. The chain used to take only the key (`llm.<kind>` or
    // `llm.main`) and pair it with the kind's default host, dropping the
    // entry's `base_url`: a proxy key went to api.openai.com / api.anthropic.com
    // / openrouter.ai. An `llm.<kind>` that declares another `provider:` no
    // longer erases the candidate — see `bind_autodetect`.
    let anthropic = provider_binding::bind_autodetect(config, "anthropic", env);
    let openai = provider_binding::bind_autodetect(config, "openai", env);
    let openrouter = provider_binding::bind_autodetect(config, DEFAULT_CLOUD_PROVIDER, env);

    for candidate in autodetect_order(anthropic.is_some(), openai.is_some(), openrouter.is_some()) {
        let (name, binding) = match candidate {
            AutodetectCandidate::Anthropic => ("anthropic", anthropic.as_ref()),
            AutodetectCandidate::OpenAi => ("openai", openai.as_ref()),
            AutodetectCandidate::OpenRouter => (DEFAULT_CLOUD_PROVIDER, openrouter.as_ref()),
            AutodetectCandidate::Ollama => {
                let ollama = OllamaProvider::new(Some(model.clone()), Some(ollama_url.clone()));
                if ollama.health_check().await.unwrap_or(false) {
                    return (
                        DEFAULT_LOCAL_PROVIDER.to_string(),
                        model,
                        Arc::new(ollama) as Arc<dyn LlmProvider>,
                    );
                }
                continue;
            }
        };
        let Some(binding) = binding else {
            continue;
        };
        let model = explicit_model(config, binding, model_override);
        // An `llm.openrouter` declaring `provider: openai` registers under
        // the entry's name, like an alias on the explicit path (GAR-582).
        let provider_id = (binding.kind() != name).then_some(name);
        let Ok(provider) = provider_binding::build_provider(binding, &model, provider_id, None)
        else {
            continue;
        };
        return (name.to_string(), model, provider);
    }

    // Last resort: Ollama with no health check (user will see the error on
    // the first message). Nothing else is left to try — this is the only
    // provider that needs no credential at all.
    let ollama = OllamaProvider::new(Some(model.clone()), Some(ollama_url));
    (
        OLLAMA_ULTIMO_RECURSO.to_string(),
        model,
        Arc::new(ollama) as Arc<dyn LlmProvider>,
    )
}

/// Resultado de um turno, do ponto de vista do REPL.
///
/// `Cancelled` existe porque o `Ctrl+C` passou a abortar apenas o turno em
/// andamento em vez de matar o processo — ver `stream_turn`.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum TurnOutcome<T, E> {
    Done(std::result::Result<T, E>),
    TimedOut,
    Cancelled,
}

/// Await the streaming LLM call while concurrently draining `rx`, writing
/// each delta to `out` as it arrives. The runtime pushes deltas through a
/// bounded channel with `send().await` (runtime.rs), so the receiver MUST be
/// polled during the call — draining only after completion deadlocks the
/// producer once the buffer fills (the original `garra chat` hang).
///
/// Returns `TurnOutcome::TimedOut` on timeout. In every path the call future
/// is dropped before the final drain, which closes the sender side so the
/// drain terminates once buffered deltas are consumed.
///
/// # Indicador de atividade
///
/// `spinner` anima a janela entre o envio e o primeiro token. Ele é um
/// **braço a mais do mesmo `select!`** — nunca uma task separada nem um sleep
/// bloqueante — justamente para não quebrar a drenagem concorrente descrita
/// acima: `rx` continua sendo consumido enquanto a garra gira. `None`
/// desativa a animação por completo (stdout redirecionado, `NO_COLOR`, etc.)
/// e nesse caso nem um byte de spinner chega ao `out`. A aparição tem uma
/// janela de ~270ms e o tempo decorrido entra em espera longa — ambos
/// derivados dos ticks dentro de `SpinnerState`, não de relógio (#936).
///
/// `prefix` (o rótulo `garra >`) é escrito exatamente uma vez, imediatamente
/// antes do primeiro delta — ou na saída, se nenhum delta chegar. Ele não pode
/// ser impresso antes da chamada como era feito: o spinner ocupa a mesma linha
/// e o `\r\x1b[2K` da limpeza apagaria o rótulo junto.
///
/// `cancel` é acordado pelo vigia de SIGINT de `run_chat` quando o usuário
/// aperta Ctrl+C durante o turno.
async fn stream_turn<F, T, E>(
    call: F,
    mut rx: mpsc::Receiver<TurnEvent>,
    timeout: std::time::Duration,
    out: &mut (impl io::Write + ?Sized),
    renderer: &mut TerminalRenderer,
    tool_log: &mut ToolLog,
    cancel: &tokio::sync::Notify,
) -> TurnOutcome<T, E>
where
    F: Future<Output = std::result::Result<T, E>>,
{
    let mut call = Box::pin(tokio::time::timeout(timeout, call));
    let mut rx_open = true;

    // O primeiro tick de `interval` dispara imediatamente; quem segura a
    // aparição é o próprio estado do spinner (#936): os primeiros ticks caem
    // dentro da janela de ~270ms e não pintam nada, então resposta rápida
    // não pisca spinner nenhum.
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(
        crate::ui::FRAME_INTERVAL_MS,
    ));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // A ordem obrigatória — apagar a animação, escrever o rótulo uma única
    // vez, só então o texto do modelo — mora no `TerminalRenderer` desde o
    // #942 (ADR 0017). Aqui só se diz *o que aconteceu*.
    //
    // O renderer é chamado **de dentro deste `select!`**, nunca de uma task
    // própria: o canal é limitado e o produtor usa `send().await`, então um
    // receptor que só drena quando sobra tempo trava o runtime. Foi o
    // travamento original do `garra chat`, e é o invariante 1 da ADR 0017.
    let result = loop {
        tokio::select! {
            r = &mut call => break match r {
                Ok(inner) => TurnOutcome::Done(inner),
                Err(_elapsed) => TurnOutcome::TimedOut,
            },
            // Cancelamento do turno, sinalizado pelo vigia de SIGINT criado em
            // `run_chat`. Sem este braço o Ctrl+C matava o processo inteiro no
            // meio do stream, levando junto o histórico da sessão.
            //
            // O sinal NÃO é registrado aqui de propósito: `tokio::signal::ctrl_c`
            // instala um handler para o resto da vida do processo, e um handler
            // sem ninguém escutando engoliria o Ctrl+C no prompt `voce >` — o
            // usuário ficaria sem como sair. Um dono único resolve os dois casos.
            _ = cancel.notified() => break TurnOutcome::Cancelled,
            _ = ticker.tick(), if renderer.has_animation() => {
                renderer.handle(UiEvent::ActivityTick, out);
            }
            maybe = rx.recv(), if rx_open => match maybe {
                Some(evento) => render_turn_event(&evento, renderer, tool_log, out),
                None => rx_open = false,
            },
        }
    };

    // Box::pin so the future (and its sender) can be dropped here even after
    // a timeout — tokio::pin! would keep it alive and hang the drain below.
    drop(call);
    if rx_open {
        while let Some(evento) = rx.recv().await {
            render_turn_event(&evento, renderer, tool_log, out);
        }
    }

    // Fim de turno incondicional: vale para sucesso, erro do provedor,
    // timeout, Ctrl+C e cancelamento. O renderer limpa a animação (idempotente)
    // e garante que o rótulo saiu.
    renderer.handle(UiEvent::TurnFinished, out);

    result
}

/// Traduz o que o runtime **contou** para o que o terminal **desenha**.
///
/// É a fronteira que a ADR 0017 fixa: `garraia-agents` fala `TurnEvent` e não
/// conhece renderer nenhum; o `ui` fala `UiEvent` e não conhece runtime. Esta
/// função de três linhas é toda a ponte, e é de propósito que ela seja
/// pequena — se um dia precisar de lógica, a lógica está no lado errado.
fn render_turn_event(
    evento: &TurnEvent,
    renderer: &mut TerminalRenderer,
    tool_log: &mut ToolLog,
    out: &mut (impl io::Write + ?Sized),
) {
    let ui = match evento {
        TurnEvent::TextDelta(texto) => UiEvent::TextDelta(texto),
        TurnEvent::ToolStarted { name, detail } => UiEvent::ToolStarted { name, detail },
        TurnEvent::ToolFinished {
            name,
            duration,
            success,
            summary,
            output,
        } => {
            // Guardar acontece **aqui**, e nao no renderer, porque o renderer
            // desenha e nao lembra (ADR 0017). O indice volta para a linha
            // desenhada: sem ele o usuario veria "/tool" no `/help` e nao teria
            // como saber qual numero pedir.
            let indice = tool_log.registrar(name, summary, *success, *duration, output.clone());
            UiEvent::ToolFinished {
                name,
                duration: *duration,
                success: *success,
                summary,
                indice: Some(indice),
            }
        }
    };
    renderer.handle(ui, out);
}

/// Run the interactive chat REPL.
///
/// Devolve o codigo de saida do processo: `0` para `/exit` e Ctrl+D, `130`
/// quando o Ctrl+C no prompt chega pelo editor de linha (#1297) — o mesmo
/// codigo que o vigia de SIGINT sempre deu no caminho sem editor. `Err` fica
/// para falha de verdade (I/O, provider), como antes.
pub async fn run_chat(
    config: AppConfig,
    provider_override: Option<String>,
    model_override: Option<String>,
    url_override: Option<String>,
    timeout_secs: u64,
    assume_yes: bool,
    persist: bool,
    resume: Option<String>,
) -> Result<i32> {
    // An explicit `--provider` short-circuits detection entirely; otherwise
    // `detect_provider` owns both the provider *and* the model, so the two can
    // no longer disagree (previously `--model` without `--provider` swapped
    // only the displayed string, leaving the built provider stale).
    let (provider_name, mut model_name, provider) = if let Some(ref p) = provider_override {
        // GAR-579: shared with `garra ask` — the explicit-provider path
        // lives in `select_explicit_provider` so chat and ask agree
        // byte-for-byte on construction + model resolution + error msgs.
        select_explicit_provider(
            &config,
            p.as_str(),
            model_override.as_deref(),
            url_override.as_deref(),
        )?
    } else {
        detect_provider(
            &config,
            url_override.as_deref(),
            model_override.as_deref(),
            assume_yes,
        )
        .await
    };

    // Keyless local daemons are the "local" mode; everything else is cloud.
    let mode = if provider_name.contains("ollama") || provider_name.contains("llamacpp") {
        "local"
    } else {
        "cloud"
    };

    // Gather current directory context
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "(desconhecido)".to_string());

    // Scan directory for context
    let dir_context = scan_directory_context(&cwd);

    // Cabecalho compacto (#935): tres linhas no lugar do quadro de doze com o
    // mascote — que nao sumiu, so deixou de ser cobrado em toda abertura
    // (`garra about` mostra ele inteiro). A listagem de arquivos que saia aqui
    // continua indo para o prompt do sistema, onde serve para alguma coisa, e
    // a inspecao detalhada virou o `/context`.
    // O que este terminal aguenta, decidido uma vez só (#942): antes a
    // conversa e a animação decidiam separado e podiam discordar.
    let caps = crate::ui::Capabilities::detect();
    let style = caps.style();
    let cwd_path = std::path::Path::new(&cwd);
    let markers = crate::ui::project_markers(cwd_path);
    // `openrouter/auto` no lugar de `auto`: modelo sem provider e ambiguo.
    let model_display = if model_name.contains('/') {
        model_name.clone()
    } else {
        format!("{provider_name}/{model_name}")
    };
    let header = crate::ui::Header {
        version: env!("CARGO_PKG_VERSION").to_string(),
        model: model_display,
        mode: mode.to_string(),
        cwd: crate::banner::shorten_path(cwd_path),
        branch: crate::ui::git_branch(cwd_path),
        project: (!markers.is_empty()).then(|| markers.join(", ")),
    };
    println!();
    print!("{}", header.render(style, caps.width));
    println!();

    // Build runtime with the same tool set the gateway wires (#1036), so
    // `garra chat`, the phone and Telegram agree on what the agent can do.
    let review = (Arc::clone(&provider), model_name.clone());
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);
    register_cli_tools(
        &runtime,
        &config,
        Some(review),
        get_api_key(&config, "brave", "BRAVE_API_KEY"),
        config.agent.bash_allowlist.clone(),
    );
    let tools_doc = tool_docs(&runtime.tool_names());

    let system_prompt = format!(
        "Voce e o GarraIA, um assistente pessoal de IA criado em Rust. \
         Seja prestativo, conciso e amigavel. Responda no idioma do usuario.\n\n\
         ## Ferramentas disponiveis\n\
         Voce tem acesso a estas ferramentas que pode usar quando necessario:\n\
         {tools_doc}\n\
         IMPORTANTE: Quando o usuario perguntar sobre arquivos, SEMPRE use as ferramentas \
         para ler/listar em vez de apenas descrever. Use 'list_dir' para listar, \
         'repo_search' para procurar e 'file_read' para ler conteudo de arquivos.\n\n\
         ## Contexto do diretorio atual\n\
         O usuario esta trabalhando em: {cwd}\n\
         {}\
         \n\
         Quando o usuario perguntar sobre arquivos, codigo ou o projeto, \
         USE as ferramentas para investigar. Nao invente — leia os arquivos reais.",
        if dir_context.is_empty() {
            String::new()
        } else {
            format!("Tipo de projeto detectado: {dir_context}\n")
        }
    );
    runtime.set_system_prompt(system_prompt);
    runtime.set_max_tokens(4096);

    // #1088: o store so existe quando a pessoa pediu. Sem `--persist` nem
    // `--resume` fica `None` — nenhum banco aberto, nenhum arquivo criado —
    // e o chat segue vivendo apenas na memoria, como sempre viveu.
    let store = open_chat_store(&config, persist, resume.as_deref())?;
    // Um renderer para a sessao inteira; cada turno o rearma com a animacao
    // daquele turno (#942). O rotulo `Garra` e a ordem de escrita passam a ser
    // responsabilidade dele — ver ADR 0017.
    let mut renderer = TerminalRenderer::new(caps, None);
    // Retomar e adotar o id que veio da linha de comando; comecar do zero e
    // sortear um. Nos dois casos o id aparece na tela (abaixo) justamente
    // para poder ser digitado de volta no `--resume`.
    //
    // #1300: `--resume` sem valor (ou `--resume latest`) aponta para a
    // ultima atividade do canal CLI — o turno interrompido de minutos atras,
    // nao uma sessao antiga qualquer. Sem alvo, comeca nova e avisa abaixo.
    let (mut session_id, comecou_nova) = id_inicial_da_sessao(
        resume.as_deref(),
        store
            .as_ref()
            .and_then(|s| s.latest_session_id("cli").ok())
            .flatten(),
        format!("cli-{}", uuid::Uuid::new_v4()),
    );
    // Ja vem cheio quando ha `--resume`; a carga acontece abaixo, depois do
    // renderer existir, para a contagem sair pela mesma moldura das outras
    // mensagens da sessao.
    let mut history: Vec<ChatMessage> = Vec::new();
    // A saida completa das ferramentas do turno, para o `/tool` (#938).
    // Limitada em entradas e em bytes — ver `ui::tool_log`.
    let mut tool_log = ToolLog::new();
    // Semente do indicador de atividade: roda a mensagem de abertura a
    // cada turno, para dois envios seguidos não começarem com a mesma frase.
    let mut turn_index: usize = 0;

    // Aviso e carga da persistencia (#1088). Fica aqui, e nao junto da
    // abertura do store, porque a contagem de turnos recuperados sai pela
    // mesma moldura das outras mensagens da sessao — e nao por `println!`,
    // que ignoraria `NO_COLOR` e pipe.
    if let Some(ref store) = store {
        let db = config.resolved_data_dir().join(SESSIONS_DB);
        if resume.is_some() && comecou_nova {
            // `--resume latest` sem alvo: primeiro uso (ou banco limpo) e
            // boot legitimo — avisa e segue, sem derrubar o programa.
            renderer.handle(
                UiEvent::Warning(&format!(
                    "Nenhuma sessao anterior encontrada em {} — comecando sessao nova: {session_id}",
                    db.display()
                )),
                &mut io::stdout(),
            );
        } else if resume.is_some() {
            let carregadas = load_history(store, &session_id, RESUME_LIMIT)?;
            // Cada turno comeca com uma pergunta. Contar `user` e mais
            // honesto que `len / 2` quando a hidratacao do gateway deixou
            // um turno pela metade na mesma sessao.
            let turnos = carregadas
                .iter()
                .filter(|m| matches!(m.role, ChatRole::User))
                .count();
            // A janela nao e a sessao inteira. Dizer quantas mensagens
            // ficaram de fora e a diferenca entre "retomei a conversa" e
            // "retomei metade achando que era tudo" — que e justamente a
            // queixa que abriu esta issue.
            let antigas = match store.get_message_count(&session_id) {
                Ok(total) => (total as usize).saturating_sub(carregadas.len()),
                Err(e) => {
                    // A conversa nao cai por causa de uma contagem — mas
                    // engolir o erro anunciaria "nada ficou fora da
                    // janela" sem ninguem saber. Avisa e segue: o mesmo
                    // fail-open da gravacao de turno.
                    renderer.handle(
                        UiEvent::Warning(&format!(
                            "Nao consegui contar o historico de {session_id} ({e}); \
                             mensagens mais antigas podem ter ficado fora da janela."
                        )),
                        &mut io::stdout(),
                    );
                    0
                }
            };
            let sufixo = if antigas > 0 {
                format!("; {antigas} mensagens mais antigas ficaram fora da janela")
            } else {
                String::new()
            };
            history = carregadas;
            // O `/status` conta turnos daqui, e nao do zero: retomar na
            // quinta pergunta e continuar na quinta, nao na primeira.
            turn_index = turnos;
            if turnos == 0 {
                renderer.handle(
                    UiEvent::Warning(&format!(
                        "A sessao {session_id} nao tem historico em {}. Comece uma conversa nova.",
                        db.display()
                    )),
                    &mut io::stdout(),
                );
            } else {
                renderer.handle(
                    UiEvent::Hint(&format!(
                        "Retomando {session_id}: {turnos} turno(s) recuperado(s){sufixo}."
                    )),
                    &mut io::stdout(),
                );
            }
        } else {
            renderer.handle(
                UiEvent::Hint(&format!(
                    "Sessao {session_id} — gravando em {}. Retome com: garraia chat --resume {session_id}",
                    db.display()
                )),
                &mut io::stdout(),
            );
        }
    }

    // De onde vem cada linha (#1297). Editor de linha (setas, historico,
    // edicao) so quando ha um humano num terminal — stdin, stdout e stderr
    // —; pipe e CI seguem no `read_line` byte a byte de sempre. A decisao e
    // uma so, no boot, e o `Capabilities::detect` acima nao serve para ela:
    // `NO_COLOR` e `TERM=dumb` tiram a cor, nao as setas.
    let usar_editor = {
        use std::io::IsTerminal as _;
        crate::chat_input::editor_de_linha_cabe(
            io::stdin().is_terminal(),
            io::stdout().is_terminal(),
            io::stderr().is_terminal(),
        )
    };
    // Historico em disco so quando a sessao e persistida (#1088): sem
    // `--persist`/`--resume` nada e escrito, e o que se digita e tao sensivel
    // quanto a resposta. `store.is_some()` e exatamente esse criterio. As
    // setas funcionam do mesmo jeito; o historico so nao sobrevive ao
    // processo.
    let historico = store
        .is_some()
        .then(|| crate::chat_input::caminho_do_historico(&crate::garraia_dir()));
    let mut leitor = crate::chat_input::LeitorDeLinha::abrir(usar_editor, historico);
    // Historico que nao abre e aviso, nao erro: o chat vale mais que as setas.
    for aviso in leitor.drenar_avisos() {
        renderer.handle(UiEvent::Warning(&aviso), &mut io::stdout());
    }
    let prompt_cru = style.user_prompt_plain();
    let prompt = style.user_prompt();

    // Dono único do SIGINT.
    //
    // `tokio::signal::ctrl_c()` instala um handler que substitui o
    // comportamento padrão do processo e permanece instalado até o fim da
    // execução. Registrá-lo dentro do turno (o lugar óbvio) teria um efeito
    // colateral silencioso e ruim: a partir do primeiro turno, o Ctrl+C no
    // prompt `voce >` deixaria de encerrar o `garra` e simplesmente sumiria,
    // porque o handler continua instalado sem ninguém escutando e o
    // `read_line` apenas reinicia no EINTR. O usuário ficaria preso.
    //
    // Com um dono único os dois casos ficam corretos:
    //   - durante o turno  -> cancela o turno e devolve o prompt;
    //   - ocioso no prompt -> encerra a sessão, como sempre encerrou.
    //
    // O editor de linha (#1297) nao muda quem e o dono — o rustyline entra
    // com a feature `signal-hook`, e e ELA que o impede de instalar o proprio
    // `sigaction(SIGINT)` a cada `readline` (sem ela haveria um segundo
    // handler disputando com este, e um `kill -INT` no prompt nunca chegaria
    // aqui) —, muda por onde o Ctrl+C do teclado chega quando o prompt esta
    // ocioso: em raw mode o terminal nao gera SIGINT (`ISIG` desligado), a
    // tecla vira `Leitura::Interrompida` no loop abaixo, e o loop encerra com
    // o mesmo 130 de sempre. Durante o turno o terminal ja voltou ao modo
    // canonico, o Ctrl+C vira SIGINT e cai aqui, no cancelamento. O braco
    // "ocioso" desta task continua valendo para o caminho sem editor e para
    // um SIGINT externo (`kill -INT`) com o editor ligado — e nesse ultimo
    // caso o terminal esta em raw mode, cujo guard de restauracao o `exit`
    // pularia, deixando o shell do usuario sem eco. Por isso a fotografia dos
    // atributos, devolvida antes de sair.
    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
    let turn_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Uma so fonte para a despedida, usada aqui e no `/exit`. Com cor quando
    // ha terminal, e sem um escape sequer quando a saida esta redirecionada.
    let despedida = if style.color {
        format!("{DIM}Ate mais! 🦀{RESET}")
    } else {
        "Ate mais!".to_string()
    };
    {
        let cancel = std::sync::Arc::clone(&cancel);
        let turn_active = std::sync::Arc::clone(&turn_active);
        let despedida = despedida.clone();
        let terminal_original = leitor
            .com_editor()
            .then(crate::chat_input::fotografar_terminal)
            .flatten();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    // Sem handler de sinal disponível: mantém o padrão do SO.
                    return;
                }
                if turn_active.load(std::sync::atomic::Ordering::SeqCst) {
                    cancel.notify_waiters();
                } else {
                    // 130 = terminado por SIGINT, a convenção do shell.
                    // A despedida sai pelo estilo detectado: dentro da task
                    // nao ha `&mut renderer`, entao o texto e montado antes e
                    // movido para ca ja pronto (#940).
                    if let Some(estado) = terminal_original.as_ref() {
                        crate::chat_input::restaurar_terminal(estado);
                    }
                    println!("\n{despedida}");
                    let _ = io::stdout().flush();
                    std::process::exit(130);
                }
            }
        });
    }

    let mut codigo_de_saida = 0;
    loop {
        let input = match leitor.ler(&prompt_cru, &prompt)? {
            crate::chat_input::Leitura::Linha(linha) => linha,
            crate::chat_input::Leitura::Fim => {
                // EOF (Ctrl+D) encerra como `/exit`. O leitor ja deixou o
                // cursor numa linha nova, nos dois caminhos.
                println!("{despedida}");
                break;
            }
            crate::chat_input::Leitura::Interrompida => {
                // Ctrl+C no prompt ocioso, entregue pelo editor em raw mode.
                // Mesmo desfecho do vigia acima: despedida e 130 — so que
                // pelo `return`, com o terminal ja restaurado pelo editor.
                println!("{despedida}");
                codigo_de_saida = 130;
                break;
            }
        };
        for aviso in leitor.drenar_avisos() {
            renderer.handle(UiEvent::Warning(&aviso), &mut io::stdout());
        }

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Handle slash commands
        match input.as_str() {
            "/exit" | "/quit" | "/sair" => {
                println!("{despedida}");
                break;
            }
            "/clear" | "/limpar" => {
                // So a memoria. O que ja foi gravado com `--persist` continua
                // no banco e voltaria num `--resume` — apagar o historico do
                // disco junto e outro slice, e precisa de confirmacao porque
                // e a unica copia (#1088).
                history.clear();
                renderer.handle(UiEvent::Hint("Historico limpo."), &mut io::stdout());
                continue;
            }
            "/help" | "/ajuda" => {
                // A lista sai pela mesma moldura das outras superficies, e nao
                // por `println!` com cor incondicional: era o `/help` que
                // aparecia com escape em `garra chat | cat` (#940).
                let itens: Vec<String> = COMANDOS
                    .iter()
                    .map(|(nome, o_que_faz)| format!("  {nome:<18} {o_que_faz}"))
                    .collect();
                renderer.handle(
                    UiEvent::List {
                        titulo: "Comandos",
                        itens: &itens,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            // As superficies de status que a #940 pede. O que elas mostram e
            // sempre o valor **vivo** — `/model` troca o modelo no meio da
            // sessao, entao ler a config diria o que era verdade no boot.
            "/status" => {
                let ferramentas = runtime.list_tool_info();
                let mut linhas = vec![
                    panel::linha("Provider", provider_name.clone()),
                    panel::linha("Modelo", model_name.clone()),
                    panel::linha("Ferramentas", ferramentas.len().to_string()),
                    panel::linha("Sessao", session_id.clone()),
                    panel::linha("Turnos", turn_index.to_string()),
                ];
                match runtime.last_turn_stats(&session_id) {
                    // O `/stats` dos canais (#984) faz a mesma distincao, e
                    // pela mesma razao: no streaming o provider nao informa
                    // token nenhum, e zero-porque-nao-sei nao e
                    // zero-porque-nao-usou.
                    Some(st) => {
                        let modelo = if st.model_confirmado {
                            format!("{} (confirmado pelo provider)", st.model)
                        } else {
                            format!("{} (pedido; o provider nao confirmou)", st.model)
                        };
                        linhas.push(panel::linha("Ultimo modelo", modelo));
                        linhas.push(panel::linha(
                            "Ultimo turno",
                            format!(
                                "{} ferramenta(s), {}",
                                st.tool_calls,
                                crate::ui::format_duration(std::time::Duration::from_millis(
                                    st.latency_ms
                                ))
                            ),
                        ));
                        linhas.push(panel::linha(
                            "Tokens",
                            if st.tokens_conhecidos {
                                format!("{} entrada, {} saida", st.input_tokens, st.output_tokens)
                            } else {
                                "nao informados neste caminho".to_string()
                            },
                        ));
                        if st.fallback {
                            linhas.push(panel::linha("Fallback", "sim — o primario falhou"));
                        }
                    }
                    None => linhas.push(panel::linha("Ultimo turno", "nenhum ainda")),
                }
                renderer.handle(
                    UiEvent::Panel {
                        titulo: "Status",
                        linhas: &linhas,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            // Quais ferramentas o agente **tem** — diferente do `/tool`, que
            // mostra o que elas **produziram**. Ate aqui `/tools` era apelido
            // de `/tool`, e nao havia como perguntar a primeira coisa.
            // O `/logs` **nao** despeja o log na conversa: ele diz onde ele
            // esta e qual comando o le. Misturar centenas de linhas de log com
            // a conversa e o oposto do que a Fase 2 deste epico foi fazer, e
            // seguir o arquivo em tempo real disputaria a mesma tela com o
            // streaming da resposta. O criterio de aceite pede "expose the log
            // location/workflow", e e isso.
            "/logs" | "/log" => {
                let caminho = crate::logs_cmd::caminho_do_log(&crate::garraia_dir());
                let existe = caminho.exists();
                let linhas = vec![
                    panel::linha("Arquivo", caminho.display().to_string()),
                    panel::linha(
                        "Estado",
                        if existe {
                            "existe"
                        } else {
                            "ainda nao foi criado"
                        },
                    ),
                    panel::linha("Ver o fim", "garra logs"),
                    panel::linha("Acompanhar", "garra logs --follow"),
                    panel::linha("Mais detalhe", "RUST_LOG=debug garra start"),
                ];
                renderer.handle(
                    UiEvent::Panel {
                        titulo: "Log",
                        linhas: &linhas,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            "/tools" | "/ferramentas" => {
                let ferramentas = runtime.list_tool_info();
                if ferramentas.is_empty() {
                    renderer.handle(
                        UiEvent::Hint("Nenhuma ferramenta registrada nesta sessao."),
                        &mut io::stdout(),
                    );
                    continue;
                }
                // Painel, e nao lista: nome e descricao sao duas colunas, e
                // so o painel alinha a continuacao. Como lista de strings ja
                // montadas, a descricao longa do `file_write` quebrava na
                // coluna zero e desmontava o alinhamento — visto rodando o
                // binario.
                let linhas: Vec<panel::Linha<'_>> = ferramentas
                    .iter()
                    .map(|(nome, descricao)| {
                        // A descricao de uma tool MCP e texto livre escrito
                        // pelo servidor. O painel tira escape, mas escape nao e
                        // o unico problema: uma descricao mal escrita pode
                        // trazer uma URL com token dentro, e ela iria para a
                        // tela em texto plano. E a mesma redacao que o
                        // `capture_tool_output` ja aplica na **saida** da
                        // ferramenta — faltava na descricao. Achado na
                        // auditoria.
                        let curta = descricao.lines().next().unwrap_or("");
                        panel::linha(nome.as_str(), garraia_security::redact_secrets(curta))
                    })
                    .collect();
                renderer.handle(
                    UiEvent::Panel {
                        titulo: "Ferramentas disponiveis",
                        linhas: &linhas,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            // A inspecao detalhada de diretorio saiu do cabecalho de abertura
            // (#935) e mora aqui: quem quer ver, pede.
            "/context" | "/contexto" => {
                let ramo = crate::ui::git_branch(std::path::Path::new(&cwd))
                    .unwrap_or_else(|| "(fora de um repositorio git)".to_string());
                let marcadores = project_summary(&cwd);
                let projeto = if marcadores.is_empty() {
                    "nenhum marcador detectado".to_string()
                } else {
                    marcadores
                };
                // **Sem** contagem de arquivos, que o exemplo da issue mostra.
                // A propria issue pede para nao varrer o diretorio so para
                // desenhar status, e num repositorio grande a varredura custa
                // mais que tudo o mais junto. O marcador de projeto ja responde
                // "que projeto e este" sem ler a arvore.
                let linhas = vec![
                    panel::linha("Diretorio", cwd.clone()),
                    panel::linha("Ramo", ramo),
                    panel::linha("Projeto", projeto),
                    panel::linha("Provider", provider_name.clone()),
                    panel::linha("Modelo", model_name.clone()),
                    panel::linha("Ferramentas", runtime.list_tool_info().len().to_string()),
                ];
                renderer.handle(
                    UiEvent::Panel {
                        titulo: "Contexto",
                        linhas: &linhas,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            // A saida completa das ferramentas (#938). O resumo de uma linha
            // do #937 deixou a conversa legivel; sem isto ele so teria
            // escondido a causa da falha.
            "/tool" | "/saida" => {
                if tool_log.vazio() {
                    renderer.handle(
                        UiEvent::Hint("Nenhuma ferramenta foi chamada ainda nesta sessao."),
                        &mut io::stdout(),
                    );
                    continue;
                }
                let itens: Vec<String> = tool_log
                    .listar()
                    .map(|e| {
                        let marca = if e.sucesso { " " } else { "!" };
                        let detalhe: String = e.detalhe.chars().take(52).collect();
                        format!("  {marca}#{:<3} {:<12} {}", e.indice, e.ferramenta, detalhe)
                    })
                    .collect();
                renderer.handle(
                    UiEvent::List {
                        titulo: "Saidas guardadas (use /tool <n> para ver inteira)",
                        itens: &itens,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            _ if input.starts_with("/tool ") || input.starts_with("/saida ") => {
                let arg = input
                    .split_once(' ')
                    .map(|(_, resto)| resto.trim())
                    .unwrap_or("");
                match arg.trim_start_matches('#').parse::<usize>() {
                    Ok(n) => match tool_log.buscar(n) {
                        Busca::Achou(e) => {
                            let estado = if e.sucesso { "ok" } else { "falhou" };
                            let mut cabecalho = vec![panel::linha(
                                "Estado",
                                format!("{estado} · {}", crate::ui::format_duration(e.duracao)),
                            )];
                            if !e.detalhe.is_empty() {
                                cabecalho.push(panel::linha("Resumo", e.detalhe.clone()));
                            }
                            renderer.handle(
                                UiEvent::Panel {
                                    titulo: &format!("#{} {}", e.indice, e.ferramenta),
                                    linhas: &cabecalho,
                                },
                                &mut io::stdout(),
                            );
                            println!();
                            // A saida ja veio redigida e saneada do
                            // `garraia-agents`; aqui e so imprimir.
                            println!("{}", e.saida);
                        }
                        // Expirada e inexistente sao mensagens diferentes de
                        // proposito: "saiu do registro" e acionavel (rode de
                        // novo), "nunca existiu" quer dizer que o numero esta
                        // errado.
                        Busca::Expirada => renderer.handle(
                            UiEvent::Hint(&format!(
                                "A saida #{n} ja saiu do registro — so as mais \
                                 recentes ficam guardadas. Rode o comando de novo \
                                 para ve-la."
                            )),
                            &mut io::stdout(),
                        ),
                        Busca::Inexistente => renderer.handle(
                            UiEvent::Hint(&format!(
                                "Nao ha saida #{n}. Use /tool para ver as disponiveis."
                            )),
                            &mut io::stdout(),
                        ),
                    },
                    Err(_) => renderer.handle(
                        UiEvent::Hint("Uso: /tool <numero>. Ex.: /tool 3"),
                        &mut io::stdout(),
                    ),
                }
                continue;
            }
            "/history" | "/historico" => {
                if history.is_empty() {
                    renderer.handle(UiEvent::Hint("Historico vazio."), &mut io::stdout());
                    continue;
                }
                let itens: Vec<String> = history
                    .iter()
                    .map(|msg| {
                        let role = match msg.role {
                            ChatRole::User => "voce",
                            ChatRole::Assistant => "garra",
                            _ => "system",
                        };
                        let text = match &msg.content {
                            MessagePart::Text(t) => t.as_str(),
                            MessagePart::Parts(_) => "(multi-part)",
                        };
                        // O texto do modelo entra aqui: o painel saneia, mas o
                        // corte de 80 tambem e defesa — uma resposta inteira
                        // por linha tornaria o historico ilegivel.
                        let preview: String = text.chars().take(80).collect();
                        format!("  {role:<6} {preview}")
                    })
                    .collect();
                renderer.handle(
                    UiEvent::List {
                        titulo: "Historico",
                        itens: &itens,
                    },
                    &mut io::stdout(),
                );
                continue;
            }
            // #1300: `/resume` retoma a SESSAO MAIS RECENTE — que, com a
            // persistencia adiantada da pergunta, e o turno interrompido de
            // minutos atras, nao uma sessao antiga. Com id, retoma esse id.
            _ if input == "/resume" || input.starts_with("/resume ") => {
                let Some(s) = store.as_ref() else {
                    renderer.handle(
                        UiEvent::Warning(
                            "Sem persistencia nesta sessao: /resume precisa do banco de sessoes.",
                        ),
                        &mut io::stdout(),
                    );
                    renderer.handle(
                        UiEvent::Hint("Reinicie com: garraia chat --resume  (ou --resume <id>)"),
                        &mut io::stdout(),
                    );
                    continue;
                };
                let arg = input["/resume".len()..].trim();
                let alvo = if arg.is_empty() {
                    match s.latest_session_id("cli") {
                        Ok(alvo) => alvo,
                        Err(e) => {
                            renderer.handle(
                                UiEvent::Warning(&format!(
                                    "Nao consegui achar a ultima sessao ({e}). Estado mantido."
                                )),
                                &mut io::stdout(),
                            );
                            continue;
                        }
                    }
                } else {
                    Some(arg.to_string())
                };
                let Some(alvo) = alvo else {
                    renderer.handle(
                        UiEvent::Warning(
                            "Nenhuma sessao anterior encontrada no banco desta sessao.",
                        ),
                        &mut io::stdout(),
                    );
                    continue;
                };
                if alvo == session_id {
                    renderer.handle(
                        UiEvent::Hint(&format!("Ja esta na sessao {alvo}.")),
                        &mut io::stdout(),
                    );
                    continue;
                }
                match load_history(s, &alvo, RESUME_LIMIT) {
                    Err(e) => renderer.handle(
                        UiEvent::Warning(&format!(
                            "Nao consegui carregar {alvo} ({e}). Estado mantido."
                        )),
                        &mut io::stdout(),
                    ),
                    Ok(carregadas) if carregadas.is_empty() => renderer.handle(
                        UiEvent::Warning(&format!(
                            "A sessao {alvo} nao tem historico. Estado mantido: {session_id}."
                        )),
                        &mut io::stdout(),
                    ),
                    Ok(carregadas) => {
                        let turnos = carregadas
                            .iter()
                            .filter(|m| matches!(m.role, ChatRole::User))
                            .count();
                        // O fim da conversa hidratada diz se o ultimo turno
                        // acabou interrompido — o resumo que a issue pede:
                        // o que ficou pendente e por que parou.
                        let interrompido = carregadas.last().and_then(|m| match &m.content {
                            MessagePart::Text(t) if t.starts_with("[turno interrompido: ") => Some(
                                t.trim_start_matches("[turno interrompido: ")
                                    .trim_end_matches(']')
                                    .to_string(),
                            ),
                            _ => None,
                        });
                        history = carregadas;
                        session_id = alvo.clone();
                        turn_index = turnos;
                        renderer.handle(
                            UiEvent::Hint(&format!(
                                "Retomando {alvo}: {turnos} turno(s) recuperado(s)."
                            )),
                            &mut io::stdout(),
                        );
                        if let Some(motivo) = interrompido {
                            renderer.handle(
                                UiEvent::Warning(&format!(
                                    "O ultimo turno desta sessao terminou interrompido ({motivo}); \
                                     a pergunta dele esta no fim do historico."
                                )),
                                &mut io::stdout(),
                            );
                        }
                    }
                }
                continue;
            }
            _ if input.starts_with("/model ") => {
                let new_model = input[7..].trim();
                if new_model.is_empty() {
                    renderer.handle(UiEvent::Hint("Uso: /model <nome>"), &mut io::stdout());
                    continue;
                }
                // On Ollama, `/model qwen3.8` means `qwen3.8:latest` — spell
                // it out so the banner and `/models` marker line up.
                let resolved = if provider_name.contains("ollama") {
                    normalize_ollama_tag(new_model).unwrap_or_else(|| new_model.to_string())
                } else {
                    new_model.to_string()
                };
                // #1298: a troca é transacional. Valida contra o catálogo
                // REAL do provider (`validar_modelo`, não a lista curada que
                // o ADR 0022 já provou insuficiente — `z-ai/glm-5.3-flash`
                // vive fora dela) ANTES de tocar o estado. Qualquer falha —
                // modelo ausente, provider indisponível, erro de rede — recusa
                // fail-closed e deixa o estado anterior intacto.
                let efeito = match runtime.default_provider() {
                    Some(p) => match p.validar_modelo(&resolved).await {
                        Ok(v) => efeito_da_validacao(&v),
                        Err(e) => {
                            renderer.handle(
                                UiEvent::Warning(&format!(
                                    "Nao consegui validar '{resolved}' no provider {provider_name}: {e}. Estado mantido."
                                )),
                                &mut io::stdout(),
                            );
                            renderer.handle(
                                UiEvent::Hint("Tente de novo, ou use /models para ver o que o provider lista."),
                                &mut io::stdout(),
                            );
                            continue;
                        }
                    },
                    None => {
                        renderer.handle(
                            UiEvent::Warning(&format!(
                                "Sem provider ativo para validar '{resolved}'. Estado mantido: provider {provider_name}, model {model_name}."
                            )),
                            &mut io::stdout(),
                        );
                        continue;
                    }
                };
                let nota_de_catalogo = "  (encontrado no catalogo completo do OpenRouter, fora da lista curada de /models)";
                let ressalva_sem_listagem =
                    "  (provider nao expoe catalogo — troca aplicada sem validacao)";
                match efeito {
                    EfeitoTrocaDeModelo::RecusarEManterEstado => {
                        renderer.handle(
                            UiEvent::Warning(&format!(
                                "'{resolved}' nao existe no catalogo de {provider_name}. Estado mantido: provider {provider_name}, model {model_name}."
                            )),
                            &mut io::stdout(),
                        );
                        renderer.handle(
                            UiEvent::Hint(
                                "Tente: /models para ver os nomes que este provider anuncia.",
                            ),
                            &mut io::stdout(),
                        );
                    }
                    EfeitoTrocaDeModelo::Aplicar => {
                        model_name = resolved;
                        renderer.handle(
                            UiEvent::Hint(&format!("Modelo alterado para: {model_name}")),
                            &mut io::stdout(),
                        );
                        renderer.handle(
                            UiEvent::Hint(&format!(
                                "  (o provider continua {provider_name} — para trocar, reinicie com --provider ou --model)"
                            )),
                            &mut io::stdout(),
                        );
                    }
                    EfeitoTrocaDeModelo::AplicarComNotaDeCatalogo => {
                        model_name = resolved;
                        renderer.handle(
                            UiEvent::Hint(&format!("Modelo alterado para: {model_name}")),
                            &mut io::stdout(),
                        );
                        renderer.handle(UiEvent::Hint(nota_de_catalogo), &mut io::stdout());
                        renderer.handle(
                            UiEvent::Hint(&format!(
                                "  (o provider continua {provider_name} — para trocar, reinicie com --provider ou --model)"
                            )),
                            &mut io::stdout(),
                        );
                    }
                    EfeitoTrocaDeModelo::AplicarSemValidacao => {
                        model_name = resolved;
                        renderer.handle(
                            UiEvent::Hint(&format!("Modelo alterado para: {model_name}")),
                            &mut io::stdout(),
                        );
                        renderer.handle(UiEvent::Hint(ressalva_sem_listagem), &mut io::stdout());
                        renderer.handle(
                            UiEvent::Hint(&format!(
                                "  (o provider continua {provider_name} — para trocar, reinicie com --provider ou --model)"
                            )),
                            &mut io::stdout(),
                        );
                    }
                }
                continue;
            }
            "/models" => {
                let provider_ref = runtime.default_provider();
                if let Some(p) = provider_ref {
                    match p.available_models().await {
                        Ok(models) => {
                            let mut itens: Vec<String> = models
                                .iter()
                                .take(20)
                                .map(|m| {
                                    let marker = if m == &model_name { " *" } else { "" };
                                    format!("  {m}{marker}")
                                })
                                .collect();
                            if models.len() > 20 {
                                itens.push(format!("  ... e mais {} modelos", models.len() - 20));
                            }
                            renderer.handle(
                                UiEvent::List {
                                    titulo: &format!("Modelos disponiveis ({provider_name})"),
                                    itens: &itens,
                                },
                                &mut io::stdout(),
                            );
                        }
                        // A mensagem do provider e de fora: vai como aviso, que
                        // ja saneia, e nao como `println!` cru.
                        Err(e) => renderer.handle(
                            UiEvent::Warning(&format!("Erro listando modelos: {e}")),
                            &mut io::stdout(),
                        ),
                    }
                }
                continue;
            }
            _ if input.starts_with("/provider ") => {
                let new_provider = input[10..].trim();
                renderer.handle(
                    UiEvent::Hint(&format!(
                        "Para trocar provider, reinicie com: garraia chat --provider {new_provider}"
                    )),
                    &mut io::stdout(),
                );
                renderer.handle(
                    UiEvent::Hint("  Para um modelo local do Ollama basta: garraia --model <tag>"),
                    &mut io::stdout(),
                );
                continue;
            }
            _ => {}
        }

        // Stream response.
        //
        // O rótulo NÃO é impresso aqui: ele vai como `prefix` para o
        // `stream_turn`, que o escreve junto do primeiro token. O indicador de
        // atividade ocupa esta linha enquanto o modelo pensa, e limpá-la
        // apagaria o rótulo se ele já estivesse na tela.
        //
        // #1300: a pergunta grava AGORA, antes do turno — não depois. Era o
        // centro do bug do `/resume`: só o turno COMPLETO tocava o disco,
        // então timeout, Ctrl+C, provider caindo ou um kill no meio deixavam
        // a sessão restaurada terminando na pergunta anterior. Falha de
        // gravação avisa e o turno segue — persistência não pode derrubar a
        // conversa (#1088).
        if let Some(ref store) = store
            && let Err(e) = persist_user_turn(store, &session_id, &input)
        {
            renderer.handle(
                UiEvent::Warning(&format!("Pergunta nao gravada: {e}")),
                &mut io::stdout(),
            );
        }
        renderer.begin_turn(caps.spinner(turn_index));
        turn_index = turn_index.wrapping_add(1);

        let (tx, rx) = mpsc::channel::<TurnEvent>(100);

        // The runtime appends the user message on top of the given history
        // (runtime.rs), so `history` must NOT contain it yet — it is pushed
        // below only after a successful turn.
        let history_clone = history.clone();
        let session_clone = session_id.clone();
        let model_clone = model_name.clone();

        // #937: o chat pede o fluxo completo — texto e ciclo de vida das
        // ferramentas. Os outros consumidores do runtime seguem no caminho
        // de texto e nao pagam nada por isto.
        // Ligado a uma variavel porque o `call` e um future que vive alem
        // desta expressao — um temporario seria descartado antes do `await`.
        let exec = exec_do_turno(&cwd, &session_clone);
        let call = runtime.process_message_streaming_with_events(
            &session_clone,
            &input,
            &history_clone,
            tx,
            None,
            None,
            None,
            Some(&model_clone),
            None,
            None,
            // #980: o diretorio do projeto resolve caminho relativo das
            // ferramentas de arquivo. **Nao e sandbox** — ver o docblock do
            // `ExecContext::working_dir`. O CLI nao tem `/mode`, entao o modo
            // fica `None`, que quer dizer "sem politica".
            &exec,
        );
        let mut stdout = io::stdout();
        turn_active.store(true, std::sync::atomic::Ordering::SeqCst);
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(timeout_secs),
            &mut stdout,
            &mut renderer,
            &mut tool_log,
            &cancel,
        )
        .await;
        turn_active.store(false, std::sync::atomic::Ordering::SeqCst);

        // #1300: os tres fins inacabados gravam o marcador de interrupcao
        // como mensagem do assistant, antes do cartao na tela. O
        // `--resume` seguinte hidrata pergunta + marcador — o alvo do
        // resume e a sessao desta conversa, nao uma antiga qualquer.
        if let Some(ref store) = store
            && let Some(marcador) = marcador_de_interrupcao(&outcome)
            && let Err(e) = persist_assistant_turn(store, &session_id, marcador)
        {
            renderer.handle(
                UiEvent::Warning(&format!("Marcador de interrupcao nao gravado: {e}")),
                &mut stdout,
            );
        }

        match outcome {
            TurnOutcome::TimedOut => {
                let cartao = ErrorCard::timeout_local(timeout_secs);
                renderer.handle(
                    UiEvent::ErrorCard {
                        titulo: &cartao.titulo,
                        detalhe: &cartao.detalhe,
                        acoes: &cartao.acoes,
                    },
                    &mut stdout,
                );
            }
            TurnOutcome::Cancelled => {
                // Ctrl+C aborta o turno, não a sessão: o histórico segue vivo.
                renderer.handle(
                    UiEvent::Hint("Cancelado. Manda outra ou /exit para sair."),
                    &mut stdout,
                );
            }
            TurnOutcome::Done(Ok(full_response)) => {
                // Deltas were already printed live during streaming
                println!();

                // #1088: gravar e opcional e nao pode derrubar a conversa —
                // disco cheio ou banco travado avisa e segue. A pergunta ja
                // foi gravada no inicio do turno (#1300); aqui entra so o
                // lado do assistant. Tambem nao e o caso de
                // `save_session_summary`: sem um resumidor no CLI (ele mora
                // no gateway) nao ha resumo honesto para gravar.
                if let Some(ref store) = store
                    && let Err(e) = persist_assistant_turn(store, &session_id, &full_response)
                {
                    renderer.handle(
                        UiEvent::Warning(&format!("Resposta nao gravada: {e}")),
                        &mut stdout,
                    );
                }

                history.push(ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text(input.clone()),
                });
                history.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Text(full_response),
                });
            }
            TurnOutcome::Done(Err(e)) => {
                // O #941 tirou daqui o `err_str.contains(...)` solto: a
                // classificacao e as acoes moram em `ui::error_card`, onde sao
                // testaveis sem terminal, e cobrem oito classes em vez de duas.
                //
                // A mensagem crua **nao** some: classe desconhecida cai num
                // cartao generico que a mostra inteira. Trocar um erro feio por
                // um erro invisivel seria pior.
                let cartao = ErrorCard::from_error(&format!("{e}"), &provider_name);
                renderer.handle(
                    UiEvent::ErrorCard {
                        titulo: &cartao.titulo,
                        detalhe: &cartao.detalhe,
                        acoes: &cartao.acoes,
                    },
                    &mut stdout,
                );
            }
        }

        println!();
    }

    Ok(codigo_de_saida)
}

#[cfg(test)]
mod testes_1225_s2 {
    use super::*;

    /// #1225 S2b: a policy de `agent.sandbox` chega as tools de programa
    /// pelo ponto de registro de PRODUCAO. `mode = all` sem backend recusa
    /// todo spawn sem consultar binario nenhum do host — deterministico, e
    /// antes da S2b esta config deixava as tools rodarem no host.
    async fn roda_recusada(
        tool: std::sync::Arc<dyn garraia_agents::Tool>,
        input: serde_json::Value,
    ) {
        let dir = tempfile::tempdir().expect("tmp");
        let ctx = garraia_agents::ToolContext {
            session_id: "wiring-1225".into(),
            user_id: None,
            is_heartbeat: false,
            approval: Default::default(),
            working_dir: Some(dir.path().to_string_lossy().into_owned()),
            project_id: None,
        };
        let out = tool.execute(&ctx, input).await;
        let texto = match out {
            Ok(o) => {
                assert!(o.is_error, "{}: {o:?}", tool.name());
                o.content
            }
            Err(e) => e.to_string(),
        };
        assert!(texto.contains("nenhum backend"), "{}: {texto}", tool.name());
    }

    #[tokio::test]
    async fn sandbox_do_config_chega_as_tools_de_programa_do_chat() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        let runtime = AgentRuntime::new();
        register_cli_tools(&runtime, &config, None, None, Vec::new());
        for (nome, input) in [
            ("repo_search", serde_json::json!({"query": "x"})),
            ("git_diff", serde_json::json!({"operation": "status"})),
        ] {
            let tool = runtime.find_tool(nome).expect("registrada");
            roda_recusada(tool, input).await;
        }
    }
}

#[cfg(test)]
mod tests {
    //! GAR-576 — Pure tests for provider/model resolution. None of these
    //! touch `std::env` or the filesystem; env presence is passed in as
    //! bool flags so the `OPENAI_API_KEY` hijack regression can be
    //! asserted without mutating process-global state.

    use super::*;
    use garraia_config::{AgentConfig, AppConfig, LlmProviderConfig};
    use std::collections::HashMap;

    // ── contexto de projeto (scan_directory_context) ────────────────────────

    #[test]
    fn contexto_filtra_diretorios_de_build_e_dependencia() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for dir in ["target", "node_modules", "dist", "src"] {
            std::fs::create_dir_all(tmp.path().join(dir)).expect("mkdir");
        }
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").expect("write");
        let ctx = scan_directory_context(tmp.path().to_str().expect("utf8"));
        assert!(ctx.contains("src"), "src deve aparecer: {ctx}");
        assert!(!ctx.contains("target"), "target deve ser filtrado: {ctx}");
        assert!(
            !ctx.contains("node_modules"),
            "node_modules deve ser filtrado: {ctx}"
        );
        assert!(!ctx.contains("dist"), "dist deve ser filtrado: {ctx}");
    }

    #[test]
    fn contexto_inclui_nome_do_projeto_do_readme() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("README.md"),
            "# Meu Projeto\n\nTexto.\n\n## Instalação\n",
        )
        .expect("write");
        std::fs::write(tmp.path().join("main.py"), "print(1)\n").expect("write");
        let ctx = scan_directory_context(tmp.path().to_str().expect("utf8"));
        assert!(ctx.contains("Projeto: Meu Projeto"), "got: {ctx}");
        // Só o PRIMEIRO heading vira nome — "## Instalação" não.
        assert!(!ctx.contains("Instalação"), "got: {ctx}");
    }

    #[test]
    fn contexto_inclui_ramo_git_quando_existe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(git_dir.join("refs").join("heads")).expect("mkdir");
        std::fs::write(
            git_dir.join("HEAD"),
            "ref: refs/heads/feat/contexto-projeto\n",
        )
        .expect("write");
        std::fs::write(tmp.path().join("app.rs"), "fn main() {}\n").expect("write");
        let ctx = scan_directory_context(tmp.path().to_str().expect("utf8"));
        assert!(ctx.contains("Ramo: feat/contexto-projeto"), "got: {ctx}");
    }

    #[test]
    fn contexto_vazio_fora_de_projeto() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = scan_directory_context(tmp.path().to_str().expect("utf8"));
        assert!(ctx.is_empty(), "got: {ctx}");
    }

    /// Apelidos aceitos no `match` que **de proposito** nao aparecem no
    /// `/help`: listar `/sair`, `/limpar` e `/historico` ao lado de `/exit`,
    /// `/clear` e `/history` dobraria a tela sem ensinar nada.
    const APELIDOS: &[&str] = &[
        "/quit",
        "/sair",
        "/limpar",
        "/ajuda",
        "/contexto",
        "/historico",
        "/saida",
        "/ferramentas",
        "/log",
    ];

    /// O `/help` nao pode divergir do `match` — e ele ja tinha divergido.
    ///
    /// A lista antiga era escrita a mao ao lado do `match` e omitia `/models`,
    /// nao tinha `/status` nem `/tools`, e prometia `/provider <nome>` como se
    /// trocasse de provider. O jeito de isso nao voltar e o CI conferir os dois
    /// lados, e nao a disciplina de quem edita.
    ///
    /// Le o proprio fonte por `include_str!`: e o mesmo recurso que os
    /// tripwires do `state.rs` usam, e o unico jeito de afirmar sobre o `match`
    /// sem transforma-lo numa tabela em runtime.
    #[test]
    fn o_help_e_o_match_nao_divergem() {
        const FONTE: &str = include_str!("chat.rs");

        // A varredura tem de excluir a **propria** tabela e o modulo de teste,
        // senao ela se satisfaz sozinha: um `/fantasma` inventado no `/help`
        // aparece como literal na tabela e passaria por "existe no match".
        // Foi o que aconteceu na primeira versao deste teste — verifiquei
        // inserindo o comando falso, e ele passou verde.
        let tabela = FONTE.find("const COMANDOS:").expect("a tabela existe");
        let fim_tabela = FONTE[tabela..]
            .find("\n];")
            .map(|i| tabela + i)
            .expect("a tabela fecha");
        let inicio_testes = FONTE.find("#[cfg(test)]").unwrap_or(FONTE.len());
        let corpo = format!("{}{}", &FONTE[..tabela], &FONTE[fim_tabela..inicio_testes]);
        let corpo = corpo.as_str();

        // Todo comando anunciado existe de verdade.
        //
        // Duas grafias porque ha duas formas de casar: comando sem argumento
        // vira braco exato (`"/status"`), e comando com argumento vira prefixo
        // (`starts_with("/model ")`) — com o espaco fazendo parte do literal.
        for (nome, _) in COMANDOS {
            let base = nome.split(' ').next().unwrap_or(nome);
            let exato = corpo.contains(&format!("\"{base}\""));
            let prefixo = corpo.contains(&format!("\"{base} \""));
            assert!(exato || prefixo, "{base} esta no /help e nao no match");
        }

        // E todo comando do match esta anunciado ou e apelido declarado.
        //
        // So os literais de comando: o corpo dos arms tem texto de ajuda com
        // barra dentro, e a busca e ancorada em `"/` seguido de letra.
        //
        // As duas terminacoes contam. `"/status"` e braco exato; `"/model "`
        // (com espaco) e prefixo de `starts_with`. Vendo so a primeira, um
        // `_ if input.starts_with("/novo ")` novo entraria sem aparecer no
        // `/help` e sem o teste notar — achado na auditoria.
        let mut vistos = std::collections::BTreeSet::new();
        let bytes = corpo.as_bytes();
        for (i, _) in corpo.match_indices("\"/") {
            let resto = &corpo[i + 2..];
            let fim = resto
                .find(|c: char| !c.is_ascii_lowercase())
                .unwrap_or(resto.len());
            let fecha = bytes.get(i + 2 + fim);
            let fecha_com_espaco = fecha == Some(&b' ') && bytes.get(i + 3 + fim) == Some(&b'"');
            if fim == 0 || !(fecha == Some(&b'"') || fecha_com_espaco) {
                continue;
            }
            vistos.insert(format!("/{}", &resto[..fim]));
        }
        for cmd in &vistos {
            let anunciado = COMANDOS
                .iter()
                .any(|(nome, _)| nome.split(' ').next() == Some(cmd.as_str()));
            assert!(
                anunciado || APELIDOS.contains(&cmd.as_str()),
                "{cmd} existe no match e nao aparece no /help nem em APELIDOS"
            );
        }
        assert!(
            vistos.contains("/status"),
            "a varredura achou algo: {vistos:?}"
        );
    }

    fn make_llm_cfg(
        provider: &str,
        model: Option<&str>,
        api_key: Option<&str>,
        base_url: Option<&str>,
    ) -> LlmProviderConfig {
        LlmProviderConfig {
            provider: provider.to_string(),
            model: model.map(String::from),
            api_key: api_key.map(String::from),
            base_url: base_url.map(String::from),
            extra: HashMap::new(),
        }
    }

    fn config_with(entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = AppConfig::default();
        for (k, v) in entries {
            cfg.llm.insert((*k).to_string(), v.clone());
        }
        cfg
    }

    fn config_with_default(default_key: &str, entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = config_with(entries);
        cfg.agent = AgentConfig {
            default_provider: Some(default_key.to_string()),
            ..AgentConfig::default()
        };
        cfg
    }

    // ─── select_explicit_provider: echo (dev-echo-provider) ────────────

    /// Com a feature, `--provider echo` seleciona o EchoProvider keyless.
    #[cfg(feature = "dev-echo-provider")]
    #[test]
    fn select_explicit_provider_echo_needs_no_key() {
        let cfg = AppConfig::default();
        let (name, model, _provider) = select_explicit_provider(&cfg, "echo", None, None)
            .expect("echo deve ser selecionável com a feature dev-echo-provider");
        assert_eq!(name, "echo");
        assert_eq!(model, "echo-stub");
    }

    /// Sem a feature, `echo` continua sendo provider desconhecido —
    /// builds de produção não ganham o caminho keyless.
    #[cfg(not(feature = "dev-echo-provider"))]
    #[test]
    fn select_explicit_provider_echo_rejected_without_feature() {
        let cfg = AppConfig::default();
        assert!(select_explicit_provider(&cfg, "echo", None, None).is_err());
    }

    // ─── resolve_provider_model ────────────────────────────────────────

    #[test]
    fn resolve_provider_model_override_wins() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", Some("openrouter/auto"));
        assert_eq!(got.as_deref(), Some("openrouter/auto"));
    }

    #[test]
    fn resolve_provider_model_key_match() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", None);
        assert_eq!(got.as_deref(), Some("openrouter/free"));
    }

    #[test]
    fn resolve_provider_model_provider_field_match() {
        // Key name is arbitrary (`my-router`), but the `provider` field
        // matches the requested kind — the helper must still find the model.
        let cfg = config_with(&[(
            "my-router",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", None);
        assert_eq!(got.as_deref(), Some("openrouter/free"));
    }

    #[test]
    fn resolve_provider_model_no_match() {
        let cfg = AppConfig::default();
        assert!(resolve_provider_model(&cfg, "openrouter", None).is_none());
    }

    #[test]
    fn resolve_provider_model_empty_string_skipped() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some(""), Some("k"), None),
        )]);
        // Empty string in config must not be returned as a valid model.
        assert!(resolve_provider_model(&cfg, "openrouter", None).is_none());
    }

    // ─── decide_default_provider ───────────────────────────────────────

    #[test]
    fn decide_default_provider_no_default_falls_through() {
        let cfg = AppConfig::default();
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_missing_llm_key_falls_through() {
        let cfg = config_with_default("missing", &[]);
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_openrouter_wins_over_openai_env() {
        // GAR-576 regression: this is the exact scenario from the bug
        // report — operator configured OpenRouter as the default, but
        // OPENAI_API_KEY is loaded from cwd `.env` and was hijacking
        // the autodetect chain. The new branch must pick OpenRouter.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg(
                    "openrouter",
                    Some("openrouter/free"),
                    Some("test-key"),
                    None,
                ),
            )],
        );
        let decision = decide_default_provider(
            &cfg, /* env_has_openai */ true, /* env_has_openrouter */ true,
            /* env_has_anthropic */ false,
        );
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "openrouter");
                assert_eq!(provider_kind, "openrouter");
                assert_eq!(model, "openrouter/free");
            }
            other => panic!("expected UseDefault(openrouter), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_falls_through_when_no_credential() {
        // default_provider points to a kind that needs a key, but neither
        // the env nor the config supplies one — fall through to the
        // legacy chain rather than building a doomed provider.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg("openrouter", Some("openrouter/free"), None, None),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_ollama_no_credential_needed() {
        // Ollama has no api_key concept — the credential gate is the
        // async health-check inside try_build_default_provider, not
        // the decision function.
        let cfg = config_with_default(
            "ollama-local",
            &[(
                "ollama-local",
                make_llm_cfg(
                    "ollama",
                    Some("llama3.2"),
                    None,
                    Some("http://localhost:11434"),
                ),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "ollama-local");
                assert_eq!(provider_kind, "ollama");
                assert_eq!(model, "llama3.2");
            }
            other => panic!("expected UseDefault(ollama), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_openai_compat_with_base_url_accepts_no_key() {
        // LM Studio scenario: provider kind is `openai` but the
        // base_url points at a local server that does not enforce an
        // api_key. The helper must accept the config and route there.
        let cfg = config_with_default(
            "lm-studio",
            &[(
                "lm-studio",
                make_llm_cfg(
                    "openai",
                    Some("local-model"),
                    None,
                    Some("http://localhost:1234/v1"),
                ),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "lm-studio");
                assert_eq!(provider_kind, "openai");
                assert_eq!(model, "local-model");
            }
            other => panic!("expected UseDefault(openai-compat), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_uses_hardcoded_fallback_when_model_missing() {
        // Config declares the provider but no model. The helper must
        // fall back to hardcoded_default_model rather than refusing.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg("openrouter", None, Some("test-key"), None),
            )],
        );
        let decision = decide_default_provider(&cfg, false, true, false);
        match decision {
            DefaultProviderDecision::UseDefault { model, .. } => {
                assert_eq!(model, "z-ai/glm-5.3-flash");
            }
            other => panic!("expected UseDefault with hardcoded model, got {other:?}"),
        }
    }

    // ─── #1180: a ordem do autodetect ─────────────────────────────────────

    /// O caso exato do relatorio de revisao do #1180: clone novo, nenhum
    /// `agent.default_provider` no config, `OPENROUTER_API_KEY` exportada.
    /// `decide_default_provider` devolve `FallThroughToChain` e quem decide
    /// passa a ser a cadeia legada — que tentava Ollama ANTES da nuvem e
    /// contradizia a decisao 2 da issue.
    #[test]
    fn autodetect_tenta_nuvem_antes_do_ollama() {
        let cfg = AppConfig::default();
        assert!(
            matches!(
                decide_default_provider(&cfg, false, true, false),
                DefaultProviderDecision::FallThroughToChain { .. }
            ),
            "sem agent.default_provider quem decide e a cadeia legada"
        );

        let ordem = autodetect_order(false, false, true);
        assert_eq!(
            ordem,
            vec![AutodetectCandidate::OpenRouter, AutodetectCandidate::Ollama],
            "com OPENROUTER_API_KEY o openrouter tem que vir ANTES do ollama"
        );
    }

    /// A ordem relativa entre os provedores de nuvem e a que ja existia
    /// (Anthropic > OpenAI > OpenRouter) — o #1180 so moveu o Ollama para o
    /// fim, nao reembaralhou a nuvem.
    #[test]
    fn autodetect_preserva_a_ordem_entre_os_provedores_de_nuvem() {
        assert_eq!(
            autodetect_order(true, true, true),
            vec![
                AutodetectCandidate::Anthropic,
                AutodetectCandidate::OpenAi,
                AutodetectCandidate::OpenRouter,
                AutodetectCandidate::Ollama,
            ]
        );
        assert_eq!(
            autodetect_order(true, false, true),
            vec![
                AutodetectCandidate::Anthropic,
                AutodetectCandidate::OpenRouter,
                AutodetectCandidate::Ollama,
            ]
        );
    }

    /// O fallback local que a issue quer PRESERVAR: sem nenhuma credencial
    /// de nuvem, o Ollama continua sendo o que sobra — e continua sendo
    /// tentado. Ele tambem entra em toda lista, sempre por ultimo, porque e
    /// o unico provedor que nao precisa de credencial nenhuma.
    #[test]
    fn autodetect_mantem_o_ollama_como_o_que_sobra() {
        assert_eq!(
            autodetect_order(false, false, false),
            vec![AutodetectCandidate::Ollama],
            "sem credencial de nuvem o local segue sendo a saida"
        );
        for (a, o, r) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let ordem = autodetect_order(a, o, r);
            assert_eq!(
                ordem.last(),
                Some(&AutodetectCandidate::Ollama),
                "ollama tem que ser o ultimo em toda combinacao ({a},{o},{r})"
            );
            assert_eq!(
                ordem
                    .iter()
                    .filter(|c| **c == AutodetectCandidate::Ollama)
                    .count(),
                1,
                "ollama entra uma vez so"
            );
        }
    }

    /// Spec-lock for the one default-model table. Every fallback in this file
    /// routes through `hardcoded_default_model`, so this is the only place the
    /// defaults are written down — changing a row here means changing the docs
    /// and example configs in lockstep.
    #[test]
    fn hardcoded_default_model_table_is_locked() {
        assert_eq!(hardcoded_default_model("ollama"), "qwen3.8:latest");
        assert_eq!(hardcoded_default_model("llamacpp"), "default");
        assert_eq!(
            hardcoded_default_model("anthropic"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(hardcoded_default_model("openai"), "gpt-4o");
        // Issue #1180: the official default. `openrouter/auto` and
        // `openrouter/free` are explicit-only from here on.
        assert_eq!(hardcoded_default_model("openrouter"), "z-ai/glm-5.3-flash");
        assert_eq!(hardcoded_default_model("echo"), "echo-stub");
        assert_eq!(hardcoded_default_model("something-else"), "auto");
    }

    /// Issue #1180 — `chat.rs` must agree with the single source of truth
    /// in `crate::defaults`, which the wizard and the MCP server read too.
    /// Without this, the table above could be edited in isolation and the
    /// four surfaces would drift apart again.
    #[test]
    fn hardcoded_defaults_match_the_shared_constants() {
        assert_eq!(
            hardcoded_default_model(crate::defaults::DEFAULT_CLOUD_PROVIDER),
            crate::defaults::DEFAULT_CLOUD_MODEL
        );
        assert_eq!(
            hardcoded_default_model(crate::defaults::DEFAULT_LOCAL_PROVIDER),
            crate::defaults::DEFAULT_LOCAL_MODEL
        );
    }

    /// The Ollama default must be byte-identical to the provider crate's own
    /// `DEFAULT_MODEL`, or `OllamaProvider::new(None, _)` and the CLI would
    /// disagree about which model "no model specified" means.
    #[test]
    fn ollama_default_matches_the_provider_crate() {
        let provider = garraia_agents::OllamaProvider::new(None, None);
        assert_eq!(
            provider.configured_model(),
            Some(hardcoded_default_model("ollama").as_str())
        );
    }

    /// Guard for the "offer to pull gpt-4o from Ollama" trap: when config
    /// already names the model under a cloud provider, the local-Ollama path
    /// must not claim it.
    #[test]
    fn configured_cloud_models_are_not_ollama_pull_candidates() {
        let cfg = config_with_default(
            "openai",
            &[
                (
                    "openai",
                    make_llm_cfg("openai", Some("gpt-4o"), Some("k"), None),
                ),
                (
                    "local",
                    make_llm_cfg("ollama", Some("qwen3.8:latest"), None, None),
                ),
            ],
        );
        assert!(model_belongs_to_configured_cloud_provider(&cfg, "gpt-4o"));
        // An Ollama-provider entry is never a reason to skip the probe.
        assert!(!model_belongs_to_configured_cloud_provider(
            &cfg,
            "qwen3.8:latest"
        ));
        // Unknown names stay eligible.
        assert!(!model_belongs_to_configured_cloud_provider(&cfg, "qwen3.8"));
    }

    /// `select_explicit_provider` must land on the same table. Ollama is the
    /// only kind constructible without an API key, so it is the one arm that
    /// can be exercised without touching env or config.
    #[test]
    fn select_explicit_provider_uses_the_default_table_for_ollama() {
        let cfg = AppConfig::default();
        let (name, model, _) = select_explicit_provider(&cfg, "ollama", None, None)
            .expect("ollama needs no credential");
        assert_eq!(name, "ollama");
        assert_eq!(model, "qwen3.8:latest");
    }

    /// `llamacpp` é o segundo arm keyless: resolução de modelo pela mesma
    /// tabela e precedência de base_url `--url` > `config.llm["llamacpp"]` >
    /// default do provider (http://localhost:8080).
    #[test]
    fn select_explicit_provider_llamacpp_resolves_model_and_base_url_precedence() {
        // Sem nada configurado: default da tabela, default do provider.
        let (name, model, provider) =
            select_explicit_provider(&AppConfig::default(), "llamacpp", None, None)
                .expect("llamacpp é keyless");
        assert_eq!(name, "llamacpp");
        assert_eq!(model, "default");
        assert_eq!(provider.provider_id(), "llama-cpp");

        // `--model` vence a config.
        let cfg = config_with_default(
            "llamacpp",
            &[(
                "llamacpp",
                make_llm_cfg("llamacpp", Some("qwen3-8b"), None, Some("http://pc:8080")),
            )],
        );
        let (name, model, _) =
            select_explicit_provider(&cfg, "llamacpp", Some("custom"), None).unwrap();
        assert_eq!(name, "llamacpp");
        assert_eq!(model, "custom");

        // A precedência do base_url (`--url` > config > default) é afirmada
        // com pedido de verdade em
        // `llamacpp_url_flag_beats_the_config_base_url_and_empty_flag_keeps_it`.
    }

    /// `--url` vence a base_url da config; `--url` vazio conta como ausente
    /// (não apaga a config). Afirmado por onde o pedido CHEGA, não por uma
    /// função auxiliar que o provider poderia deixar de usar.
    #[tokio::test]
    async fn llamacpp_url_flag_beats_the_config_base_url_and_empty_flag_keeps_it() {
        use crate::provider_binding::mock_endpoint::MockEndpoint;
        for (flag_aponta_para_b, flag_vazia) in [(false, false), (true, false), (false, true)] {
            let a = MockEndpoint::start().await;
            let b = MockEndpoint::start().await;
            let cfg = config_with(&[(
                "llamacpp",
                make_llm_cfg("llamacpp", Some("m"), None, Some(&a.uri())),
            )]);
            let flag = if flag_aponta_para_b {
                Some(b.uri())
            } else if flag_vazia {
                Some(String::new())
            } else {
                None
            };
            let (_, _, provider) =
                select_explicit_provider(&cfg, "llamacpp", None, flag.as_deref()).unwrap();
            texto_da_chamada(&provider).await;
            let (esperado, outro) = if flag_aponta_para_b {
                (&b, &a)
            } else {
                (&a, &b)
            };
            assert_eq!(esperado.paths().await.len(), 1, "flag={flag:?}");
            assert!(outro.paths().await.is_empty(), "flag={flag:?}");
        }
    }

    /// Provider desconhecido lista `llamacpp` junto dos demais no bail —
    /// o contrato de mensagem é o que os testes E2E de onboarding citam.
    #[test]
    fn select_explicit_provider_unknown_lists_all_keyless_and_cloud_kinds() {
        // `expect_err` exige `T: Debug` e o trio de retorno carrega um
        // `Arc<dyn LlmProvider>` sem Debug — casar com `let Err` direto.
        let Err(err) = select_explicit_provider(&AppConfig::default(), "fogos", None, None) else {
            panic!("provider desconhecido deve falhar");
        };
        let msg = format!("{err}");
        for kind in ["ollama", "llamacpp", "anthropic", "openai", "openrouter"] {
            assert!(msg.contains(kind), "mensagem `{msg}` não cita `{kind}`");
        }
    }

    // ─── base_url + credencial da MESMA entrada (smoke v0.4.4, O1) ──────

    fn pedido_minimo() -> garraia_agents::LlmRequest {
        garraia_agents::LlmRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("oi".to_string()),
            }],
            system: None,
            max_tokens: Some(16),
            temperature: None,
            tools: Vec::new(),
        }
    }

    /// Uma chamada de verdade; o texto so traz o SENTINEL se o pedido chegou
    /// ao endpoint falso — o host padrao do provider nao o conhece.
    async fn texto_da_chamada(provider: &Arc<dyn LlmProvider>) -> String {
        let resposta = provider
            .complete(&pedido_minimo())
            .await
            .unwrap_or_else(|e| panic!("chamada ao provider falhou: {e}"));
        resposta
            .content
            .iter()
            .filter_map(|b| match b {
                garraia_agents::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// `-p <provider>` com `base_url` configurada: para CADA provider
    /// explicito, o pedido chega na base_url configurada, com a chave daquela
    /// entrada, e a resposta vem de la (nada vai ao host padrao). Era o
    /// `ask_explicit.out` do smoke: `-p openai` mandava `llm.openai.api_key`
    /// para https://api.openai.com.
    #[tokio::test]
    async fn explicit_provider_calls_the_configured_base_url_for_every_kind() {
        use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        // (nome passado em -p, tipo da entrada, chave, sufixo da base, caminho esperado)
        let rows: [(&str, &str, Option<&str>, &str, &str); 6] = [
            (
                "openai",
                "openai",
                Some("k-openai"),
                "/v1",
                "/v1/chat/completions",
            ),
            (
                "openrouter",
                "openrouter",
                Some("k-openrouter"),
                "/api/v1",
                "/api/v1/chat/completions",
            ),
            (
                "anthropic",
                "anthropic",
                Some("k-anthropic"),
                "",
                "/v1/messages",
            ),
            ("ollama", "ollama", None, "", "/api/chat"),
            ("llamacpp", "llamacpp", None, "", "/v1/chat/completions"),
            // Alias em llm: (aceito pela policy do MCP): usa o tipo declarado.
            (
                "lmstudio",
                "openai",
                Some("k-lmstudio"),
                "/v1",
                "/v1/chat/completions",
            ),
        ];
        for (name, kind, key, suffix, expected_path) in rows {
            let mock = MockEndpoint::start().await;
            let base = format!("{}{suffix}", mock.uri());
            let cfg = config_with(&[(name, make_llm_cfg(kind, Some("m"), key, Some(&base)))]);
            let (display, model, provider) = select_explicit_provider(&cfg, name, None, None)
                .unwrap_or_else(|e| panic!("-p {name}: {e:#}"));
            assert_eq!(display, name);
            assert_eq!(model, "m", "-p {name}: modelo da propria entrada");
            let texto = texto_da_chamada(&provider).await;
            assert_eq!(
                texto, SENTINEL,
                "-p {name}: resposta nao veio da base_url configurada"
            );
            assert_eq!(
                mock.paths().await,
                vec![expected_path.to_string()],
                "-p {name}"
            );
            if let Some(key) = key {
                assert_eq!(mock.credentials().await, vec![key.to_string()], "-p {name}");
            }
        }
    }

    /// Negativo: a chave da entrada A nunca vai para o endpoint da entrada B,
    /// nem pelo caminho explicito nem pelo do `agent.default_provider`.
    #[tokio::test]
    async fn key_of_entry_a_is_never_sent_to_entry_b() {
        use crate::provider_binding::mock_endpoint::MockEndpoint;
        let a = MockEndpoint::start().await;
        let b = MockEndpoint::start().await;
        let entries = [
            (
                "openai",
                make_llm_cfg(
                    "openai",
                    Some("m"),
                    Some("chave-a"),
                    Some(&format!("{}/v1", a.uri())),
                ),
            ),
            (
                "lmstudio",
                make_llm_cfg(
                    "openai",
                    Some("m"),
                    Some("chave-b"),
                    Some(&format!("{}/v1", b.uri())),
                ),
            ),
        ];

        // -p lmstudio → so B, so com a chave de B.
        let cfg = config_with(&entries);
        let (_, _, p) = select_explicit_provider(&cfg, "lmstudio", None, None)
            .unwrap_or_else(|e| panic!("{e:#}"));
        texto_da_chamada(&p).await;
        // -p openai → so A, so com a chave de A.
        let (_, _, p) = select_explicit_provider(&cfg, "openai", None, None)
            .unwrap_or_else(|e| panic!("{e:#}"));
        texto_da_chamada(&p).await;
        assert_eq!(a.credentials().await, vec!["chave-a".to_string()]);
        assert_eq!(b.credentials().await, vec!["chave-b".to_string()]);

        // Caminho do default_provider: `default_provider: lmstudio` com um
        // `llm.openai` ao lado. Antes, a chave de `llm.openai` ia para B.
        let a = MockEndpoint::start().await;
        let b = MockEndpoint::start().await;
        let cfg = config_with_default(
            "lmstudio",
            &[
                (
                    "openai",
                    make_llm_cfg(
                        "openai",
                        Some("m"),
                        Some("chave-a"),
                        Some(&format!("{}/v1", a.uri())),
                    ),
                ),
                (
                    "lmstudio",
                    make_llm_cfg(
                        "openai",
                        Some("m"),
                        Some("chave-b"),
                        Some(&format!("{}/v1", b.uri())),
                    ),
                ),
            ],
        );
        let entry = cfg.llm.get("lmstudio").cloned().expect("entrada");
        let p = try_build_default_provider("lmstudio", &entry, "m", &|_: &str| None)
            .await
            .expect("default provider construivel");
        texto_da_chamada(&p).await;
        assert!(a.credentials().await.is_empty(), "A nao pode receber nada");
        assert_eq!(b.credentials().await, vec!["chave-b".to_string()]);
    }

    /// Autodetect (sem `agent.default_provider`): cada nuvem entra no
    /// encadeamento vinculada INTEIRA. Antes o encadeamento pegava so a chave
    /// de `llm.<tipo>` e a mandava para o host padrao, largando a `base_url`.
    #[tokio::test]
    async fn autodetect_chain_calls_the_entry_base_url_not_the_default_host() {
        use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        let sem_env = |_: &str| None;
        for (kind, suffix, expected_path) in [
            ("anthropic", "", "/v1/messages"),
            ("openai", "/v1", "/v1/chat/completions"),
            ("openrouter", "/api/v1", "/api/v1/chat/completions"),
        ] {
            let mock = MockEndpoint::start().await;
            let base = format!("{}{suffix}", mock.uri());
            let cfg = config_with(&[(
                kind,
                make_llm_cfg(kind, Some("m"), Some("k-proxy"), Some(&base)),
            )]);
            assert!(
                cfg.agent.default_provider.is_none(),
                "pre-condicao: autodetect"
            );
            let (name, model, provider) =
                detect_provider_with_env(&cfg, None, None, false, &sem_env).await;
            assert_eq!((name.as_str(), model.as_str()), (kind, "m"));
            assert_eq!(texto_da_chamada(&provider).await, SENTINEL, "{kind}");
            assert_eq!(
                mock.paths().await,
                vec![expected_path.to_string()],
                "{kind}"
            );
            assert_eq!(
                mock.credentials().await,
                vec!["k-proxy".to_string()],
                "{kind}"
            );
        }
    }

    /// Caminho do `agent.default_provider` por `detect_provider`: a entrada
    /// padrao traz endpoint, chave E modelo — nem a chave do `llm.openai` ao
    /// lado, nem uma `OPENAI_API_KEY` velha do ambiente, nem o modelo do
    /// `llm.openai`.
    #[tokio::test]
    async fn default_provider_path_uses_only_its_own_entry() {
        use crate::provider_binding::mock_endpoint::MockEndpoint;
        let a = MockEndpoint::start().await;
        let b = MockEndpoint::start().await;
        let cfg = config_with_default(
            "lmstudio",
            &[
                (
                    "openai",
                    make_llm_cfg(
                        "openai",
                        Some("modelo-a"),
                        Some("chave-a"),
                        Some(&format!("{}/v1", a.uri())),
                    ),
                ),
                (
                    "lmstudio",
                    make_llm_cfg(
                        "openai",
                        Some("modelo-b"),
                        Some("chave-b"),
                        Some(&format!("{}/v1", b.uri())),
                    ),
                ),
            ],
        );
        let env = |var: &str| (var == "OPENAI_API_KEY").then(|| "sk-velha-do-env".to_string());
        let (name, model, provider) = detect_provider_with_env(&cfg, None, None, false, &env).await;
        assert_eq!((name.as_str(), model.as_str()), ("lmstudio", "modelo-b"));
        texto_da_chamada(&provider).await;
        assert!(a.paths().await.is_empty(), "A nao pode receber nada");
        assert_eq!(b.credentials().await, vec!["chave-b".to_string()]);
    }

    /// `--url` avulso: a `OPENAI_API_KEY` e a `GARRAIA_EMBEDDING_API_KEY` sao
    /// credenciais de outros endpoints e nunca vao para o endereco digitado.
    #[tokio::test]
    async fn url_flag_never_forwards_the_openai_or_embedding_key() {
        use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        let mock = MockEndpoint::start().await;
        let env = |var: &str| match var {
            "OPENAI_API_KEY" => Some("sk-da-openai".to_string()),
            "GARRAIA_EMBEDDING_API_KEY" => Some("k-embeddings".to_string()),
            _ => None,
        };
        let url = format!("{}/v1", mock.uri());
        let (_, _, provider) =
            detect_provider_with_env(&AppConfig::default(), Some(&url), Some("m"), false, &env)
                .await;
        assert_eq!(texto_da_chamada(&provider).await, SENTINEL);
        assert_eq!(
            mock.credentials().await,
            vec![provider_binding::KEYLESS_PLACEHOLDER.to_string()]
        );
    }

    /// Dentro de uma entrada, a chave dela vence a variavel de ambiente. E a
    /// variavel NUNCA preenche uma entrada que aponta para o proprio
    /// endpoint: `llm.openai { base_url: <proxy> }` sem `api_key`, com uma
    /// `OPENAI_API_KEY` velha no ambiente (ou no `.env` do diretorio
    /// corrente), manda o marcador de "sem chave" ao proxy — a chave da API
    /// da OpenAI so vai para a API da OpenAI (achado do verificador).
    #[tokio::test]
    async fn explicit_entry_key_beats_a_stale_env_key() {
        use crate::provider_binding::mock_endpoint::MockEndpoint;
        let env = |var: &str| (var == "OPENAI_API_KEY").then(|| "sk-velha".to_string());
        for (entry_key, expected) in [
            (Some("da-entrada"), "da-entrada"),
            (None, provider_binding::KEYLESS_PLACEHOLDER),
        ] {
            let mock = MockEndpoint::start().await;
            let cfg = config_with(&[(
                "openai",
                make_llm_cfg(
                    "openai",
                    Some("m"),
                    entry_key,
                    Some(&format!("{}/v1", mock.uri())),
                ),
            )]);
            let (_, _, p) = select_explicit_provider_with_env(&cfg, "openai", None, None, &env)
                .unwrap_or_else(|e| panic!("{e:#}"));
            texto_da_chamada(&p).await;
            assert_eq!(mock.credentials().await, vec![expected.to_string()]);
        }
    }

    /// O mesmo achado pelo caminho do `agent.default_provider` (onde, antes
    /// deste branch, a `OPENAI_API_KEY` ja ia para a `base_url` da entrada
    /// padrao) e pela autodeteccao: a variavel do ambiente nunca chega na
    /// `base_url` propria de uma entrada sem chave.
    #[tokio::test]
    async fn env_key_never_reaches_an_entry_base_url_on_default_or_autodetect() {
        use crate::provider_binding::mock_endpoint::MockEndpoint;
        let env = |var: &str| match var {
            "OPENAI_API_KEY" => Some("sk-do-env".to_string()),
            "ANTHROPIC_API_KEY" => Some("sk-ant-do-env".to_string()),
            "OPENROUTER_API_KEY" => Some("sk-or-do-env".to_string()),
            _ => None,
        };
        let do_env = ["sk-do-env", "sk-ant-do-env", "sk-or-do-env"];

        // default_provider: openai → o proxy recebe o marcador.
        let mock = MockEndpoint::start().await;
        let cfg = config_with_default(
            "openai",
            &[(
                "openai",
                make_llm_cfg(
                    "openai",
                    Some("m"),
                    None,
                    Some(&format!("{}/v1", mock.uri())),
                ),
            )],
        );
        let (name, _, provider) = detect_provider_with_env(&cfg, None, None, false, &env).await;
        assert_eq!(name, "openai");
        texto_da_chamada(&provider).await;
        assert_eq!(
            mock.credentials().await,
            vec![provider_binding::KEYLESS_PLACEHOLDER.to_string()]
        );

        // default_provider anthropic/openrouter sem chave propria, apontando
        // para o proxy: a decisao nao conta a env, e nada chega ao proxy
        // (nem por esse caminho nem pela autodeteccao que vem depois).
        for kind in ["anthropic", "openrouter"] {
            let mock = MockEndpoint::start().await;
            let cfg = config_with_default(
                kind,
                &[(kind, make_llm_cfg(kind, Some("m"), None, Some(&mock.uri())))],
            );
            assert!(
                matches!(
                    decide_default_provider(&cfg, true, true, true),
                    DefaultProviderDecision::FallThroughToChain { .. }
                ),
                "{kind}: a env nao e credencial para a base_url da entrada"
            );
            let (name, _, _) = detect_provider_with_env(&cfg, None, None, false, &env).await;
            assert_ne!(name, kind, "{kind}: nao ha credencial para o proxy");
            assert!(
                mock.paths().await.is_empty(),
                "{kind}: o proxy recebeu pedido"
            );
        }

        // Autodeteccao: `llm.openai` sem chave apontando para o proxy.
        let mock = MockEndpoint::start().await;
        let cfg = config_with(&[(
            "openai",
            make_llm_cfg(
                "openai",
                Some("m"),
                None,
                Some(&format!("{}/v1", mock.uri())),
            ),
        )]);
        let only_openai_env =
            |var: &str| (var == "OPENAI_API_KEY").then(|| "sk-do-env".to_string());
        let (name, _, _) =
            detect_provider_with_env(&cfg, None, None, false, &only_openai_env).await;
        assert_ne!(name, "openai", "sem chave propria o proxy nao e candidato");
        let vistas = mock.credentials().await;
        assert!(
            !vistas.iter().any(|c| do_env.contains(&c.as_str())),
            "a env chegou no proxy: {vistas:?}"
        );
    }

    /// Achado do verificador (LOW): `llm.openrouter` declarando
    /// `provider: openai`, com chave propria e sem `agent.default_provider`,
    /// era autodetectado como OpenRouter antes deste branch; o
    /// `provider_binding` descartava o candidato e caia no Ollama. Volta a
    /// ser o OpenRouter, vinculado inteiro: a chave dele, a base_url dele.
    #[tokio::test]
    async fn autodetect_keeps_a_candidate_whose_entry_declares_another_kind() {
        use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        let mock = MockEndpoint::start().await;
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg(
                "openai",
                Some("m"),
                Some("k-or"),
                Some(&format!("{}/api/v1", mock.uri())),
            ),
        )]);
        let (name, model, provider) =
            detect_provider_with_env(&cfg, None, None, false, &|_| None).await;
        assert_eq!((name.as_str(), model.as_str()), ("openrouter", "m"));
        // Registrado com o nome da entrada, como um alias no caminho explicito.
        assert_eq!(provider.provider_id(), "openrouter");
        assert_eq!(texto_da_chamada(&provider).await, SENTINEL);
        assert_eq!(
            mock.paths().await,
            vec!["/api/v1/chat/completions".to_string()]
        );
        assert_eq!(mock.credentials().await, vec!["k-or".to_string()]);
    }

    /// O fallback legado `llm.main`: `-p anthropic` com a `llm.main` do tipo
    /// vai para a base_url DELA. Antes so a chave da `llm.main` era usada, e
    /// ia para https://api.anthropic.com.
    #[tokio::test]
    async fn main_entry_fallback_keeps_its_base_url() {
        use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
        let mock = MockEndpoint::start().await;
        let cfg = config_with(&[(
            "main",
            make_llm_cfg("anthropic", Some("m"), Some("k-main"), Some(&mock.uri())),
        )]);
        let (_, _, p) = select_explicit_provider_with_env(&cfg, "anthropic", None, None, &|_| None)
            .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(texto_da_chamada(&p).await, SENTINEL);
        assert_eq!(mock.credentials().await, vec!["k-main".to_string()]);
    }

    /// Sem entrada nenhuma para o tipo, `-p openai` nao pega a chave de outra
    /// entrada do mesmo tipo (ela iria para api.openai.com): falha fechado.
    #[test]
    fn explicit_kind_without_entry_never_borrows_another_entrys_key() {
        let cfg = config_with(&[(
            "lmstudio",
            make_llm_cfg(
                "openai",
                Some("m"),
                Some("k-lm"),
                Some("http://127.0.0.1:1/v1"),
            ),
        )]);
        let Err(err) = select_explicit_provider_with_env(&cfg, "openai", None, None, &|_| None)
        else {
            panic!("sem chave para api.openai.com, deve falhar");
        };
        assert!(format!("{err}").contains("OPENAI_API_KEY"), "{err}");
    }

    #[test]
    fn decide_default_provider_prefers_the_default_entrys_own_model() {
        let cfg = config_with_default(
            "lmstudio",
            &[
                (
                    "openai",
                    make_llm_cfg("openai", Some("gpt-4o"), Some("k"), None),
                ),
                (
                    "lmstudio",
                    make_llm_cfg(
                        "openai",
                        Some("local-model"),
                        None,
                        Some("http://127.0.0.1:1234/v1"),
                    ),
                ),
            ],
        );
        match decide_default_provider(&cfg, false, false, false) {
            DefaultProviderDecision::UseDefault {
                config_key, model, ..
            } => {
                assert_eq!(config_key, "lmstudio");
                assert_eq!(model, "local-model");
            }
            other => panic!("esperava UseDefault(lmstudio), veio {other:?}"),
        }
    }

    // ─── stream_turn (regression: bounded-channel deadlock, GAR chat hang) ─

    /// A producer that pushes far more deltas than the channel capacity used
    /// to deadlock forever: the old REPL only drained AFTER the call
    /// completed, so `send().await` wedged once the buffer filled. The
    /// 5s outer timeout turns a regression into a failure instead of a hang.
    #[tokio::test]
    async fn stream_turn_drains_concurrently_without_deadlock() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(2);
        let call = async move {
            for i in 0..300 {
                tx.send(TurnEvent::TextDelta(format!("d{i} ")))
                    .await
                    .map_err(|_| "receiver dropped")?;
            }
            Ok::<String, &'static str>("full".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        // Com o spinner LIGADO de propósito: o braço extra do `select!` não
        // pode roubar a vez da drenagem e re-introduzir o deadlock original.
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_secs(30),
                &mut out,
                &mut test_renderer(test_spinner()),
                &mut ToolLog::new(),
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("stream_turn must not deadlock with >capacity deltas");

        assert_eq!(outcome, TurnOutcome::Done(Ok("full".to_string())));
        let printed = strip_spinner(&String::from_utf8(out).expect("utf8"));
        assert!(printed.starts_with("d0 "));
        assert!(printed.ends_with("d299 "));
        // Separadores, e nao espacos: com a quebra por largura do #939 o
        // espaco que separa duas palavras vira `\n` quando a linha enche. O
        // que este teste mede e que os 300 deltas chegaram — e sao 300
        // separadores de um jeito ou de outro.
        assert_eq!(printed.matches(char::is_whitespace).count(), 300);
        for i in 0..300 {
            assert!(printed.contains(&format!("d{i}")), "faltou o delta d{i}");
        }
    }

    /// On timeout the call future must be dropped (closing the sender) and
    /// already-buffered deltas must still be flushed before returning None.
    #[tokio::test]
    async fn stream_turn_times_out_and_flushes_buffered_deltas() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            tx.send(TurnEvent::TextDelta("partial ".to_string()))
                .await
                .map_err(|_| "receiver dropped")?;
            // Never completes: simulates a stalled provider/SSE stream.
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_millis(50),
                &mut out,
                &mut test_renderer(None),
                &mut ToolLog::new(),
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("timeout path must not hang");

        assert_eq!(outcome, TurnOutcome::TimedOut);
        assert_eq!(String::from_utf8(out).expect("utf8"), "partial ");
    }

    // ---- Ajuda para os testes do indicador de atividade -------------------

    /// Spinner determinístico para teste: estilo ASCII (comparável byte a byte)
    /// e largura fixa, sem depender do terminal do runner de CI.
    fn test_spinner() -> Option<crate::ui::Spinner> {
        Some(crate::ui::Spinner::new(
            crate::ui::SpinnerStyle::Ascii,
            80,
            0,
        ))
    }

    /// Capacidades fixas para teste, no mesmo espírito do `test_spinner`:
    /// ASCII e largura 80, sem perguntar nada ao terminal do runner.
    fn test_caps() -> crate::ui::Capabilities {
        crate::ui::Capabilities {
            interactive: true,
            unicode: false,
            animation: true,
            width: 80,
        }
    }

    /// A ponte `TurnEvent` -> `UiEvent` no caminho real do `stream_turn`
    /// (#937): o runtime conta, o renderer desenha, e a ordem entre texto e
    /// ferramenta e a ordem de emissao — que e o motivo de haver um canal so.
    #[tokio::test]
    async fn stream_turn_desenha_texto_e_ferramenta_na_ordem_emitida() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            tx.send(TurnEvent::TextDelta("vou rodar os testes. ".into()))
                .await
                .map_err(|_| "receiver dropped")?;
            tx.send(TurnEvent::ToolStarted {
                name: "bash".into(),
                detail: "cargo test".into(),
            })
            .await
            .map_err(|_| "receiver dropped")?;
            tx.send(TurnEvent::ToolFinished {
                name: "bash".into(),
                duration: std::time::Duration::from_millis(6300),
                success: true,
                summary: "148 passed".into(),
                output: String::new(),
            })
            .await
            .map_err(|_| "receiver dropped")?;
            tx.send(TurnEvent::TextDelta("passou tudo.".into()))
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("pronto".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_secs(5),
                &mut out,
                &mut TerminalRenderer::with_prefix(crate::ui::Capabilities::PLAIN, None, ""),
                &mut ToolLog::new(),
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("stream_turn nao pode travar");

        assert_eq!(outcome, TurnOutcome::Done(Ok("pronto".to_string())));
        // O `| #0` no fim da linha e o ponteiro do #938: a saida inteira
        // ficou guardada e este e o numero que o usuario digita no `/tool`.
        // Afirma-lo aqui, no caminho real do `stream_turn`, e o que prova que
        // o indice atravessa a ponte `TurnEvent` -> `ToolLog` -> `UiEvent`.
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "vou rodar os testes. * Bash cargo test\n  |- 148 passed | 6.3s | #0\npassou tudo."
        );
    }

    /// Renderer sem rótulo: estes testes afirmam o texto do modelo e os
    /// quadros da animação, não o `Garra` — que tem teste próprio em `ui`.
    fn test_renderer(spinner: Option<crate::ui::Spinner>) -> TerminalRenderer {
        TerminalRenderer::with_prefix(test_caps(), spinner, "")
    }

    /// Remove as sequências ANSI de `raw`, preservando os `\r`.
    fn strip_ansi(raw: &str) -> String {
        let mut out = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Consome o CSI inteiro (ESC '[' ... letra final).
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for t in chars.by_ref() {
                        if t.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    /// Devolve só o que o modelo escreveu, descartando as linhas do spinner.
    ///
    /// Cada quadro é reescrito a partir de um `\r`, então basta quebrar por
    /// `\r` e jogar fora os segmentos que começam com um desenho da garra.
    /// Descartar *segmentos inteiros* (em vez de só o glifo) é o que torna a
    /// asserção estável: o texto da mensagem — "Afiando as garras..." — sai
    /// junto, e um tick que ganhe a corrida do `select!` contra o primeiro
    /// delta não vira teste intermitente.
    fn strip_spinner(raw: &str) -> String {
        strip_ansi(raw)
            .split('\r')
            .filter(|segment| !ASCII_FRAMES.iter().any(|f| segment.starts_with(f)))
            .collect::<Vec<_>>()
            .join("")
    }

    /// Os quadros do estilo ASCII, que é o usado por `test_spinner`.
    const ASCII_FRAMES: [&str; 6] = ["<   >", "</  >", "<// >", "<///>", "< //>", "<  />"];

    /// Qualquer quadro da animação presente no texto cru?
    fn contains_spinner_frame(raw: &str) -> bool {
        ASCII_FRAMES.iter().any(|f| raw.contains(f))
    }

    /// O indicador tem de aparecer ANTES do primeiro token, que é exatamente a
    /// janela em que o REPL parecia congelado no `garra >`.
    ///
    /// Tempo virtual (`start_paused`): a janela de aparição de ~270ms (#936)
    /// torna o real-time frouxo demais — 30ms de margem num runner carregado
    /// é flake garantido; pausado, cada tick cai no instante exato.
    #[tokio::test(start_paused = true)]
    async fn spinner_runs_before_the_first_token() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            // Latência de provedor: vários quadros cabem aqui antes do 1o token.
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            tx.send(TurnEvent::TextDelta("resposta".to_string()))
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("resposta".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;

        assert_eq!(outcome, TurnOutcome::Done(Ok("resposta".to_string())));
        let raw = String::from_utf8(out).expect("utf8");
        assert!(
            contains_spinner_frame(&raw),
            "nenhum quadro desenhado durante a espera: {raw:?}"
        );
        let first_frame = raw.find("<").expect("quadro presente");
        let first_token = raw.find("resposta").expect("token presente");
        assert!(
            first_frame < first_token,
            "o spinner tem de vir antes do primeiro token"
        );
    }

    /// A linha do spinner é apagada antes do texto, e nenhum quadro sobrevive
    /// depois que a resposta começa a sair.
    #[tokio::test(start_paused = true)]
    async fn spinner_is_cleared_before_streamed_output() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            tx.send(TurnEvent::TextDelta("alpha ".to_string()))
                .await
                .map_err(|_| "receiver dropped")?;
            tx.send(TurnEvent::TextDelta("beta".to_string()))
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("alpha beta".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;

        let raw = String::from_utf8(out).expect("utf8");
        let clear_at = raw.find("\r\x1b[2K").expect("sequência de limpeza emitida");
        let token_at = raw.find("alpha").expect("token presente");
        assert!(clear_at < token_at, "limpa a linha antes de escrever");

        // Depois do primeiro token não pode haver mais nenhum quadro: a linha
        // agora pertence à resposta e um quadro colidiria com ela.
        assert!(
            !contains_spinner_frame(&raw[token_at..]),
            "quadro desenhado depois do primeiro token: {:?}",
            &raw[token_at..]
        );

        // O texto do modelo sai íntegro e o rótulo aparece exatamente uma vez.
        let cleaned = strip_spinner(&raw);
        assert!(cleaned.contains("garra > alpha beta"), "saiu: {cleaned:?}");
        assert_eq!(cleaned.matches("garra >").count(), 1);
    }

    /// Erro do provedor precisa limpar a animação — nada de spinner órfão.
    #[tokio::test(start_paused = true)]
    async fn spinner_is_cleaned_up_on_provider_error() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            drop(tx);
            Err::<String, &'static str>("provider exploded")
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;

        assert_eq!(outcome, TurnOutcome::Done(Err("provider exploded")));
        let raw = String::from_utf8(out).expect("utf8");
        assert!(raw.contains("\r\x1b[2K"), "a linha tem de ser apagada");
        assert!(
            raw.ends_with("\r\x1b[2Kgarra > "),
            "limpa a linha e ainda assim emite o rótulo: {raw:?}"
        );
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
    }

    /// Timeout também limpa.
    #[tokio::test(start_paused = true)]
    async fn spinner_is_cleaned_up_on_timeout() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            let _keep = tx;
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_millis(700),
                &mut out,
                &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
                &mut ToolLog::new(),
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("timeout path must not hang");

        assert_eq!(outcome, TurnOutcome::TimedOut);
        let raw = String::from_utf8(out).expect("utf8");
        assert!(contains_spinner_frame(&raw), "girou durante a espera");
        assert!(raw.contains("\r\x1b[2K"), "limpou ao estourar o tempo");
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
    }

    /// Ctrl+C durante a espera: cancela o turno, limpa a animação e devolve o
    /// prompt — sem matar o processo e sem deixar o cursor escondido.
    #[tokio::test(start_paused = true)]
    async fn cancellation_stops_and_cleans_up_the_spinner() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            let _keep = tx;
            // Provedor que nunca responde: só o cancelamento tira a gente daqui.
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = std::sync::Arc::clone(&cancel);
        tokio::spawn(async move {
            // Tempo para alguns quadros aparecerem antes do "Ctrl+C".
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            signal.notify_waiters();
        });

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_secs(30),
                &mut out,
                &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
                &mut ToolLog::new(),
                &cancel,
            ),
        )
        .await
        .expect("cancelamento não pode travar");

        assert_eq!(outcome, TurnOutcome::Cancelled);
        let raw = String::from_utf8(out).expect("utf8");
        assert!(contains_spinner_frame(&raw), "girou antes de cancelar");
        assert!(raw.contains("\r\x1b[2K"), "limpou a linha ao cancelar");
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
        assert!(raw.ends_with("\r\x1b[2Kgarra > "), "termina limpo: {raw:?}");
    }

    /// Superfície não-TTY (stdout redirecionado / pipe): `detect` devolve
    /// `None` e a saída tem de ser byte a byte igual à de antes do spinner.
    #[tokio::test]
    async fn non_tty_output_contains_no_animation_frames() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            tx.send(TurnEvent::TextDelta("resposta limpa".to_string()))
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("resposta limpa".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            // exatamente o que `Capabilities::detect` devolve num pipe: sem animacao
            &mut TerminalRenderer::with_prefix(crate::ui::Capabilities::PLAIN, None, "garra > "),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;

        let raw = String::from_utf8(out).expect("utf8");
        assert_eq!(raw, "garra > resposta limpa");
        assert!(!contains_spinner_frame(&raw));
        assert!(
            !raw.contains('\r'),
            "sem carriage return em saída redirecionada"
        );
        assert!(
            !raw.contains('\x1b'),
            "sem escape ANSI em saída redirecionada"
        );
    }

    /// O rótulo sai uma única vez mesmo com muitos deltas — regressão contra
    /// duplicação de linha quando o provedor entrega token a token.
    #[tokio::test]
    async fn prefix_is_written_exactly_once_across_many_deltas() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(4);
        let call = async move {
            for i in 0..50 {
                tx.send(TurnEvent::TextDelta(format!("t{i}")))
                    .await
                    .map_err(|_| "receiver dropped")?;
            }
            Ok::<String, &'static str>("done".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            &mut TerminalRenderer::with_prefix(test_caps(), test_spinner(), "garra > "),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;

        let cleaned = strip_spinner(&String::from_utf8(out).expect("utf8"));
        assert_eq!(cleaned.matches("garra >").count(), 1);
        assert!(cleaned.contains("garra > t0t1t2"));
    }

    /// Errors from the call are passed through untouched.
    #[tokio::test]
    async fn stream_turn_propagates_call_error() {
        let (tx, rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let call = async move {
            drop(tx);
            Err::<String, &'static str>("provider exploded")
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            &mut test_renderer(None),
            &mut ToolLog::new(),
            &tokio::sync::Notify::new(),
        )
        .await;
        assert_eq!(outcome, TurnOutcome::Done(Err("provider exploded")));
        assert!(out.is_empty());
    }
}

#[cfg(test)]
mod cli_tools_tests {
    use super::*;

    /// #1036: a CLI registra o conjunto do gateway mais `git_diff`; sem chave
    /// do Brave nao ha `web_search`, sem provider nao ha `code_review`.
    #[test]
    fn registers_the_gateway_tool_set_plus_git_diff() {
        let runtime = AgentRuntime::new();
        register_cli_tools(&runtime, &AppConfig::default(), None, None, vec![]);
        let names = runtime.tool_names();
        for expected in [
            "file_read",
            "file_write",
            "bash",
            "git_diff",
            "list_dir",
            "repo_search",
            "run_tests",
            "web_fetch",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} ausente em {names:?}"
            );
        }
        assert!(
            !names.iter().any(|n| n == "web_search"),
            "sem chave do Brave nao ha web_search: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n == "code_review"),
            "sem provider nao ha code_review: {names:?}"
        );
    }

    #[test]
    fn brave_key_turns_web_search_on() {
        let runtime = AgentRuntime::new();
        register_cli_tools(
            &runtime,
            &AppConfig::default(),
            None,
            Some("k".into()),
            vec![],
        );
        assert!(runtime.tool_names().iter().any(|n| n == "web_search"));
    }

    /// O prompt descreve exatamente o que esta registrado — nem mais, nem
    /// menos. E o que impede a lista escrita a mao de voltar a mentir.
    #[test]
    fn prompt_lists_every_registered_tool_and_nothing_else() {
        let runtime = AgentRuntime::new();
        register_cli_tools(&runtime, &AppConfig::default(), None, None, vec![]);
        let names = runtime.tool_names();
        let doc = tool_docs(&names);
        for n in &names {
            assert!(doc.contains(&format!("**{n}**")), "{n} sem linha no prompt");
        }
        assert!(!doc.contains("web_search"));
        assert_eq!(doc.matches("- **").count(), names.len());
    }
}

#[cfg(test)]
mod persist_tests {
    //! #1088 — o `garra chat` so toca o disco quando `--persist`/`--resume`
    //! pede. Nenhum teste aqui fala com LLM: o store e `in_memory()`, ou um
    //! diretorio temporario quando o que esta em jogo e o arquivo em si.

    use super::*;

    fn config_no_dir(dir: &std::path::Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..AppConfig::default()
        }
    }

    fn papel(m: &ChatMessage) -> &'static str {
        match m.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            _ => "outro",
        }
    }

    fn textos(h: &[ChatMessage]) -> Vec<&str> {
        h.iter()
            .filter_map(|m| match &m.content {
                MessagePart::Text(t) => Some(t.as_str()),
                MessagePart::Parts(_) => None,
            })
            .collect()
    }

    /// O padrao, preso por teste: nenhum store e nenhum arquivo. E o que o
    /// chat sempre fez, e o motivo de a persistencia ser opt-in em vez de
    /// ligada para todo mundo.
    #[test]
    fn sem_flag_nao_abre_store_nem_cria_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_chat_store(&config_no_dir(dir.path()), false, None).expect("sem erro");
        assert!(store.is_none(), "sem flag nao ha store");
        assert!(
            !dir.path().join(SESSIONS_DB).exists(),
            "nenhum banco deveria ter sido criado"
        );
        assert!(
            dir.path().read_dir().expect("le o dir").next().is_none(),
            "o diretorio de dados nao deveria ganhar nada"
        );
    }

    /// Cada flag basta sozinha: `--persist` e `--resume` abrem o store, e o
    /// `--persist` cria o banco no diretorio de dados do config.
    #[test]
    fn persist_ou_resume_abrem_o_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            open_chat_store(&config_no_dir(dir.path()), true, None)
                .expect("sem erro")
                .is_some(),
            "--persist abre o store"
        );
        assert!(
            dir.path().join(SESSIONS_DB).exists(),
            "--persist cria o banco"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            open_chat_store(&config_no_dir(dir.path()), false, Some("cli-x"))
                .expect("sem erro")
                .is_some(),
            "--resume abre o store mesmo sem --persist"
        );
    }

    /// Ida e volta: dois turnos gravados voltam em ordem cronologica, cada
    /// mensagem com o papel com que foi gravada.
    #[test]
    fn dois_turnos_voltam_em_ordem() {
        let store = SessionStore::in_memory().expect("store em memoria");
        persist_user_turn(&store, "cli-teste", "oi").expect("pergunta 1");
        persist_assistant_turn(&store, "cli-teste", "ola").expect("resposta 1");
        persist_user_turn(&store, "cli-teste", "tudo bem?").expect("pergunta 2");
        persist_assistant_turn(&store, "cli-teste", "tudo").expect("resposta 2");

        let historico = load_history(&store, "cli-teste", RESUME_LIMIT).expect("carrega");
        assert_eq!(historico.len(), 4, "dois turnos sao quatro mensagens");
        let papeis: Vec<&str> = historico.iter().map(papel).collect();
        assert_eq!(papeis, vec!["user", "assistant", "user", "assistant"]);
        assert_eq!(textos(&historico), vec!["oi", "ola", "tudo bem?", "tudo"]);
    }

    /// Um id que nao existe devolve historico vazio em vez de erro: e o que
    /// um `--resume` digitado errado encontra, e a sessao comeca do zero.
    #[test]
    fn retomar_id_inexistente_devolve_vazio() {
        let store = SessionStore::in_memory().expect("store em memoria");
        let historico = load_history(&store, "cli-nao-existe", RESUME_LIMIT).expect("sem erro");
        assert!(historico.is_empty());
    }

    /// `persist_user_turn` grava a pergunta com o `Utc::now()` do inicio do
    /// turno e `persist_assistant_turn` com o do fim — timestamps sempre
    /// diferentes. A ordem entre elas nao pode depender do timestamp de
    /// qualquer forma: `load_recent_messages` ordena por `rowid` (ordem de
    /// insercao), e e isso que mantem o turno deterministico. Aqui o teste
    /// força o pior caso — todas as mensagens da sessao com timestamp
    /// identico — e prende a ordem de insercao.
    #[test]
    fn timestamps_iguais_mantem_ordem_de_insercao() {
        let store = SessionStore::in_memory().expect("store em memoria");
        // A FK de messages exige a sessao antes do primeiro append — o
        // mesmo passo que o `append_turn` da vida real da.
        store
            .upsert_session("cli-iguais", "cli", "local", &serde_json::json!({}))
            .expect("upsert da sessao");
        let t = chrono::Utc::now();
        let meta = serde_json::json!({ "channel_id": "cli", "user_id": "local" });
        store
            .append_message("cli-iguais", "user", "primeiro", t, &meta)
            .expect("append 1");
        store
            .append_message("cli-iguais", "assistant", "segundo", t, &meta)
            .expect("append 2");
        store
            .append_message("cli-iguais", "user", "terceiro", t, &meta)
            .expect("append 3");

        let msgs = store
            .load_recent_messages("cli-iguais", 10)
            .expect("carrega");
        let conteudos: Vec<&str> = msgs.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            conteudos,
            vec!["primeiro", "segundo", "terceiro"],
            "timestamp identico nao pode reordenar a sessao"
        );
    }

    /// #1227 (slice 1): abrir o `sessions.db` no CLI tambem e uma subida —
    /// runs `running` de uma queda anterior viram `interrupted` aqui, do
    /// mesmo jeito que no gateway. Se a chamada sair, o CLI reabre o banco
    /// do gateway e engole a trilha. O scan usa `concat!` para o proprio
    /// teste nao casar consigo mesmo.
    #[test]
    fn abertura_do_store_marca_runs_interrompidos() {
        let src = include_str!("chat.rs");
        let alvo = concat!("log_interrupted", "_runs(&store)");
        let copias = src.matches(alvo).count();
        assert_eq!(copias, 1, "esperava 1 chamada de subida, achei {copias}");
    }

    // ── #1298: /model transacional ─────────────────────────────────────────

    /// O `/model` só toca o estado depois da validação: `Ausente` recusa e
    /// mantém provider/model anteriores; rota válida fora da lista curada
    /// aplica com confirmação explícita; sem listagem aplica com ressalva.
    #[test]
    fn model_transacional_recusa_ausente_e_explicita_indireta() {
        assert_eq!(
            efeito_da_validacao(&ValidacaoDeModelo::Listado),
            EfeitoTrocaDeModelo::Aplicar
        );
        // Namespace de terceiro servido via OpenRouter (`z-ai/...` fora da
        // curada): troca SIM, com a confirmação dizendo de onde veio.
        assert_eq!(
            efeito_da_validacao(&ValidacaoDeModelo::ListadoForaDaCurada),
            EfeitoTrocaDeModelo::AplicarComNotaDeCatalogo
        );
        // Modelo que o catálogo real não tem: o estado anterior fica intacto.
        assert_eq!(
            efeito_da_validacao(&ValidacaoDeModelo::Ausente),
            EfeitoTrocaDeModelo::RecusarEManterEstado
        );
        // Provider sem catálogo: aplica, mas nunca como sucesso validado.
        assert_eq!(
            efeito_da_validacao(&ValidacaoDeModelo::SemListagem),
            EfeitoTrocaDeModelo::AplicarSemValidacao
        );
    }

    // ── #1300: /resume volta ao ultimo turno interrompido ──────────────────

    /// Os tres fins inacabados ganham marcador; o sucesso nao.
    #[test]
    fn marcador_de_interrupcao_cobre_os_tres_fins_inacabados() {
        use garraia_common::Error;
        assert_eq!(
            marcador_de_interrupcao(&TurnOutcome::<String, Error>::TimedOut),
            Some("[turno interrompido: timeout]")
        );
        assert_eq!(
            marcador_de_interrupcao(&TurnOutcome::<String, Error>::Cancelled),
            Some("[turno interrompido: cancelado]")
        );
        assert_eq!(
            marcador_de_interrupcao(&TurnOutcome::<String, Error>::Done(Err(Error::Agent(
                "provider caiu".into()
            )))),
            Some("[turno interrompido: erro]")
        );
        assert_eq!(
            marcador_de_interrupcao(&TurnOutcome::<String, Error>::Done(Ok(
                "resposta completa".into()
            ))),
            None,
            "turno completo nao tem o que marcar"
        );
    }

    /// Persistencia fracionada (#1300): a pergunta grava ANTES do turno, o
    /// marcador grava depois — e um crash duro no meio deixa a pergunta sola
    /// no banco, que e exatamente o que o `--resume` seguinte recupera.
    #[test]
    fn persistencia_fracionada_sobrevive_a_crash_no_meio_do_turno() {
        let store = SessionStore::in_memory().expect("store em memoria");

        // O turno comecou: a pergunta ja esta no disco.
        persist_user_turn(&store, "cli-crash", "ajuda no SOUL.md").expect("user persistido");
        // ...crash duro aqui (kill -9, pcao de luz): NADA mais roda. A
        // sessao seguinte com `--resume latest` acha a pergunta.
        let msgs = store
            .load_recent_messages("cli-crash", 10)
            .expect("carrega apos crash");
        assert_eq!(msgs.len(), 1, "so a pergunta existe");
        assert_eq!(msgs[0].content, "ajuda no SOUL.md");

        // Em vez do crash, o turno acabou em timeout: o marcador entra como
        // mensagem do assistant e o historico hidratado conta a verdade.
        let marcador =
            marcador_de_interrupcao(&TurnOutcome::<String, garraia_common::Error>::TimedOut)
                .expect("timeout tem marcador");
        persist_assistant_turn(&store, "cli-crash", marcador).expect("marcador persistido");
        let msgs = store
            .load_recent_messages("cli-crash", 10)
            .expect("carrega apos marcador");
        let conteudos: Vec<&str> = msgs.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            conteudos,
            vec!["ajuda no SOUL.md", "[turno interrompido: timeout]"],
            "o resume hidrata pergunta + marcador, na ordem em que aconteceram"
        );
    }

    /// `--resume latest` resolve para a sessao mais ativa do canal CLI. A
    /// ordem no banco mora em `latest_session_id` (garraia-db, testado la);
    /// aqui a decisao de boot do CLI: id explicito vence, `latest` cai para
    /// o alvo do banco, e sem alvo a sessao comeca nova (com aviso) em vez
    /// de derrubar o programa — primeiro uso com `--resume` e legitimo.
    #[test]
    fn id_inicial_da_sessao_resolve_latest_com_fallback_para_nova() {
        // Id explicito: vence sempre, nao toca no banco.
        let (id, nova) = id_inicial_da_sessao(Some("cli-xyz"), None, "cli-fresh".into());
        assert_eq!(id, "cli-xyz");
        assert!(!nova);
        // `latest` com alvo no banco: o alvo.
        let (id, nova) =
            id_inicial_da_sessao(Some("latest"), Some("cli-alvo".into()), "cli-fresh".into());
        assert_eq!(id, "cli-alvo");
        assert!(!nova);
        // `latest` sem alvo (banco vazio / canal errado): sessao nova.
        let (id, nova) = id_inicial_da_sessao(Some("latest"), None, "cli-fresh".into());
        assert_eq!(id, "cli-fresh");
        assert!(nova, "banco vazio nao e erro de boot");
        // Sem --resume: sessao nova (comportamento #1088 preservado).
        let (id, nova) = id_inicial_da_sessao(None, Some("cli-alvo".into()), "cli-fresh".into());
        assert_eq!(id, "cli-fresh");
        assert!(nova);
    }
}

#[cfg(test)]
mod aprovacao_tests {
    //! #1343 — o `garraia chat` retoma o pedido de confirmacao no turno
    //! seguinte. O historico do CLI e texto puro (`MessagePart::Text`), entao
    //! antes do escopo o "sim" nunca aprovava: a ferramenta perguntava de
    //! novo para sempre.

    use super::*;
    use garraia_agents::tools::approval::ApprovalFingerprint;
    use garraia_agents::{
        ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
        Tool, ToolContext, ToolOutput,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TOOL: &str = "apaga_arquivo";
    const ALVO: &str = "/tmp/garraia-1343-cli";

    struct ApagaArquivo(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Tool for ApagaArquivo {
        fn name(&self) -> &str {
            TOOL
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            c: &ToolContext,
            _i: serde_json::Value,
        ) -> garraia_common::Result<ToolOutput> {
            if c.approval.covers(TOOL, ALVO) {
                self.0.fetch_add(1, Ordering::SeqCst);
                return Ok(ToolOutput::success("apagado"));
            }
            let m = ApprovalFingerprint::of(TOOL, ALVO).marker();
            Ok(ToolOutput::confirmation_request(format!(
                "Confirma apagar {ALVO}? {m}"
            )))
        }
    }

    /// A cada mensagem humana pede a tool; depois do resultado, texto. Sem
    /// `stream_complete`: o runtime cai no batch, como com provider local.
    struct Roteiro;

    #[async_trait::async_trait]
    impl LlmProvider for Roteiro {
        fn provider_id(&self) -> &str {
            "roteiro"
        }
        async fn complete(&self, r: &LlmRequest) -> garraia_common::Result<LlmResponse> {
            let humano = matches!(
                r.messages.last(),
                Some(ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text(_),
                })
            );
            let content = if humano {
                vec![ContentBlock::ToolUse {
                    id: "t".into(),
                    name: TOOL.into(),
                    input: serde_json::json!({ "alvo": ALVO }),
                }]
            } else {
                vec![ContentBlock::Text {
                    text: "feito".into(),
                }]
            };
            Ok(LlmResponse {
                content,
                model: "m".into(),
                stop_reason: None,
                usage: None,
            })
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(true)
        }
    }

    /// Um turno pelo mesmo ponto de entrada do REPL, com o mesmo
    /// `exec_do_turno`, e o historico crescendo so com texto.
    async fn turno(
        rt: &AgentRuntime,
        sessao: &str,
        historico: &mut Vec<ChatMessage>,
        texto: &str,
    ) -> String {
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
        let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let exec = exec_do_turno("/tmp", sessao);
        let r = rt
            .process_message_streaming_with_events(
                sessao, texto, historico, tx, None, None, None, None, None, None, &exec,
            )
            .await
            .expect("turno");
        dreno.await.expect("dreno");
        historico.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(texto.to_string()),
        });
        historico.push(ChatMessage {
            role: ChatRole::Assistant,
            content: MessagePart::Text(r.clone()),
        });
        r
    }

    #[test]
    fn exec_do_turno_escopa_canal_sessao_e_o_terminal() {
        let exec = exec_do_turno("/tmp", "sess-1");
        let escopo = exec.approval_scope.expect("o CLI opta pelo escopo");
        assert_eq!(escopo.channel(), CANAL_CLI);
        assert_eq!(escopo.session_id(), "sess-1");
        assert_eq!(escopo.sender(), REMETENTE_CLI);
        assert_eq!(exec.working_dir.as_deref(), Some("/tmp"));
    }

    #[tokio::test]
    async fn sim_no_turno_seguinte_roda_uma_vez_e_replay_pausa() {
        let rt = AgentRuntime::new();
        let vezes = Arc::new(AtomicUsize::new(0));
        rt.register_tool(Box::new(ApagaArquivo(Arc::clone(&vezes))));
        rt.register_provider(Arc::new(Roteiro));
        let mut h = Vec::new();

        // #1373: o marcador interno nao chega ao texto do humano; o pedido e
        // reconhecido pela frase da ferramenta de teste.
        let e_pedido = |r: &str| {
            assert!(
                !r.contains(garraia_agents::tools::approval::MARKER_PREFIX),
                "o marcador interno chegou ao terminal: {r}"
            );
            r.contains("Confirma apagar")
        };
        let r1 = turno(&rt, "cli-1343", &mut h, "apaga o arquivo").await;
        assert!(e_pedido(&r1), "pausa: {r1}");
        assert_eq!(vezes.load(Ordering::SeqCst), 0);

        let r2 = turno(&rt, "cli-1343", &mut h, "sim").await;
        assert!(!e_pedido(&r2), "{r2}");
        assert_eq!(vezes.load(Ordering::SeqCst), 1, "roda uma vez");

        let r3 = turno(&rt, "cli-1343", &mut h, "sim").await;
        assert!(e_pedido(&r3), "replay pausa: {r3}");
        assert_eq!(vezes.load(Ordering::SeqCst), 1, "replay nao roda");
    }

    /// `/resume` para outra sessao troca o escopo: o pedido da sessao
    /// anterior nao e aprovado pelo "sim" dado na nova.
    #[tokio::test]
    async fn sim_em_outra_sessao_nao_aprova() {
        let rt = AgentRuntime::new();
        let vezes = Arc::new(AtomicUsize::new(0));
        rt.register_tool(Box::new(ApagaArquivo(Arc::clone(&vezes))));
        rt.register_provider(Arc::new(Roteiro));
        let (mut a, mut b) = (Vec::new(), Vec::new());

        turno(&rt, "cli-a", &mut a, "apaga o arquivo").await;
        turno(&rt, "cli-b", &mut b, "sim").await;
        assert_eq!(vezes.load(Ordering::SeqCst), 0);
    }
}
