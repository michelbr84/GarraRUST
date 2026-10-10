//! O `ServerHandler` da ponte: onde as decisoes de [`super::politica`] encontram
//! o `AppState`.
//!
//! Molde: `crates/garraia-cli/src/mcp_server.rs`, que ja implementa
//! `rmcp::ServerHandler` para o servidor stdio (`garra mcp-server`, tool
//! `garra_ask`). Aquele cabecalho dizia "Stdio transport only. No HTTP /
//! Streamable HTTP in this PR" — e vale para o stdio ate hoje. Este arquivo e a
//! continuacao declarada daquele trabalho, do outro lado do transporte — e por
//! isso repete de proposito as escolhas dele: despachante puro, envelope JSON
//! inteiro como conteudo de texto, e a superficie anunciada resolvida por uma
//! politica em vez de um `if` espalhado pelo `call_tool`.
//!
//! ## Inferencia: so `garra_ask`, so com `allow_ask` (#1612)
//!
//! `garra_ask` chama o LLM a pedido de um orquestrador externo, gastando a chave
//! de provider do dono. Por isso ela tem interruptor proprio, lista de modelos e
//! teto de chamadas com sessao propria — ver [`super::politica`]. O resultado e o
//! mesmo envelope `garra.ask.v1` do servidor stdio, e o nucleo que faz a chamada
//! e o `garraia-ask`, compartilhado com a CLI.
//!
//! ## Delegacao de tarefas: `garra_agent`, so com `allow_agent` (#1615)
//!
//! Mudanca de spec: `garra_agent` era so stdio, e a spec do `mcp_http` proibia
//! HTTP para essa tool. A #1615 a abre por HTTP, com opt-in explicito — o stdio
//! segue como estava, e o default da instalacao nao anuncia nada.
//!
//! O agente roda **como o proprio gateway**, e essa e a fronteira que importa:
//!
//! - **Quem pode chamar:** so os orquestradores cuja lista `tools` tem `agent`
//!   (o dono, pelo `gateway.api_key`, passa). Recusa com `tool_not_allowed`.
//! - **Quanto pode gastar:** `allow_agent`, depois um teto proprio de execucoes
//!   por minuto (`agent_budget_per_minute`, sessao `mcp-http-agent`), que nunca
//!   divide o de envio nem o de `garra_ask`. Estourou: `over_budget`.
//! - **O que pode tocar:** o sandbox de `garraia-agents` (#1272). Sem sandbox
//!   valido o `bash` nao existe. As file tools ficam presas as raizes de arquivo
//!   da config, e o pedido **nao** escolhe diretorio (sem `working_dir`).
//! - **Tempo:** `agent_max_seconds` (default 300, teto 600) cobre o laco inteiro.
//! - **Texto:** a entrada e a saida passam por [`super::redacao`]. Prompt,
//!   resposta e saida de tool nunca vao para o log.
//!
//! Ordem fail-closed: interruptor, depois identidade e lista de tools, depois o
//! modelo, depois o teto, e so entao a execucao. A sessao de trabalho recebe o
//! nome do orquestrador, entao duas origens nao dividem historico.
//!
//! ## O que este handler NAO faz
//!
//! - **Nao chama o LLM sem `allow_ask`.** Desligado, `garra_ask` nao e anunciada e
//!   uma chamada direta e recusada antes de qualquer provider ser resolvido.
//! - **Nao registra tool no `AgentRuntime` do `garra_ask`.** A direcao aqui e de
//!   fora para dentro; o caminho de dentro para fora e o `McpManager`, em
//!   `crate::mcp`. O `AgentRuntime` do `garra_ask` so tem o provider. O
//!   `garra_agent` registra as suas tools, no nucleo `crate::agente_mcp`, e so
//!   existe com `allow_agent` (#1615).
//! - **Nao decide quem e o dono, so quem e quem.** A credencial vem do header
//!   `Authorization` e e comparada, aqui, com os orquestradores e com o
//!   `gateway.api_key` (#1613, ver [`identificar`](super::politica::identificar)).
//!   Quem recusa a credencial que nao e de ninguem e o `api_key_layer` do
//!   router, por fora — ver [`super::build_mcp_http_routes`]. Aqui a recusa
//!   `unauthorized` existe so como cinto e suspensorio.
//!
//! ## Orquestradores (#1613)
//!
//! Cada chamada e atribuida a um `Orquestrador` e passa pela politica dele
//! antes da politica global: a tool precisa estar na lista dele, e o destino
//! do envio tambem. Nenhuma das duas listas destrava o que a outra nao libera.
//! Toda chamada gera uma linha de auditoria com o orquestrador, a tool, o
//! destino pseudonimizado e o resultado — nunca o token nem o texto enviado.

use std::sync::{Arc, Weak};

use axum::http::request::Parts;
use rmcp::ErrorData as McpError;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Extensions,
    Implementation, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};

use garraia_ask::{ARG_TIMEOUT_SECS_DEFAULT, AskOptions, AskOutcome, error_envelope};
use garraia_config::defaults::{DEFAULT_CLOUD_MODEL, DEFAULT_CLOUD_PROVIDER};

use super::ferramentas::{
    self, ArgsAgent, ArgsAsk, ArgsListChats, ArgsReadHistory, ArgsSendMessage, TOOL_AGENT,
    TOOL_ASK, TOOL_LIST_CHATS, TOOL_PAIR_STATUS, TOOL_READ_HISTORY, TOOL_SEND_MESSAGE, TOOL_STATUS,
    e_ferramenta_conhecida,
};
use super::politica::{
    Orquestrador, PoliticaMcpHttp, RecusaAgent, RecusaAsk, RecusaOrquestrador, SESSAO_DO_TETO,
    SESSAO_DO_TETO_AGENT, SESSAO_DO_TETO_ASK, identificar,
};
use super::redacao;
use crate::agente_mcp::{self, AgentOptions, AgentOutcome, agent_error_envelope};
use crate::auth_common::extract_bearer;
use crate::channel_send::{SendBudget, with_channel_address};
use crate::push_channels::PushMounted;
use crate::state::AppState;

/// O schema do envelope de resposta, versionado como o `garra.ask.v1` do
/// servidor stdio. Versionar desde a primeira versao e o que permite mudar o
/// corpo depois sem quebrar quem leu a primeira.
const SCHEMA: &str = "garra.mcp.v1";

/// O handler de uma sessao MCP.
///
/// Uma instancia por sessao (o `service_factory` do
/// `StreamableHttpService` chama o construtor a cada `initialize`), e por isso o
/// teto de envios chega de fora, em `Arc`: um orcamento criado por sessao seria
/// um orcamento que o chamador zera abrindo outra sessao.
#[derive(Clone)]
pub struct ManipuladorMcpHttp {
    /// **Weak** pelo mesmo motivo do `TelegramSendTool`: o `AppState` vive mais
    /// que qualquer sessao MCP, e um `Arc` daqui para la fecharia um ciclo que
    /// so nao vaza porque nada nunca e dropado.
    state: Weak<AppState>,
    /// Quantos canais push subiram — o mesmo dado que o `/api/channels` usa
    /// para nao chamar de "offline" um canal que nem entra no registry.
    push: PushMounted,
    /// Anti-amplificacao dos envios, compartilhado por todas as sessoes.
    orcamento: Arc<SendBudget>,
    /// Teto de chamadas de `garra_ask` (#1612). Separado do de envio: gastar
    /// inferencia nao come a cota de mensagens, nem o contrario.
    orcamento_ask: Arc<SendBudget>,
    /// Teto de execucoes de `garra_agent` (#1615). Terceiro orcamento, separado
    /// dos outros dois: uma execucao gasta inferencia e roda tools.
    orcamento_agent: Arc<SendBudget>,
}

impl ManipuladorMcpHttp {
    pub fn novo(
        state: &Arc<AppState>,
        push: PushMounted,
        orcamento: Arc<SendBudget>,
        orcamento_ask: Arc<SendBudget>,
        orcamento_agent: Arc<SendBudget>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            push,
            orcamento,
            orcamento_ask,
            orcamento_agent,
        }
    }

    /// A politica de agora, ou `None` se o gateway ja esta indo embora.
    fn politica(&self) -> Option<(Arc<AppState>, PoliticaMcpHttp)> {
        let state = self.state.upgrade()?;
        let politica = PoliticaMcpHttp::da_config(&state.current_config());
        Some((state, politica))
    }

    /// Quem fez esta chamada, pela credencial do header `Authorization` (#1613).
    ///
    /// Le o header da propria request: o rmcp injeta as `Parts` HTTP nas
    /// extensoes do contexto, e o transporte e stateless, entao a identidade
    /// e por pedido e nunca fica presa numa sessao. `None` quando nao ha
    /// header ou quando a credencial nao e de ninguem.
    fn quem_chama(state: &AppState, extensoes: &Extensions) -> Option<Orquestrador> {
        let parts = extensoes.get::<Parts>()?;
        let apresentada = extract_bearer(&parts.headers)?;
        let config = state.current_config();
        let orquestradores = config
            .gateway
            .mcp_http
            .orquestradores_validos()
            .into_iter()
            .map(|o| (o.nome.clone(), o.token()))
            .collect();
        identificar(
            orquestradores,
            config.gateway.api_key_normalizada(),
            apresentada,
        )
    }

    /// `garra_status`.
    async fn status(&self, state: &Arc<AppState>, politica: &PoliticaMcpHttp) -> JsonValue {
        let canais = crate::channels_view::channel_rows(state, self.push).await;
        json!({
            "schema": SCHEMA,
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": state.boot_time.elapsed().as_secs(),
            "chats_in_memory": state.sessions.len(),
            "channels": canais
                .iter()
                .map(|c| json!({
                    "id": c.id,
                    "status": c.status,
                    "needs_secret": c.needs_secret,
                }))
                .collect::<Vec<_>>(),
            // O que a ponte pode fazer, dito pela propria ponte: e assim que o
            // orquestrador descobre que o envio esta desligado sem ter de
            // tentar um envio para descobrir.
            "mcp_http": {
                "send_enabled": politica.anuncia_envio(),
                "allowed_targets": politica.destinos_liberados(),
                "ask_enabled": politica.anuncia_ask(),
                "agent_enabled": politica.anuncia_agent(),
            },
        })
    }

    /// `garra_list_chats`.
    ///
    /// Le so o que esta em memoria — o mesmo recorte do `GET /api/sessions`.
    /// Uma varredura do `sessions.db` seria outra consulta e outro custo, e o
    /// caso de uso ("com o que este Garra esta lidando agora") e satisfeito
    /// pelo que esta vivo.
    fn list_chats(&self, state: &Arc<AppState>, args: &ArgsListChats) -> JsonValue {
        let filtro = args
            .channel
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());
        let mut chats: Vec<JsonValue> = state
            .sessions
            .iter()
            .filter(|entrada| match filtro {
                None => true,
                Some(canal) => {
                    entrada.channel_id.as_deref() == Some(canal)
                        || entrada.canais_dos_turnos.iter().any(|c| c == canal)
                }
            })
            .map(|entrada| {
                json!({
                    "chat": entrada.id.clone(),
                    "channel": entrada.channel_id.clone(),
                    "channels_seen": entrada.canais_dos_turnos.iter().cloned().collect::<Vec<_>>(),
                    "messages": entrada.history.len(),
                    "connected": entrada.connected,
                    "idle_secs": entrada.last_active.elapsed().as_secs(),
                })
            })
            .collect();
        // Ordem estavel: o `DashMap` nao tem ordem, e um `tools/list` de chats
        // que muda de ordem a cada chamada faz o modelo do outro lado achar que
        // a lista mudou.
        chats.sort_by(|a, b| a["chat"].as_str().cmp(&b["chat"].as_str()));
        json!({
            "schema": SCHEMA,
            "ok": true,
            "chats": chats,
        })
    }

    /// `garra_read_history`.
    ///
    /// Hidrata a sessao antes de ler, como o `GET /api/sessions/{id}/history`
    /// faz: sem isso uma conversa que existe no `sessions.db` mas nao esta em
    /// memoria voltaria vazia, e "vazia" e indistinguivel de "nao existe".
    ///
    /// **Sem `channel_id`.** O `hydrate_session_history` grava a superficie que
    /// o chamador declara em `canais_dos_turnos`, e uma leitura por MCP nao e
    /// um turno de canal nenhum: passar um nome aqui contaminaria o dado que o
    /// `garra_status` consulta para decidir o que retem do operador.
    ///
    /// **A sessao inventada e desfeita.** `hydrate_session_history` CRIA a
    /// sessao em memoria quando ela nao existe — o que e o certo para os
    /// caminhos que o chamam (um turno esta comecando ali), e errado aqui: um
    /// `chat` qualquer numa leitura faria o `DashMap` de sessoes crescer por
    /// pedido, e cada id inventado apareceria no `garra_list_chats` como se
    /// fosse conversa. O rate limit por IP reduz a velocidade disso, nao o teto.
    /// Entao: se a sessao nao existia antes e a hidratacao nao trouxe mensagem
    /// nenhuma, a entrada sai. Conversa de verdade (em memoria ou no
    /// `sessions.db`) nunca cai nesse ramo.
    async fn read_history(
        &self,
        state: &Arc<AppState>,
        politica: &PoliticaMcpHttp,
        args: &ArgsReadHistory,
    ) -> JsonValue {
        let chat = args.chat.trim();
        let existia = state.sessions.contains_key(chat);
        state.hydrate_session_history(chat, None, None).await;
        let historico = state.session_history(chat);
        if !existia {
            // `remove_if` e nao `remove`: entre a hidratacao e a limpeza cabe um
            // turno de verdade nascendo com este mesmo id, e um `remove` cego
            // apagaria a sessao viva que ele acabou de criar. O predicado corre
            // dentro do shard, ja com o valor em maos, entao a decisao "esta
            // vazia" e a remocao sao o mesmo passo.
            state.sessions.remove_if(chat, |_, s| s.history.is_empty());
        }
        let limite = politica.limite_do_historico(args.limit);
        let inicio = historico.len().saturating_sub(limite);
        let mensagens: Vec<JsonValue> = crate::api::mensagens_em_json(&historico[inicio..])
            .into_iter()
            .map(|mut m| {
                // A redacao e a mesma do log (`garraia_security::redact_secrets`):
                // um token que o usuario colou numa conversa nao viaja para um
                // orquestrador externo so porque virou historico.
                if let Some(texto) = m["content"].as_str() {
                    m["content"] = json!(garraia_security::redact_secrets(texto));
                }
                m
            })
            .collect();
        json!({
            "schema": SCHEMA,
            "ok": true,
            "chat": chat,
            "total_messages": historico.len(),
            "returned": mensagens.len(),
            "redacted": true,
            "messages": mensagens,
        })
    }

    /// `garra_pair_status`.
    async fn pair_status(&self, state: &Arc<AppState>) -> JsonValue {
        let canais = crate::channels_view::channel_rows(state, self.push).await;
        // Lock envenenado devolve "desconhecido" em vez de derrubar um caminho
        // de leitura — e a mesma escolha de `registry_commands_for_http`.
        let (modo, dono) = match state.allowlist.lock() {
            Ok(lista) => (
                match lista.mode() {
                    garraia_security::AllowlistMode::Open => "open",
                    garraia_security::AllowlistMode::Restricted => "restricted",
                },
                lista.owner().is_some(),
            ),
            Err(_) => ("unknown", false),
        };
        json!({
            "schema": SCHEMA,
            "ok": true,
            // Nunca o id do dono: a pergunta e "esta pareado?", nao "quem e".
            "owner_claimed": dono,
            "allowlist_mode": modo,
            "channels": canais
                .iter()
                .map(|c| json!({
                    "id": c.id,
                    "display_name": c.display_name,
                    "status": c.status,
                    "awaiting_secret": c.needs_secret,
                }))
                .collect::<Vec<_>>(),
        })
    }

    /// `garra_send_message` — o unico caminho de escrita desta ponte.
    ///
    /// Ordem: politica, teto, entrega. O teto e cobrado **depois** de a politica
    /// aprovar, pelo mesmo motivo do `telegram_send`: um envio recusado que
    /// gastasse cota deixaria um chamador sondando destinos trancar o dono fora
    /// das proprias notificacoes.
    async fn send_message(
        &self,
        state: &Arc<AppState>,
        politica: &PoliticaMcpHttp,
        quem: &Orquestrador,
        args: &ArgsSendMessage,
    ) -> Result<JsonValue, JsonValue> {
        // A primeira trava e a do orquestrador, antes da global: assim ele so
        // ve "nao liberado" para chat que nao e dele, e nao aprende a allowlist
        // do operador. A global ainda vale depois — a interseccao manda.
        if let Err(recusa) = politica.decidir_destino_de(quem, args.chat_id) {
            tracing::warn!(
                canal = %args.channel,
                motivo = recusa.codigo(),
                "mcp_http: garra_send_message recusado"
            );
            return Err(envelope_de_erro(recusa.codigo(), recusa.explicacao()));
        }
        if let Err(recusa) = politica.decidir_envio(&args.channel, args.chat_id) {
            // `channel` entra no log (e um nome de canal, nao um destino);
            // `chat_id` nao entra nunca.
            tracing::warn!(
                canal = %args.channel,
                motivo = recusa.codigo(),
                "mcp_http: garra_send_message recusado"
            );
            return Err(json!({
                "schema": SCHEMA,
                "ok": false,
                "error": { "kind": recusa.codigo(), "message": recusa.explicacao() },
            }));
        }

        if let Err(usados) = self
            .orcamento
            .try_consume(SESSAO_DO_TETO, std::time::Instant::now())
        {
            tracing::warn!(usados, "mcp_http: teto de envios da ponte atingido");
            return Err(json!({
                "schema": SCHEMA,
                "ok": false,
                "error": {
                    "kind": "rate_limited",
                    "message": format!(
                        "limite de {usados} mensagens por minuto na ponte MCP atingido. \
                         Junte o que falta dizer numa unica mensagem."
                    ),
                },
            }));
        }

        let metadata =
            with_channel_address(&json!({}), &args.channel, Some(&args.chat_id.to_string()));
        // #1613: o que sai pelo canal e o texto redigido. Um token que o
        // orquestrador colou no pedido nao chega ao chat de ninguem.
        let texto = redacao::redigir(&args.text);
        let mensagem = garraia_common::Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: garraia_common::SessionId::from_string(SESSAO_DO_TETO),
            channel_id: garraia_common::ChannelId::from_string(&args.channel),
            user_id: garraia_common::UserId::from_string("genesis"),
            direction: garraia_common::MessageDirection::Outgoing,
            content: garraia_common::MessageContent::Text(texto),
            timestamp: chrono::Utc::now(),
            metadata,
        };

        let canais = state.channels.read().await;
        let Some(canal) = canais.get(args.channel.as_str()) else {
            return Err(json!({
                "schema": SCHEMA,
                "ok": false,
                "error": {
                    "kind": "channel_offline",
                    "message": "o canal esta liberado na config, mas nao esta registrado \
                                neste gateway agora. Veja `garra_status`.",
                },
            }));
        };
        match canal.send_message(&mensagem).await {
            Ok(()) => {
                tracing::info!(
                    canal = %args.channel,
                    chars = args.text.chars().count(),
                    "mcp_http: garra_send_message entregue"
                );
                Ok(json!({
                    "schema": SCHEMA,
                    "ok": true,
                    "channel": args.channel,
                    "delivered": true,
                }))
            }
            Err(e) => {
                // O detalhe do canal fica no log, nao na resposta. E texto
                // arbitrario de um servico externo: hoje o teloxide mascara o
                // token do bot antes de formatar o erro de rede, mas isso e
                // garantia de UMA dependencia, e cada canal novo traz o proprio
                // formato. Devolver codigo estavel e a escolha que nao depende
                // de quem esta do outro lado se comportar.
                tracing::warn!(
                    canal = %args.channel,
                    erro = %garraia_security::redact_secrets(&e.to_string()),
                    "mcp_http: garra_send_message falhou na entrega"
                );
                Err(json!({
                    "schema": SCHEMA,
                    "ok": false,
                    "error": {
                        "kind": "delivery_failed",
                        "message": "o canal aceitou o pedido e a entrega falhou. \
                                    O motivo esta no log do gateway.",
                    },
                }))
            }
        }
    }

    /// `garra_ask` (#1612): politica, teto e so depois o LLM.
    ///
    /// A ordem e a do envio: uma recusa de politica nao gasta cota, e o teto e
    /// cobrado antes de qualquer provider ser montado. O sucesso e o envelope
    /// `garra.ask.v1` do stdio, inteiro. Prompt e resposta nunca vao para o log:
    /// so provider, modelo e latencia.
    async fn ask(
        &self,
        state: &Arc<AppState>,
        politica: &PoliticaMcpHttp,
        args: &ArgsAsk,
    ) -> Result<JsonValue, JsonValue> {
        let modelo = args
            .model
            .as_deref()
            .map(str::trim)
            .unwrap_or(DEFAULT_CLOUD_MODEL)
            .to_string();
        let provider = args
            .provider
            .clone()
            .unwrap_or_else(|| DEFAULT_CLOUD_PROVIDER.to_string());

        if let Err(recusa) = politica.decidir_ask(&modelo) {
            // O modelo pedido nao entra no log: e texto de quem chamou, e o
            // motivo ja diz tudo o que o operador precisa.
            tracing::warn!(motivo = recusa.codigo(), "mcp_http: garra_ask recusado");
            return Err(error_envelope(recusa.codigo(), recusa.explicacao()));
        }

        if let Err(usados) = self
            .orcamento_ask
            .try_consume(SESSAO_DO_TETO_ASK, std::time::Instant::now())
        {
            let recusa = RecusaAsk::OrcamentoEsgotado;
            tracing::warn!(
                usados,
                motivo = recusa.codigo(),
                "mcp_http: garra_ask recusado"
            );
            return Err(error_envelope(recusa.codigo(), recusa.explicacao()));
        }

        let opts = AskOptions {
            message: args.message.clone(),
            provider_override: Some(provider.clone()),
            model_override: Some(modelo.clone()),
            url_override: None,
            timeout_secs: args.timeout_secs.unwrap_or(ARG_TIMEOUT_SECS_DEFAULT),
            system_prompt_override: args.system_prompt.clone(),
            // Ponte sem terminal: nunca baixa modelo nem pergunta nada.
            assume_yes: false,
        };
        let config = state.current_config();
        let outcome = garraia_ask::ask_oneshot(&config, opts).await;
        match &outcome {
            AskOutcome::Success { latency_ms, .. } => {
                tracing::info!(
                    provider = %provider,
                    modelo = %modelo,
                    latency_ms = %latency_ms,
                    "mcp_http: garra_ask concluido"
                );
                Ok(outcome.to_envelope())
            }
            AskOutcome::Failure(erro) => {
                tracing::warn!(kind = erro.kind_str(), "mcp_http: garra_ask falhou");
                Err(redigir_erro(outcome.to_envelope()))
            }
        }
    }

    /// `garra_agent` (#1615): politica, teto e so depois o agente completo.
    ///
    /// A ordem e a do `garra_ask`: uma recusa de politica nao gasta cota, e o teto
    /// e cobrado antes de qualquer provider ser montado. A entrada e a saida passam
    /// por [`redacao`]: um token colado no pedido nao vai ao modelo, e um token que
    /// o agente imprimir nao volta ao orquestrador. Nada disso vai para o log:
    /// prompt, resposta e saida de tool ficam de fora; so provider, modelo, tempo
    /// e a contagem de tools entram.
    ///
    /// O diretorio de trabalho NAO vem do pedido: o `working_dir` do stdio e
    /// deliberadamente fora da ponte. As file tools usam o jail das raizes de
    /// arquivo da config, e a sessao de trabalho tem o nome do orquestrador, para
    /// duas origens nunca compartilharem historico nem id.
    async fn agent(
        &self,
        state: &Arc<AppState>,
        politica: &PoliticaMcpHttp,
        quem: &Orquestrador,
        args: &ArgsAgent,
    ) -> Result<JsonValue, JsonValue> {
        let modelo = args
            .model
            .as_deref()
            .map(str::trim)
            .unwrap_or(DEFAULT_CLOUD_MODEL)
            .to_string();
        let provider = args
            .provider
            .clone()
            .unwrap_or_else(|| DEFAULT_CLOUD_PROVIDER.to_string());

        if let Err(recusa) = politica.decidir_agent(quem, &modelo) {
            tracing::warn!(
                orquestrador = quem.rotulo(),
                motivo = recusa.codigo(),
                "mcp_http: garra_agent recusado"
            );
            return Err(recusa_do_agente(&provider, &modelo, recusa));
        }

        if let Err(usados) = self
            .orcamento_agent
            .try_consume(SESSAO_DO_TETO_AGENT, std::time::Instant::now())
        {
            let recusa = RecusaAgent::OrcamentoEsgotado;
            tracing::warn!(
                usados,
                orquestrador = quem.rotulo(),
                motivo = recusa.codigo(),
                "mcp_http: garra_agent recusado"
            );
            return Err(recusa_do_agente(&provider, &modelo, recusa));
        }

        let config = state.current_config();
        let opts = AgentOptions {
            message: redacao::redigir(&args.message),
            provider: provider.clone(),
            model: modelo.clone(),
            timeout_secs: args
                .timeout_secs
                .unwrap_or_else(|| politica.teto_do_agente_secs()),
            system_prompt: args.system_prompt.as_deref().map(redacao::redigir),
            working_dir: None,
        };
        // So as raizes da config: o CWD do processo atende a CLI, nao um pedido
        // de terceiro (ver `FileJail::from_config_roots_plus_cwd`).
        let jail = garraia_agents::FileJail::from_config_roots(&config.agent.file_roots);
        let prefixo = format!("mcp-http-{}", quem.rotulo());
        let outcome = agente_mcp::agent_oneshot(&config, &opts, &jail, &prefixo).await;
        let envelope = redigir_envelope_do_agente(outcome.to_envelope());
        match &outcome {
            AgentOutcome::Success {
                latency_ms,
                tool_calls,
                ..
            } => {
                tracing::info!(
                    orquestrador = quem.rotulo(),
                    provider = %provider,
                    modelo = %modelo,
                    latency_ms = %latency_ms,
                    ferramentas = tool_calls.len(),
                    "mcp_http: garra_agent concluido"
                );
                Ok(envelope)
            }
            AgentOutcome::Failure {
                error, tool_calls, ..
            } => {
                tracing::warn!(
                    orquestrador = quem.rotulo(),
                    kind = error.kind_str(),
                    ferramentas = tool_calls.len(),
                    "mcp_http: garra_agent falhou"
                );
                Err(envelope)
            }
        }
    }

    /// Despacho puro de nome para resultado, para o `call_tool` ficar com uma
    /// forma so: `Ok(valor)` = sucesso, `Err(valor)` = envelope de erro.
    ///
    /// `Err` aqui **nao** e erro de protocolo: e um `CallToolResult::error`, que
    /// o modelo do outro lado le e pode agir sobre. Erro de protocolo
    /// (`McpError`) fica para argumento malformado e tool inexistente, que sao
    /// bug do chamador e nao resultado.
    async fn despachar(
        &self,
        state: &Arc<AppState>,
        politica: &PoliticaMcpHttp,
        quem: &Orquestrador,
        nome: &str,
        argumentos: JsonValue,
    ) -> Result<Result<JsonValue, JsonValue>, McpError> {
        match nome {
            TOOL_STATUS => {
                exigir_sem_argumentos(nome, &argumentos)?;
                Ok(Ok(self.status(state, politica).await))
            }
            TOOL_PAIR_STATUS => {
                exigir_sem_argumentos(nome, &argumentos)?;
                Ok(Ok(self.pair_status(state).await))
            }
            TOOL_LIST_CHATS => {
                let args: ArgsListChats = desserializar(argumentos)?;
                Ok(Ok(self.list_chats(state, &args)))
            }
            TOOL_READ_HISTORY => {
                let args: ArgsReadHistory = desserializar(argumentos)?;
                ferramentas::validar_read_history(&args)
                    .map_err(|e| McpError::invalid_params(e, None))?;
                Ok(Ok(self.read_history(state, politica, &args).await))
            }
            TOOL_SEND_MESSAGE => {
                let args: ArgsSendMessage = desserializar(argumentos)?;
                ferramentas::validar_send_message(&args)
                    .map_err(|e| McpError::invalid_params(e, None))?;
                Ok(self.send_message(state, politica, quem, &args).await)
            }
            TOOL_ASK => {
                let args: ArgsAsk = desserializar(argumentos)?;
                ferramentas::validar_ask(&args).map_err(|e| McpError::invalid_params(e, None))?;
                Ok(self.ask(state, politica, &args).await)
            }
            TOOL_AGENT => {
                let args: ArgsAgent = desserializar(argumentos)?;
                ferramentas::validar_agent(&args, politica.teto_do_agente_secs())
                    .map_err(|e| McpError::invalid_params(e, None))?;
                Ok(self.agent(state, politica, quem, &args).await)
            }
            outro => Err(McpError::invalid_params(
                format!("tool desconhecida: '{outro}'"),
                None,
            )),
        }
    }
}

/// Um envelope `garra.mcp.v1` de recusa, com o `kind` e a explicacao dados.
fn envelope_de_erro(kind: &str, message: &str) -> JsonValue {
    json!({
        "schema": SCHEMA,
        "ok": false,
        "error": { "kind": kind, "message": message },
    })
}

/// O envelope `garra.agent.v1` de uma recusa de `garra_agent`, sem execucao.
fn recusa_do_agente(provider: &str, modelo: &str, recusa: RecusaAgent) -> JsonValue {
    agent_error_envelope(recusa.codigo(), recusa.explicacao(), provider, modelo, &[])
}

/// Redige o texto livre de um envelope `garra.agent.v1` (#1615): a resposta, a
/// mensagem de erro e o resumo de cada tool. O resumo sai do runtime, mas um
/// `bash` que ecoe um token deixaria esse token no resumo, e ele nao sai daqui.
fn redigir_envelope_do_agente(mut envelope: JsonValue) -> JsonValue {
    if let Some(resposta) = envelope.get("answer").and_then(JsonValue::as_str) {
        let redigida = redacao::redigir(resposta);
        envelope["answer"] = json!(redigida);
    }
    if let Some(mensagem) = envelope["error"]["message"].as_str().map(redacao::redigir) {
        envelope["error"]["message"] = json!(mensagem);
    }
    if let Some(chamadas) = envelope
        .get_mut("tool_calls")
        .and_then(JsonValue::as_array_mut)
    {
        for chamada in chamadas {
            if let Some(resumo) = chamada["summary"].as_str().map(redacao::redigir) {
                chamada["summary"] = json!(resumo);
            }
        }
    }
    envelope
}

/// Redige a mensagem de erro de um envelope `garra.ask.v1` (#1613). O provedor
/// pode ecoar um segredo na resposta de erro, e isso nao sai para o cliente.
fn redigir_erro(mut envelope: JsonValue) -> JsonValue {
    let redigida = envelope["error"]["message"].as_str().map(redacao::redigir);
    if let Some(mensagem) = redigida {
        envelope["error"]["message"] = json!(mensagem);
    }
    envelope
}

/// Pseudonimo de um destino para o log: os 4 primeiros bytes do SHA-256 do id.
///
/// E pseudonimo, nao anonimato. Chat ids sao poucos e curtos, entao quem tiver
/// o log consegue testar candidatos. O que o pseudonimo garante e que o log
/// nao traz o id de bate-pronto, e que o operador o reconhece aplicando o mesmo
/// calculo a propria lista.
fn pseudonimo_do_destino(chat_id: i64) -> String {
    let digest = Sha256::digest(chat_id.to_string().as_bytes());
    let hex: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("h:{hex}")
}

/// A linha de auditoria de uma chamada da ponte (#1613).
///
/// Uma por chamada: `info` quando deu certo, `warn` quando foi recusada. O
/// nome de tool vindo de quem chama so entra no log se for uma tool da ponte;
/// o resto vira `desconhecida`. Nunca entra token nem o texto da mensagem.
fn auditar(quem: Option<&Orquestrador>, ferramenta: &str, destino: Option<i64>, resultado: &str) {
    let orquestrador = quem.map_or("desconhecido", Orquestrador::rotulo);
    let ferramenta = if e_ferramenta_conhecida(ferramenta) {
        ferramenta
    } else {
        "desconhecida"
    };
    let destino = destino.map_or_else(|| "-".to_string(), pseudonimo_do_destino);
    if resultado == "ok" {
        tracing::info!(
            orquestrador,
            tool = ferramenta,
            destino = %destino,
            resultado,
            "mcp_http: chamada"
        );
    } else {
        tracing::warn!(
            orquestrador,
            tool = ferramenta,
            destino = %destino,
            resultado,
            "mcp_http: chamada recusada"
        );
    }
}

/// `tools/call` de uma tool sem argumento: um objeto vazio ou nada.
///
/// Recusar o resto e o que mantem o `additionalProperties: false` do schema
/// valendo tambem onde nao existe `struct` para o serde recusar.
fn exigir_sem_argumentos(nome: &str, argumentos: &JsonValue) -> Result<(), McpError> {
    let vazio = match argumentos {
        JsonValue::Object(m) => m.is_empty(),
        JsonValue::Null => true,
        _ => false,
    };
    if vazio {
        Ok(())
    } else {
        Err(McpError::invalid_params(
            format!("{nome} nao recebe argumentos"),
            None,
        ))
    }
}

fn desserializar<T: serde::de::DeserializeOwned>(argumentos: JsonValue) -> Result<T, McpError> {
    serde_json::from_value(argumentos)
        .map_err(|e| McpError::invalid_params(format!("argumentos invalidos: {e}"), None))
}

impl ServerHandler for ManipuladorMcpHttp {
    /// Anuncia a capability `tools` no `initialize`.
    ///
    /// Sem este override o `get_info` default do rmcp devolve `capabilities: {}`,
    /// e o host le isso como "servidor sem tools" e nunca chama `tools/list` —
    /// a pegadinha que a GAR-585 pagou uma vez no servidor stdio. Nenhuma outra
    /// capability entra: esta ponte nao tem prompts, resources nem sampling.
    ///
    /// ## O `server_info` e nosso, nao do SDK
    ///
    /// `ServerInfo::new` preenche `server_info` com
    /// `Implementation::from_build_env()`, que resolve para o `CARGO_PKG_*` do
    /// **rmcp** — sondando a ponte de verdade, o `initialize` respondia
    /// `{"name":"rmcp","version":"3.3.0"}`. E o nome que o host mostra ao
    /// usuario na lista de servidores conectados, entao todo servidor escrito
    /// em rmcp apareceria como o mesmo "rmcp", e o operador com dois servidores
    /// MCP na maquina nao saberia qual e qual. Aqui ele diz GarraIA, com a
    /// versao deste crate.
    /// `ServerInfo` e `#[non_exhaustive]` no rmcp, entao a identidade e escrita
    /// por mutacao em vez de struct-expression com `..` — que nao compila fora
    /// da crate dele.
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("garraia-gateway", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            "Ponte MCP do GarraIA (gateway). Leitura: garra_status, garra_list_chats, \
             garra_read_history (segredos redigidos), garra_pair_status. Escrita: \
             garra_send_message, que so alcanca destinos que o operador liberou na config — \
             nao ha como aprovar um destino novo por aqui, e ela nem aparece na lista de \
             tools quando nenhum envio poderia sair. Inferencia: garra_ask, que gasta a \
             chave de provider do operador; so aparece quando ele a liberou, e so aceita \
             os modelos da lista dele. Agente: garra_agent, que roda tarefas com \
             ferramentas no computador do operador; so aparece quando ele a liberou, e \
             tem teto proprio de execucoes e de tempo."
                .to_string(),
        );
        info
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = match self.politica() {
            // #1613: cada orquestrador ve so as tools que a sua lista libera. Um
            // orquestrador sem identidade nao ve nenhuma — e sem dizer por que.
            Some((state, p)) => match Self::quem_chama(&state, &context.extensions) {
                Some(quem) => ferramentas::tools_anunciadas(&p)
                    .into_iter()
                    .filter(|t| p.anuncia(&quem, &t.name))
                    .collect(),
                None => Vec::new(),
            },
            // Gateway indo embora: superficie vazia em vez de erro. Um
            // `tools/list` durante o shutdown nao e falha do chamador.
            None => Vec::new(),
        };
        let result = ListToolsResult::with_all_items(tools);
        // #1518 / SEP-2549: desde a spec `2026-07-28` o `tools/list` EXIGE
        // `ttlMs` e `cacheScope`, e o `with_all_items` os deixa em `None`, fora
        // do fio: o host descartava a lista inteira. Dicas so para quem negociou
        // >= 2026-07-28, como o `#[tool_handler]` do rmcp. TTL 0 porque a
        // politica muda com a config; `Private` porque a lista depende dela e
        // da credencial de quem chama.
        let exige_dicas_de_cache = context
            .protocol_version()
            .is_some_and(|versao| versao >= ProtocolVersion::V_2026_07_28);
        Ok(if exige_dicas_de_cache {
            result.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
        } else {
            result
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some((state, politica)) = self.politica() else {
            return Err(McpError::internal_error("gateway encerrando", None));
        };
        // Lido antes de `arguments` ser movido: so a tool de envio tem destino.
        let destino = if request.name == TOOL_SEND_MESSAGE {
            request
                .arguments
                .as_ref()
                .and_then(|m| m.get("chat_id"))
                .and_then(JsonValue::as_i64)
        } else {
            None
        };
        let quem = Self::quem_chama(&state, &context.extensions);
        let argumentos = request
            .arguments
            .map(JsonValue::Object)
            .unwrap_or(JsonValue::Null);
        let resultado = match quem.as_ref() {
            None => {
                let recusa = RecusaOrquestrador::NaoAutorizado;
                Ok(Err(envelope_de_erro(recusa.codigo(), recusa.explicacao())))
            }
            Some(q) => {
                // `garra_agent` decide a propria ordem: o interruptor vem antes da
                // lista do orquestrador, e isso se resolve dentro de `agent`.
                let autorizada = if request.name == TOOL_AGENT {
                    Ok(())
                } else {
                    politica.decidir_ferramenta(q, &request.name)
                };
                match autorizada {
                    Ok(()) => {
                        self.despachar(&state, &politica, q, &request.name, argumentos)
                            .await
                    }
                    Err(recusa) => Ok(Err(envelope_de_erro(recusa.codigo(), recusa.explicacao()))),
                }
            }
        };
        let codigo = match &resultado {
            Ok(Ok(_)) => "ok".to_string(),
            Ok(Err(valor)) => valor["error"]["kind"]
                .as_str()
                .unwrap_or("erro")
                .to_string(),
            Err(_) => "erro_de_protocolo".to_string(),
        };
        auditar(quem.as_ref(), &request.name, destino, &codigo);
        let resultado = resultado?;
        let (valor, ok) = match resultado {
            Ok(v) => (v, true),
            Err(v) => (v, false),
        };
        let texto = serde_json::to_string(&valor).unwrap_or_else(|_| {
            format!(
                "{{\"schema\":\"{SCHEMA}\",\"ok\":false,\"error\":\
                 {{\"kind\":\"io\",\"message\":\"json serialization failed\"}}}}"
            )
        });
        let conteudo = vec![ContentBlock::text(texto)];
        if ok {
            Ok(CallToolResult::success(conteudo).into())
        } else {
            Ok(CallToolResult::error(conteudo).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mensagem de erro do provedor pode ecoar a chave: o envelope que sai
    /// para o cliente chega sem ela, e o `kind` continua o mesmo.
    #[test]
    fn erro_de_ask_redige_a_mensagem_do_provedor() {
        let envelope = json!({
            "schema": "garra.ask.v1",
            "ok": false,
            "error": {
                "kind": "provider_error",
                "message": "401 from sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789",
            },
        });
        let saida = redigir_erro(envelope);
        let mensagem = saida["error"]["message"].as_str().unwrap_or_default();
        assert!(!mensagem.contains("sk-or-v1-"), "{mensagem}");
        assert_eq!(saida["error"]["kind"], "provider_error");
    }

    /// O pseudonimo e estavel para o mesmo destino, diferente para outro, e
    /// nao traz o id de bate-pronto.
    #[test]
    fn pseudonimo_do_destino_e_estavel_e_nao_traz_o_id() {
        let p = pseudonimo_do_destino(-100_123_456);
        assert!(!p.contains("100123456"), "{p}");
        assert_eq!(p, pseudonimo_do_destino(-100_123_456));
        assert_ne!(p, pseudonimo_do_destino(-100_123_457));
    }
}
