// Plan 0049: `mod tests` sits mid-file; trailing private helpers
// (`estimate_tokens`, `trim_messages_to_budget`, `extract_text`) are used by
// the runtime public API above. Moving them before the tests would disrupt
// git blame on a 1.9kloc file — inner allow at module scope.
#![allow(clippy::items_after_test_module)]

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use futures::future::join_all;
use futures::{Stream, StreamExt};
use garraia_common::{Error, Result, metrics};
use garraia_db::{MemoryEntry, MemoryProvider, MemoryRole, NewMemoryEntry, RecallQuery};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

use crate::context_policy::ContextPolicy;
use crate::embeddings::EmbeddingProvider;
use crate::exec_context::ExecContext;
use crate::execution_budget::{ExecutionBudget, VereditoDeLoop};
use crate::memory_extractor::LlmMemoryExtractor;
use crate::provider_resilience::ResilienceManager;
use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent, ToolDefinition,
};
use crate::tools::approval::{ApprovalFingerprint, ToolApproval};
use crate::tools::{Tool, ToolContext, ToolOutput};
use crate::turn_events::{
    TurnSink, capture_tool_output, summarize_tool_input, summarize_tool_output,
};

/// Where a registered tool came from. Issue #924: without this the runtime
/// cannot tell a native tool from an MCP one, so it cannot replace just the
/// MCP half when a server reconnects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSource {
    Native,
    Mcp { server: String },
}

struct RegisteredTool {
    tool: Arc<dyn Tool>,
    source: ToolSource,
}

/// What one `replace_mcp_tools` call changed. Returned so callers can log a
/// real delta instead of "sync ran".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSyncDelta {
    pub removed: usize,
    pub added: usize,
}

/// One row of [`AgentRuntime::tool_inventory`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInventoryEntry {
    pub name: String,
    pub description: String,
    /// `"native"` or `"mcp"`.
    pub source: String,
    /// The MCP server this tool came from, when `source == "mcp"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// GAR-187 + #1078 item 2: a aprovacao humana de UM pedido pendente.
///
/// Devolve [`ToolApproval::Granted`] com a impressao digital do pedido
/// quando as duas condicoes valem:
///
/// 1. A ultima mensagem do lado do usuario carrega um marcador
///    `[CONFIRM_REQUIRED:<hex>]` **vindo de um resultado de ferramenta**, e
/// 2. `user_text` e uma palavra de aprovacao ("sim", "yes", "confirmar", …).
///
/// Duas mudancas em relacao a versao GAR-187, e as duas fecham buraco:
///
/// **A impressao digital.** Antes devolvia `bool` e a aprovacao valia para o
/// turno inteiro: o usuario dizia "ok" a um `ls -la` e o modelo executava
/// qualquer outra coisa naquele turno. Agora a aprovacao carrega o hash de
/// `(ferramenta, assunto)` e so cobre aquele pedido.
///
/// **So resultado de ferramenta.** Antes o marcador tambem era procurado no
/// TEXTO do assistente, entao um modelo com saida nao sanitizada escrevia
/// `[CONFIRM_REQUIRED]` na propria narracao, plantava um pedido que nunca
/// existiu, e colhia o "ok" inocente do usuario na mensagem seguinte. Um
/// pedido legitimo sempre chega como `ContentBlock::ToolResult`, porque e a
/// ferramenta que o emite — texto do modelo nunca cria aprovacao.
///
/// Le so as ultimas 6 mensagens, e a MAIS RECENTE ganha: se dois pedidos
/// ficaram pendentes, o "ok" responde ao ultimo, que e o que o usuario
/// acabou de ler.
///
/// **Sem mensagem humana no meio (#1340).** A janela de 6 mensagens dizia
/// so "recente", e o pedido mais novo dela valia mesmo que o humano ja
/// tivesse respondido e a conversa tivesse seguido. A sequencia: o turno 1
/// pausa pedindo X; o humano diz "nao"; o modelo pergunta outra coisa em
/// texto; um "ok" mais tarde — resposta a essa outra pergunta — aprovava X.
///
/// Agora o historico tem de TERMINAR no resultado pausado, com no maximo a
/// narracao do assistente depois dele — que e a forma da retomada GAR-187 em
/// todo canal: `[… , ToolResult(pausa), texto do assistente com o pedido]`.
/// So a primeira mensagem do lado do usuario, de tras para frente, e
/// consultada: se ela for mensagem humana (ou nao carregar resultado de
/// ferramenta), nao ha pedido pendente, e se ela for um resultado sem
/// marcador o "ok" tambem nao alcanca nenhum pedido mais antigo — aquele o
/// humano ja respondeu. A janela de 6 continua valendo por cima disso.
fn detect_confirmation_approval(history: &[ChatMessage], user_text: &str) -> ToolApproval {
    if !crate::tools::pending_approval::is_approval_word(user_text) {
        return ToolApproval::None;
    }

    for msg in history.iter().rev().take(6) {
        // #1339 (revisao do #1337): resultado de tool so chega ao historico
        // pelo lado do usuario. Um bloco `tool_result` dentro da RESPOSTA do
        // provider (`from_anthropic_response` mapeia esse tipo) viraria uma
        // mensagem do assistente — e texto do modelo nunca cria aprovacao.
        if !matches!(msg.role, ChatRole::User) {
            continue;
        }
        // So `MessagePart::Parts` carrega resultado de ferramenta.
        // `MessagePart::Text` no lado do usuario e MENSAGEM HUMANA — e #1340:
        // ela FECHA a janela em vez de ser pulada. O humano ja falou depois do
        // pedido, entao o "ok" de agora responde a outra coisa.
        let MessagePart::Parts(parts) = &msg.content else {
            return ToolApproval::None;
        };
        // #1339: dentro da mensagem tambem vale "o mais recente ganha". Numa
        // volta com chamadas paralelas o pedido pausado e o ultimo resultado
        // (a volta para no primeiro pedido), entao ler de tras para frente
        // escolhe o pedido e nunca um resultado anterior da mesma volta.
        for p in parts.iter().rev() {
            if let ContentBlock::ToolResult { content, .. } = p
                && let Some(fp) = ApprovalFingerprint::from_marker(content)
            {
                return ToolApproval::Granted(fp.as_str().to_string());
            }
        }
        // #1340: esta era a ULTIMA mensagem do lado do usuario e ela nao e um
        // pedido pausado — ou e mensagem humana em blocos (texto, imagem), ou
        // e resultado de ferramenta sem marcador. Nos dois casos a busca para
        // aqui: um pedido mais antigo ja teve a sua vez de ser respondido.
        return ToolApproval::None;
    }
    ToolApproval::None
}

/// GAR-210: Returns true for errors that warrant a retry or provider fallback.
/// Detects rate-limit (429) and transient server errors (502/503/529).
///
/// Estes **tiveram resposta**: o provider respondeu dizendo "agora nao". A
/// politica certa e insistir com backoff no mesmo endereco, e por isso a
/// leitura de texto continua aqui — o status vem no corpo formatado pelo
/// provider, nao num tipo. Falha de transporte nao passa mais por este
/// classificador: ela tem classe propria (`is_transport_error`).
fn is_retryable_error(err: &Error) -> bool {
    let msg = err.to_string().to_lowercase();
    // #1299: roteamento impossível (`No allowed providers are available`) é
    // determinístico — a interseção modelo × provider.only não muda por
    // repetição. A assinatura vale MAIS que o número de status: mesmo que o
    // corpo um dia carregue um "status=503" no meio do texto, não há retry.
    if msg.contains("no allowed providers are available") {
        return false;
    }
    msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("rate_limit")
        || msg.contains("too many requests")
        || msg.contains("status=502")
        || msg.contains("status=503")
        || msg.contains("status=529")
        || msg.contains("upstream")
}

/// #1249: a requisicao nao chegou a ter resposta — rede caida, DNS morto,
/// conexao recusada, timeout de conexao.
///
/// Tipado de proposito. A classificacao e feita em
/// `providers::erro_de_envio`, no ponto onde o `reqwest::Error` ainda existe
/// como tipo; aqui so se le a classe. Antes disto, `"openai request failed:
/// error sending request for url (...)"` nao casava com nenhum padrao de
/// `is_retryable_error` e o turno morria em `Err(e) => return Err(e)` **antes**
/// do laco de fallback — o usuario tirava o cabo e recebia erro em vez do
/// Ollama que estava rodando na propria maquina (ADR 0022).
///
/// Consequencia da classe: **uma** tentativa no primario e o fallback entra.
/// Nao ha o que reganhar repetindo um endereco inalcancavel, e os ~3,5s de
/// backoff do orcamento de retry seriam gastos antes de chegar ao local.
fn is_transport_error(err: &Error) -> bool {
    matches!(err, Error::Transport(_))
}

/// Resolve provider ID from model override.
/// Models like "openrouter/auto", "openai/gpt-4o" have the provider as prefix.
///
/// Publica desde o #1029: e a regra que o `POST /v1/chat/completions` aplica
/// ao campo `model`, e o `GET /v1/models` precisa dela para listar so o que
/// aquele campo consegue rotear — a lista e a rota tem de concordar.
pub fn resolve_provider_from_model(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    // Check for provider prefix (e.g., "openrouter/auto", "anthropic/claude-3")
    if let Some((provider, _)) = model.split_once('/') {
        let provider = provider.to_lowercase();
        // Map common provider names to registered provider IDs
        match provider.as_str() {
            "openrouter" => Some("openrouter".to_string()),
            "openai" => Some("openai".to_string()),
            "anthropic" => Some("anthropic".to_string()),
            "ollama" => Some("ollama".to_string()),
            "deepseek" => Some("deepseek".to_string()),
            "mistral" => Some("mistral".to_string()),
            "gemini" => Some("gemini".to_string()),
            "cohere" => Some("cohere".to_string()),
            "jais" => Some("jais".to_string()),
            "qwen" => Some("qwen".to_string()),
            "yi" => Some("yi".to_string()),
            "moonshot" | "kimi" => Some("moonshot".to_string()),
            "minimax" => Some("minimax".to_string()),
            "sansa" => Some("sansa".to_string()),
            "falcon" => Some("falcon".to_string()),
            _ => Some(provider), // Use as-is for unknown providers
        }
    } else {
        None
    }
}

/// Manages agent sessions, tool execution, and LLM provider routing.
pub struct AgentRuntime {
    providers: RwLock<Vec<Arc<dyn LlmProvider>>>,
    default_provider: RwLock<Option<String>>,
    memory: Option<Arc<dyn MemoryProvider>>,
    embeddings: Option<Arc<dyn EmbeddingProvider>>,
    /// Issue #924: era `Vec<Box<dyn Tool>>` e congelava no boot, quando o
    /// `AgentRuntime` entra num `Arc`. As tools MCP so eram registradas ali;
    /// se o connect do boot falhasse e o health monitor reconectasse depois,
    /// `list_servers()` passava a reportar o servidor conectado com N tools e
    /// o runtime seguia sem nenhuma delas — inclusive para o
    /// `tool_definitions()` que alimenta o tool-calling do LLM.
    ///
    /// `RwLock` da mutabilidade interior (registro pos-`Arc`), e `Arc<dyn Tool>`
    /// e o que permite `find_tool` devolver algo proprio em vez de um
    /// emprestimo preso ao guard.
    tools: RwLock<Vec<RegisteredTool>>,
    system_prompt: Option<String>,
    /// Plan 0250 (GAR-771): default persona used when `system_prompt` is unset.
    /// `Friendly` gives Garra a warm default voice; `Neutral` restores the
    /// pre-0250 behavior (no default system prompt).
    persona_mode: crate::persona::PersonaMode,
    /// Plan 0250: language for the default persona copy (PT-BR default).
    persona_lang: crate::persona::Lang,
    max_tokens: Option<u32>,
    max_context_tokens: Option<usize>,
    max_tool_calls: Option<usize>,
    memory_extractor: LlmMemoryExtractor,
    /// GAR-210: Circuit breaker + model cache manager.
    resilience: Arc<ResilienceManager>,
    /// GAR-210: Ordered fallback provider IDs (tried when primary fails with 429/5xx).
    fallback_providers_list: RwLock<Vec<String>>,
    /// GAR-208: Sliding window + summarization policy.
    context_policy: ContextPolicy,
    /// #982/TODO 2026-09-02: knobs do auto-learning de fatos.
    /// `auto_extract = false` pula a chamada LLM extra por turno;
    /// `max_facts` limita os fatos gravados por turno (maior confidence).
    auto_extract: bool,
    max_facts: Option<u32>,
    /// Model to use when tools are available and the default model may not support function calling.
    /// Overrides model_override for any request that has tools registered.
    tools_model: RwLock<Option<String>>,
    /// #952: o que nao merece vetor. Ver `crate::memory_noise`.
    noise_policy: crate::memory_noise::NoisePolicy,
    /// #984: o que o ultimo turno de cada sessao realmente usou.
    ///
    /// Mora aqui, e nao no gateway, porque quem resolve provider, modelo e
    /// fallback e o runtime — perguntar a config devolveria o configurado, que
    /// a issue distingue explicitamente do efetivo.
    turn_stats: RwLock<crate::turn_stats::TurnStatsRegistry>,
    /// #1343: pedidos de confirmacao pausados, esperando o "sim" do proximo
    /// turno. So os turnos com `ExecContext::approval_scope` escrevem e leem
    /// aqui. Mora no runtime porque ele e o unico ponto que gateway e CLI
    /// compartilham — um mapa por processo cobre todo caminho.
    pending_approvals: crate::tools::pending_approval::PendingApprovals,
    /// #1449: o workspace padrao das file tools, **escopado por sessao**.
    ///
    /// `Some` so quando quem montou o runtime decidiu que a fonte das raizes e
    /// o workspace padrao (nada declarado em `agent.file_roots` / na env). Com
    /// raiz declarada e `None`: a declaracao vence sozinha e nao ha escopo por
    /// sessao nenhum, como antes da #1378. A CLI nunca o preenche — lá o jail
    /// ja soma o CWD de quem rodou o binario.
    ///
    /// Mora no runtime porque e aqui que o `working_dir` efetivo de cada turno
    /// e decidido, no mesmo ponto em que o [`crate::tools::ToolContext`] e
    /// montado. Ver [`AgentRuntime::working_dir_efetivo`].
    workspace_padrao: Option<crate::tools::SessionWorkspace>,
}

/// O TEXTO do aviso de ferramenta MCP escondida pelo whitelist (#1264).
///
/// Separado do `warn!` pelo mesmo motivo de todo estado puro deste repo: o
/// criterio de aceite 3 da #1264 pede **assercao sobre o aviso**, e nao so
/// sobre o retorno do portao — e afirmar sobre log emitido num subprocesso
/// seria capturar tracing, que a arvore nao tem infra pra fazer. O que o
/// wrapper de log emite e exatamente a linha que esta funcao monta; o teste
/// dela e a assercao do aviso.
///
/// Uma linha por ferramenta escondida, ja com modo e a sintaxe que libera.
/// Vazia quando nada a avisar: sem whitelist, sem escondida.
fn mensagens_mcp_fora_da_whitelist(
    portao: &crate::modes::ToolGate,
    todas: &[crate::ToolDefinition],
) -> Vec<String> {
    if !portao.restringe_por_whitelist() {
        return Vec::new();
    }
    let escondidas: Vec<&str> = todas
        .iter()
        .map(|d| d.name.as_str())
        .filter(|n| crate::modes::ToolGate::eh_ferramenta_mcp(n) && !portao.permite(n))
        .collect();
    if escondidas.is_empty() {
        return Vec::new();
    }
    let modo = portao.nome_do_modo().unwrap_or("");
    escondidas
        .iter()
        .map(|nome| {
            format!(
                "modo restrito `{modo}` escondeu a ferramenta MCP `{nome}` nao \
                 declarada: o modelo nao vai ve-la neste turno. Declare \
                 `servidor/*` (ou o nome completo `servidor__ferramenta`) na \
                 `allowed` do perfil para liberar."
            )
        })
        .collect()
}

/// Emite, em `warn!`, o aviso montado por [`mensagens_mcp_fora_da_whitelist`].
///
/// `warn!` de proposito: quem precisa ver isto e o operador que conectou o
/// servidor MCP, nao o modelo. **Recebe a lista INTEIRA**, antes do filtro do
/// portao: o aviso e justamente sobre o que o filtro tirou, e uma lista ja
/// filtrada nao tem mais o que mostrar.
fn avisar_mcp_fora_da_whitelist(portao: &crate::modes::ToolGate, todas: &[crate::ToolDefinition]) {
    for mensagem in mensagens_mcp_fora_da_whitelist(portao, todas) {
        warn!("{mensagem}");
    }
}

/// O TEXTO do aviso de whitelist ligada e vazia (#1264), `Some` quando ha
/// o que avisar.
///
/// Mesma divisao de [`mensagens_mcp_fora_da_whitelist`]: o conteudo e puro e
/// testavel, o `warn!` e o sink fino.
fn mensagem_whitelist_vazia(portao: &crate::modes::ToolGate) -> Option<String> {
    if !portao.whitelist_ligada_mas_vazia() {
        return None;
    }
    let modo = portao.nome_do_modo().unwrap_or("");
    Some(format!(
        "perfil `{modo}` tem `whitelist_mode` ligado e `allowed` vazia: a \
         restricao esta ligada e nao restringe nada, toda ferramenta passa. \
         Popule `allowed` ou desligue `whitelist_mode` (#1264)."
    ))
}

/// Avisa quando o perfil ligou `whitelist_mode` e deixou `allowed` vazia (#1264).
///
/// Whitelist vazia continua **permitindo tudo** — a opcao (b) da #1264,
/// escolhida para nao quebrar perfil existente. O que nao pode continuar e o
/// silencio: o operador ligou a restricao, nao populou a lista e nao recebia
/// restricao nenhuma nem aviso nenhum.
///
/// Este e o aviso do caminho **vivo**, onde o perfil vem de modo customizado
/// (banco) ou de agent card A2A. O `garra config check` nao pode cobrir esse
/// perfil: ele le config, e a config nao carrega modos — os customizados vivem
/// no banco (`get_custom_modes`). Os perfis nativos sao constantes de
/// compilacao com listas nao-vazias, e o que os segura ali e o teste
/// `ferramenta_mcp_nao_declarada_e_barrada_por_whitelist`, que os exercita
/// todos por nome.
fn avisar_whitelist_vazia(portao: &crate::modes::ToolGate) {
    if let Some(mensagem) = mensagem_whitelist_vazia(portao) {
        warn!("{mensagem}");
    }
}

/// Injeta o objetivo da sessao no prompt de sistema (#983).
///
/// Vai no **system prompt**, e nao na mensagem do usuario, porque objetivo e
/// enquadramento do turno inteiro e nao mais uma coisa que a pessoa disse. Se
/// fosse concatenado na mensagem, sumiria da janela junto com ela quando o
/// historico fosse podado — que e exatamente o que o criterio de aceite quis
/// evitar ao pedir que o runtime recebesse o objetivo explicitamente.
///
/// Vazio ou so espaco nao entra: `/goal` sem argumento e consulta, e nao um
/// objetivo em branco.
fn com_objetivo(system: Option<String>, goal: Option<&str>) -> Option<String> {
    match goal.map(str::trim).filter(|g| !g.is_empty()) {
        None => system,
        Some(g) => {
            let bloco = format!("Objetivo declarado desta sessao: {g}");
            Some(match system {
                Some(s) => format!("{s}\n\n{bloco}"),
                None => bloco,
            })
        }
    }
}

/// O nome da tool de autoinspecao do gateway (#1347). Ela e registrada pelo
/// `garraia-gateway`; o runtime so precisa do nome para saber se ela esta
/// entre as oferecidas no turno.
const GARRA_STATUS_TOOL: &str = "garra_status";

/// A instrucao que manda consultar `garra_status` antes de negar uma
/// integracao (#1347), em PT.
///
/// Publica de proposito: e o contrato entre a nota e o formato do relatorio
/// do `garra_status` no gateway, e os testes de la afirmam contra ela. Desde
/// a #1347 (fatia 2) cada item de `channels` traz `status` (`active` /
/// `offline`) e sai da mesma funcao do `/api/channels` — canal nao ligado fica
/// de fora —, e um turno restrito lista em `withheld` o que foi retido.
///
/// A lista so cobre canais de mensagens: o web chat, a API, a CLI e o MCP
/// nunca passam pelo registro de canais do gateway, e a nota diz isso com os
/// ids que o gateway exclui (`channels_view::FORA_DO_RELATORIO_DO_AGENTE`),
/// em vez de afirmar que todo canal ausente esta desligado — o usuario do web
/// chat que perguntava se o web chat estava disponivel ouvia "nao". A
/// superficie da conversa vai em `session.channel`.
///
/// Ferramentas ficam de fora da nota: a lista de ferramentas que o modelo
/// recebeu no turno e a fonte de verdade para elas.
pub const NOTA_GARRA_STATUS_PT: &str = "Antes de dizer que nao tem acesso a um canal \
ou integracao, chame `garra_status` e responda a partir dele. Cada canal da lista \
`channels` do relatorio traz um `status`: `active` e um canal em que voce esta \
conectado agora, e `offline` e um canal configurado que esta fora do ar. A lista \
cobre so os canais de mensagens: um canal de mensagens ausente dela nao esta ligado \
neste Garra. O web chat e a API (`web`, `api`) e a CLI e o servidor MCP (`cli`, \
`mcp`) nunca aparecem nela, e a ausencia deles nao diz nada; o canal desta conversa \
esta em `session.channel`. Um campo citado em `withheld` foi retido nesta conversa: o \
dado nao e divulgado aqui, e isso nao diz se o recurso existe ou nao — nunca leia um \
campo retido como capacidade ausente.";

/// A mesma instrucao em EN. Mesmo contrato de [`NOTA_GARRA_STATUS_PT`].
pub const NOTA_GARRA_STATUS_EN: &str = "Before saying you do not have access to a \
channel or integration, call `garra_status` and answer from it. Each channel in the \
report's `channels` list carries a `status`: `active` is a channel you are connected \
to right now, and `offline` is a configured channel that is down. The list covers \
messaging channels only: a messaging channel missing from it is not enabled on this \
Garra. The web chat and the API (`web`, `api`) and the CLI and the MCP server \
(`cli`, `mcp`) never appear in it, and their absence says nothing; the channel of \
this conversation is in `session.channel`. A field named in `withheld` was held \
back in this conversation: the data is not disclosed here, which tells you nothing \
about whether the thing exists — never read a withheld field as a missing capability.";

/// Acrescenta a instrucao de consultar `garra_status` ao prompt de sistema
/// que venceu (#1347) — so quando a tool esta entre as oferecidas no turno.
///
/// O prompt de um modo (`system_prompt_template`) SUBSTITUI a persona, e a
/// persona era o unico lugar com essa instrucao: no piso `search` do
/// WhatsApp, o modelo respondia "nao tenho acesso ao WhatsApp" conectado ao
/// WhatsApp. Aqui a nota entra DEPOIS de qualquer prompt de operador ou de
/// modo, como o objetivo e a memoria entram — nada e substituido. Sem a tool
/// na lista (CLI, modo que a nega), nao entra: o modelo nunca e mandado
/// chamar uma tool que nao tem.
fn com_nota_de_capacidades(
    system: Option<String>,
    tool_defs: &[ToolDefinition],
    lang: crate::persona::Lang,
) -> Option<String> {
    if !tool_defs.iter().any(|d| d.name == GARRA_STATUS_TOOL) {
        return system;
    }
    let nota = match lang {
        crate::persona::Lang::Pt => NOTA_GARRA_STATUS_PT,
        crate::persona::Lang::En => NOTA_GARRA_STATUS_EN,
    };
    Some(match system {
        Some(s) => format!("{s}\n\n{nota}"),
        None => nota.to_string(),
    })
}

/// O desfecho de uma chamada de tool, no unico ponto de despacho (#1226 S-A).
///
/// As quatro copias do loop de turno recebem um destes desfechos e tratam so
/// a parte que e delas (empilhar resultado, pausar o turno, falhar com
/// erro) — o orcamento, o gate do modo, os eventos, o timeout, a execucao e
/// a deteccao de confirmacao vivem todos em [`AgentRuntime::
/// dispatch_tool_call`].
#[derive(Debug)]
enum DispatchOutcome {
    /// A tool rodou e nao pediu confirmacao: o `ToolResult` pronto para
    /// entrar na lista do turno. `is_error` e o `ToolOutput::is_error` da
    /// execucao — as quatro copias do loop normal o ignoram (uma tool que
    /// falhou ainda entra na lista, e o modelo decide o que fazer), mas
    /// `AgentRuntime::executar_tool_program` (#1226 S-B) precisa dele: sem
    /// isto, um passo que falhou (tool desconhecida, timeout, erro da
    /// propria tool) virava `"ok": true` no relatorio do programa, e os
    /// passos seguintes rodavam sobre uma dependencia quebrada.
    Result(ContentBlock, bool),

    /// O gate do modo negou (#988): o `ToolResult` de recusa, tambem pronto
    /// para a lista — o modelo le e segue sem a ferramenta. Nao e erro do
    /// turno de proposito.
    Denied(ContentBlock),

    /// A tool pede confirmacao humana (GAR-187): o resultado entra na lista
    /// e o turno pausa, devolvendo `prompt` a quem chama.
    ///
    /// `tool` e `fingerprint` existem para o registro entre turnos (#1343):
    /// a impressao digital sai do PRIMEIRO marcador bem formado da saida do
    /// pedido — que o #1339 garante ser o verdadeiro. Sem marcador bem
    /// formado, `None`, e nada e registrado (fail-closed).
    Paused {
        tool_result: ContentBlock,
        prompt: String,
        tool: String,
        fingerprint: Option<ApprovalFingerprint>,
    },

    /// Primeira deteccao de loop da tarefa (#1295 item 1): a chamada NAO
    /// rodou, e este `ToolResult` leva a observacao corretiva ao modelo. As
    /// quatro copias do loop tratam como `Denied` — empilham e seguem —, mas
    /// e variante propria porque o `tool_program` rotula o passo: um aviso
    /// de loop nao e "negado pelo gate do modo".
    LoopWarning(ContentBlock),

    /// O `ExecutionBudget` detectou loop por assinatura: o turno inteiro
    /// falha. Quem chama converte em `Error::Agent` tal qual — a mensagem
    /// ja vem pronta de [`ExecutionBudget::mensagem_de_loop`] (#1295), com
    /// nome da tool, contagem da janela e o input repetido, para que as
    /// quatro copias do loop nao tenham cada uma o seu texto de erro.
    BudgetExceeded { mensagem: String },
}

/// #1226 S-B: o nome intrinseco de `tool_program`. Nao e uma `Tool`
/// registrada em `self.tools` — `dispatch_tool_call` intercepta este nome
/// antes de `find_tool`, e `AgentRuntime::executar_tool_program` e quem
/// resolve cada passo, sempre pelo `find_tool` real e pelo mesmo
/// `dispatch_tool_call`.
const TOOL_PROGRAM_NAME: &str = "tool_program";

/// Troca todo `[CONFIRM_REQUIRED:` de um texto por uma grafia que
/// `ApprovalFingerprint::from_marker` nao reconhece (#1226, revisao do
/// #1337). Usado no relatorio parcial de um `tool_program` pausado, que
/// carrega saida de passos anteriores: nenhum texto vindo de tool pode
/// competir com o marcador verdadeiro do pedido de confirmacao.
fn neutralizar_marcadores(texto: &str) -> String {
    texto.replace(
        crate::tools::approval::MARKER_PREFIX,
        "[CONFIRM_REQUIRED(neutralizado):",
    )
}

/// O que o despacho devolve para uma chamada barrada pelo detector de loop
/// (#1295), com o par de eventos que a poe no `/tool` da CLI (item 3).
///
/// Detectar roda ANTES do `tool_started` do despacho — a chamada nao
/// executa —, entao sem este par a chamada barrada nao aparecia no
/// `tool_log` e o `/tool <n>` mostrava so as chamadas identicas anteriores,
/// sem o veredito. O resumo do input e o `summarize_tool_input` (redigido);
/// a saida e a mensagem do detector, que ja sai redigida de
/// `ExecutionBudget::mensagem_de_loop`.
async fn desfecho_de_loop(
    sink: Option<&TurnSink>,
    id: &str,
    name: &str,
    input: &serde_json::Value,
    veredito: VereditoDeLoop,
) -> DispatchOutcome {
    let (texto, resumo) = match &veredito {
        VereditoDeLoop::Avisar(t) => (t, "bloqueada: loop detectado (aviso ao modelo)"),
        VereditoDeLoop::Abortar(t) => (t, "bloqueada: loop detectado (turno abortado)"),
    };
    warn!(tool = %name, "{resumo}");
    if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
        sink.tool_started(name, summarize_tool_input(name, input))
            .await;
        sink.tool_finished(
            name,
            std::time::Duration::ZERO,
            false,
            resumo.to_string(),
            capture_tool_output(texto),
        )
        .await;
    }
    match veredito {
        VereditoDeLoop::Avisar(aviso) => DispatchOutcome::LoopWarning(ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: neutralizar_marcadores(&aviso),
        }),
        VereditoDeLoop::Abortar(mensagem) => DispatchOutcome::BudgetExceeded { mensagem },
    }
}

/// #1339: so um pedido de confirmacao pode carregar marcador no historico.
///
/// `detect_confirmation_approval` aceita o marcador de qualquer
/// `ToolResult` recente. Uma tool comum que devolva um marcador VERDADEIRO
/// copiado de outro lugar (pagina lida por `web_fetch`, arquivo, resultado
/// MCP) competia com o pedido de verdade — e o "ok" do humano podia cobrir o
/// `(tool, assunto)` errado. Marcador forjado nunca autoriza (HMAC por
/// processo), mas copia de um verdadeiro autorizaria. Aqui, toda saida que
/// NAO e pedido de confirmacao tem o prefixo neutralizado antes de entrar no
/// historico; o pedido (`requires_confirmation`) passa intacto.
fn saida_sem_marcador_alheio(mut output: ToolOutput) -> ToolOutput {
    if !output.requires_confirmation {
        output.content = neutralizar_marcadores(&output.content);
    }
    output
}

/// Teto de passos de um `tool_program` (#1226 S-B, criterio da issue).
const MAX_PROGRAM_STEPS: usize = 16;

/// Teto agregado do `tool_program` inteiro, **alem** do timeout por passo
/// que `budget.timeout()` ja aplica em cada `dispatch_tool_call` (#1226
/// S-B). Sem isto, um programa de `MAX_PROGRAM_STEPS` passos herdaria so o
/// produto `passos * timeout_por_passo` como teto implicito. Checado a
/// cada passo (`inicio.elapsed()` em `executar_tool_program`), nao
/// envolvendo o loop inteiro num `tokio::time::timeout` — achado de
/// auditoria F-2: a versao que envolvia o future derrubava o relatorio dos
/// passos ja executados junto com o estouro.
///
/// Por ser checado ENTRE passos (nao dentro de um), o teto real e
/// `PROGRAM_AGGREGATE_TIMEOUT_SECS + budget.timeout()` no pior caso — um
/// unico passo em voo no momento do estouro nao e preemptado. Com o
/// timeout padrao (30s) isso e irrelevante; com `GARRA_TOOL_TIMEOUT_SECS`
/// configurado bem acima do padrao, o teto limita o **acumulo** entre
/// passos, nao um passo isolado (reauditado, F-2: aceitavel — quem
/// configura um timeout de horas por passo ja aceita uma chamada de horas
/// no loop normal).
///
/// 120s, e nao um numero maior: sob o orcamento padrao (10 chamadas por
/// turno — #979, o modo nunca levanta este teto) um `tool_program` cabe no
/// maximo 9 passos internos por turno de qualquer forma, entao um teto
/// agregado maior que `9 * tool_timeout_secs` nunca dispararia sob config
/// padrao, virando defesa morta. 120s ainda e generoso para o caso comum
/// (passos rapidos, sem LLM aninhado) e aperta de verdade quando o operador
/// sobe `GARRA_TOOL_TIMEOUT_SECS`.
const PROGRAM_AGGREGATE_TIMEOUT_SECS: u64 = 120;

/// A definicao que o modelo ve na lista de `tools` do `LlmRequest` — o
/// unico lugar onde `tool_program` se torna alcancavel. Nao vem de
/// `self.tools` (nao e uma `Tool` registrada): `AgentRuntime::
/// tool_definitions` so a anexa quando ha ao menos uma tool real registrada
/// (achado de revisao — anexar sempre fazia um runtime sem tool nenhuma
/// deixar de bater no `tool_count == 0` de `apply_tools_model_override`), e
/// o MESMO filtro `portao.permite(&d.name)` que ja roda nos tres pontos de
/// montagem do turno decide se o modelo chega a ve-la.
fn definicao_tool_program() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_PROGRAM_NAME.to_string(),
        description: format!(
            "Executa uma sequencia de ate {MAX_PROGRAM_STEPS} chamadas de ferramenta em \
             um unico turno, sem voltar ao modelo entre passos. Cada passo passa pelo \
             mesmo portao de seguranca do modo atual. Um passo negado, um passo que \
             falha, ou o orcamento do turno se esgotando no meio, encerram o programa \
             ali — a resposta traz os passos ja executados e o indice de onde parou; \
             se foi por orcamento, os passos restantes cabem num tool_program novo no \
             proximo turno. Use `as` para nomear a saida de um passo que devolve um \
             numero inteiro (saida que nao e inteiro faz o passo falhar), e `\"$nome\"` \
             no `args` de um passo seguinte para reusa-la: o valor entra como numero. \
             Um `\"$nome\"` sem valor salvo por um passo anterior deste programa faz o \
             passo falhar antes de rodar — nunca chega a ferramenta como texto. Se um \
             passo pedir confirmacao humana, o programa pausa ali e a resposta traz os \
             passos ja executados e as variaveis salvas (`vars`): apos a aprovacao, \
             reenvie so os passos a partir do indice pausado, nunca o programa inteiro \
             (os anteriores ja rodaram), trocando cada `\"$nome\"` de passo anterior \
             pelo numero que veio em `vars`."
        ),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_PROGRAM_STEPS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Nome da ferramenta a executar neste passo."
                            },
                            "args": {
                                "description": "Entrada da ferramenta. Um valor string igual \
                                    a \"$nome\" e substituido pelo inteiro salvo com esse \
                                    nome por um passo anterior; sem valor salvo, o passo \
                                    falha antes de rodar."
                            },
                            "as": {
                                "type": "string",
                                "description": "Nome de variavel para guardar a saida deste \
                                    passo. A saida tem de ser um numero inteiro; se nao \
                                    for, o passo falha."
                            }
                        },
                        "required": ["tool"]
                    }
                }
            },
            "required": ["steps"]
        }),
    }
}

/// Um passo interpretado de `tool_program.steps` (#1226 S-B).
struct PassoDoPrograma {
    tool: String,
    args: serde_json::Value,
    salvar_como: Option<String>,
}

/// Como um `tool_program` que nao abortou o turno terminou (#1226).
enum DesfechoDoPrograma {
    /// Terminou, ou parou no meio (passo negado, passo que falhou, orcamento
    /// do turno, teto agregado, programa mal formado): o `ToolOutput` que
    /// entra na lista do turno, com o relatorio.
    Saida(ToolOutput),

    /// Um passo pediu confirmacao humana (GAR-187) e o programa pausou.
    /// **Dois textos, de proposito** (achado de revisao sobre o F-1):
    ///
    /// - `para_o_humano` e o que sobe como resposta do turno — so o prefixo
    ///   que localiza o passo mais o pedido do passo. Nunca a saida crua dos
    ///   passos anteriores, que ficaria colada ao pedido em que o humano
    ///   decide aprovar (F-1). Ainda carrega o marcador do passo: quem o tira
    ///   e o despacho do envelope, ao montar o `prompt` (W3 da v0.4.5), depois
    ///   de ler dele a impressao digital.
    /// - `para_o_modelo` e o `ToolResult` que entra no historico: comeca
    ///   pelo MESMO texto do humano e acrescenta o relatorio parcial (passos
    ///   ja executados, `parou_no_passo`, `vars`). Sem ele o modelo nunca via
    ///   o que os passos 0..i-1 devolveram, embora eles tenham rodado — e na
    ///   retomada nao tinha como trocar `"$nome"` pelo valor. O texto do
    ///   humano vir primeiro nao e estetica: `ApprovalFingerprint::
    ///   from_marker` pega o **primeiro** marcador do conteudo, e assim o
    ///   marcador verdadeiro do pedido vence qualquer coisa parecida com um
    ///   marcador que a saida de um passo anterior traga.
    Pausa {
        para_o_modelo: String,
        para_o_humano: String,
    },
}

/// Le `{"steps": [...]}` do input de `tool_program`. Erro de forma (sem
/// `steps`, passo sem `tool`) volta como texto simples — quem chama envolve
/// em `Ok(ToolOutput::error(..))`, nunca aborta o turno: e um programa mal
/// formado, nao um orcamento estourado.
fn interpretar_passos_do_programa(
    input: &serde_json::Value,
) -> std::result::Result<Vec<PassoDoPrograma>, String> {
    let steps = input
        .get("steps")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "tool_program precisa de `steps: []`".to_string())?;
    if steps.is_empty() {
        return Err("tool_program precisa de ao menos um passo em `steps`".to_string());
    }
    steps
        .iter()
        .enumerate()
        .map(|(i, passo)| {
            let tool = passo
                .get("tool")
                .and_then(|t| t.as_str())
                .ok_or_else(|| format!("passo {i}: falta `tool`"))?
                .to_string();
            // Achado de revisao (sugestao): `args` ausente vira objeto
            // vazio, nao `null` — a maioria das tools desserializa o
            // input com `serde_json::from_value`, e `{}` casa com structs
            // de campos todos-opcionais onde `null` so daria erro.
            let args = passo.get("args").cloned().unwrap_or(serde_json::json!({}));
            let salvar_como = passo.get("as").and_then(|a| a.as_str()).map(str::to_string);
            Ok(PassoDoPrograma {
                tool,
                args,
                salvar_como,
            })
        })
        .collect()
}

/// Substitui, recursivamente, todo valor string exatamente igual a `"$nome"`
/// pelo inteiro salvo sob `nome` (#1226 S-B). Substituicao **so de valor
/// inteiro**, de proposito: ao contrario do prototipo descontinuado de
/// `garraia-tools` (que reusava qualquer string, na integra), aqui o valor
/// substituido sai como `serde_json::Value::Number`, nunca como texto — um
/// passo nao consegue injetar conteudo arbitrario de outro passo num campo
/// sensivel (caminho, comando) via `$var`, so um numero.
///
/// **Referencia sem valor falha, nunca vira literal** (#1226, achado de
/// revisao). Antes, um `"$nome"` que nao casava com nada seguia para a
/// ferramenta como o texto `"$nome"` — e um `bash` com `command: "$n"`
/// expandia uma variavel de ambiente qualquer no lugar do valor que o
/// programa pretendia. O caso comum era o da retomada depois de uma pausa
/// (GAR-187): o programa reenviado a partir do passo pausado nao carrega os
/// `vars` dos passos anteriores. Agora `Err(nome)` volta com a primeira
/// referencia sem valor, e quem chama falha o passo antes de despachar.
///
/// E referencia um valor string que seja exatamente `$` seguido de um nome
/// declarado com `as` neste programa (`declarados`) ou com cara de nome
/// (ver [`parece_nome_de_variavel`]). Qualquer outra string com `$` — o
/// `"$HOME/bin/x"` de um comando, um `"$"` de regex — nao e referencia e
/// segue intacta, como sempre seguiu: o modelo poderia manda-la direto.
fn substituir_vars_inteiras(
    valor: &mut serde_json::Value,
    vars: &HashMap<String, i64>,
    declarados: &std::collections::HashSet<&str>,
) -> std::result::Result<(), String> {
    match valor {
        serde_json::Value::String(s) => {
            if let Some(nome) = s.strip_prefix('$') {
                if let Some(&n) = vars.get(nome) {
                    *valor = serde_json::json!(n);
                } else if declarados.contains(nome) || parece_nome_de_variavel(nome) {
                    return Err(nome.to_string());
                }
            }
            Ok(())
        }
        serde_json::Value::Array(itens) => {
            for item in itens {
                substituir_vars_inteiras(item, vars, declarados)?;
            }
            Ok(())
        }
        serde_json::Value::Object(mapa) => {
            for v in mapa.values_mut() {
                substituir_vars_inteiras(v, vars, declarados)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// O que vem depois do `$` tem cara de nome de variavel do programa
/// (#1226): nao vazio, so letras ASCII, digitos, `_` ou `-`. E o criterio
/// que separa `"$total"` (referencia — sem valor, o passo falha) de
/// `"$HOME/bin/x"` ou `"$"` (texto que o modelo escreveu, segue intacto).
fn parece_nome_de_variavel(nome: &str) -> bool {
    !nome.is_empty()
        && nome
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl AgentRuntime {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(Vec::new()),
            default_provider: RwLock::new(None),
            memory: None,
            embeddings: None,
            tools: RwLock::new(Vec::new()),
            system_prompt: None,
            persona_mode: crate::persona::PersonaMode::default(),
            persona_lang: crate::persona::Lang::default(),
            max_tokens: None,
            max_context_tokens: None,
            max_tool_calls: None,
            memory_extractor: LlmMemoryExtractor::new(),
            resilience: Arc::new(ResilienceManager::new()),
            fallback_providers_list: RwLock::new(Vec::new()),
            turn_stats: RwLock::new(crate::turn_stats::TurnStatsRegistry::new()),
            context_policy: ContextPolicy::default(),
            tools_model: RwLock::new(None),
            noise_policy: crate::memory_noise::NoisePolicy::default(),
            auto_extract: true,
            max_facts: None,
            pending_approvals: crate::tools::pending_approval::PendingApprovals::new(),
            workspace_padrao: None,
        }
    }

    /// #1449: liga o workspace padrao escopado por sessao.
    ///
    /// Chamado pelo boot do gateway **so** quando nada foi declarado em
    /// `agent.file_roots` / `GARRAIA_FILE_ROOTS` — com raiz declarada a
    /// declaracao vence sozinha e este campo fica `None`.
    pub fn set_workspace_padrao(&mut self, workspace: Option<crate::tools::SessionWorkspace>) {
        self.workspace_padrao = workspace;
    }

    /// O workspace padrao em vigor, se ha um.
    pub fn workspace_padrao(&self) -> Option<&crate::tools::SessionWorkspace> {
        self.workspace_padrao.as_ref()
    }

    /// O `working_dir` efetivo deste turno (#1449).
    ///
    /// Precedencia, e ela importa:
    ///
    /// 1. O `working_dir` que a sessao declara ([`ExecContext::working_dir`]) —
    ///    uma sessao com projeto ja passou por `project_root::confine` e nao
    ///    muda de lugar por causa desta correcao.
    /// 2. Senao, e so se houver workspace padrao, o subdiretorio **desta
    ///    sessao** dentro dele, criado preguicosamente.
    /// 3. Senao, `None` — e o fail-closed da #1244: sem raiz efetiva,
    ///    `file_read`, `file_write` e `list_dir` recusam com a mensagem unica.
    ///
    /// O valor vira o `session_dir` da chamada a
    /// [`crate::tools::FileJail::confine`], que e o mecanismo de isolamento por
    /// sessao que o projeto ja usava — nao ha um segundo caminho de
    /// confinamento aqui.
    pub fn working_dir_efetivo(&self, exec: &ExecContext, session_id: &str) -> Option<String> {
        if let Some(declarado) = exec
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
        {
            return Some(declarado.to_string());
        }
        let caminho = self
            .workspace_padrao
            .as_ref()?
            .garantir_para_sessao(session_id)?;
        Some(caminho.to_string_lossy().into_owned())
    }

    /// O [`crate::tools::ToolContext`] de uma invocacao de ferramenta.
    ///
    /// **Ponto unico de montagem** nos caminhos de execucao do runtime: e o que
    /// garante que todo turno passe por [`Self::working_dir_efetivo`], em vez
    /// de um dos quatro lacos de tool-call lembrar do escopo por sessao e outro
    /// esquecer (#1449). Um teste varre este fonte para que nao volte a haver
    /// um `ToolContext` montado a mao aqui.
    pub fn contexto_de_ferramenta(
        &self,
        exec: &ExecContext,
        session_id: &str,
        user_id: Option<&str>,
        approval: crate::tools::approval::ToolApproval,
        is_heartbeat: bool,
    ) -> crate::tools::ToolContext {
        crate::tools::ToolContext {
            session_id: session_id.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            is_heartbeat,
            approval,
            working_dir: self.working_dir_efetivo(exec, session_id),
            project_id: None,
        }
    }

    /// Set the model to use when tools are available (overrides model_override for tool-capable requests).
    pub fn set_tools_model(&self, model: Option<String>) {
        *self.tools_model.write().unwrap() = model;
    }

    /// If `tools_model` is configured and there are tools registered, re-resolve
    /// (provider, model) so that tool-capable requests use a model that supports
    /// function calling (e.g. when the configured model is a cheap one such
    /// as `openrouter/free`, which does not).
    /// Returns the original pair unchanged when no override applies.
    fn apply_tools_model_override(
        &self,
        provider: Arc<dyn LlmProvider>,
        effective_model: String,
        tool_count: usize,
    ) -> (Arc<dyn LlmProvider>, String) {
        if tool_count == 0 {
            return (provider, effective_model);
        }
        let tm = self.tools_model.read().unwrap().clone();
        let Some(tools_model) = tm.filter(|s| !s.is_empty()) else {
            return (provider, effective_model);
        };
        // Re-resolve provider: try the prefix (e.g. "google"), then "openrouter", then keep original.
        let new_provider = resolve_provider_from_model(&tools_model)
            .and_then(|pid| self.get_provider(&pid))
            .or_else(|| self.get_provider("openrouter"))
            .unwrap_or_else(|| provider.clone());
        info!(
            "tools_model override: '{}' → '{}' (provider: {})",
            effective_model,
            tools_model,
            new_provider.provider_id()
        );
        (new_provider, tools_model)
    }

    /// GAR-208: Set the context window / summarization policy.
    pub fn set_context_policy(&mut self, policy: ContextPolicy) {
        self.context_policy = policy;
    }

    /// GAR-208: Return a reference to the current context policy.
    pub fn context_policy(&self) -> &ContextPolicy {
        &self.context_policy
    }

    /// #952: define o que nao merece vetor na ingestao.
    pub fn set_noise_policy(&mut self, policy: crate::memory_noise::NoisePolicy) {
        self.noise_policy = policy;
    }

    /// Configura o auto-learning de fatos (`memory.auto_extract` /
    /// `memory.max_facts` do config.yml). Default preserva o comportamento
    /// histórico: extração ligada, sem teto.
    pub fn set_memory_extraction_policy(&mut self, auto_extract: bool, max_facts: Option<u32>) {
        self.auto_extract = auto_extract;
        self.max_facts = max_facts;
    }

    /// #952: a politica em vigor. A CLI precisa dela para reindexar com o
    /// **mesmo** criterio da ingestao — sem isso o `garra memory reindex`
    /// reembeddaria exatamente o ruido que a ingestao acabou de pular.
    pub fn noise_policy(&self) -> &crate::memory_noise::NoisePolicy {
        &self.noise_policy
    }

    /// GAR-210: Set the ordered fallback provider list (tried on 429/5xx).
    pub fn set_fallback_providers(&self, providers: Vec<String>) {
        *self.fallback_providers_list.write().unwrap() = providers;
    }

    /// GAR-210: Return the configured fallback provider IDs.
    pub fn fallback_providers(&self) -> Vec<String> {
        self.fallback_providers_list.read().unwrap().clone()
    }

    /// O que o ultimo turno desta sessao usou de fato (#984).
    pub fn last_turn_stats(&self, session_id: &str) -> Option<crate::turn_stats::TurnStats> {
        self.turn_stats.read().ok()?.get(session_id).cloned()
    }

    /// Registra o turno. Chamado no fim de cada caminho de execucao.
    fn record_turn_stats(&self, session_id: &str, stats: crate::turn_stats::TurnStats) {
        if let Ok(mut reg) = self.turn_stats.write() {
            reg.record(session_id, stats);
        }
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    /// Plan 0250 (GAR-771): choose the default persona voice used when no
    /// explicit `system_prompt` is set.
    pub fn set_persona_mode(&mut self, mode: crate::persona::PersonaMode) {
        self.persona_mode = mode;
    }

    /// Plan 0250: set the language for the default persona copy.
    pub fn set_persona_lang(&mut self, lang: crate::persona::Lang) {
        self.persona_lang = lang;
    }

    /// Plan 0250: resolve the base system prompt, applying the default persona
    /// fallback when no explicit prompt is configured. An explicit
    /// (non-empty) prompt always wins; `PersonaMode::Neutral` yields `None`
    /// (pre-0250 behavior).
    fn base_system_prompt(&self, explicit: Option<&str>) -> Option<String> {
        crate::persona::resolve_system_prompt(explicit, self.persona_mode, self.persona_lang)
    }

    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = Some(max_tokens);
    }

    pub fn set_max_context_tokens(&mut self, max_context_tokens: usize) {
        self.max_context_tokens = Some(max_context_tokens);
    }

    pub fn set_max_tool_calls(&mut self, max_tool_calls: usize) {
        self.max_tool_calls = Some(max_tool_calls);
    }

    pub fn register_provider(&self, provider: Arc<dyn LlmProvider>) {
        let id = provider.provider_id().to_string();
        info!("registered LLM provider: {}", id);
        {
            let mut default = self.default_provider.write().unwrap();
            if default.is_none() {
                *default = Some(id);
            }
        }
        self.providers.write().unwrap().push(provider);
    }

    /// GAR-208: Return all registered providers (cloned Arc handles).
    pub fn list_providers(&self) -> Vec<Arc<dyn LlmProvider>> {
        self.providers.read().unwrap().clone()
    }

    pub fn get_provider(&self, id: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .find(|p| p.provider_id() == id)
            .cloned()
    }

    pub fn default_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        let default_id = self.default_provider.read().unwrap().clone();
        default_id.and_then(|id| self.get_provider(&id))
    }

    /// Return the IDs of all registered providers.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .map(|p| p.provider_id().to_string())
            .collect()
    }

    /// Set the default provider by ID. Returns `true` if the provider exists.
    pub fn set_default_provider_id(&self, id: &str) -> bool {
        let exists = self
            .providers
            .read()
            .unwrap()
            .iter()
            .any(|p| p.provider_id() == id);
        if exists {
            *self.default_provider.write().unwrap() = Some(id.to_string());
        }
        exists
    }

    /// Return the current default provider ID.
    pub fn default_provider_id(&self) -> Option<String> {
        self.default_provider.read().unwrap().clone()
    }

    pub fn set_memory_provider(&mut self, memory: Arc<dyn MemoryProvider>) {
        self.memory = Some(memory);
        info!("memory provider attached to agent runtime");
    }

    pub fn has_memory_provider(&self) -> bool {
        self.memory.is_some()
    }

    pub fn memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.memory.clone()
    }

    /// Return (name, description) pairs for all registered tools.
    pub fn list_tool_info(&self) -> Vec<(String, String)> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| (r.tool.name().to_string(), r.tool.description().to_string()))
            .collect()
    }

    /// O provider de embeddings ativo, se houver.
    ///
    /// Existe para o boot poder chamar `health_check()` (#951) — que ate
    /// aqui era codigo morto, declarado no trait e nunca invocado — e para a
    /// reindexacao da CLI (#953) reusar exatamente o mesmo provider que o
    /// runtime usa, em vez de construir um segundo por fora.
    pub fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.embeddings.clone()
    }

    pub fn set_embedding_provider(&mut self, embeddings: Arc<dyn EmbeddingProvider>) {
        self.embeddings = Some(embeddings);
        info!("embedding provider attached to agent runtime");
    }

    pub fn has_embedding_provider(&self) -> bool {
        self.embeddings.is_some()
    }

    pub async fn on_session_start(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
    ) -> Result<()> {
        self.remember_system_event(
            session_id,
            continuity_key,
            "session_started",
            "Session started",
        )
        .await
    }

    pub async fn on_session_end(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
    ) -> Result<()> {
        self.remember_system_event(session_id, continuity_key, "session_ended", "Session ended")
            .await
    }

    pub async fn remember_turn(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<()> {
        let Some(memory) = &self.memory else {
            return Ok(());
        };
        // Skip storing empty turns — the memory store rejects blank content.
        if user_input.trim().is_empty() && assistant_output.trim().is_empty() {
            return Ok(());
        }

        if !user_input.trim().is_empty() {
            let user_embedding = self.embed_turn_unless_noise(user_input, "user").await;
            let user_embedding_model = self.embedding_model_for(&user_embedding);
            memory
                .remember(NewMemoryEntry {
                    tenant_id: "default".to_string(),
                    session_id: session_id.to_string(),
                    channel_id: None,
                    user_id: user_id.map(|s| s.to_string()),
                    continuity_key: continuity_key.map(|s| s.to_string()),
                    role: MemoryRole::User,
                    content: user_input.to_string(),
                    embedding: user_embedding,
                    embedding_model: user_embedding_model,
                    metadata: serde_json::json!({ "kind": "turn_user" }),
                })
                .await?;
        }

        if !assistant_output.trim().is_empty() {
            let assistant_embedding = self
                .embed_turn_unless_noise(assistant_output, "assistant")
                .await;
            let assistant_embedding_model = self.embedding_model_for(&assistant_embedding);
            memory
                .remember(NewMemoryEntry {
                    tenant_id: "default".to_string(),
                    session_id: session_id.to_string(),
                    channel_id: None,
                    user_id: user_id.map(|s| s.to_string()),
                    continuity_key: continuity_key.map(|s| s.to_string()),
                    role: MemoryRole::Assistant,
                    content: assistant_output.to_string(),
                    embedding: assistant_embedding,
                    embedding_model: assistant_embedding_model,
                    metadata: serde_json::json!({ "kind": "turn_assistant" }),
                })
                .await?;
        }

        Ok(())
    }

    pub async fn recall_context(
        &self,
        query_text: &str,
        session_id: Option<&str>,
        continuity_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let Some(memory) = &self.memory else {
            return Ok(Vec::new());
        };

        // #957: mede o recall **inteiro** — o embedding da consulta mais a
        // busca —, porque e isso que o usuario espera. Medir so a busca
        // esconderia o caso que mais dói: o provider de embeddings lento
        // fazendo o recall demorar antes mesmo de o banco ser tocado.
        let inicio = std::time::Instant::now();

        let query_embedding = self.embed_query(query_text).await;
        // O filtro por modelo só faz sentido acompanhando um embedding (#954):
        // sem vetor de consulta, o recall é textual e não deve ser estreitado.
        let embedding_model = query_embedding
            .is_some()
            .then(|| self.embedding_model())
            .flatten();

        // #1042: os dois escopos se somavam (o store faz AND entre
        // `session_id` e `continuity_key`, `memory_store.rs` SQL e KNN), entao
        // ligar `shared_continuity` nao compartilhava nada: uma sessao nova so
        // enxergava o que ela mesma tinha gravado sob a mesma chave. A chave de
        // continuidade **substitui** o escopo de sessao — e o que ela promete,
        // e o que `get_continuity_context` ja fazia do outro lado.
        let session_scope = match continuity_key {
            Some(_) => None,
            None => session_id.map(|s| s.to_string()),
        };

        let resultado = memory
            .recall(RecallQuery {
                tenant_id: None,
                query_text: Some(query_text.to_string()),
                query_embedding,
                embedding_model,
                session_id: session_scope,
                continuity_key: continuity_key.map(|s| s.to_string()),
                limit,
            })
            .await;

        // Medido tambem quando falha: um recall que morre em 30s de timeout e
        // exatamente a latencia que o operador precisa ver. Contar so o
        // sucesso faria o painel melhorar quando o sistema piora.
        metrics::record_recall_latency(inicio.elapsed().as_secs_f64());

        resultado
    }

    /// Register a natively-built tool. Takes `&self` since #924 — the runtime
    /// is behind an `Arc` by the time some tools exist.
    pub fn register_tool(&self, tool: Box<dyn Tool>) {
        info!("registered tool: {}", tool.name());
        self.tools.write().unwrap().push(RegisteredTool {
            tool: Arc::from(tool),
            source: ToolSource::Native,
        });
    }

    /// Replace every tool sourced from `server` with `tools`.
    ///
    /// Idempotent by construction: calling it twice with the same inventory
    /// leaves the same list, which is what makes it safe to run on every
    /// health-monitor tick. A server that disconnected and came back with a
    /// smaller tool list shrinks correctly instead of accumulating stale
    /// entries — `find_tool` is a linear scan where duplicates would silently
    /// shadow each other.
    pub fn replace_mcp_tools(&self, server: &str, tools: Vec<Box<dyn Tool>>) -> ToolSyncDelta {
        let mut guard = self.tools.write().unwrap();
        let before = guard.len();
        guard.retain(|r| !matches!(&r.source, ToolSource::Mcp { server: s } if s == server));
        let removed = before - guard.len();
        let added = tools.len();
        for tool in tools {
            guard.push(RegisteredTool {
                tool: Arc::from(tool),
                source: ToolSource::Mcp {
                    server: server.to_string(),
                },
            });
        }
        ToolSyncDelta { removed, added }
    }

    /// Pull the MCP half of the tool list from the manager and make the
    /// runtime match it.
    ///
    /// Issue #924: this is the one function that closes the gap. Before it,
    /// MCP tools reached the runtime exactly once — in `Server::run`, before
    /// the runtime went into an `Arc` — so a server that connected late (or
    /// reconnected, or was added through the admin API) had live tools that
    /// `list_servers()` reported and `tool_definitions()` did not, which means
    /// the LLM could not call them. Calling this from the boot path, the health
    /// monitor and the admin handlers keeps all three honest.
    ///
    /// Safe to call on every tick: `replace_mcp_tools` is idempotent per
    /// server, and servers absent from the manager keep whatever they had —
    /// a read failure must not silently strip working tools.
    #[cfg(feature = "mcp")]
    pub async fn sync_mcp_tools(&self, manager: &Arc<crate::mcp::McpManager>) -> usize {
        let by_server = manager.tools_by_server().await;
        let mut total = 0;
        for (server, tools) in by_server {
            let count = tools.len();
            let delta = self.replace_mcp_tools(&server, tools);
            total += count;
            if delta.removed != delta.added {
                info!(
                    server = %server,
                    removed = delta.removed,
                    added = delta.added,
                    "MCP tool inventory changed in AgentRuntime"
                );
            }
        }
        total
    }

    /// GAR-159: List names of all registered tools (for API endpoints and diagnostics).
    pub fn tool_names(&self) -> Vec<String> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| r.tool.name().to_string())
            .collect()
    }

    /// Issue #924: name + origin for every registered tool, so an API can say
    /// which tools are native and which came from which MCP server instead of
    /// reporting a bare count that disagrees with `list_servers()`.
    pub fn tool_inventory(&self) -> Vec<ToolInventoryEntry> {
        // Lock envenenado e recuperado com `into_inner()`: envenenado significa
        // so "alguem entrou em panico segurando o lock", e o `Vec` dentro dele
        // nao tem invariante para quebrar. Isto roda no boot do gateway
        // (`whatsapp_linked`, #1327) e na admin API — um `unwrap()` aqui
        // derrubaria os dois por um panic de outra tarefa.
        self.tools
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|r| ToolInventoryEntry {
                name: r.tool.name().to_string(),
                description: r.tool.description().to_string(),
                source: match &r.source {
                    ToolSource::Native => "native".to_string(),
                    ToolSource::Mcp { .. } => "mcp".to_string(),
                },
                server: match &r.source {
                    ToolSource::Native => None,
                    ToolSource::Mcp { server } => Some(server.clone()),
                },
            })
            .collect()
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .read()
            .unwrap()
            .iter()
            .map(|r| ToolDefinition {
                name: r.tool.name().to_string(),
                description: r.tool.description().to_string(),
                input_schema: r.tool.input_schema(),
            })
            .collect();
        // #1226 S-B: intrinseca, nunca registrada em `self.tools` — ver
        // `definicao_tool_program`. So aparece quando ha ao menos uma tool
        // real pra um programa executar (achado de revisao: anexar sempre
        // fazia `tool_count` nunca ser 0, e um runtime sem tool nenhuma
        // passava a acionar `apply_tools_model_override` do mesmo jeito que
        // um runtime com tools de verdade).
        if !defs.is_empty() {
            defs.push(definicao_tool_program());
        }
        defs
    }

    /// A tool registrada com este nome, se houver.
    ///
    /// Publica desde a #1244: um teste precisa alcancar a tool **como o
    /// runtime a registrou** — e o ponto de chamada de producao, nao o
    /// construtor, que este repositorio ja errou cinco vezes.
    pub fn find_tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .find(|r| r.tool.name() == name)
            .map(|r| Arc::clone(&r.tool))
    }

    /// Run the full conversation loop: recall context, call LLM, execute tools, return response.
    #[instrument(skip_all, fields(session_id = %session_id))]
    pub async fn process_message(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
    ) -> Result<String> {
        self.process_message_with_context(session_id, user_text, conversation_history, None, None)
            .await
    }

    /// Same as `process_message` but includes continuity/user context for shared memory.
    #[instrument(skip_all, fields(session_id = %session_id, has_user_id = user_id.is_some()))]
    pub async fn process_message_with_context(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<String> {
        self.process_message_with_agent_config(
            session_id,
            user_text,
            conversation_history,
            continuity_key,
            user_id,
            None,
            None,
            None,
            None,
            &ExecContext::default(),
        )
        .await
    }

    /// Process a scheduled heartbeat message. Tools receive `is_heartbeat = true`
    /// so that recursive scheduling is blocked.
    #[instrument(skip_all, fields(session_id = %session_id, has_user_id = user_id.is_some()))]
    pub async fn process_heartbeat(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<String> {
        self.process_message_impl(
            session_id,
            user_text,
            conversation_history,
            continuity_key,
            user_id,
            true,
            &ExecContext::default(),
        )
        .await
    }

    /// Process a message with explicit agent config overrides (for multi-agent routing).
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(session_id = %session_id, provider = ?provider_id, model = ?model_override))]
    pub async fn process_message_with_agent_config(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
        exec: &ExecContext,
    ) -> Result<String> {
        // Resolve provider: first try explicit provider_id, then try deriving from model_override
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else if let Some(model) = model_override {
            if let Some(resolved_provider_id) = resolve_provider_from_model(model) {
                if let Some(provider) = self.get_provider(&resolved_provider_id) {
                    info!(
                        "Resolved provider '{}' from model override '{}'",
                        resolved_provider_id, model
                    );
                    provider
                } else {
                    // Provider not registered — if model uses `org/model` format, try openrouter
                    // (it proxies minimax, yi, moonshot, etc.) before falling to the global default.
                    if model.contains('/') {
                        if let Some(or_provider) = self.get_provider("openrouter") {
                            warn!(
                                "Provider '{}' not registered; routing '{}' via openrouter",
                                resolved_provider_id, model
                            );
                            or_provider
                        } else {
                            warn!(
                                "Provider '{}' not found, falling back to default",
                                resolved_provider_id
                            );
                            self.default_provider()
                                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                        }
                    } else {
                        warn!(
                            "Provider '{}' not found, falling back to default",
                            resolved_provider_id
                        );
                        self.default_provider()
                            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                    }
                }
            } else {
                // No provider prefix in model, use default
                self.default_provider()
                    .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
            }
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        // O portao sai daqui de cima porque o prompt e o `max_tokens` do modo
        // (#986) entram nas resolucoes logo abaixo.
        let portao = crate::modes::ToolGate::para_o_turno(exec, user_text);

        // Plan 0250 (GAR-771): resolve override → config prompt → default
        // persona. An explicit prompt always wins; the persona only fills in
        // when nothing is configured (and not in Neutral mode).
        // Precedencia: override explicito do chamador > prompt do modo (#986) >
        // prompt configurado no runtime. O do modo entra no meio porque quem
        // passou um prompt na chamada pediu aquele, e quem escolheu um modo
        // customizado pediu o dele.
        let explicit_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| portao.system_prompt().map(|s| s.to_string()))
            .or_else(|| self.system_prompt.clone());
        let effective_system_prompt = com_objetivo(
            self.base_system_prompt(explicit_prompt.as_deref()),
            exec.goal.as_deref(),
        );
        let effective_model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
            .unwrap_or_default();
        // Mesma precedencia (#986): chamador > modo > runtime > default.
        let effective_max_tokens = max_tokens_override
            .or(self.max_tokens)
            .or_else(|| portao.max_tokens())
            .unwrap_or(4096);

        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        // #988: a politica do modo filtra o que o modelo chega a ver. Isso e
        // UX — o modelo nao perde turno pedindo o que nao pode. A garantia de
        // seguranca e o guard antes do `execute`, porque o modelo pode inventar
        // um nome que nunca esteve na lista.
        let todas_as_tools = self.tool_definitions();
        // #1264: os avisos leem a lista INTEIRA, antes do filtro do portao — e
        // sobre o que o filtro tirou que eles falam.
        avisar_mcp_fora_da_whitelist(&portao, &todas_as_tools);
        avisar_whitelist_vazia(&portao);
        let tool_defs: Vec<_> = todas_as_tools
            .into_iter()
            .filter(|d| portao.permite(&d.name))
            .collect();
        // #1347: depois do filtro, porque a nota so entra quando
        // `garra_status` esta entre as tools que o modelo vai ver.
        let system = com_nota_de_capacidades(system, &tool_defs, self.persona_lang);
        let (provider, effective_model) =
            self.apply_tools_model_override(provider, effective_model, tool_defs.len());
        info!(
            "agent starting: provider={}, tools={}, history_msgs={}",
            provider.provider_id(),
            tool_defs.len(),
            conversation_history.len()
        );

        // GAR-187: detect if the user approved a pending tool confirmation
        let aprovacao = self.aprovacao_do_turno(exec, session_id, conversation_history, user_text);

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        // #979: os limites do modo valem, no lugar dos fixos. Precedencia:
        // override explicito do runtime > limites do modo > padrao. Quem passou
        // `max_tool_calls` na mao pediu aquele numero.
        let mut budget = match (self.max_tool_calls, portao.limites()) {
            (Some(limit), _) => ExecutionBudget::com_limite(limit),
            (None, Some(limits)) => ExecutionBudget::com_limites_do_modo(limits),
            (None, None) => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        // #984: o que o turno realmente usar. Acumula ao longo do loop porque
        // um turno com ferramenta faz varias chamadas ao LLM, e o `/stats`
        // quer o total, nao a ultima.
        let inicio_do_turno = std::time::Instant::now();
        let mut turno = crate::turn_stats::TurnStats {
            mode: portao.nome_do_modo().map(|m| m.to_string()),
            ..crate::turn_stats::TurnStats::default()
        };

        loop {
            // Auto-reset turn limit when reached (but task limit not reached)
            // This allows multi-turn agent loops without failing
            if budget.atingiu_limite_turno() {
                budget.resetar_turno();
                info!("auto-reset turn budget, continuing agent loop");
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }

            let request = LlmRequest {
                model: effective_model.clone(),
                messages: messages.clone(),
                system: system.clone(),
                max_tokens: Some(effective_max_tokens),
                temperature: None,
                tools: tool_defs.clone(),
            };

            let (response, provider_usado) = self
                .complete_reportando_provider(&provider, &request)
                .await?;
            // #984: o modelo vem da **resposta**, e nao do pedido — e o unico
            // valor que sobreviveu a todas as resolucoes (override, prefixo de
            // modelo, `tools_model`, fallback).
            turno.fallback = provider_usado != provider.provider_id();
            turno.provider = provider_usado;
            turno.model = response.model.clone();
            turno.model_confirmado = true;
            if let Some(u) = &response.usage {
                turno.tokens_conhecidos = true;
                turno.input_tokens = turno.input_tokens.saturating_add(u.input_tokens);
                turno.output_tokens = turno.output_tokens.saturating_add(u.output_tokens);
            }

            let has_tool_use = response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                let final_text = extract_text(&response.content);
                info!(
                    "agent finished without tool calls (stop_reason={:?}, response_len={})",
                    response.stop_reason,
                    final_text.len()
                );
                if let Err(e) = self
                    .remember_turn(session_id, continuity_key, user_id, user_text, &final_text)
                    .await
                {
                    warn!("failed to store turn in memory: {}", e);
                }
                turno.tool_calls = budget.chamadas_na_tarefa();
                turno.latency_ms = inicio_do_turno.elapsed().as_millis() as u64;
                self.record_turn_stats(session_id, turno);
                return Ok(final_text);
            }

            // Agent chose to call tools — log which ones
            let tool_names: Vec<&str> = response
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { name, .. } = b {
                        Some(name.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            info!("agent calling tools: {:?}", tool_names);

            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: MessagePart::Parts(response.content.clone()),
            });

            let mut tool_results = Vec::new();
            let mut confirmation_response: Option<String> = None;
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let context = self.contexto_de_ferramenta(
                        exec,
                        session_id,
                        user_id,
                        aprovacao.clone(),
                        false,
                    );

                    match self
                        .dispatch_tool_call(&portao, &mut budget, None, &context, id, name, input)
                        .await
                    {
                        DispatchOutcome::Result(bloco, _)
                        | DispatchOutcome::Denied(bloco)
                        | DispatchOutcome::LoopWarning(bloco) => {
                            tool_results.push(bloco);
                        }
                        DispatchOutcome::Paused {
                            tool_result,
                            prompt,
                            tool,
                            fingerprint,
                        } => {
                            self.registrar_pausa(exec, session_id, &tool, fingerprint);
                            tool_results.push(tool_result);
                            confirmation_response = Some(prompt);
                            break;
                        }
                        DispatchOutcome::BudgetExceeded { mensagem } => {
                            return Err(Error::Agent(mensagem));
                        }
                    }
                }
            }

            messages.push(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Parts(tool_results),
            });

            // GAR-187: if a confirmation was requested, return the prompt immediately
            if let Some(confirmation_msg) = confirmation_response {
                return Ok(confirmation_msg);
            }
        }
    }

    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
            is_heartbeat,
            provider_id = tracing::field::Empty,
        )
    )]
    async fn process_message_impl(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        is_heartbeat: bool,
        exec: &ExecContext,
    ) -> Result<String> {
        let provider: Arc<dyn LlmProvider> = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;
        tracing::Span::current().record("provider_id", provider.provider_id());

        // Build system message: system_prompt + memory context
        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        // O portao sai daqui de cima porque o prompt do modo (#986) entra no
        // `system` logo abaixo.
        let portao = crate::modes::ToolGate::para_o_turno(exec, user_text);

        // Plan 0250 (GAR-771): apply default persona fallback here too.
        let prompt_do_modo = portao
            .system_prompt()
            .map(|s| s.to_string())
            .or_else(|| self.system_prompt.clone());
        let effective_system_prompt = com_objetivo(
            self.base_system_prompt(prompt_do_modo.as_deref()),
            exec.goal.as_deref(),
        );
        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        // #988: a politica do modo filtra o que o modelo chega a ver. Isso e
        // UX — o modelo nao perde turno pedindo o que nao pode. A garantia de
        // seguranca e o guard antes do `execute`, porque o modelo pode inventar
        // um nome que nunca esteve na lista.
        let todas_as_tools = self.tool_definitions();
        // #1264: os avisos leem a lista INTEIRA, antes do filtro do portao — e
        // sobre o que o filtro tirou que eles falam.
        avisar_mcp_fora_da_whitelist(&portao, &todas_as_tools);
        avisar_whitelist_vazia(&portao);
        let tool_defs: Vec<_> = todas_as_tools
            .into_iter()
            .filter(|d| portao.permite(&d.name))
            .collect();
        // #1347: depois do filtro, porque a nota so entra quando
        // `garra_status` esta entre as tools que o modelo vai ver.
        let system = com_nota_de_capacidades(system, &tool_defs, self.persona_lang);
        let (provider, tools_model_override) =
            self.apply_tools_model_override(provider, String::new(), tool_defs.len());

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        // Trim conversation history to fit context window
        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        // GAR-187: detect if the user approved a pending tool confirmation
        let aprovacao = self.aprovacao_do_turno(exec, session_id, conversation_history, user_text);

        // #979: os limites do modo valem, no lugar dos fixos. Precedencia:
        // override explicito do runtime > limites do modo > padrao. Quem passou
        // `max_tool_calls` na mao pediu aquele numero.
        let mut budget = match (self.max_tool_calls, portao.limites()) {
            (Some(limit), _) => ExecutionBudget::com_limite(limit),
            (None, Some(limits)) => ExecutionBudget::com_limites_do_modo(limits),
            (None, None) => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        loop {
            // Check if turn or task limit reached
            if budget.atingiu_limite_turno() {
                return Err(Error::Agent(format!(
                    "turn budget exceeded: {}",
                    budget.status()
                )));
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }

            let request = LlmRequest {
                model: tools_model_override.clone(),
                messages: messages.clone(),
                system: system.clone(),
                // #986: o `defaults` do modo customizado chega ao pedido.
                max_tokens: Some(
                    self.max_tokens
                        .or_else(|| portao.max_tokens())
                        .unwrap_or(4096),
                ),
                temperature: portao.temperature(),
                tools: tool_defs.clone(),
            };

            let response = provider.complete(&request).await?;

            let has_tool_use = response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                let final_text = extract_text(&response.content);

                // Store turn in memory (best-effort)
                if let Err(e) = self
                    .remember_turn(session_id, continuity_key, user_id, user_text, &final_text)
                    .await
                {
                    warn!("failed to store turn in memory: {}", e);
                }

                // Auto-learning: extrair fatos da mensagem do usuário. A chamada LLM
                // extra é skippada por `memory.auto_extract` (TODO 2026-09-02) — quando
                // desligada, o turno responde sem o custo da extração.
                let facts_result = if self.auto_extract {
                    self.memory_extractor.extract_facts(self, user_text).await
                } else {
                    Ok(Vec::new())
                };
                if let Ok(facts) = facts_result {
                    // Validação + teto por turno (`memory.max_facts`), maior confidence
                    // primeiro — extraído como função pura para ser afirmável em teste.
                    let facts = select_learned_facts(facts, self.max_facts);
                    for fact in facts {
                        let content = format!(
                            "[FACT] type={} key={} value={} confidence={:.2}",
                            fact.fact_type, fact.key, fact.value, fact.confidence
                        );
                        if let Some(memory) = &self.memory {
                            // Store fact in memory with embedding
                            let embedding = self.embed_document(&content).await;
                            let fact_embedding_model = self.embedding_model_for(&embedding);
                            let _ = memory
                                .remember(NewMemoryEntry {
                                    tenant_id: "default".to_string(),
                                    session_id: session_id.to_string(),
                                    channel_id: None,
                                    user_id: user_id.map(|s| s.to_string()),
                                    continuity_key: continuity_key.map(|s| s.to_string()),
                                    role: MemoryRole::User,
                                    content,
                                    embedding,
                                    embedding_model: fact_embedding_model,
                                    metadata: serde_json::json!({ "kind": "learned_fact" }),
                                })
                                .await;
                            info!("stored learned fact: {}={}", fact.key, fact.value);
                        }
                    }
                }

                budget.resetar_turno();
                return Ok(final_text);
            }

            // Append the assistant's response (including tool_use blocks) to history
            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: MessagePart::Parts(response.content.clone()),
            });

            // Execute each tool and collect results
            let mut tool_results = Vec::new();
            let mut confirmation_response: Option<String> = None;
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let context = self.contexto_de_ferramenta(
                        exec,
                        session_id,
                        user_id,
                        aprovacao.clone(),
                        is_heartbeat,
                    );

                    match self
                        .dispatch_tool_call(&portao, &mut budget, None, &context, id, name, input)
                        .await
                    {
                        DispatchOutcome::Result(bloco, _)
                        | DispatchOutcome::Denied(bloco)
                        | DispatchOutcome::LoopWarning(bloco) => {
                            tool_results.push(bloco);
                        }
                        DispatchOutcome::Paused {
                            tool_result,
                            prompt,
                            tool,
                            fingerprint,
                        } => {
                            self.registrar_pausa(exec, session_id, &tool, fingerprint);
                            tool_results.push(tool_result);
                            confirmation_response = Some(prompt);
                            break;
                        }
                        DispatchOutcome::BudgetExceeded { mensagem } => {
                            return Err(Error::Agent(mensagem));
                        }
                    }
                }
            }

            // Append tool results as a user message
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Parts(tool_results),
            });

            // GAR-187: if a confirmation was requested, return the prompt immediately
            if let Some(confirmation_msg) = confirmation_response {
                return Ok(confirmation_msg);
            }
        }
    }

    /// Run the conversation loop with streaming. Text deltas are sent through
    /// `delta_tx` as they arrive. Returns the final accumulated response text.
    pub async fn process_message_streaming(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        model_override: Option<&str>,
    ) -> Result<String> {
        self.process_message_streaming_with_context(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            None,
            None,
            model_override,
        )
        .await
    }

    /// Streaming variant with continuity/user context for shared memory.
    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
        )
    )]
    pub async fn process_message_streaming_with_context(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<String> {
        self.process_message_streaming_with_agent_config(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            continuity_key,
            user_id,
            None,
            model_override,
            None,
            None,
            &ExecContext::default(),
        )
        .await
    }

    /// Streaming variant with explicit agent config overrides (for multi-agent routing or dynamic models).
    #[allow(clippy::too_many_arguments)]
    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
            provider_id = tracing::field::Empty,
            model = tracing::field::Empty,
        )
    )]
    #[allow(clippy::too_many_arguments)]
    pub async fn process_message_streaming_with_agent_config(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
        exec: &ExecContext,
    ) -> Result<String> {
        self.stream_turn_with_sink(
            session_id,
            user_text,
            conversation_history,
            TurnSink::Text(delta_tx),
            continuity_key,
            user_id,
            provider_id,
            model_override,
            system_prompt_override,
            max_tokens_override,
            exec,
        )
        .await
    }

    /// Como [`Self::process_message_streaming`], mas entregando o **fluxo
    /// completo** do turno: texto e ciclo de vida das ferramentas (#937).
    ///
    /// Existe para o `garra chat` poder desenhar o que o agente esta fazendo
    /// sem ler log. Os outros chamadores continuam no caminho de texto e nao
    /// pagam nada por isto — ver [`TurnSink`].
    #[allow(clippy::too_many_arguments)]
    pub async fn process_message_streaming_with_events(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        events_tx: mpsc::Sender<crate::turn_events::TurnEvent>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
        exec: &ExecContext,
    ) -> Result<String> {
        self.stream_turn_with_sink(
            session_id,
            user_text,
            conversation_history,
            TurnSink::Events(events_tx),
            continuity_key,
            user_id,
            provider_id,
            model_override,
            system_prompt_override,
            max_tokens_override,
            exec,
        )
        .await
    }

    /// O turno em si. Um sink so, entao a ordem entre texto e evento de
    /// ferramenta e a ordem de emissao (ver `turn_events`).
    #[allow(clippy::too_many_arguments)]
    async fn stream_turn_with_sink(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        sink: TurnSink,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
        exec: &ExecContext,
    ) -> Result<String> {
        // Resolve provider: first try explicit provider_id, then try deriving from model_override
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else if let Some(model) = model_override {
            if let Some(resolved_provider_id) = resolve_provider_from_model(model) {
                if let Some(provider) = self.get_provider(&resolved_provider_id) {
                    info!(
                        "Resolved provider '{}' from model override '{}'",
                        resolved_provider_id, model
                    );
                    provider
                } else {
                    // Provider not registered — if model uses `org/model` format, try openrouter
                    // (it proxies minimax, yi, moonshot, etc.) before falling to the global default.
                    if model.contains('/') {
                        if let Some(or_provider) = self.get_provider("openrouter") {
                            warn!(
                                "Provider '{}' not registered; routing '{}' via openrouter",
                                resolved_provider_id, model
                            );
                            or_provider
                        } else {
                            warn!(
                                "Provider '{}' not found, falling back to default",
                                resolved_provider_id
                            );
                            self.default_provider()
                                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                        }
                    } else {
                        warn!(
                            "Provider '{}' not found, falling back to default",
                            resolved_provider_id
                        );
                        self.default_provider()
                            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                    }
                }
            } else {
                // No provider prefix in model, use default
                self.default_provider()
                    .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
            }
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        // O portao sai daqui de cima, como no `process_message_with_agent_config`:
        // o prompt e o `max_tokens` do modo (#986) entram nas resolucoes logo
        // abaixo. Ate a #1347 (fatia 3) este ramo montava o prompt sem o
        // template do modo, e o mesmo turno no piso `search` recebia um prompt
        // no batch e outro no streaming.
        let portao = crate::modes::ToolGate::para_o_turno(exec, user_text);

        // Plan 0250 (GAR-771): resolve override → config prompt → default
        // persona. An explicit prompt always wins; the persona only fills in
        // when nothing is configured (and not in Neutral mode).
        // Precedencia identica a do ramo batch: override explicito do
        // chamador > prompt do modo (#986) > prompt configurado no runtime.
        let explicit_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| portao.system_prompt().map(|s| s.to_string()))
            .or_else(|| self.system_prompt.clone());
        let effective_system_prompt = com_objetivo(
            self.base_system_prompt(explicit_prompt.as_deref()),
            exec.goal.as_deref(),
        );
        let effective_model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
            .unwrap_or_default();
        // Mesma precedencia (#986): chamador > runtime > modo > default.
        let effective_max_tokens = max_tokens_override
            .or(self.max_tokens)
            .or_else(|| portao.max_tokens())
            .unwrap_or(4096);

        // Build system message (same as process_message)
        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        // #988: a politica do modo filtra o que o modelo chega a ver. Isso e
        // UX — o modelo nao perde turno pedindo o que nao pode. A garantia de
        // seguranca e o guard antes do `execute`, porque o modelo pode inventar
        // um nome que nunca esteve na lista.
        let todas_as_tools = self.tool_definitions();
        // #1264: os avisos leem a lista INTEIRA, antes do filtro do portao — e
        // sobre o que o filtro tirou que eles falam.
        avisar_mcp_fora_da_whitelist(&portao, &todas_as_tools);
        avisar_whitelist_vazia(&portao);
        let tool_defs: Vec<_> = todas_as_tools
            .into_iter()
            .filter(|d| portao.permite(&d.name))
            .collect();
        // #1347: depois do filtro, porque a nota so entra quando
        // `garra_status` esta entre as tools que o modelo vai ver.
        let system = com_nota_de_capacidades(system, &tool_defs, self.persona_lang);
        let (provider, effective_model) =
            self.apply_tools_model_override(provider, effective_model, tool_defs.len());
        info!(
            "agent streaming: provider={}, tools={}, history_msgs={}",
            provider.provider_id(),
            tool_defs.len(),
            conversation_history.len()
        );

        // GAR-187: detect if the user approved a pending tool confirmation
        let aprovacao = self.aprovacao_do_turno(exec, session_id, conversation_history, user_text);

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut full_response = String::new();

        // #979: os limites do modo valem, no lugar dos fixos. Precedencia:
        // override explicito do runtime > limites do modo > padrao. Quem passou
        // `max_tool_calls` na mao pediu aquele numero.
        let mut budget = match (self.max_tool_calls, portao.limites()) {
            (Some(limit), _) => ExecutionBudget::com_limite(limit),
            (None, Some(limits)) => ExecutionBudget::com_limites_do_modo(limits),
            (None, None) => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        // #984: mesmo registro do caminho nao-streaming. O `/stats` nao pode
        // saber menos sobre um turno so porque ele veio em pedacos.
        let inicio_do_turno = std::time::Instant::now();
        let mut turno = crate::turn_stats::TurnStats {
            mode: portao.nome_do_modo().map(|m| m.to_string()),
            ..crate::turn_stats::TurnStats::default()
        };

        // #1048: quando o stream termina sem texto e sem ferramenta, o canal
        // publica uma bolha vazia. `stream_complete_with_fallback` nao ve
        // isso: ele devolve o stream antes de qualquer evento existir, e
        // `is_retryable_error` so olha texto de erro. A deteccao tem de ficar
        // aqui, no consumidor.
        //
        // `refazer_em_batch` desvia a proxima volta do loop para o caminho
        // batch, que tem retry/fallback de verdade. `redo_ja_usado` limita a
        // um redo por turno: nenhuma guarda do loop conta turno vazio
        // (`atingiu_limite_turno` se auto-reseta logo abaixo e
        // `pode_chamar_ferramenta` so cresce dentro do loop de ferramentas),
        // entao sem o contador um provider que devolve vazio sempre giraria
        // sem parar.
        let mut refazer_em_batch = false;
        let mut redo_ja_usado = false;

        loop {
            // Auto-reset turn limit when reached (but task limit not reached)
            // This allows multi-turn agent loops without failing
            if budget.atingiu_limite_turno() {
                budget.resetar_turno();
                info!("auto-reset turn budget, continuing agent loop");
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }
            let request = LlmRequest {
                model: effective_model.clone(),
                messages: messages.clone(),
                system: system.clone(),
                max_tokens: Some(effective_max_tokens),
                temperature: None,
                tools: tool_defs.clone(),
            };

            tracing::info!(
                "Sending LlmRequest to provider={}, model={}, tools_count={}",
                provider.provider_id(),
                request.model,
                request.tools.len()
            );

            // Try streaming with fallback; fall back to non-streaming if unsupported
            //
            // #1048: com `refazer_em_batch` o streaming e pulado de proposito.
            // `None` cai no mesmo ramo de `Some(Err(_))` — o batch — em vez de
            // duplicar as ~180 linhas de execucao de ferramentas daquele ramo.
            let stream_result = if refazer_em_batch {
                None
            } else {
                Some(
                    self.stream_complete_with_fallback(&provider, &request)
                        .await,
                )
            };

            match stream_result {
                Some(Ok(mut stream)) => {
                    // Consume stream, collecting the full response and forwarding text deltas
                    let mut response_text = String::new();
                    let mut tool_uses: Vec<(String, String, String)> = Vec::new(); // (id, name, input_json)
                    let mut current_tool: Option<(String, String, String)> = None;
                    let mut _stop_reason: Option<String> = None;
                    let mut debug_event_count = 0;

                    // #1176: `stream.next()` pode devolver `Err` no meio do
                    // turno (rede movel, upstream do provider free cortando o
                    // corpo — o `reqwest::Error` que vira "stream read error:
                    // error decoding response body"). Antes disto o `event?`
                    // propagava direto, matando o turno inteiro sem nenhuma
                    // tentativa mesmo quando nada ainda tinha ido ao sink.
                    // Guarda o erro em vez de usar `?` para decidir DEPOIS do
                    // loop se vale a pena refazer.
                    let mut stream_broke: Option<Error> = None;

                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(StreamEvent::TextDelta(text)) => {
                                response_text.push_str(&text);
                                sink.text(text).await;
                            }
                            Ok(StreamEvent::ToolUseStart { id, name, .. }) => {
                                current_tool = Some((id, name, String::new()));
                            }
                            Ok(StreamEvent::InputJsonDelta(json)) => {
                                if let Some((_, _, ref mut input)) = current_tool {
                                    input.push_str(&json);
                                }
                            }
                            Ok(StreamEvent::ContentBlockStop { .. }) => {
                                if let Some(tool) = current_tool.take() {
                                    tool_uses.push(tool);
                                }
                            }
                            Ok(StreamEvent::MessageDelta {
                                stop_reason: sr, ..
                            }) => {
                                _stop_reason = sr;
                            }
                            Ok(StreamEvent::MessageStop) => break,
                            Err(e) => {
                                stream_broke = Some(e);
                                break;
                            }
                        }
                        debug_event_count += 1;
                    }

                    // Some OpenAI-compatible streaming APIs (e.g. OpenRouter via /v1/chat/completions)
                    // don't emit an explicit ContentBlockStop event for tool calls.
                    // If we ended the stream and still have a pending tool, flush it so it executes.
                    if let Some(tool) = current_tool.take() {
                        tool_uses.push(tool);
                    }

                    if let Some(e) = stream_broke {
                        // Mesma regra do #1048 (turno vazio): se nada ainda
                        // tinha ido ao sink (nem texto, nem ferramenta
                        // completa) e o redo do turno ainda nao foi usado,
                        // refaz em batch — que tem retry/fallback de verdade
                        // (`stream_complete_with_fallback` so protege a
                        // abertura do stream, nao a leitura). Chamar
                        // `stream_complete_with_fallback` de novo aqui
                        // tentaria o mesmo provider quebrado sem esperar.
                        // Com texto ja entregue ao usuario ou o redo ja
                        // gasto, devolve o erro original: reenviar
                        // duplicaria o que ja apareceu no canal.
                        if response_text.trim().is_empty() && tool_uses.is_empty() && !redo_ja_usado
                        {
                            warn!(
                                error = %e,
                                events = debug_event_count,
                                "stream quebrou no meio sem conteudo entregue; refazendo em batch (#1176)"
                            );
                            redo_ja_usado = true;
                            refazer_em_batch = true;
                            continue;
                        }
                        return Err(e);
                    }

                    tracing::info!(
                        "Stream finished. events={}, text_len={}, tool_uses={}",
                        debug_event_count,
                        response_text.len(),
                        tool_uses.len()
                    );

                    if tool_uses.is_empty() {
                        // #1048: volta sem texto e sem ferramenta. Sem isto o
                        // `return Ok` logo abaixo devolve string vazia e o
                        // canal publica uma bolha em branco.
                        //
                        // `trim().is_empty()` e nao `is_empty()`: e a mesma
                        // regra de `extract_text_opt`, usada pelo ramo batch
                        // logo adiante. Com as duas medindo "vazio" de jeitos
                        // diferentes, um modelo que cospe so espaco passaria
                        // por um caminho e nao pelo outro.
                        if response_text.trim().is_empty() {
                            if !redo_ja_usado {
                                warn!(
                                    events = debug_event_count,
                                    text_len = response_text.len(),
                                    tool_uses = tool_uses.len(),
                                    "turno de streaming vazio; refazendo em batch (#1048)"
                                );
                                redo_ja_usado = true;
                                refazer_em_batch = true;
                                continue;
                            }
                            // Redo ja gasto. So e erro quando NADA foi ao sink
                            // no turno inteiro — a mesma regra que o ramo
                            // batch aplica em `None if full_response
                            // .is_empty()`. As duas guardas nasceram deste
                            // mesmo commit e discordar seria pior que o bug
                            // original: com `full_response` cheio, o texto ja
                            // esta na tela do usuario, e um `Err` aqui o
                            // apagaria — o Telegram edita a mensagem ja
                            // publicada para "Sorry, an error occurred"
                            // (`telegram.rs`), e `remember_turn` e
                            // `record_turn_stats` seriam pulados, sumindo com
                            // o turno do historico e das metricas depois de as
                            // ferramentas ja terem rodado.
                            if full_response.is_empty() {
                                return Err(Error::Agent(
                                    "o modelo devolveu um turno vazio no streaming e no batch"
                                        .into(),
                                ));
                            }
                            // Com texto entregue, cai no caminho normal abaixo
                            // e encerra o turno com o que ha.
                        }
                        full_response.push_str(&response_text);

                        if let Err(e) = self
                            .remember_turn(
                                session_id,
                                continuity_key,
                                user_id,
                                user_text,
                                &full_response,
                            )
                            .await
                        {
                            warn!("failed to store turn in memory: {}", e);
                        }

                        // #984: no streaming nao ha `LlmResponse`, entao o
                        // modelo aqui e o **pedido** e nao ha contagem de
                        // tokens. Os dois campos ficam marcados como nao
                        // confirmados; o `/stats` diz isso em vez de fingir.
                        turno.provider = provider.provider_id().to_string();
                        turno.model = effective_model.clone();
                        turno.tool_calls = budget.chamadas_na_tarefa();
                        turno.latency_ms = inicio_do_turno.elapsed().as_millis() as u64;
                        self.record_turn_stats(session_id, turno);

                        return Ok(full_response);
                    }

                    // Build assistant response with text + tool_use blocks
                    let mut content_blocks = Vec::new();
                    if !response_text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: response_text.clone(),
                        });
                        full_response.push_str(&response_text);
                    }

                    for (id, name, input_json) in &tool_uses {
                        let input: serde_json::Value =
                            serde_json::from_str(input_json).unwrap_or_default();
                        content_blocks.push(ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input,
                        });
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: MessagePart::Parts(content_blocks),
                    });

                    // Execute tools
                    let mut tool_results = Vec::new();
                    let mut confirmation_response: Option<String> = None;
                    for (id, name, input_json) in &tool_uses {
                        let input: serde_json::Value =
                            serde_json::from_str(input_json).unwrap_or_default();
                        let context = self.contexto_de_ferramenta(
                            exec,
                            session_id,
                            user_id,
                            aprovacao.clone(),
                            false,
                        );

                        match self
                            .dispatch_tool_call(
                                &portao,
                                &mut budget,
                                Some(&sink),
                                &context,
                                id,
                                name,
                                &input,
                            )
                            .await
                        {
                            DispatchOutcome::Result(bloco, _)
                            | DispatchOutcome::Denied(bloco)
                            | DispatchOutcome::LoopWarning(bloco) => {
                                tool_results.push(bloco);
                            }
                            DispatchOutcome::Paused {
                                tool_result,
                                prompt,
                                tool,
                                fingerprint,
                            } => {
                                self.registrar_pausa(exec, session_id, &tool, fingerprint);
                                tool_results.push(tool_result);
                                confirmation_response = Some(prompt);
                                break;
                            }
                            DispatchOutcome::BudgetExceeded { mensagem } => {
                                return Err(Error::Agent(mensagem));
                            }
                        }
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: MessagePart::Parts(tool_results),
                    });

                    // GAR-187: if confirmation needed, send prompt via stream and return
                    if let Some(confirmation_msg) = confirmation_response {
                        sink.text(confirmation_msg.clone()).await;
                        full_response.push_str(&confirmation_msg);
                        return Ok(full_response);
                    }

                    // Add separator between iterations
                    if !full_response.is_empty() {
                        full_response.push_str("\n\n");
                        sink.text("\n\n".to_string()).await;
                    }
                }
                _ => {
                    // Streaming not supported (`Some(Err(_))`) ou pulado de
                    // proposito pelo #1048 (`None`): os dois caem aqui, no
                    // nao-streaming, que tem retry/fallback de verdade.
                    refazer_em_batch = false;
                    // Streaming not supported — fall back to non-streaming with retry/fallback
                    //
                    // #984/#940: aqui ha uma `LlmResponse` de verdade, entao o
                    // turno pode ser anotado **melhor** que no caminho de
                    // streaming: o modelo vem confirmado pelo provider e a
                    // contagem de tokens existe. Ate aqui este ramo nao
                    // anotava nada, e o `/stats` respondia "nenhum turno
                    // ainda" depois de um turno inteiro — em todo provider
                    // que nao faz streaming. Achado rodando o binario com o
                    // `EchoProvider`, que cai exatamente aqui.
                    let (response, provider_usado) = self
                        .complete_reportando_provider(&provider, &request)
                        .await?;
                    turno.fallback = provider_usado != provider.provider_id();
                    turno.provider = provider_usado;
                    turno.model = response.model.clone();
                    turno.model_confirmado = true;
                    if let Some(u) = &response.usage {
                        turno.tokens_conhecidos = true;
                        turno.input_tokens = turno.input_tokens.saturating_add(u.input_tokens);
                        turno.output_tokens = turno.output_tokens.saturating_add(u.output_tokens);
                    }

                    let tool_calls_count = response
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                        .count();

                    tracing::info!(
                        "Batch fallback finished. text_len={}, tool_uses={}",
                        // #1048: `extract_text` mediria o marcador de vazio
                        // (43 chars) e o log diria que houve resposta.
                        extract_text_opt(&response.content).map_or(0, |t| t.len()),
                        tool_calls_count
                    );

                    let has_tool_use = tool_calls_count > 0;

                    if !has_tool_use {
                        // #1048: o batch nao deixava bolha em branco — deixava
                        // o marcador `[no textual response provided by the
                        // model]`, em ingles, que o usuario le como resposta do
                        // modelo. Quando nada mais foi dito no turno, um erro
                        // explicito e melhor: o canal mostra a falha em vez de
                        // publicar um marcador interno.
                        let final_text = match extract_text_opt(&response.content) {
                            Some(texto) => texto,
                            None if full_response.is_empty() => {
                                return Err(Error::Agent(
                                    "o modelo devolveu um turno vazio (batch: sem texto e sem ferramenta)"
                                        .into(),
                                ));
                            }
                            // Ja houve texto nas voltas anteriores deste turno,
                            // entao o canal nao fica sem resposta: nao e erro.
                            None => String::new(),
                        };
                        sink.text(final_text.clone()).await;
                        full_response.push_str(&final_text);

                        if let Err(e) = self
                            .remember_turn(
                                session_id,
                                continuity_key,
                                user_id,
                                user_text,
                                &full_response,
                            )
                            .await
                        {
                            warn!("failed to store turn in memory: {}", e);
                        }

                        turno.tool_calls = budget.chamadas_na_tarefa();
                        turno.latency_ms = inicio_do_turno.elapsed().as_millis() as u64;
                        self.record_turn_stats(session_id, turno);

                        return Ok(full_response);
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: MessagePart::Parts(response.content.clone()),
                    });

                    let mut tool_results = Vec::new();
                    let mut confirmation_response: Option<String> = None;
                    for block in &response.content {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            let context = self.contexto_de_ferramenta(
                                exec,
                                session_id,
                                user_id,
                                aprovacao.clone(),
                                false,
                            );

                            match self
                                .dispatch_tool_call(
                                    &portao,
                                    &mut budget,
                                    Some(&sink),
                                    &context,
                                    id,
                                    name,
                                    input,
                                )
                                .await
                            {
                                DispatchOutcome::Result(bloco, _)
                                | DispatchOutcome::Denied(bloco)
                                | DispatchOutcome::LoopWarning(bloco) => {
                                    tool_results.push(bloco);
                                }
                                DispatchOutcome::Paused {
                                    tool_result,
                                    prompt,
                                    tool,
                                    fingerprint,
                                } => {
                                    self.registrar_pausa(exec, session_id, &tool, fingerprint);
                                    tool_results.push(tool_result);
                                    confirmation_response = Some(prompt);
                                    break;
                                }
                                DispatchOutcome::BudgetExceeded { mensagem } => {
                                    return Err(Error::Agent(mensagem));
                                }
                            }
                        }
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: MessagePart::Parts(tool_results),
                    });

                    // GAR-187: if confirmation needed, send prompt via stream and return
                    if let Some(confirmation_msg) = confirmation_response {
                        sink.text(confirmation_msg.clone()).await;
                        full_response.push_str(&confirmation_msg);
                        return Ok(full_response);
                    }
                }
            }
        }
    }

    /// A aprovacao humana que vale neste turno (GAR-187, #1343).
    ///
    /// Sem `approval_scope`, a deteccao antiga pelo historico, intacta. Com
    /// escopo, so o registro do servidor conta, e o historico e ignorado —
    /// marcador copiado ou forjado nele nao aprova nada. O registro e
    /// consumido aqui em todo desfecho (ver
    /// [`crate::tools::pending_approval::PendingApprovals::resolve`]).
    ///
    /// Um escopo cuja sessao nao e a do turno e erro de quem chama: nada e
    /// aprovado (fail-closed), e o registro da sessao do escopo nao e tocado.
    fn aprovacao_do_turno(
        &self,
        exec: &ExecContext,
        session_id: &str,
        conversation_history: &[ChatMessage],
        user_text: &str,
    ) -> ToolApproval {
        match exec.approval_scope.as_ref() {
            None => detect_confirmation_approval(conversation_history, user_text),
            Some(scope) if scope.session_id() != session_id => {
                warn!(
                    channel = %scope.channel(),
                    "approval_scope de outra sessao: nenhuma aprovacao neste turno"
                );
                ToolApproval::None
            }
            Some(scope) => {
                self.pending_approvals
                    .resolve(scope, user_text, std::time::Instant::now())
            }
        }
    }

    /// Grava o pedido que acabou de pausar o turno (#1343), quando o turno
    /// tem escopo e o pedido tem impressao digital bem formada. Fora disso
    /// nao grava nada, e a pausa e terminal como antes.
    fn registrar_pausa(
        &self,
        exec: &ExecContext,
        session_id: &str,
        tool: &str,
        fingerprint: Option<ApprovalFingerprint>,
    ) {
        let Some(scope) = exec.approval_scope.as_ref() else {
            return;
        };
        if scope.session_id() != session_id {
            return;
        }
        let Some(fp) = fingerprint else {
            warn!(
                channel = %scope.channel(),
                tool = %tool,
                "pausa sem marcador bem formado: nada registrado"
            );
            return;
        };
        self.pending_approvals
            .register(scope, tool, fp, std::time::Instant::now());
    }

    /// O unico ponto de despacho de tool do `AgentRuntime` (#1226 S-A).
    ///
    /// Tudo que as quatro copias do loop de turno faziam inline ao executar
    /// uma tool vive aqui, sempre na mesma ordem: orcamento
    /// (`registrar_chamada` + deteccao de loop por assinatura), evento de
    /// inicio (#937), gate do modo (#988), execucao com timeout, log e
    /// evento de fim, e deteccao de confirmacao humana (GAR-187). O
    /// desfecho volta como [`DispatchOutcome`] e quem chama trata so o
    /// controle do turno. Um caminho novo de despacho chama daqui — o teste
    /// `despacho_de_tool_tem_um_unico_ponto_de_gate` reprova copia que
    /// consulte o gate por conta propria.
    ///
    /// `sink` e `None` nos caminhos sem canal de eventos (os dois
    /// nao-streaming), que nao emitem `tool_started`/`tool_finished` —
    /// exatamente como antes da extracao. Nos caminhos com sink, todo
    /// `tool_started` tem o seu `tool_finished`, inclusive o de uma tool
    /// negada pelo gate (fecha com `success: false` e o resumo da recusa —
    /// #1226, achado de revisao: dentro de um `tool_program` o inicio sem
    /// fim do passo negado ficava pendurado entre o par do proprio
    /// programa).
    async fn dispatch_tool_call(
        &self,
        portao: &crate::modes::ToolGate,
        budget: &mut ExecutionBudget,
        sink: Option<&TurnSink>,
        context: &ToolContext,
        id: &str,
        name: &str,
        input: &serde_json::Value,
    ) -> DispatchOutcome {
        if name == TOOL_PROGRAM_NAME {
            // #1226 (achado de revisao): o envelope gasta orcamento, mas nao
            // entra na janela de loop. Cada passo volta por aqui e registra a
            // propria assinatura; com o envelope na janela, `tool_program{
            // steps:[X]}` repetido a cada volta deixava a janela alternando
            // `[tp, X, tp]` e o corte de 3 chamadas identicas nunca vinha.
            // Nada a detectar aqui: a janela nao mudou desde a ultima
            // checagem, que ja teria cortado.
            budget.registrar_contagem();
        } else {
            // registra chamada com payload para detecção de loop por assinatura
            budget.registrar_chamada(name, input);

            // detecta loop (#1295: a primeira deteccao da tarefa avisa o
            // modelo; a seguinte aborta com o diagnostico do input repetido)
            if let Some(veredito) = budget.veredito_de_loop(name, input) {
                return desfecho_de_loop(sink, id, name, input, veredito).await;
            }
        }

        // #937: o resumo do input so e montado quando alguem vai desenha-lo.
        // `summarize_tool_input` ja redige segredo na origem.
        if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
            sink.tool_started(name, summarize_tool_input(name, input))
                .await;
        }
        let iniciado_em = std::time::Instant::now();

        // executa com timeout
        // #988: o guard de seguranca. O filtro na montagem tira a
        // ferramenta da lista que o modelo ve, mas o modelo pode
        // pedir um nome que nunca esteve la — o criterio de aceite
        // e "nenhuma ferramenta proibida e executada, **mesmo que
        // solicitada pelo LLM**". A recusa volta como saida de
        // ferramenta, e nao como erro do turno: o modelo le, e
        // segue sem ela.
        if !portao.permite(name) {
            // O nome vem do portao, e nao do `exec`: com `auto`
            // escolhido, quem barrou foi o modo **deduzido**, e
            // dizer "nao e permitida no modo `auto`" nao explica
            // nada a quem le.
            let modo = portao.nome_do_modo().unwrap_or("");
            // #1339 (revisao do #1337): a recusa repete o nome da tool como o
            // MODELO mandou. Um "nome" com a copia de um marcador verdadeiro
            // entraria no historico intacto por este caminho, que volta antes
            // de `saida_sem_marcador_alheio`.
            let recusa = neutralizar_marcadores(&crate::modes::ToolGate::recusa(name, modo));
            // #1226 (achado de revisao): fecha o `tool_started` de cima.
            // Sem isto, um passo negado dentro de um `tool_program` deixava
            // um inicio sem fim entre o par do proprio programa — a UI de
            // streaming herdava uma linha aberta, o mesmo sintoma que o F-4
            // corrigiu para o programa. A recusa e texto do runtime (nome da
            // tool + nome do modo), sem saida de ferramenta.
            if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
                sink.tool_finished(
                    name,
                    iniciado_em.elapsed(),
                    false,
                    summarize_tool_output(&recusa, false),
                    String::new(),
                )
                .await;
            }
            return DispatchOutcome::Denied(ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: recusa,
            });
        }

        // So o `tool_program` pausado preenche isto: o `ToolResult` do
        // modelo leva o relatorio parcial, e o `prompt` do humano fica curto
        // (ver [`DesfechoDoPrograma::Pausa`]). Toda outra tool pausada usa o
        // mesmo texto para os dois, como sempre.
        let mut conteudo_para_o_modelo: Option<String> = None;
        let output = if name == TOOL_PROGRAM_NAME {
            let chamadas_antes = budget.chamadas_na_tarefa();
            let desfecho = self
                .executar_tool_program(portao, budget, sink, context, id, input)
                .await;
            // #1226 (revisao do #1337): o envelope fica fora da janela de
            // loop porque os passos registram a propria assinatura. Mas um
            // programa que para ANTES de despachar qualquer passo (mal
            // formado, mais de 16 passos, `$var` indefinida no passo 0) nao
            // registra nada, e o mesmo programa repetido a cada volta so
            // parava no teto da tarefa (50 voltas de LLM em vez de 3). Sem
            // passo despachado, quem entra na janela e o proprio envelope.
            let desfecho = match desfecho {
                Ok(DesfechoDoPrograma::Saida(saida))
                    if budget.chamadas_na_tarefa() == chamadas_antes =>
                {
                    // So a assinatura: o envelope ja foi contado la em cima
                    // (`registrar_contagem`), e o orcamento continua 1 + N.
                    budget.registrar_assinatura(name, input);
                    match budget.veredito_de_loop(name, input) {
                        None => Ok(DesfechoDoPrograma::Saida(saida)),
                        // #1295: o mesmo aviso-uma-vez do loop normal. O
                        // `tool_started` do envelope ja saiu la em cima, entao
                        // aqui so o fim, com o rotulo de loop.
                        Some(VereditoDeLoop::Avisar(aviso)) => {
                            if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
                                sink.tool_finished(
                                    name,
                                    iniciado_em.elapsed(),
                                    false,
                                    "bloqueada: loop detectado (aviso ao modelo)".to_string(),
                                    capture_tool_output(&aviso),
                                )
                                .await;
                            }
                            return DispatchOutcome::LoopWarning(ContentBlock::ToolResult {
                                tool_use_id: id.to_string(),
                                content: neutralizar_marcadores(&aviso),
                            });
                        }
                        Some(VereditoDeLoop::Abortar(mensagem)) => Err(mensagem),
                    }
                }
                outro => outro,
            };
            match desfecho {
                Ok(DesfechoDoPrograma::Saida(saida)) => saida,
                Ok(DesfechoDoPrograma::Pausa {
                    para_o_modelo,
                    para_o_humano,
                }) => {
                    conteudo_para_o_modelo = Some(para_o_modelo);
                    ToolOutput::confirmation_request(para_o_humano)
                }
                Err(mensagem) => {
                    // Achado de auditoria (F-4, #1226 S-B): sem isto o
                    // `tool_started` de cima ficava sem o `tool_finished`
                    // correspondente — a UI de streaming herdava um
                    // spinner pendurado. So se chega aqui quando a TAREFA
                    // esgotou (o so-turno virou `Ok` gracioso, achado F-3)
                    // ou quando um passo interno detectou loop — `mensagem`
                    // ja e o texto de um dos dois (`ExecutionBudget::status`
                    // ou `mensagem_de_loop`, esta ja redigida), sem valor
                    // cru do modelo.
                    if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
                        sink.tool_finished(
                            name,
                            iniciado_em.elapsed(),
                            false,
                            summarize_tool_output(&mensagem, false),
                            String::new(),
                        )
                        .await;
                    }
                    return DispatchOutcome::BudgetExceeded { mensagem };
                }
            }
        } else {
            match self.find_tool(name) {
                Some(tool) => {
                    let execucao = tool.execute(context, input.clone());
                    // #1347 (revisao da onda A): `garra_status` relata as
                    // ferramentas que ESTE portao libera, e nao todas as
                    // registradas. So ela recebe a lista: montar a cada
                    // chamada de outra tool seria custo sem leitor.
                    let execucao = async {
                        if name == GARRA_STATUS_TOOL {
                            let liberadas: Vec<String> = self
                                .tool_names()
                                .into_iter()
                                .filter(|n| portao.permite(n))
                                .collect();
                            crate::tools::turn_tools::com_ferramentas_do_turno(
                                liberadas,
                                portao.restringe_por_whitelist(),
                                execucao,
                            )
                            .await
                        } else {
                            execucao.await
                        }
                    };
                    // #1380 (revisao da onda C): o BIT do portao vale para
                    // toda tool, e nao so para `garra_status`. A lista acima
                    // continua restrita a ela por causa do custo de monta-la;
                    // um `bool` nao tem esse custo. Sem este escopo, quem
                    // chamasse `turno_restrito()` de qualquer outra tool lia
                    // `None` para sempre — foi o que aconteceu com a recusa
                    // do `repo_search`, cujo ramo de operador local virou
                    // inalcancavel em producao.
                    let execucao = crate::tools::turn_tools::com_restricao_do_turno(
                        portao.restringe_por_whitelist(),
                        execucao,
                    );
                    match timeout(budget.timeout(), execucao).await {
                        Ok(result) => result.unwrap_or_else(|e| ToolOutput::error(e.to_string())),
                        Err(_) => ToolOutput::error(format!("tool timeout: {}", name)),
                    }
                }
                None => ToolOutput::error(format!("unknown tool: {}", name)),
            }
        };
        info!("tool '{}' result: is_error={}", name, output.is_error);
        let output = saida_sem_marcador_alheio(output);

        // W3 (v0.4.5): o texto de um pedido de confirmacao como o HUMANO o le,
        // sem o marcador interno. Calculado uma vez e usado nos dois lugares
        // em que o pedido chega a alguem: a linha da ferramenta nos sinks de
        // eventos (o `tool_finished` que a `garraia chat` desenha e que o
        // `/ws` manda como resumo) e o `prompt` do turno pausado. O conteudo
        // CRU, com o marcador, continua sendo o do `ToolResult` e o de onde
        // sai a `fingerprint` — os dois lugares de onde a aprovacao sai.
        let pedido_ao_humano = output
            .requires_confirmation
            .then(|| ApprovalFingerprint::strip_marker(&output.content));

        if let Some(sink) = sink.filter(|s| s.wants_tool_events()) {
            let ok = !output.is_error;
            let visivel = pedido_ao_humano.as_deref().unwrap_or(&output.content);
            sink.tool_finished(
                name,
                iniciado_em.elapsed(),
                ok,
                summarize_tool_output(visivel, ok),
                capture_tool_output(visivel),
            )
            .await;
        }

        // GAR-187: pause agent loop if tool requires user confirmation.
        // Acoplamento fragil com o `is_error` do #1226 S-B, registrado por
        // auditoria: `ToolOutput::confirmation_request` tambem marca
        // `is_error: true`, e este check TEM de vir antes de qualquer
        // lugar que trate `is_error` como falha definitiva (como o passo
        // de `executar_tool_program`, que para o programa em erro) — senao
        // uma tool pedindo confirmacao vira "passo falhou" em vez de
        // pausar. A ordem aqui ja esta certa; nao inverter.
        if output.requires_confirmation {
            tracing::info!(
                session = %context.session_id,
                "agent paused: awaiting user confirmation"
            );
            let fingerprint = ApprovalFingerprint::from_marker(&output.content);
            return DispatchOutcome::Paused {
                tool_result: ContentBlock::ToolResult {
                    tool_use_id: id.to_string(),
                    content: conteudo_para_o_modelo.unwrap_or_else(|| output.content.clone()),
                },
                // W3 (v0.4.5): o unico lugar em que o pedido vira o texto que
                // o humano le, nas quatro copias do loop e no `tool_program`.
                // O marcador fica no `ToolResult` (o historico) e na
                // `fingerprint` (o registro de pendencias) — os dois lugares
                // de onde a aprovacao sai. No texto ele so aparecia para o
                // usuario do canal, que nunca aprovou nada digitando-o.
                prompt: pedido_ao_humano
                    .unwrap_or_else(|| ApprovalFingerprint::strip_marker(&output.content)),
                tool: name.to_string(),
                fingerprint,
            };
        }

        DispatchOutcome::Result(
            ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: output.content,
            },
            output.is_error,
        )
    }

    /// #1226 S-B: intrinseca de `tool_program`, interceptada dentro de
    /// [`AgentRuntime::dispatch_tool_call`] — nao e uma `Tool` registrada em
    /// `self.tools`.
    ///
    /// Cada passo resolve pelo mesmo `find_tool` e passa pelo mesmo
    /// `dispatch_tool_call` do loop normal (chamada recursiva — o teste
    /// `despacho_de_tool_tem_um_unico_ponto_de_gate` cobra que a consulta
    /// ao portao continue existindo num unico lugar no fonte): um programa
    /// nao alcanca ferramenta que o loop normal negaria no mesmo `ExecContext`,
    /// nem pula o orcamento por passo, nem a deteccao de loop, nem os
    /// eventos de tool. A deteccao de loop vale tambem ENTRE programas: o
    /// envelope conta no orcamento mas nao entra na janela de assinaturas
    /// (`ExecutionBudget::registrar_contagem`), entao o mesmo passo repetido
    /// em programas de um passo so, volta apos volta, corta na terceira
    /// repeticao como cortaria fora do programa.
    ///
    /// `Err` sai daqui so quando o orcamento da **tarefa** (nao so do turno)
    /// estoura, ou quando a deteccao de loop por assinatura dispara num
    /// passo — as duas classes que ja abortam o turno inteiro no loop
    /// principal. Estourar so o teto do **turno** (#979: `atingiu_limite_
    /// turno`, com folga na tarefa) e diferente: o loop principal, nesse
    /// caso, so reseta o contador e continua — abortar o turno aqui seria
    /// o `tool_program` se sair PIOR do que as mesmas chamadas feitas uma a
    /// uma pelo modelo (achado de revisao). Entao esse caso, como qualquer
    /// outra parada no meio (passo negado, passo com erro, `"$nome"` sem
    /// valor, saida de `as` que nao e inteiro, programa mal formado,
    /// aninhamento) volta `Ok(DesfechoDoPrograma::Saida)` com o relatorio
    /// parcial, para o modelo ler e continuar no proximo turno — nao e
    /// motivo para abortar a conversa. Um passo que pede confirmacao volta
    /// `Ok(DesfechoDoPrograma::Pausa)`, com um texto para o humano e outro
    /// para o modelo.
    async fn executar_tool_program(
        &self,
        portao: &crate::modes::ToolGate,
        budget: &mut ExecutionBudget,
        sink: Option<&TurnSink>,
        context: &ToolContext,
        id: &str,
        input: &serde_json::Value,
    ) -> std::result::Result<DesfechoDoPrograma, String> {
        use DesfechoDoPrograma::Saida;

        let passos = match interpretar_passos_do_programa(input) {
            Ok(p) => p,
            Err(e) => return Ok(Saida(ToolOutput::error(e))),
        };
        if passos.len() > MAX_PROGRAM_STEPS {
            return Ok(Saida(ToolOutput::error(format!(
                "tool_program com {} passos excede o orcamento de {MAX_PROGRAM_STEPS}",
                passos.len()
            ))));
        }

        // Os nomes que ALGUM passo deste programa declara com `as`: um
        // `"$nome"` que casa com um deles mas ainda nao tem valor (passo
        // posterior, ou anterior que nao rodou) e referencia sem valor, mesmo
        // que o nome nao tenha cara de identificador.
        let declarados: std::collections::HashSet<&str> = passos
            .iter()
            .filter_map(|p| p.salvar_como.as_deref())
            .collect();
        let mut vars: HashMap<String, i64> = HashMap::new();
        let mut executados = Vec::with_capacity(passos.len());
        // Teto agregado (#1226 S-B, achado de auditoria F-2): checado a
        // cada passo com o relogio do tokio (respeita `start_paused` nos
        // testes), nao envolvendo o loop inteiro num `tokio::time::timeout`
        // — a versao antiga derrubava o future no estouro e levava
        // `executados` junto, informando so "excedeu o teto" sem dizer
        // quantos passos ja tinham rodado (e ja tido efeito colateral).
        let inicio = tokio::time::Instant::now();

        for (i, passo) in passos.iter().enumerate() {
            // Recusa de aninhamento: o passo nunca chega a um segundo
            // `dispatch_tool_call` para `tool_program` — este `if` e a
            // UNICA defesa (nao ha um segundo guard dentro da recursao).
            if passo.tool == TOOL_PROGRAM_NAME {
                executados.push(serde_json::json!({
                    "step": i, "tool": passo.tool, "ok": false,
                    "erro": "aninhamento recusado",
                }));
                return Ok(Saida(ToolOutput::error(
                    serde_json::json!({
                        "steps": executados,
                        "parou_no_passo": i,
                        "motivo": "tool_program nao pode chamar tool_program \
                                   (aninhamento recusado)",
                    })
                    .to_string(),
                )));
            }
            if inicio.elapsed() >= std::time::Duration::from_secs(PROGRAM_AGGREGATE_TIMEOUT_SECS) {
                return Ok(Saida(ToolOutput::error(
                    serde_json::json!({
                        "steps": executados,
                        "parou_no_passo": i,
                        "motivo": format!(
                            "tool_program excedeu o teto agregado de \
                             {PROGRAM_AGGREGATE_TIMEOUT_SECS}s"
                        ),
                    })
                    .to_string(),
                )));
            }
            if !budget.pode_chamar_ferramenta() {
                // #979: o loop principal, no mesmo caso, so reseta o
                // contador do turno e segue — nunca aborta so por causa
                // disto quando a tarefa ainda tem folga. `tool_program` faz
                // o analogo: para graciosamente, e quem chama (o loop
                // principal, na proxima rodada) reseta e continua.
                if budget.atingiu_limite_turno() {
                    return Ok(Saida(ToolOutput::error(
                        serde_json::json!({
                            "steps": executados,
                            "parou_no_passo": i,
                            "motivo": "orcamento do turno esgotado (a tarefa ainda \
                                       tem folga); continue no proximo turno",
                        })
                        .to_string(),
                    )));
                }
                return Err(format!(
                    "execution budget exceeded no passo {i} do tool_program: {}",
                    budget.status()
                ));
            }

            let mut args = passo.args.clone();
            if let Err(nome) = substituir_vars_inteiras(&mut args, &vars, &declarados) {
                // #1226 (achado de revisao): referencia sem valor falha o
                // passo ANTES do despacho — nunca chega a ferramenta como o
                // literal `"$nome"` (um `bash` expandiria no lugar uma
                // variavel de ambiente qualquer). A mensagem nomeia a
                // variavel e nada mais: o passo nao rodou, nao ha saida.
                let erro = format!("variavel ${nome} nao definida");
                executados.push(serde_json::json!({
                    "step": i, "tool": passo.tool, "ok": false, "erro": erro,
                }));
                return Ok(Saida(ToolOutput::error(
                    serde_json::json!({
                        "steps": executados,
                        "parou_no_passo": i,
                        "motivo": format!(
                            "{erro}: \"$nome\" so vale depois que um passo anterior \
                             deste programa salva o valor com `as`"
                        ),
                    })
                    .to_string(),
                )));
            }
            let step_id = format!("{id}#{i}");

            // Box::pin: dispatch_tool_call <-> executar_tool_program e
            // recursao mutua de async fn — sem o box o compilador nao
            // consegue calcular um tamanho finito para o par de futures.
            let desfecho = Box::pin(self.dispatch_tool_call(
                portao,
                budget,
                sink,
                context,
                &step_id,
                &passo.tool,
                &args,
            ))
            .await;

            match desfecho {
                DispatchOutcome::Result(ContentBlock::ToolResult { content, .. }, is_error) => {
                    if is_error {
                        // Achado de revisao (bloqueador 1): sem este ramo,
                        // um passo que falhou (tool desconhecida, timeout,
                        // erro da propria tool) virava "ok": true, e os
                        // passos seguintes rodavam sobre uma dependencia
                        // quebrada. Fail-fast, como um passo negado.
                        executados.push(serde_json::json!({
                            "step": i, "tool": passo.tool, "ok": false,
                            "erro": content,
                        }));
                        return Ok(Saida(ToolOutput::error(
                            serde_json::json!({
                                "steps": executados,
                                "parou_no_passo": i,
                                "motivo": "a ferramenta do passo falhou",
                            })
                            .to_string(),
                        )));
                    }
                    if let Some(nome_var) = &passo.salvar_como {
                        match content.trim().parse::<i64>() {
                            Ok(n) => {
                                vars.insert(nome_var.clone(), n);
                            }
                            Err(_) => {
                                // #1226 (achado de revisao): antes a variavel
                                // so deixava de existir e o passo saia
                                // `"ok": true` — o passo seguinte que usasse
                                // `"$nome"` recebia o literal. `as` promete um
                                // inteiro; saida que nao cumpre falha AQUI,
                                // onde a causa esta. A saida entra no
                                // relatorio porque o passo de fato rodou.
                                executados.push(serde_json::json!({
                                    "step": i, "tool": passo.tool, "ok": false,
                                    "output": content,
                                    "erro": format!(
                                        "a saida nao e um numero inteiro; `as: {nome_var}` \
                                         exige inteiro"
                                    ),
                                }));
                                return Ok(Saida(ToolOutput::error(
                                    serde_json::json!({
                                        "steps": executados,
                                        "parou_no_passo": i,
                                        "motivo": "a saida do passo nao e um numero inteiro, \
                                                   e `as` exige inteiro",
                                    })
                                    .to_string(),
                                )));
                            }
                        }
                    }
                    executados.push(serde_json::json!({
                        "step": i,
                        "tool": passo.tool,
                        "ok": true,
                        "output": content,
                    }));
                }
                DispatchOutcome::Denied(ContentBlock::ToolResult { content, .. }) => {
                    executados.push(serde_json::json!({
                        "step": i,
                        "tool": passo.tool,
                        "ok": false,
                        "denied": content,
                    }));
                    return Ok(Saida(ToolOutput::error(
                        serde_json::json!({
                            "steps": executados,
                            "parou_no_passo": i,
                            "motivo": "negado pelo gate do modo",
                        })
                        .to_string(),
                    )));
                }
                DispatchOutcome::LoopWarning(ContentBlock::ToolResult { content, .. }) => {
                    // #1295: o passo fechou a janela de loop e NAO rodou. O
                    // programa para aqui com o aviso, rotulado como loop (nao
                    // como gate); a proxima repeticao aborta o turno.
                    executados.push(serde_json::json!({
                        "step": i,
                        "tool": passo.tool,
                        "ok": false,
                        "loop": content,
                    }));
                    return Ok(Saida(ToolOutput::error(
                        serde_json::json!({
                            "steps": executados,
                            "parou_no_passo": i,
                            "motivo": "loop detectado: aviso corretivo",
                        })
                        .to_string(),
                    )));
                }
                DispatchOutcome::Paused {
                    tool_result,
                    prompt,
                    ..
                } => {
                    // W3 (v0.4.5): o `prompt` do passo ja chega sem o
                    // marcador — e o texto do humano. O envelope precisa do
                    // pedido CRU, com o marcador: e dele que o despacho do
                    // `tool_program` tira a `fingerprint` registrada e e ele
                    // que o `ToolResult` do modelo carrega para o historico.
                    // O cru e o conteudo do `ToolResult` do passo (um passo
                    // nunca e `tool_program`, entao nao ha relatorio ali). O
                    // `prompt` so serve de reserva: sem marcador, a pausa
                    // nao registra nada e o humano e perguntado de novo
                    // (fail-closed).
                    let prompt = match tool_result {
                        ContentBlock::ToolResult { content, .. } => content,
                        _ => prompt,
                    };
                    // Achado de auditoria/revisao (F-1 / importante 1): o
                    // texto do humano vira a RESPOSTA QUE ELE LE, na mesma
                    // mensagem em que decide aprovar — por isso so o prefixo
                    // curto (nao-JSON), que localiza o passo e diz como
                    // retomar, mais o pedido do passo. Nunca a saida crua dos
                    // passos anteriores (arquivo, stdout).
                    //
                    // O modelo, ao contrario, PRECISA dela (achado de revisao
                    // #1226): os passos 0..i-1 rodaram, e sem o relatorio ele
                    // nao via o que devolveram nem tinha os `vars` para
                    // trocar `"$nome"` na retomada. O `ToolResult` do modelo
                    // comeca pelo mesmo texto do humano — o marcador
                    // `[CONFIRM_REQUIRED:…]` do pedido e o primeiro do
                    // conteudo, e e o primeiro que `ApprovalFingerprint::
                    // from_marker` pega — e segue com o relatorio parcial.
                    let total = passos.len();
                    let para_o_humano = format!(
                        "[tool_program pausado no passo {i} de {total}; ao \
                         confirmar, reenvie so os passos a partir do {i}] {prompt}"
                    );
                    executados.push(serde_json::json!({
                        "step": i, "tool": passo.tool, "ok": false,
                        "aguardando_confirmacao": true,
                    }));
                    let relatorio = serde_json::json!({
                        "steps": executados,
                        "parou_no_passo": i,
                        "vars": vars,
                    });
                    // Endurecimento (revisao do #1337): o relatorio carrega
                    // saida de passo anterior, que pode trazer texto com
                    // cara de marcador. Neutralizado aqui, a seguranca da
                    // aprovacao deixa de depender da ordem (o marcador real
                    // vir primeiro) e passa a valer por construcao: o unico
                    // `[CONFIRM_REQUIRED:` do conteudo e o do pedido.
                    let relatorio = neutralizar_marcadores(&relatorio.to_string());
                    return Ok(DesfechoDoPrograma::Pausa {
                        para_o_modelo: format!("{para_o_humano}\n{relatorio}"),
                        para_o_humano,
                    });
                }
                DispatchOutcome::BudgetExceeded { mensagem } => {
                    return Err(mensagem);
                }
                DispatchOutcome::Result(_, _)
                | DispatchOutcome::Denied(_)
                | DispatchOutcome::LoopWarning(_) => {
                    // `dispatch_tool_call` so constroi `ToolResult` para
                    // estas duas variantes; nunca deveria acontecer, mas o
                    // repo nao usa `unwrap`/`unreachable!` em codigo de
                    // producao — vira erro de passo, nao panico.
                    executados.push(serde_json::json!({
                        "step": i, "tool": passo.tool, "ok": false,
                        "erro": "resposta inesperada do despacho de ferramenta",
                    }));
                    return Ok(Saida(ToolOutput::error(
                        serde_json::json!({
                            "steps": executados,
                            "parou_no_passo": i,
                            "motivo": "resposta inesperada do despacho de ferramenta",
                        })
                        .to_string(),
                    )));
                }
            }
        }

        Ok(Saida(ToolOutput::success(
            serde_json::json!({ "steps": executados }).to_string(),
        )))
    }

    // ── GAR-210: Retry + fallback helpers ────────────────────────────────────

    /// Try `primary` provider with exponential-backoff retries, then fall through
    /// the configured `fallback_providers_list` on retryable errors (429, 5xx).
    ///
    /// `pub` so the Anthropic-compatible shim (`POST /v1/messages`) can reuse
    /// the primary→backup chain instead of reimplementing it. That endpoint is a
    /// proxy, not an agent: it must not go through `process_message_*`, which
    /// injects GarraIA's own tools and executes them itself — the caller needs
    /// the raw `tool_use` blocks back so it can run its own tools.
    pub async fn complete_with_fallback(
        &self,
        primary: &Arc<dyn LlmProvider>,
        request: &LlmRequest,
    ) -> Result<LlmResponse> {
        self.complete_reportando_provider(primary, request)
            .await
            .map(|(resp, _)| resp)
    }

    /// Igual, mas diz **quem** serviu (#984).
    ///
    /// O `/stats` precisa distinguir "o primario respondeu" de "o primario caiu
    /// e um fallback respondeu", e a `LlmResponse` sozinha nao conta essa
    /// historia: ela traz o modelo, nao o provider. Sem isto, "provider
    /// efetivo" seria um palpite.
    pub async fn complete_reportando_provider(
        &self,
        primary: &Arc<dyn LlmProvider>,
        request: &LlmRequest,
    ) -> Result<(LlmResponse, String)> {
        let primary_id = primary.provider_id().to_string();
        let retry_policy = &self.resilience.retry_policy;

        // --- Try primary with retries ---
        let primary_cb = self.resilience.circuit_breaker(&primary_id).await;
        if primary_cb.allow_request().await {
            let mut last_err: Option<Error> = None;
            for attempt in 0..=retry_policy.max_retries {
                match primary.complete(request).await {
                    Ok(resp) => {
                        primary_cb.record_success().await;
                        return Ok((resp, primary_id.clone()));
                    }
                    // #1249: rede caida. Uma tentativa e cai para o fallback,
                    // sem gastar o orcamento de retry: repetir 4 vezes um
                    // endereco inalcancavel queima ~3,5s de backoff antes de
                    // chegar ao provider local, que era a resposta desde o
                    // inicio. O breaker recebe a falha mesmo assim — o
                    // provider esta falhando de verdade.
                    //
                    // Vem **antes** do braco retryable de proposito: transporte
                    // e a classe mais especifica, e a decisao e outra.
                    Err(e) if is_transport_error(&e) => {
                        warn!(
                            "provider '{}' inalcancavel na tentativa {} ({}); indo direto ao fallback",
                            primary_id,
                            attempt + 1,
                            e
                        );
                        primary_cb.record_failure().await;
                        break;
                    }
                    Err(e) if is_retryable_error(&e) => {
                        warn!(
                            "provider '{}' attempt {} failed (retryable): {}",
                            primary_id,
                            attempt + 1,
                            e
                        );
                        primary_cb.record_failure().await;
                        if attempt < retry_policy.max_retries {
                            tokio::time::sleep(retry_policy.delay_for_attempt(attempt)).await;
                        }
                        last_err = Some(e);
                    }
                    Err(e) => return Err(e),
                }
            }
            if let Some(e) = last_err {
                warn!("provider '{}' exhausted retries: {}", primary_id, e);
            }
        } else {
            warn!("provider '{}' circuit open, skipping primary", primary_id);
        }

        // --- Try fallback providers ---
        let fallbacks = self.fallback_providers_list.read().unwrap().clone();
        for fallback_id in &fallbacks {
            if *fallback_id == primary_id {
                continue;
            }
            let Some(fallback) = self.get_provider(fallback_id) else {
                continue;
            };
            let cb = self.resilience.circuit_breaker(fallback_id).await;
            if !cb.allow_request().await {
                warn!("fallback '{}' circuit open, skipping", fallback_id);
                continue;
            }
            info!("provider fallback: trying '{}'", fallback_id);
            match fallback.complete(request).await {
                Ok(resp) => {
                    cb.record_success().await;
                    return Ok((resp, fallback_id.clone()));
                }
                Err(e) => {
                    warn!("fallback '{}' failed: {}", fallback_id, e);
                    cb.record_failure().await;
                }
            }
        }

        Err(Error::Agent(format!(
            "all providers failed (primary: {primary_id}, fallbacks: [{}])",
            fallbacks.join(", ")
        )))
    }

    /// Like `complete_with_fallback` but for streaming.
    /// Tries primary, then fallbacks, returning the first successful stream.
    /// Streaming counterpart of [`complete_with_fallback`], `pub` for the same
    /// reason.
    pub async fn stream_complete_with_fallback(
        &self,
        primary: &Arc<dyn LlmProvider>,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let primary_id = primary.provider_id().to_string();

        match primary.stream_complete(request).await {
            Ok(stream) => return Ok(stream),
            // #1249: o braco de streaming e separado do batch e ficou de fora
            // do fix anterior. Aqui nao existe orcamento de retry para queimar
            // — e uma tentativa e o fallback —, entao transporte entra na
            // mesma porta que 429/5xx.
            Err(e) if is_retryable_error(&e) || is_transport_error(&e) => {
                warn!(
                    "streaming provider '{}' failed, trying fallbacks: {}",
                    primary_id, e
                );
                let cb = self.resilience.circuit_breaker(&primary_id).await;
                cb.record_failure().await;
            }
            Err(e) => return Err(e),
        }

        let fallbacks = self.fallback_providers_list.read().unwrap().clone();
        for fallback_id in &fallbacks {
            if *fallback_id == primary_id {
                continue;
            }
            let Some(fallback) = self.get_provider(fallback_id) else {
                continue;
            };
            let cb = self.resilience.circuit_breaker(fallback_id).await;
            if !cb.allow_request().await {
                continue;
            }
            info!("streaming fallback: trying '{}'", fallback_id);
            match fallback.stream_complete(request).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    warn!("streaming fallback '{}' failed: {}", fallback_id, e);
                    cb.record_failure().await;
                }
            }
        }

        Err(Error::Agent(format!(
            "all streaming providers failed (primary: {primary_id})"
        )))
    }

    pub async fn health_check_all(&self) -> Result<Vec<(String, bool)>> {
        let providers: Vec<Arc<dyn LlmProvider>> = self.providers.read().unwrap().clone();
        let checks = providers.iter().map(|provider| async {
            let provider_id = provider.provider_id().to_string();
            let ok = provider.health_check().await.unwrap_or(false);
            (provider_id, ok)
        });

        Ok(join_all(checks).await)
    }

    async fn remember_system_event(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
        event: &str,
        content: &str,
    ) -> Result<()> {
        let Some(memory) = &self.memory else {
            return Ok(());
        };

        memory
            .remember(NewMemoryEntry {
                tenant_id: "default".to_string(),
                session_id: session_id.to_string(),
                channel_id: None,
                user_id: None,
                continuity_key: continuity_key.map(|s| s.to_string()),
                role: MemoryRole::System,
                content: content.to_string(),
                embedding: None,
                embedding_model: None,
                metadata: serde_json::json!({ "kind": event }),
            })
            .await?;

        Ok(())
    }

    /// Embedding de um turno, a menos que a politica de ruido recuse (#952).
    ///
    /// A entrada e gravada de qualquer jeito: o que se decide aqui e se ela
    /// entra no indice vetorial. Recusar cedo tambem poupa uma ida ao
    /// provider por "ok" — que numa conversa longa nao e pouco.
    ///
    /// Vale so para turno. Fato extraido (`[FACT] ...`) nao passa por aqui:
    /// ele ja e sinal filtrado por um LLM com limiar de confianca, e o texto
    /// que se grava e sempre longo.
    async fn embed_turn_unless_noise(&self, text: &str, papel: &str) -> Option<Vec<f32>> {
        if self.noise_policy.is_noise(text) {
            metrics::inc_ingested(metrics::IngestOutcome::Noise);
            debug!(
                papel,
                chars = text.chars().count(),
                "memoria: turno gravado sem vetor por ser ruido para a busca \
                 semantica (#952); a entrada continua no historico e no recall \
                 textual. Ajuste em `memory.ingestion`."
            );
            return None;
        }

        // #957: o desfecho e contado **aqui**, e nao dentro do
        // `embed_document`, porque so este nivel sabe distinguir os quatro
        // casos. La embaixo, "sem provider" e "provider falhou" saem os dois
        // como `None` — e sao a diferenca entre "ninguem configurou" e
        // "configurou e esta quebrado", que e exatamente o que o operador
        // precisa separar.
        if self.embeddings.is_none() {
            metrics::inc_ingested(metrics::IngestOutcome::NoProvider);
            return None;
        }

        let vetor = self.embed_document(text).await;
        metrics::inc_ingested(if vetor.is_some() {
            metrics::IngestOutcome::Embedded
        } else {
            metrics::IngestOutcome::Failed
        });
        vetor
    }

    async fn embed_document(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        // #957: a medicao cerca a chamada ao provider e nada mais. Incluir o
        // que vem antes ou depois faria o histograma medir o GarraIA em vez de
        // medir o provider, que e a pergunta que o operador tem.
        let inicio = std::time::Instant::now();
        let resultado = provider.embed_documents(&[text.to_string()]).await;

        // Mede sucesso **e** falha, pela mesma razao que o `recall_context`:
        // uma chamada que morre em 30s de timeout e exatamente a latencia que
        // o operador precisa ver. Registrar so o ramo `Ok` faria a p95
        // melhorar durante uma rajada de timeout, que e quando o painel mais
        // precisa piorar. (Apontado pela auditoria do #957: a primeira versao
        // media so o sucesso aqui e os dois no recall, e a assimetria nao
        // tinha defesa.)
        //
        // Sem label de desfecho: o contador de falha ao lado ja responde
        // "quantas falharam", e separar o histograma em duas series faria a
        // pergunta que importa — "quanto o provider esta demorando" — exigir
        // somar as duas de volta.
        metrics::record_embed_latency(
            provider.provider_id(),
            metrics::EmbedOp::Document,
            inicio.elapsed().as_secs_f64(),
        );

        match resultado {
            // Lote vazio conta como falha, e nao como `None` silencioso.
            // Nenhum dos tres providers reais devolve lote vazio para uma
            // entrada, mas tratar isso como sucesso-sem-vetor produziria um
            // `ingested_total{outcome=failed}` sem par em
            // `embed_failures_total`, e o operador veria dois numeros que nao
            // fecham. Apontado pela auditoria do #957.
            Ok(vectors) if vectors.is_empty() => {
                metrics::inc_embed_failure(provider.provider_id(), metrics::EmbedOp::Document);
                warn!(
                    provider = provider.provider_id(),
                    model = provider.model(),
                    "memoria: o provider de embeddings devolveu lote vazio para um \
                     texto; a entrada vai ser gravada sem vetor (#948)"
                );
                None
            }
            Ok(mut vectors) => vectors.pop(),
            Err(e) => {
                // O `.ok()` que existia aqui era o primeiro elo da cadeia de
                // perda silenciosa do #948: a entrada era gravada sem vetor,
                // ficava invisivel para a busca semantica para sempre, e nada
                // no log dizia que tinha acontecido.
                //
                // O #948 tirou o silencio do log; o #957 tira do painel. Log
                // conta o caso, metrica conta a tendencia — e e a tendencia
                // que faz alguem descobrir que o provider caiu antes de o
                // recall degradar.
                metrics::inc_embed_failure(provider.provider_id(), metrics::EmbedOp::Document);
                warn!(
                    provider = provider.provider_id(),
                    model = provider.model(),
                    "memoria: embedding do documento falhou; a entrada vai ser gravada \
                     sem vetor e so volta a ser encontravel por busca semantica depois \
                     de uma reindexacao (#948): {e}"
                );
                None
            }
        }
    }

    async fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        let inicio = std::time::Instant::now();
        let resultado = provider.embed_query(text).await;

        // Sucesso e falha, como no `embed_document` e pelo mesmo motivo.
        metrics::record_embed_latency(
            provider.provider_id(),
            metrics::EmbedOp::Query,
            inicio.elapsed().as_secs_f64(),
        );

        match resultado {
            Ok(vector) => Some(vector),
            Err(e) => {
                metrics::inc_embed_failure(provider.provider_id(), metrics::EmbedOp::Query);
                warn!(
                    provider = provider.provider_id(),
                    model = provider.model(),
                    "memoria: embedding da consulta falhou; o recall deste turno cai \
                     para o caminho textual, sem semantica (#948): {e}"
                );
                None
            }
        }
    }

    fn embedding_model(&self) -> Option<String> {
        self.embeddings
            .as_ref()
            .map(|provider| provider.model().to_string())
    }

    /// Modelo a gravar ao lado de um embedding.
    ///
    /// `None` quando nao ha vetor: a coluna `embedding_model` descreve o
    /// vetor, entao preenche-la sem vetor faz a linha mentir — a entrada
    /// parece indexada por um modelo e nao esta. Com o filtro de modelo do
    /// #954 ativo, essa mentira ainda fazia a entrada perder o eixo semantico
    /// sem que ninguem entendesse por que.
    fn embedding_model_for(&self, embedding: &Option<Vec<f32>>) -> Option<String> {
        embedding.as_ref().and_then(|_| self.embedding_model())
    }

    /// Simple chat completion for use by memory extractor
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        model: Option<String>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String> {
        let provider = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;

        let request = LlmRequest {
            model: model.unwrap_or_default(),
            messages,
            system,
            max_tokens: Some(max_tokens.unwrap_or(4096)),
            temperature: temperature.map(|t| t as f64),
            tools: tools.unwrap_or_default(),
        };

        let response = provider.complete(&request).await?;
        Ok(extract_text(&response.content))
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

/// Rough token estimate: ~4 characters per token.
fn estimate_tokens(
    messages: &[ChatMessage],
    system: &Option<String>,
    tools: &[ToolDefinition],
) -> usize {
    let mut chars: usize = 0;
    if let Some(s) = system {
        chars += s.len();
    }
    for msg in messages {
        match &msg.content {
            MessagePart::Text(t) => chars += t.len(),
            MessagePart::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentBlock::Text { text } => chars += text.len(),
                        ContentBlock::ToolUse { input, .. } => chars += input.to_string().len(),
                        ContentBlock::ToolResult { content, .. } => chars += content.len(),
                        ContentBlock::Image { .. } => chars += 1000,
                    }
                }
            }
        }
    }
    for tool in tools {
        chars += tool.description.len() + tool.input_schema.to_string().len();
    }
    chars / 4
}

/// Drop the oldest messages until the estimated token count fits the budget.
/// Always keeps at least the last message (the current user input).
/// Removes messages in pairs (assistant + tool-result) to avoid breaking
/// the conversation protocol required by LLM APIs.
fn trim_messages_to_budget(
    messages: &mut Vec<ChatMessage>,
    system: &Option<String>,
    tools: &[ToolDefinition],
    max_tokens: usize,
) {
    while messages.len() > 1 && estimate_tokens(messages, system, tools) > max_tokens {
        let has_tool_use = matches!(
            &messages[0].content,
            MessagePart::Parts(parts) if parts.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        );

        messages.remove(0);

        if has_tool_use && messages.len() > 1 {
            let is_tool_result = matches!(
                &messages[0].content,
                MessagePart::Parts(parts) if parts.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
            );
            if is_tool_result {
                messages.remove(0);
            }
        }
    }
}

/// Texto dos blocos, ou `None` quando o modelo nao produziu nenhum.
///
/// #1048: [`extract_text`] troca o vazio por um marcador em ingles, o que
/// serve para os caminhos que precisam de uma `String` sempre — mas apaga
/// justamente a informacao de que o turno veio vazio, e faz `text_len` no log
/// medir o marcador em vez da resposta. Quem precisa **decidir** com base
/// nisso usa esta versao.
fn extract_text_opt(content: &[ContentBlock]) -> Option<String> {
    let text = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_text(content: &[ContentBlock]) -> String {
    extract_text_opt(content)
        .unwrap_or_else(|| "[no textual response provided by the model]".to_string())
}

/// Validação + teto do auto-learning de fatos (TODO 2026-09-02).
///
/// Mantém apenas fatos com `confidence >= 0.80` e key/value não vazios
/// (regra que antes vivia inline no loop). Com `max_facts = Some(n)`,
/// sobrevivem os `n` de **maior confidence** — empate resolve pela ordem
/// de chegada (sort estável). Função pura para ser afirmável em teste.
fn select_learned_facts(
    facts: Vec<crate::memory_extractor::StructuredFact>,
    max_facts: Option<u32>,
) -> Vec<crate::memory_extractor::StructuredFact> {
    let mut valid: Vec<_> = facts
        .into_iter()
        .filter(|f| f.confidence >= 0.80 && !f.key.trim().is_empty() && !f.value.trim().is_empty())
        .collect();
    if let Some(cap) = max_facts {
        valid.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        valid.truncate(cap as usize);
    }
    valid
}
