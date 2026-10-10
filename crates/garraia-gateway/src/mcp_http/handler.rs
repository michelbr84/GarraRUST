//! O `ServerHandler` da ponte: onde as decisoes de [`super::politica`] encontram
//! o `AppState`.
//!
//! Molde: `crates/garraia-cli/src/mcp_server.rs`, que ja implementa
//! `rmcp::ServerHandler` para o servidor stdio (`garra mcp-server`, tool
//! `garra_ask`) e diz no proprio cabecalho "Stdio transport only. No HTTP /
//! Streamable HTTP in this PR". Este arquivo e a continuacao declarada daquele
//! trabalho, do outro lado do transporte — e por isso repete de proposito as
//! escolhas dele: despachante puro, envelope JSON inteiro como conteudo de
//! texto, e a superficie anunciada resolvida por uma politica em vez de um
//! `if` espalhado pelo `call_tool`.
//!
//! ## Inferencia: so `garra_ask`, so com `allow_ask` (#1612)
//!
//! `garra_ask` chama o LLM a pedido de um orquestrador externo, gastando a chave
//! de provider do dono. Por isso ela tem interruptor proprio, lista de modelos e
//! teto de chamadas com sessao propria — ver [`super::politica`]. O resultado e o
//! mesmo envelope `garra.ask.v1` do servidor stdio, e o nucleo que faz a chamada
//! e o `garraia-ask`, compartilhado com a CLI.
//!
//! ## O que este handler NAO faz
//!
//! - **Nao chama o LLM sem `allow_ask`.** Desligado, `garra_ask` nao e anunciada e
//!   uma chamada direta e recusada antes de qualquer provider ser resolvido.
//! - **Nao registra tool no `AgentRuntime`.** A direcao aqui e de fora para
//!   dentro; o caminho de dentro para fora e o `McpManager`, em
//!   `crate::mcp`. O `AgentRuntime` que o `garra_ask` usa so tem o provider.
//! - **Nao decide autenticacao.** Quem exige o `gateway.api_key` e o
//!   `api_key_layer` do router, por fora — ver [`super::build_mcp_http_routes`].

use std::sync::{Arc, Weak};

use rmcp::ErrorData as McpError;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
    Implementation, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde_json::{Value as JsonValue, json};

use garraia_ask::{ARG_TIMEOUT_SECS_DEFAULT, AskOptions, AskOutcome, error_envelope};
use garraia_config::defaults::{DEFAULT_CLOUD_MODEL, DEFAULT_CLOUD_PROVIDER};

use super::ferramentas::{
    self, ArgsAsk, ArgsListChats, ArgsReadHistory, ArgsSendMessage, TOOL_ASK, TOOL_LIST_CHATS,
    TOOL_PAIR_STATUS, TOOL_READ_HISTORY, TOOL_SEND_MESSAGE, TOOL_STATUS,
};
use super::politica::{PoliticaMcpHttp, RecusaAsk, SESSAO_DO_TETO, SESSAO_DO_TETO_ASK};
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
}

impl ManipuladorMcpHttp {
    pub fn novo(
        state: &Arc<AppState>,
        push: PushMounted,
        orcamento: Arc<SendBudget>,
        orcamento_ask: Arc<SendBudget>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            push,
            orcamento,
            orcamento_ask,
        }
    }

    /// A politica de agora, ou `None` se o gateway ja esta indo embora.
    fn politica(&self) -> Option<(Arc<AppState>, PoliticaMcpHttp)> {
        let state = self.state.upgrade()?;
        let politica = PoliticaMcpHttp::da_config(&state.current_config());
        Some((state, politica))
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
        args: &ArgsSendMessage,
    ) -> Result<JsonValue, JsonValue> {
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
        let mensagem = garraia_common::Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: garraia_common::SessionId::from_string(SESSAO_DO_TETO),
            channel_id: garraia_common::ChannelId::from_string(&args.channel),
            user_id: garraia_common::UserId::from_string("genesis"),
            direction: garraia_common::MessageDirection::Outgoing,
            content: garraia_common::MessageContent::Text(args.text.clone()),
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
                Err(outcome.to_envelope())
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
        nome: &str,
        argumentos: JsonValue,
    ) -> Result<Result<JsonValue, JsonValue>, McpError> {
        let Some((state, politica)) = self.politica() else {
            return Err(McpError::internal_error("gateway encerrando", None));
        };

        match nome {
            TOOL_STATUS => {
                exigir_sem_argumentos(nome, &argumentos)?;
                Ok(Ok(self.status(&state, &politica).await))
            }
            TOOL_PAIR_STATUS => {
                exigir_sem_argumentos(nome, &argumentos)?;
                Ok(Ok(self.pair_status(&state).await))
            }
            TOOL_LIST_CHATS => {
                let args: ArgsListChats = desserializar(argumentos)?;
                Ok(Ok(self.list_chats(&state, &args)))
            }
            TOOL_READ_HISTORY => {
                let args: ArgsReadHistory = desserializar(argumentos)?;
                ferramentas::validar_read_history(&args)
                    .map_err(|e| McpError::invalid_params(e, None))?;
                Ok(Ok(self.read_history(&state, &politica, &args).await))
            }
            TOOL_SEND_MESSAGE => {
                let args: ArgsSendMessage = desserializar(argumentos)?;
                ferramentas::validar_send_message(&args)
                    .map_err(|e| McpError::invalid_params(e, None))?;
                Ok(self.send_message(&state, &politica, &args).await)
            }
            TOOL_ASK => {
                let args: ArgsAsk = desserializar(argumentos)?;
                ferramentas::validar_ask(&args).map_err(|e| McpError::invalid_params(e, None))?;
                Ok(self.ask(&state, &politica, &args).await)
            }
            outro => Err(McpError::invalid_params(
                format!("tool desconhecida: '{outro}'"),
                None,
            )),
        }
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
             os modelos da lista dele."
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
            Some((_, p)) => ferramentas::tools_anunciadas(&p),
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
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let argumentos = request
            .arguments
            .map(JsonValue::Object)
            .unwrap_or(JsonValue::Null);
        let resultado = self.despachar(&request.name, argumentos).await?;
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
