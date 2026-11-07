use std::collections::HashMap;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::SinkExt;
use futures::stream::StreamExt;
use tokio::time::Instant;
use tracing::{info, warn};

use garraia_agents::{ChatMessage, TurnEvent};

use crate::state::SharedState;

const MAX_WS_FRAME_BYTES: usize = 64 * 1024;
const MAX_WS_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_WS_TEXT_BYTES: usize = 32 * 1024;

/// Heartbeat: send ping every 30 seconds.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Close the connection if no pong received within 90 seconds.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(90);

/// Per-WebSocket message rate limit: max messages per sliding window.
const WS_RATE_LIMIT_MAX: u32 = 30;
/// Sliding window duration for per-WebSocket rate limiting.
const WS_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// Capacidade do canal de eventos do turno (#1047).
///
/// O produtor usa `send().await`, entao um cliente lento nao perde evento:
/// ele atrasa o turno. Cem eventos e folga suficiente para uma rajada de
/// deltas enquanto o socket escoa.
const TURN_EVENT_CHANNEL: usize = 100;

/// Quantas mensagens do cliente ficam na fila enquanto um turno roda.
///
/// Antes do #1047 o loop nao lia o socket durante o turno e essas mensagens
/// esperavam no buffer do TCP. Agora o socket e lido (senao o `stop` nunca
/// chegaria), entao a fila precisa existir explicitamente — sem ela a
/// mensagem seria descartada, o que seria uma regressao.
const MAX_DEFERRED_MESSAGES: usize = 8;

/// WebSocket upgrade handler.
pub async fn ws_handler(
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    uri: Uri,
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
) -> Response {
    // #1182: anti-CSRF do handshake. CORS nao vale para WebSocket — o
    // `new WebSocket("ws://127.0.0.1:3888/ws")` de uma pagina qualquer sobe
    // sem preflight —, entao a unica barreira de navegador aqui e o `Origin`
    // do handshake. Cliente nao-navegador (app, CLI, `curl`) nao manda
    // `Origin` e nao e afetado; para ele o gate continua sendo `api_key`.
    // O `cross_origin_guard` do router ja julgou este handshake; repetir
    // aqui e defesa em profundidade (o handler nao depende da montagem).
    if !crate::origin_guard::ws_upgrade_permitido(
        &crate::origin_guard::Pedido::de(&headers, &uri),
        crate::origin_guard::esquema_efetivo(&state.config.gateway),
        &crate::origin_guard::origens_validas_silenciosa(&state.config.gateway),
    ) {
        // Nada do pedido e ecoado no corpo; o log nao leva o valor do header.
        warn!("WebSocket upgrade rejected: cross-origin");
        return (StatusCode::FORBIDDEN, crate::origin_guard::CORPO_WS).into_response();
    }

    let gate = crate::gateway_auth::ApiKeyGate::from_config(&state.config.gateway);
    if gate.is_enabled() {
        // A query e aceita **aqui** e so aqui: o handshake WebSocket de um
        // navegador nao leva header. No REST (#1045) a chave vai so no
        // header, porque `fetch` leva e chave em query acaba em log.
        let token_from_query = params.get("token").or_else(|| params.get("api_key"));
        let token_from_header = crate::auth_common::extract_bearer(&headers);
        let token = token_from_query.map(|s| s.as_str()).or(token_from_header);

        // #1045: a comparacao artesanal daqui saia cedo quando os
        // comprimentos diferiam — um oraculo de tamanho por tempo de
        // resposta, o mesmo M-1 que o `/metrics` corrigiu no plan 0024. O
        // helper compartilhado passa os dois por SHA-256 antes do `ct_eq`.
        if !gate.admits(token) {
            warn!("WebSocket connection rejected: invalid API key");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    ws.max_frame_size(MAX_WS_FRAME_BYTES)
        .max_message_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();

    // #1343: quem pode aprovar, no turno seguinte, um pedido de confirmacao
    // que pausar um turno DESTA conexao. O chat web nao tem login e o
    // `resume` sem token e aceito enquanto a sessao esta em memoria, entao o
    // `session_id` nao prova quem e o humano; a conexao prova. O nonce nasce
    // aqui e nunca sai para o cliente: reconectou, pergunta de novo.
    let conexao = crate::approval_scope::nonce_de_conexao();

    // Declarados aqui, e nao junto do loop principal, porque desde o #1047 o
    // socket e lido durante o turno — e a primeira mensagem ja e um turno.
    let mut last_pong = Instant::now();
    // Per-WebSocket sliding window rate limiter
    let mut msg_timestamps: std::collections::VecDeque<Instant> = std::collections::VecDeque::new();
    // #1754: o ultimo turno de voz concluido e a nota de interrupcao para o
    // proximo. Vive na conexao, como o nonce de aprovacao.
    let mut voz = VozDaConexao::default();

    // Wait for the first message to decide: new session or resume.
    let session_id = match tokio::time::timeout(Duration::from_secs(10), receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            if is_init_message(&text) {
                // Client is requesting a fresh session (no resume)
                let id = state.create_session();
                info!("new WebSocket connection (init): session={}", id);
                let token = ws_issue_token(&state, &id).await;
                let mut welcome = serde_json::json!({
                    "type": "connected",
                    "session_id": id,
                });
                if let Some(ref t) = token {
                    welcome["session_token"] = serde_json::Value::String(t.clone());
                }
                if sender
                    .send(Message::Text(welcome.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
                id
            } else if let Some((resume_id, resume_token)) = try_parse_resume_with_token(&text) {
                // GAR-202: If a token is provided, validate it before resuming.
                //
                // `token_verified` and `token_ok` are NOT the same thing, and
                // issue #922 turns on the difference. `token_ok` stays
                // permissive (no token → fall through to the in-memory
                // existence check, preserving the old behaviour for clients
                // that never sent one). `token_verified` is the strict form:
                // a token was presented AND it belongs to this exact session.
                // Only the strict form authorizes re-adopting a session that
                // is no longer in memory.
                let token_verified = if let (Some(t), Some(manager)) =
                    (&resume_token, state.chat_session_manager.as_ref())
                {
                    let idle = state.current_config().gateway.session_idle_secs;
                    manager
                        .validate_token(t, idle)
                        .await
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some(resume_id.as_str())
                } else {
                    false
                };
                let token_ok = token_verified || resume_token.is_none();

                // #1462: sem token verificado, o `session_id` e um valor que
                // o cliente escolheu — vale a mesma regra do `X-Session-Id`
                // e do `{id}` de `/api/sessions/{id}/*`: so sessao das
                // superficies locais do operador. A de um canal esta em
                // memoria no caso normal (a hidratacao do canal a poe la), e
                // retoma-la anexava o chat web a conversa de outra pessoa,
                // com o historico dela indo para o modelo no turno seguinte.
                // Recusada, a sessao nao e tocada: nem `connected`, nem
                // `last_active`. O token verificado continua provando dono.
                let alcancavel_por_id = if token_verified {
                    true
                } else {
                    match state.id_de_sessao_do_cliente_alcanca(&resume_id).await {
                        Ok(alcanca) => alcanca,
                        Err(e) => {
                            warn!(erro = %e, "falhou ao ler o sessions.db para conferir o resume");
                            false
                        }
                    }
                };

                // Issue #922: a gateway restart (or the TTL sweep) empties the
                // in-memory map while `sessions.db` still holds the whole
                // conversation. The client sent `resume`, this lookup missed,
                // and it silently got a fresh UUID whose hydration then found
                // nothing — the reported `history_msgs=0`. With a token that
                // proves ownership, re-adopt the id instead; the hydration in
                // `process_text_message` then loads the history from disk.
                //
                // Existence in the database is deliberately NOT enough on its
                // own: session ids are UUIDs that show up in logs and client
                // storage, and adopting one on request alone would let anyone
                // who learned an id resume someone else's conversation.
                let resumed = if !alcancavel_por_id {
                    warn!(
                        "resume sem token numa sessao de outra superficie; tratado como expirado"
                    );
                    false
                } else if state.resume_session(&resume_id) {
                    token_ok
                } else if token_verified {
                    info!("re-adopting session from store: {}", resume_id);
                    state.adopt_verified_session(&resume_id);
                    true
                } else {
                    false
                };

                if resumed {
                    info!("resumed WebSocket session: {}", resume_id);

                    // Send resume-ack with history length
                    let history_len = state
                        .sessions
                        .get(&resume_id)
                        .map(|s| s.history.len())
                        .unwrap_or(0);
                    // Issue a fresh token on successful resume
                    let new_token = ws_issue_token(&state, &resume_id).await;
                    // Revoke old token (best-effort)
                    if let (Some(old_t), Some(manager)) =
                        (&resume_token, state.chat_session_manager.as_ref())
                    {
                        let _ = manager.revoke_token(old_t).await;
                    }
                    let mut ack = serde_json::json!({
                        "type": "resumed",
                        "session_id": resume_id,
                        "history_length": history_len,
                    });
                    if let Some(ref t) = new_token {
                        ack["session_token"] = serde_json::Value::String(t.clone());
                    }
                    if sender
                        .send(Message::Text(ack.to_string().into()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    resume_id
                } else {
                    // Session expired, doesn't exist, or token invalid — create fresh
                    let id = state.create_session();
                    info!("resume failed (expired/invalid), new session: {}", id);
                    let token = ws_issue_token(&state, &id).await;
                    let mut welcome = serde_json::json!({
                        "type": "connected",
                        "session_id": id,
                        "note": "previous session expired",
                    });
                    if let Some(ref t) = token {
                        welcome["session_token"] = serde_json::Value::String(t.clone());
                    }
                    if sender
                        .send(Message::Text(welcome.to_string().into()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    id
                }
            } else {
                // First message is a regular chat message — create session, process it
                let id = state.create_session();
                info!("new WebSocket connection: session={}", id);
                let token = ws_issue_token(&state, &id).await;
                let mut welcome = serde_json::json!({
                    "type": "connected",
                    "session_id": id,
                });
                if let Some(ref t) = token {
                    welcome["session_token"] = serde_json::Value::String(t.clone());
                }
                if sender
                    .send(Message::Text(welcome.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }

                // Process this first message as a chat message
                if drain_turns(
                    text.to_string(),
                    &id,
                    &conexao,
                    &state,
                    &mut sender,
                    &mut receiver,
                    &mut last_pong,
                    &mut msg_timestamps,
                    &mut voz,
                )
                .await
                .is_break()
                {
                    state.disconnect_session(&id);
                    return;
                }
                id
            }
        }
        _ => {
            // Timeout or error reading first message — create session anyway
            let id = state.create_session();
            info!("new WebSocket connection: session={}", id);
            let token = ws_issue_token(&state, &id).await;
            let mut welcome = serde_json::json!({
                "type": "connected",
                "session_id": id,
            });
            if let Some(ref t) = token {
                welcome["session_token"] = serde_json::Value::String(t.clone());
            }
            let _ = sender.send(Message::Text(welcome.to_string().into())).await;
            id
        }
    };

    // Main message loop with heartbeat
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    // Don't send ping immediately
    heartbeat.tick().await;

    let mut log_rx = state.log_tx.subscribe();

    loop {
        tokio::select! {
            Ok(log_msg) = log_rx.recv() => {
                if sender.send(Message::Text(log_msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            _ = heartbeat.tick() => {
                // Check pong timeout
                if last_pong.elapsed() > HEARTBEAT_TIMEOUT {
                    warn!("heartbeat timeout: session={}", session_id);
                    break;
                }
                // Send ping
                if sender.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_pong = Instant::now(); // text counts as activity
                        if let Some(mut session) = state.sessions.get_mut(&session_id) {
                            session.last_active = std::time::Instant::now();
                        }

                        // Per-WebSocket sliding window rate limit
                        if !rate_limit_admits(&mut msg_timestamps) {
                            warn!("rate limited: session={}, msgs_in_window={}", session_id, msg_timestamps.len());
                            let _ = sender.send(Message::Text(rate_limited_frame().into())).await;
                            continue;
                        }

                        let text_len = text.len();
                        info!("received message: session={}, len={}", session_id, text_len);
                        if text_message_too_large(text_len) {
                            warn!(
                                "dropping oversized ws text message: session={}, len={}, limit={}",
                                session_id, text_len, MAX_WS_TEXT_BYTES
                            );
                            let _ = sender.send(Message::Text(too_large_frame().into())).await;
                            break;
                        }

                        // Um `stop` pode escapar de `run_streaming_turn` — o
                        // laco de la sai assim que o turno termina, mesmo com
                        // uma mensagem ainda por ler no socket. Sem este
                        // guarda ele cairia em `parse_user_message`, que na
                        // falta de `content` usa o JSON inteiro como texto:
                        // o modelo receberia `{"type":"stop"}` como pergunta.
                        // Fora de turno, `stop` e no-op idempotente. Com
                        // `turn_id` e `heard_chars` de um turno de voz ja
                        // concluido (barge-in durante a reproducao), vira a
                        // nota de contexto do proximo turno de voz (#1754).
                        if let Some(stop) = parse_stop(&text) {
                            voz.registrar_interrupcao_tardia(&stop);
                            let _ = sender
                                .send(Message::Text(
                                    stopped_frame(&session_id, stop.turn_id_ecoado()).into(),
                                ))
                                .await;
                            continue;
                        }

                        if drain_turns(
                            text.to_string(),
                            &session_id,
                            &conexao,
                            &state,
                            &mut sender,
                            &mut receiver,
                            &mut last_pong,
                            &mut msg_timestamps,
                            &mut voz,
                        )
                        .await
                        .is_break()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_pong = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed: session={}", session_id);
                        break;
                    }
                    Some(Err(e)) => {
                        warn!("WebSocket error: session={}, error={}", session_id, e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }

    // Mark disconnected (don't remove — allow resume within TTL)
    state.disconnect_session(&session_id);
    info!("session disconnected (resumable): {}", session_id);
}

/// Roda um turno e, em seguida, os que o cliente enfileirou durante ele.
///
/// Itera em vez de recursar: `process_text_message` so empurra em
/// `deferred`, e quem esvazia a fila e este laco.
///
/// `MAX_DEFERRED_MESSAGES` limita a fila **por turno**, nao por chamada: um
/// cliente que enfileira o maximo a cada turno mantem o laco rodando. Quem
/// segura isso e a janela do rate limit, que vale para os dois caminhos de
/// entrada — e cada volta do laco custa um turno de LLM inteiro.
///
/// `Break` significa que o socket morreu — o chamador encerra a conexao.
#[allow(clippy::too_many_arguments)]
async fn drain_turns(
    first: String,
    session_id: &str,
    conexao: &str,
    state: &SharedState,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    last_pong: &mut Instant,
    msg_timestamps: &mut std::collections::VecDeque<Instant>,
    voz: &mut VozDaConexao,
) -> std::ops::ControlFlow<()> {
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    pending.push_back(first);

    while let Some(text) = pending.pop_front() {
        match process_text_message(
            &text,
            session_id,
            conexao,
            state,
            sender,
            receiver,
            last_pong,
            &mut pending,
            msg_timestamps,
            voz,
        )
        .await
        {
            TurnOutcome::Reply(reply) => {
                if sender
                    .send(Message::Text(reply.to_string().into()))
                    .await
                    .is_err()
                {
                    return std::ops::ControlFlow::Break(());
                }
            }
            TurnOutcome::Quiet => {}
            TurnOutcome::ClientGone => return std::ops::ControlFlow::Break(()),
        }
    }

    std::ops::ControlFlow::Continue(())
}

/// O que sobrou de um turno para o loop principal fazer.
enum TurnOutcome {
    /// Frame final (`message` ou `error`) a enviar.
    Reply(serde_json::Value),
    /// Nada a enviar: a mensagem foi recusada inline, ou o turno foi
    /// cancelado e o frame `stopped` ja saiu.
    Quiet,
    /// O socket morreu no meio do turno; o loop principal termina.
    ClientGone,
}

/// Process a text message through validation and the agent runtime.
///
/// Desde o #1047 o turno roda numa task propria e os eventos (`TextDelta`,
/// `ToolStarted`, `ToolFinished`) sao drenados enquanto ele acontece. Isso
/// serve a tres coisas de uma vez: o cliente ve o texto nascendo, o `stop`
/// chega (antes o loop nao lia o socket durante o turno) e o heartbeat
/// deixa de passar fome num turno longo.
///
/// Os frames intermediarios so saem com `"stream": true` na mensagem do
/// cliente. Sem a flag a sequencia e a de sempre — so o `message` final —,
/// e o console web, cujo `default:` imprime frame desconhecido como
/// mensagem de sistema, nao muda de comportamento.
///
/// Mensagens que o cliente mandar durante o turno vao para `deferred`, que o
/// chamador drena depois. Antes elas esperavam no buffer do TCP; agora que o
/// socket e lido ativamente, a fila precisa ser explicita.
#[allow(clippy::too_many_arguments)]
async fn process_text_message(
    text: &str,
    session_id: &str,
    conexao: &str,
    state: &SharedState,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    last_pong: &mut Instant,
    deferred: &mut std::collections::VecDeque<String>,
    msg_timestamps: &mut std::collections::VecDeque<Instant>,
    voz: &mut VozDaConexao,
) -> TurnOutcome {
    let msg = parse_user_message(text);

    // #1754: `turn_id` e opcional, mas quando vem tem de ser um identificador
    // curto e seguro — ele volta em todo frame do turno.
    if msg.turn_id_invalido {
        let err = serde_json::json!({
            "type": "error",
            "session_id": session_id,
            "code": "invalid_turn_id",
            "message": "turn_id must have 1-64 characters [A-Za-z0-9_-]",
        });
        let _ = sender.send(Message::Text(err.to_string().into())).await;
        return TurnOutcome::Quiet;
    }

    // Input validation
    let user_text = garraia_security::InputValidator::sanitize(&msg.content);
    if garraia_security::InputValidator::check_prompt_injection(&user_text) {
        warn!("prompt injection detected: session={}", session_id);
        let err = serde_json::json!({
            "type": "error",
            "session_id": session_id,
            "code": "prompt_injection_detected",
            "message": "input rejected: potential prompt injection detected",
        });
        let _ = sender.send(Message::Text(err.to_string().into())).await;
        return TurnOutcome::Quiet;
    }

    // Ensure session exists and hydrate persisted history for web chat.
    state
        .hydrate_session_history(session_id, Some("web"), None)
        .await;
    let mut history: Vec<ChatMessage> = state.session_history(session_id);
    if msg.voice {
        // This is an additional presentation hint, never a replacement for the
        // persona, tool policy, approval scope or configured system prompt.
        // Providers that only accept a leading system prompt (Anthropic) drop
        // it; the conversation still works, without the spoken-style hint.
        history.push(ChatMessage {
            role: garraia_agents::ChatRole::System,
            content: garraia_agents::MessagePart::Text(VOICE_STYLE_HINT.to_string()),
        });
        // A nota e montada pelo servidor a partir do texto que ele mesmo
        // gerou; nada vindo do navegador vira instrucao de sistema.
        if let Some(nota) = voz.nota_pendente.take() {
            history.push(ChatMessage {
                role: garraia_agents::ChatRole::System,
                content: garraia_agents::MessagePart::Text(nota),
            });
        }
    }
    let continuity_key = state.continuity_key();
    // Lido **antes** do `spawn`: dentro da task seguraria o lock do store
    // pelo tempo do turno inteiro (mesma licao do `parrot_ws.rs`).
    let exec = crate::approval_scope::com_escopo(
        state
            .exec_context_for_msg(session_id, None, Some(&user_text))
            .await,
        crate::approval_scope::CANAL_WEB,
        session_id,
        conexao,
    );

    let (events_tx, events_rx) = tokio::sync::mpsc::channel::<TurnEvent>(TURN_EVENT_CHANNEL);
    let agents = state.agents.clone();
    let sid = session_id.to_string();
    let text_for_agent = user_text.clone();
    let provider_id = msg.provider.clone();
    let model_override = msg.model.clone();
    let voice = msg.voice;
    let task = tokio::spawn(async move {
        let work = agents.process_message_streaming_with_events(
            &sid,
            &text_for_agent,
            &history,
            events_tx,
            continuity_key.as_deref(),
            None,
            provider_id.as_deref(),
            model_override.as_deref(),
            None,
            voice.then_some(1024),
            &exec,
        );
        if voice {
            tokio::time::timeout(Duration::from_secs(120), work)
                .await
                .map_err(|_| garraia_common::Error::Agent("Voice turn timed out".into()))?
        } else {
            work.await
        }
    });

    let turn = run_streaming_turn(
        sender,
        receiver,
        session_id,
        events_rx,
        task,
        msg.stream,
        msg.turn_id.as_deref(),
        last_pong,
        deferred,
        msg_timestamps,
        voz,
    )
    .await;

    match turn {
        TurnEnd::ClientGone => TurnOutcome::ClientGone,
        TurnEnd::Stopped {
            partial,
            heard_chars,
        } => {
            if msg.voice {
                // Keep the user's interrupted request for corrections such as
                // "explain that more simply". Never claim all generated audio
                // was heard. Legacy text cancellation keeps its old semantics.
                let interrupted = interrupted_voice_text(&partial, heard_chars);
                state
                    .persist_turn(session_id, Some("web"), None, &user_text, &interrupted)
                    .await;
            }
            TurnOutcome::Quiet
        }
        TurnEnd::Completed(response_text) => {
            state
                .persist_turn(session_id, Some("web"), None, &user_text, &response_text)
                .await;

            let mut reply = serde_json::json!({
                "type": "message",
                "session_id": session_id,
                "content": response_text,
            });
            com_turn_id(&mut reply, msg.turn_id.as_deref());
            if msg.voice {
                // #1754: quem respondeu de fato. Com `fallback_providers`
                // configurado pelo operador, o primario pode ter caido e outro
                // provider respondido — a UI de voz mostra, em vez de trocar
                // de provider em silencio.
                if let Some(stats) = state.agents.last_turn_stats(session_id) {
                    reply["provider"] = serde_json::Value::String(stats.provider.clone());
                    if let Some(model) = crate::resposta_turno::modelo_do_turno(&stats) {
                        reply["model"] = serde_json::Value::String(model.to_string());
                    }
                    reply["fallback"] = serde_json::Value::Bool(stats.fallback);
                }
                if let Some(turn_id) = &msg.turn_id {
                    voz.ultimo_turno = Some((turn_id.clone(), response_text));
                }
            }
            TurnOutcome::Reply(reply)
        }
        TurnEnd::Failed(message) => {
            warn!("agent error: session={}, error={}", session_id, message);
            let mut reply = serde_json::json!({
                "type": "error",
                "session_id": session_id,
                "code": "agent_error",
                "message": message,
            });
            com_turn_id(&mut reply, msg.turn_id.as_deref());
            TurnOutcome::Reply(reply)
        }
    }
}

/// Dica de apresentacao acrescentada ao historico de um turno de voz.
const VOICE_STYLE_HINT: &str = "This is a live voice conversation. Answer naturally in the user's language, with short spoken sentences. Prefer a concise first answer and offer detail when useful. Preserve accuracy and necessary qualifications. Avoid reading Markdown syntax or code aloud. Treat an interrupted answer as incomplete; generated text may not have been heard.";

/// Teto do trecho falado citado numa nota de interrupcao.
const MAX_NOTE_SPOKEN_CHARS: usize = 2000;

/// Mark generated text as interrupted without pretending it was spoken fully.
///
/// `heard_chars` e o quanto do texto o cliente efetivamente tocou (#1754):
/// com ele, o historico separa o trecho falado do gerado e nao ouvido. Sem
/// ele, o texto inteiro fica marcado como "pode nao ter sido ouvido".
fn interrupted_voice_text(partial: &str, heard_chars: Option<usize>) -> String {
    if partial.is_empty() {
        return "[Voice response interrupted before a reply was delivered.]".to_string();
    }
    let Some(heard) = heard_chars else {
        return format!(
            "[Voice response interrupted. The following text was generated but may not have been heard:]\n{partial}"
        );
    };
    let spoken: String = partial.chars().take(heard).collect();
    let unheard: String = partial.chars().skip(heard).collect();
    match (spoken.is_empty(), unheard.is_empty()) {
        (true, _) => format!(
            "[Voice response interrupted before any of it was spoken. Generated but not heard:]\n{unheard}"
        ),
        (false, true) => {
            format!(
                "[Voice response interrupted by the user after this text was spoken:]\n{spoken}"
            )
        }
        (false, false) => format!(
            "[Voice response interrupted by the user. Spoken aloud:]\n{spoken}\n[Generated but not heard:]\n{unheard}"
        ),
    }
}

/// Nota de contexto para quando o usuario interrompeu a **reproducao** de uma
/// resposta que o servidor ja tinha concluido e persistido inteira (#1754).
///
/// `None` quando tudo foi falado. O texto citado vem da resposta guardada pelo
/// servidor, nunca do navegador: o cliente so manda um numero.
fn nota_de_interrupcao_tardia(resposta: &str, heard_chars: usize) -> Option<String> {
    let total = resposta.chars().count();
    if heard_chars >= total {
        return None;
    }
    let mut spoken: String = resposta.chars().take(heard_chars).collect();
    if heard_chars > MAX_NOTE_SPOKEN_CHARS {
        let skip = heard_chars - MAX_NOTE_SPOKEN_CHARS;
        spoken = format!("…{}", spoken.chars().skip(skip).collect::<String>());
    }
    Some(format!(
        "[Voice note: the user interrupted your previous spoken answer. Only about the first {heard_chars} of {total} characters were spoken aloud; the rest was generated but not heard. Spoken part:]\n{spoken}"
    ))
}

/// Memoria de voz de uma conexao `/ws` (#1754).
#[derive(Debug, Default)]
struct VozDaConexao {
    /// `turn_id` e texto do ultimo turno de voz concluido nesta conexao.
    ultimo_turno: Option<(String, String)>,
    /// Nota de interrupcao para o proximo turno de voz, se houver.
    nota_pendente: Option<String>,
}

impl VozDaConexao {
    /// Um `stop` que chega depois de o turno terminar (barge-in durante a
    /// reproducao). So vale para o ultimo turno de voz concluido, e uma vez.
    fn registrar_interrupcao_tardia(&mut self, stop: &PedidoDeStop) {
        let (AlvoDoStop::Turno(alvo), Some(heard)) = (&stop.alvo, stop.heard_chars) else {
            return;
        };
        if self.ultimo_turno.as_ref().is_none_or(|(id, _)| id != alvo) {
            return;
        }
        if let Some((_, resposta)) = self.ultimo_turno.take() {
            self.nota_pendente = nota_de_interrupcao_tardia(&resposta, heard);
        }
    }
}

/// Como o turno terminou, do ponto de vista do socket.
#[derive(Debug)]
enum TurnEnd {
    Completed(String),
    Failed(String),
    Stopped {
        partial: String,
        heard_chars: Option<usize>,
    },
    ClientGone,
}

/// Drena os eventos do turno **enquanto** ele roda, sem parar de ler o
/// socket.
///
/// Tres braços: o canal de eventos, a task do turno e o proprio cliente. Ler
/// o cliente e o que torna o `stop` possivel — e o mesmo motivo pelo qual o
/// pong volta a ser contado durante um turno longo.
///
/// O frame final so sai depois que `events_rx` fecha, para nenhum `delta`
/// chegar depois do `message`.
#[allow(clippy::too_many_arguments)]
async fn run_streaming_turn<S, R>(
    sender: &mut S,
    receiver: &mut R,
    session_id: &str,
    mut events_rx: tokio::sync::mpsc::Receiver<TurnEvent>,
    mut task: tokio::task::JoinHandle<garraia_common::Result<String>>,
    stream_frames: bool,
    turn_id: Option<&str>,
    last_pong: &mut Instant,
    deferred: &mut std::collections::VecDeque<String>,
    msg_timestamps: &mut std::collections::VecDeque<Instant>,
    voz: &mut VozDaConexao,
) -> TurnEnd
where
    S: futures::Sink<Message> + Unpin,
    R: futures::Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    let mut rx_open = true;
    let mut finished: Option<Result<garraia_common::Result<String>, tokio::task::JoinError>> = None;
    let mut stopped = false;
    let mut heard_chars = None;
    let mut partial = String::new();
    let mut client_gone = false;

    while rx_open || finished.is_none() {
        tokio::select! {
            event = events_rx.recv(), if rx_open => match event {
                Some(event) => {
                    if !stopped && let TurnEvent::TextDelta(text) = &event {
                        let remaining = 32 * 1024_usize - partial.len();
                        let mut end = text.len().min(remaining);
                        while !text.is_char_boundary(end) { end -= 1; }
                        partial.push_str(&text[..end]);
                    }
                    if stream_frames && !stopped
                        && !client_gone
                        && sender
                            .send(Message::Text(
                                turn_event_frame(session_id, turn_id, &event).into(),
                            ))
                            .await
                            .is_err()
                    {
                        // Cliente foi embora no meio do stream. Abortar aqui
                        // poupa os tokens do resto do turno, que nao tem mais
                        // para quem ir.
                        client_gone = true;
                        task.abort();
                    }
                }
                None => rx_open = false,
            },
            joined = &mut task, if finished.is_none() => {
                finished = Some(joined);
            }
            incoming = receiver.next(), if !client_gone => match incoming {
                Some(Ok(Message::Text(raw))) => {
                    *last_pong = Instant::now();
                    match parse_stop(&raw) {
                        Some(stop)
                            if stop.session_id.as_deref().is_some_and(|s| s != session_id) =>
                        {
                            let err = serde_json::json!({
                                "type": "error",
                                "session_id": session_id,
                                "code": "session_mismatch",
                                "message": "stop refers to a different session",
                            });
                            let _ = sender.send(Message::Text(err.to_string().into())).await;
                        }
                        // #1754: um `stop` com `turn_id` de outro turno e de
                        // um turno velho (o cliente interrompeu a reproducao
                        // dele). Nao cancela o turno atual: so confirma.
                        Some(stop) if !stop.cancela(turn_id) => {
                            voz.registrar_interrupcao_tardia(&stop);
                            let _ = sender
                                .send(Message::Text(
                                    stopped_frame(session_id, stop.turn_id_ecoado()).into(),
                                ))
                                .await;
                        }
                        Some(stop) => {
                            heard_chars = stop.heard_chars;
                            stopped = true;
                            task.abort();
                        }
                        // Os mesmos dois guardas do loop principal, na mesma
                        // ordem. Sem eles este braco seria um desvio: a
                        // mensagem que entra durante o turno viraria um turno
                        // igual aos outros, sem ter passado nem pelo teto de
                        // tamanho nem pela janela do rate limit.
                        None if text_message_too_large(raw.len()) => {
                            warn!(
                                "dropping oversized ws text message mid-turn: session={}, len={}, limit={}",
                                session_id,
                                raw.len(),
                                MAX_WS_TEXT_BYTES
                            );
                            let _ = sender.send(Message::Text(too_large_frame().into())).await;
                            client_gone = true;
                            task.abort();
                        }
                        None if !rate_limit_admits(msg_timestamps) => {
                            warn!(
                                "rate limited mid-turn: session={}, msgs_in_window={}",
                                session_id,
                                msg_timestamps.len()
                            );
                            let _ = sender.send(Message::Text(rate_limited_frame().into())).await;
                        }
                        None => {
                            if deferred.len() < MAX_DEFERRED_MESSAGES {
                                deferred.push_back(raw.to_string());
                            } else {
                                let err = serde_json::json!({
                                    "type": "error",
                                    "session_id": session_id,
                                    "code": "turn_in_progress",
                                    "message": "too many messages queued while a turn is running",
                                });
                                let _ = sender.send(Message::Text(err.to_string().into())).await;
                            }
                        }
                    }
                }
                Some(Ok(Message::Pong(_))) => *last_pong = Instant::now(),
                Some(Ok(Message::Close(_))) | None => {
                    client_gone = true;
                    task.abort();
                }
                Some(Err(e)) => {
                    warn!("WebSocket error mid-turn: session={}, error={}", session_id, e);
                    client_gone = true;
                    task.abort();
                }
                _ => {}
            },
        }
    }

    if client_gone {
        return TurnEnd::ClientGone;
    }

    if stopped {
        // O `stop` pode chegar depois de a resposta ficar pronta. O cliente
        // pediu para parar, entao ela e descartada — mas os tokens ja foram
        // gastos e nada e persistido, o que sem log e invisivel em producao.
        if let Some(Ok(Ok(ref pronta))) = finished {
            warn!(
                "turno cancelado com a resposta ja pronta: session={}, len={}",
                session_id,
                pronta.len()
            );
        }
        if sender
            .send(Message::Text(stopped_frame(session_id, turn_id).into()))
            .await
            .is_err()
        {
            return TurnEnd::ClientGone;
        }
        return TurnEnd::Stopped {
            partial,
            heard_chars,
        };
    }

    // Os dois ultimos bracos sao, hoje, inalcancaveis, e estao aqui como rede:
    // `task.abort()` so acontece com `stopped` ou `client_gone`, e os dois ja
    // devolveram acima; e a condicao do `while` so solta com `finished`
    // preenchido. Tratados como desfecho normal em vez de `unreachable!`
    // porque um panic aqui derrubaria a conexao de um usuario — e porque um
    // refactor futuro pode torna-los alcancaveis sem ninguem notar.
    match finished {
        Some(Ok(Ok(text))) => TurnEnd::Completed(text),
        Some(Ok(Err(e))) => TurnEnd::Failed(e.to_string()),
        Some(Err(e)) => {
            if e.is_cancelled() {
                return TurnEnd::Stopped {
                    partial,
                    heard_chars,
                };
            }
            warn!("agent task failed: session={}, error={}", session_id, e);
            TurnEnd::Failed("internal agent task failure".to_string())
        }
        None => TurnEnd::Failed("turn ended without a result".to_string()),
    }
}

/// A janela deslizante do rate limit, num lugar so.
///
/// Vive numa funcao porque desde o #1047 ha **dois** caminhos por onde uma
/// mensagem do cliente entra: o loop principal e o braco do socket dentro de
/// `run_streaming_turn`. Dois lugares contando de formas diferentes seriam
/// dois limites diferentes, e o segundo caminho seria um desvio do primeiro.
///
/// Devolve `false` quando a mensagem passa do teto; nesse caso ela **nao** e
/// contabilizada, para uma rajada nao empurrar a janela para frente.
fn rate_limit_admits(timestamps: &mut std::collections::VecDeque<Instant>) -> bool {
    let now = Instant::now();
    timestamps.retain(|ts| now.duration_since(*ts) < WS_RATE_LIMIT_WINDOW);
    if timestamps.len() >= WS_RATE_LIMIT_MAX as usize {
        return false;
    }
    timestamps.push_back(now);
    true
}

fn rate_limited_frame() -> String {
    serde_json::json!({
        "type": "error",
        "code": "rate_limited",
        "message": "too many messages, please slow down",
        "retry_after_secs": WS_RATE_LIMIT_WINDOW.as_secs(),
    })
    .to_string()
}

fn too_large_frame() -> String {
    serde_json::json!({
        "type": "error",
        "code": "message_too_large",
        "max_bytes": MAX_WS_TEXT_BYTES,
    })
    .to_string()
}

/// Um evento do turno virando frame do protocolo `/ws`.
///
/// `ToolFinished.output` fica **de fora** de proposito: e a saida inteira da
/// ferramenta, ja redigida mas grande (dezenas de KiB), e o frame existe para
/// dizer que a ferramenta terminou, nao para transportar o resultado dela.
fn turn_event_frame(session_id: &str, turn_id: Option<&str>, event: &TurnEvent) -> String {
    let mut value = match event {
        TurnEvent::TextDelta(content) => serde_json::json!({
            "type": "delta",
            "session_id": session_id,
            "content": content,
        }),
        TurnEvent::ToolStarted { name, detail, .. } => serde_json::json!({
            "type": "tool_started",
            "session_id": session_id,
            "name": name,
            "detail": detail,
        }),
        TurnEvent::ToolFinished {
            name,
            duration,
            success,
            summary,
            ..
        } => serde_json::json!({
            "type": "tool_finished",
            "session_id": session_id,
            "name": name,
            "success": success,
            "summary": summary,
            "duration_ms": duration.as_millis() as u64,
        }),
    };
    com_turn_id(&mut value, turn_id);
    value.to_string()
}

fn stopped_frame(session_id: &str, turn_id: Option<&str>) -> String {
    let mut value = serde_json::json!({ "type": "stopped", "session_id": session_id });
    com_turn_id(&mut value, turn_id);
    value.to_string()
}

/// Poe o `turn_id` no frame quando o turno tem um (#1754). Sem `turn_id` o
/// frame fica byte-identico ao de antes.
fn com_turn_id(frame: &mut serde_json::Value, turn_id: Option<&str>) {
    if let (Some(turn_id), Some(obj)) = (turn_id, frame.as_object_mut()) {
        obj.insert(
            "turn_id".to_string(),
            serde_json::Value::String(turn_id.to_string()),
        );
    }
}

/// `turn_id` aceito: 1 a 64 caracteres `[A-Za-z0-9_-]`. Curto e sem nada que
/// precise de escape, porque volta em todo frame do turno.
fn turn_id_valido(turn_id: &str) -> bool {
    (1..=64).contains(&turn_id.len())
        && turn_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Para qual turno um `stop` aponta.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AlvoDoStop {
    /// Sem `turn_id`: o turno em andamento, seja ele qual for (contrato antigo).
    Atual,
    /// Um turno especifico.
    Turno(String),
    /// `turn_id` presente mas invalido: nao cancela nada.
    Invalido,
}

/// Um pedido de cancelamento ja destrinchado.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PedidoDeStop {
    /// Sessao alvo; `None` e "a sessao em que eu estou".
    session_id: Option<String>,
    alvo: AlvoDoStop,
    /// Quantos caracteres da resposta o cliente efetivamente tocou.
    heard_chars: Option<usize>,
}

impl PedidoDeStop {
    /// `true` quando este `stop` cancela o turno `ativo`.
    fn cancela(&self, ativo: Option<&str>) -> bool {
        match &self.alvo {
            AlvoDoStop::Atual => true,
            AlvoDoStop::Turno(alvo) => ativo == Some(alvo.as_str()),
            AlvoDoStop::Invalido => false,
        }
    }

    /// O `turn_id` que volta no `stopped`.
    fn turn_id_ecoado(&self) -> Option<&str> {
        match &self.alvo {
            AlvoDoStop::Turno(turn_id) => Some(turn_id),
            _ => None,
        }
    }
}

/// Um pedido de cancelamento: `{"type": "stop"}`, com `session_id`,
/// `turn_id` e `heard_chars` opcionais (os dois ultimos desde o #1754).
///
/// Devolve `None` quando a mensagem nao e um `stop`.
fn parse_stop(raw: &str) -> Option<PedidoDeStop> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    if value.get("type")?.as_str()? != "stop" {
        return None;
    }
    let alvo = match value.get("turn_id") {
        None | Some(serde_json::Value::Null) => AlvoDoStop::Atual,
        Some(v) => match v.as_str().filter(|t| turn_id_valido(t)) {
            Some(t) => AlvoDoStop::Turno(t.to_string()),
            None => AlvoDoStop::Invalido,
        },
    };
    Some(PedidoDeStop {
        session_id: value
            .get("session_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        alvo,
        heard_chars: value
            .get("heard_chars")
            .and_then(|v| v.as_u64())
            .map(|n| usize::try_from(n.min(1_000_000)).unwrap_or(1_000_000)),
    })
}

/// Try to parse a resume request: `{"type": "resume", "session_id": "..."}`.
fn is_init_message(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v.get("type")?.as_str().map(|s| s == "init"))
        .unwrap_or(false)
}

/// Parse a resume message and return (session_id, optional_token).
fn try_parse_resume_with_token(raw: &str) -> Option<(String, Option<String>)> {
    let v = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    if v.get("type")?.as_str()? != "resume" {
        return None;
    }
    let session_id = v.get("session_id")?.as_str()?.to_string();
    let token = v
        .get("session_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    Some((session_id, token))
}

/// Issue a GAR-202 session token. Returns `None` if no manager is available.
async fn ws_issue_token(state: &crate::state::SharedState, session_id: &str) -> Option<String> {
    let manager = state.chat_session_manager.as_ref()?;
    let cfg = state.current_config();
    match manager
        .create_token(session_id, "web", cfg.gateway.session_ttl_secs, None, None)
        .await
    {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(error = %e, "ws: failed to issue session token");
            None
        }
    }
}

fn text_message_too_large(len: usize) -> bool {
    len > MAX_WS_TEXT_BYTES
}

/// Try to extract `"content"` plus optional `"provider"`, `"model"`, and `"tenant_id"` from JSON,
/// otherwise use the raw text as content with no overrides.
/// Uma mensagem do cliente, ja destrinchada.
#[derive(Debug, Default, PartialEq, Eq)]
struct UserMessage {
    content: String,
    provider: Option<String>,
    model: Option<String>,
    tenant_id: Option<String>,
    /// `"stream": true` liga os frames intermediarios (#1047). Default
    /// `false`: quem nao pede continua vendo so o `message` final, que e o
    /// contrato de antes.
    stream: bool,
    /// Voice presentation and interrupted-context retention are opt-in.
    voice: bool,
    /// Identificador do turno escolhido pelo cliente (#1754); volta em todo
    /// frame do turno para o cliente descartar o que for de turno velho.
    turn_id: Option<String>,
    /// `turn_id` veio, mas fora do formato aceito.
    turn_id_invalido: bool,
}

fn parse_user_message(raw: &str) -> UserMessage {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw)
        && let Some(text) = v.get("content").and_then(|c| c.as_str())
    {
        let provider = v
            .get("provider")
            .and_then(|p| p.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let tenant_id = v
            .get("tenant_id")
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
        let (turn_id, turn_id_invalido) = match v.get("turn_id") {
            None | Some(serde_json::Value::Null) => (None, false),
            Some(t) => match t.as_str().filter(|t| turn_id_valido(t)) {
                Some(t) => (Some(t.to_string()), false),
                None => (None, true),
            },
        };
        return UserMessage {
            content: text.to_string(),
            provider,
            model,
            tenant_id,
            stream,
            voice: v.get("voice").and_then(|v| v.as_bool()).unwrap_or(false),
            turn_id,
            turn_id_invalido,
        };
    }
    UserMessage {
        content: raw.to_string(),
        ..UserMessage::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Duration, Instant, MAX_WS_TEXT_BYTES, Message, TurnEnd, parse_stop, parse_user_message,
        stopped_frame, text_message_too_large, try_parse_resume_with_token, turn_event_frame,
    };
    use futures::FutureExt;
    use futures::StreamExt;
    use garraia_agents::TurnEvent;

    /// Reproduz a decisão de autorização do handler de `resume` (#922) sem
    /// subir um socket: a tabela é o contrato, e o caso perigoso é o da
    /// última linha — id conhecido, sem token, sessão fora da memória.
    fn resume_decision(
        in_memory: bool,
        token_present: bool,
        token_valid_for_this_session: bool,
    ) -> (bool, bool) {
        let token_verified = token_present && token_valid_for_this_session;
        let token_ok = token_verified || !token_present;
        if in_memory {
            (token_ok, false)
        } else if token_verified {
            (true, true) // re-adotada a partir do store
        } else {
            (false, false)
        }
    }

    #[test]
    fn resume_in_memory_keeps_working_without_a_token() {
        // Comportamento pré-existente, preservado: cliente antigo que nunca
        // mandou token continua retomando enquanto a sessão está viva.
        assert_eq!(resume_decision(true, false, false), (true, false));
    }

    #[test]
    fn resume_in_memory_is_refused_by_a_token_for_another_session() {
        // Apresentar um token errado é pior que não apresentar nenhum.
        assert_eq!(resume_decision(true, true, false), (false, false));
        assert_eq!(resume_decision(true, true, true), (true, false));
    }

    /// O coração da #922: sessão fora da memória (gateway reiniciou, ou o TTL
    /// varreu) volta do disco — mas só com prova de posse.
    #[test]
    fn evicted_session_is_readopted_only_with_a_valid_token() {
        assert_eq!(resume_decision(false, true, true), (true, true));
    }

    /// **Sem token, um id conhecido não retoma nada.** Session ids são UUIDs
    /// que aparecem em log e no storage do cliente; adotar um a pedido deixaria
    /// qualquer um que descobrisse um id entrar na conversa alheia. Este é o
    /// teste que impede a "correção" tentadora de aceitar existência no banco
    /// como autorização.
    #[test]
    fn evicted_session_is_not_readopted_without_proof_of_ownership() {
        assert_eq!(resume_decision(false, false, false), (false, false));
        assert_eq!(resume_decision(false, true, false), (false, false));
    }

    /// O frame de resume aceita token opcional — o cliente antigo (que só
    /// manda session_id) tem de continuar parseando.
    #[test]
    fn resume_frame_parses_with_and_without_a_token() {
        let (id, tok) = try_parse_resume_with_token(r#"{"type":"resume","session_id":"s1"}"#)
            .expect("sem token deve parsear");
        assert_eq!(id, "s1");
        assert!(tok.is_none());

        let (id, tok) = try_parse_resume_with_token(
            r#"{"type":"resume","session_id":"s1","session_token":"t1"}"#,
        )
        .expect("com token deve parsear");
        assert_eq!(id, "s1");
        assert_eq!(tok.as_deref(), Some("t1"));

        assert!(try_parse_resume_with_token(r#"{"type":"init"}"#).is_none());
    }

    #[test]
    fn text_message_size_guard_uses_strict_upper_bound() {
        assert!(!text_message_too_large(MAX_WS_TEXT_BYTES));
        assert!(text_message_too_large(MAX_WS_TEXT_BYTES + 1));
    }

    #[test]
    fn parse_user_message_extracts_provider() {
        let json = r#"{"content": "hello", "provider": "anthropic"}"#;
        let msg = parse_user_message(json);
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.provider, Some("anthropic".to_string()));
        assert_eq!(msg.model, None);
        assert_eq!(msg.tenant_id, None);
    }

    #[test]
    fn parse_user_message_extracts_provider_and_model() {
        let json = r#"{"content": "hello", "provider": "ollama", "model": "kimi"}"#;
        let msg = parse_user_message(json);
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.provider, Some("ollama".to_string()));
        assert_eq!(msg.model, Some("kimi".to_string()));
    }

    #[test]
    fn parse_user_message_no_provider() {
        let json = r#"{"content": "hello"}"#;
        let msg = parse_user_message(json);
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.provider, None);
        assert_eq!(msg.model, None);
    }

    #[test]
    fn parse_user_message_plain_text() {
        let msg = parse_user_message("just plain text");
        assert_eq!(msg.content, "just plain text");
        assert_eq!(msg.provider, None);
        assert_eq!(msg.model, None);
    }

    // ─── issue #1047: streaming opt-in, frames e cancelamento ──────────────

    /// A flag e opt-in de verdade: quem nao pede nao recebe frame novo. E o
    /// que mantem o console web — cujo `default:` do `handleServerEvent`
    /// imprime frame desconhecido como mensagem de sistema — igual ao de
    /// antes deste PR.
    #[test]
    fn stream_e_opt_in_e_default_desligado() {
        assert!(parse_user_message(r#"{"content": "oi", "stream": true}"#).stream);
        assert!(!parse_user_message(r#"{"content": "oi", "stream": false}"#).stream);
        assert!(!parse_user_message(r#"{"content": "oi"}"#).stream);
        assert!(!parse_user_message("texto cru").stream);
        // Tipo errado nao liga o streaming por acidente.
        assert!(!parse_user_message(r#"{"content": "oi", "stream": "sim"}"#).stream);
    }

    #[test]
    fn delta_vira_frame_com_o_texto_do_modelo() {
        let frame: serde_json::Value = serde_json::from_str(&turn_event_frame(
            "s1",
            None,
            &TurnEvent::TextDelta("um ".into()),
        ))
        .expect("frame valido");
        assert_eq!(frame["type"], "delta");
        assert_eq!(frame["session_id"], "s1");
        assert_eq!(frame["content"], "um ");
    }

    #[test]
    fn tool_started_vira_frame_com_nome_e_detalhe() {
        let frame: serde_json::Value = serde_json::from_str(&turn_event_frame(
            "s1",
            None,
            &TurnEvent::ToolStarted {
                id: "call-ws".into(),
                name: "bash".into(),
                detail: "cargo test".into(),
                input: serde_json::json!({ "command": "cargo test" }),
            },
        ))
        .expect("frame valido");
        assert_eq!(frame["type"], "tool_started");
        assert_eq!(frame["name"], "bash");
        assert_eq!(frame["detail"], "cargo test");
    }

    /// `output` fica fora do frame de proposito: e a saida inteira da
    /// ferramenta, ate dezenas de KiB, e o frame existe para dizer que ela
    /// terminou — nao para transportar o resultado.
    #[test]
    fn tool_finished_leva_o_resumo_e_nunca_a_saida_inteira() {
        let frame: serde_json::Value = serde_json::from_str(&turn_event_frame(
            "s1",
            None,
            &TurnEvent::ToolFinished {
                id: "call-ws".into(),
                name: "bash".into(),
                duration: std::time::Duration::from_millis(1500),
                success: true,
                summary: "148 passou".into(),
                output: "x".repeat(64 * 1024),
            },
        ))
        .expect("frame valido");
        assert_eq!(frame["type"], "tool_finished");
        assert_eq!(frame["name"], "bash");
        assert_eq!(frame["success"], true);
        assert_eq!(frame["summary"], "148 passou");
        assert_eq!(frame["duration_ms"], 1500);
        assert!(
            frame.get("output").is_none(),
            "a saida inteira da ferramenta vazou para o frame: {frame}"
        );
    }

    #[test]
    fn stopped_frame_nomeia_a_sessao() {
        let frame: serde_json::Value =
            serde_json::from_str(&stopped_frame("s1", None)).expect("frame valido");
        assert_eq!(frame["type"], "stopped");
        assert_eq!(frame["session_id"], "s1");
    }

    // ─── o laco do turno, sem provider e sem socket ───────────────────────
    //
    // `run_streaming_turn` e generico sobre sink e stream justamente para
    // caber aqui: os canais em memoria fazem o papel do cliente, e o
    // `JoinHandle` faz o papel do turno. Nada de LLM, nada de rede — o que
    // se afirma e o laco: ordem dos frames, opt-in, cancelamento e fila.

    type ClienteFalso = futures::channel::mpsc::UnboundedReceiver<Result<Message, axum::Error>>;

    fn cliente(mensagens: &[&str]) -> ClienteFalso {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        for m in mensagens {
            tx.unbounded_send(Ok(Message::Text((*m).to_string().into())))
                .expect("cliente falso aceita a mensagem");
        }
        // O `tx` fica vivo ate o fim do processo de teste: solta-lo aqui
        // fecharia o stream, e o laco leria `None` — que ele trata como
        // "cliente foi embora", nao como "cliente calado".
        let _mantem_aberto = std::mem::ManuallyDrop::new(tx);
        rx
    }

    /// Os `type` dos frames que sairam, na ordem.
    fn tipos_dos_frames(rx: futures::channel::mpsc::UnboundedReceiver<Message>) -> Vec<String> {
        frames(rx)
            .iter()
            .map(|f| f["type"].as_str().unwrap_or("?").to_string())
            .collect()
    }

    fn frames(rx: futures::channel::mpsc::UnboundedReceiver<Message>) -> Vec<serde_json::Value> {
        rx.collect::<Vec<_>>()
            .now_or_never()
            // Sem o `drop(sink)` antes daqui a coleta fica pendente e
            // devolveria `None`; falhar com esta frase e mais util do que
            // comparar contra uma lista vazia.
            .expect("solte o sink antes de coletar os frames")
            .into_iter()
            .filter_map(|m| match m {
                Message::Text(t) => serde_json::from_str(&t).ok(),
                _ => None,
            })
            .collect()
    }

    fn eventos_prontos(eventos: Vec<TurnEvent>) -> tokio::sync::mpsc::Receiver<TurnEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(super::TURN_EVENT_CHANNEL);
        for e in eventos {
            tx.try_send(e).expect("canal comporta os eventos do teste");
        }
        drop(tx);
        rx
    }

    /// Com a flag ligada, cada evento vira um frame, na ordem em que o
    /// runtime os emitiu — e todos saem **antes** do fim do turno, porque o
    /// laco so devolve depois que o canal de eventos fecha.
    #[tokio::test]
    async fn com_a_flag_cada_evento_vira_frame_na_ordem() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let eventos = eventos_prontos(vec![
            TurnEvent::TextDelta("um ".into()),
            TurnEvent::ToolStarted {
                id: "call-eco".into(),
                name: "eco".into(),
                detail: "oi".into(),
                input: serde_json::json!({ "texto": "oi" }),
            },
            TurnEvent::ToolFinished {
                id: "call-eco".into(),
                name: "eco".into(),
                duration: std::time::Duration::from_millis(7),
                success: true,
                summary: "ok".into(),
                output: "saida enorme".into(),
            },
            TurnEvent::TextDelta("dois".into()),
        ]);
        let task = tokio::spawn(async { Ok("um dois".to_string()) });

        let fim = super::run_streaming_turn(
            &mut sink,
            &mut cliente,
            "s1",
            eventos,
            task,
            true,
            None,
            &mut pong,
            &mut fila,
            &mut janela,
            &mut super::VozDaConexao::default(),
        )
        .await;

        assert!(
            matches!(fim, TurnEnd::Completed(ref t) if t == "um dois"),
            "{fim:?}"
        );
        drop(sink);
        assert_eq!(
            tipos_dos_frames(sink_rx),
            ["delta", "tool_started", "tool_finished", "delta"]
        );
    }

    /// Sem a flag, o mesmo turno nao emite frame nenhum pelo caminho — o
    /// cliente so recebe o `message` final, que quem manda e o chamador.
    /// E esta a garantia de que o console web nao muda.
    #[tokio::test]
    async fn sem_a_flag_nenhum_frame_intermediario_sai() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let eventos = eventos_prontos(vec![
            TurnEvent::TextDelta("um ".into()),
            TurnEvent::TextDelta("dois".into()),
        ]);
        let task = tokio::spawn(async { Ok("um dois".to_string()) });

        let fim = super::run_streaming_turn(
            &mut sink,
            &mut cliente,
            "s1",
            eventos,
            task,
            false,
            None,
            &mut pong,
            &mut fila,
            &mut janela,
            &mut super::VozDaConexao::default(),
        )
        .await;

        assert!(matches!(fim, TurnEnd::Completed(_)), "{fim:?}");
        drop(sink);
        assert!(
            frames(sink_rx).is_empty(),
            "frame intermediario saiu sem ninguem pedir"
        );
    }

    /// O `stop` do cliente aborta a task e responde `stopped`. O turno segue
    /// pendurado por 60 s se nao for cancelado, entao o `timeout` de 5 s e a
    /// prova de que o abort aconteceu de verdade.
    #[tokio::test]
    async fn stop_do_cliente_cancela_o_turno_e_responde_stopped() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"type":"stop","session_id":"s1"}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let eventos = eventos_prontos(vec![TurnEvent::TextDelta("parcial".into())]);
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok("nunca chega".to_string())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos,
                task,
                true,
                None,
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("o stop tem de soltar o turno antes dos 5 s");

        assert!(matches!(fim, TurnEnd::Stopped { .. }), "{fim:?}");
        drop(sink);
        let tipos = tipos_dos_frames(sink_rx);
        assert_eq!(
            tipos.last().map(String::as_str),
            Some("stopped"),
            "o ultimo frame tinha de ser o stopped: {tipos:?}"
        );
        assert!(
            !tipos.iter().any(|t| t == "message"),
            "turno cancelado nao manda resposta: {tipos:?}"
        );
    }

    /// `stop` que nomeia outra sessao nao cancela nada — responde
    /// `session_mismatch` e o turno segue.
    #[tokio::test]
    async fn stop_de_outra_sessao_nao_cancela() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"type":"stop","session_id":"outra"}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let eventos = eventos_prontos(vec![]);
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok("terminei".to_string())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos,
                task,
                true,
                None,
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("o turno tinha de terminar sozinho");

        assert!(
            matches!(fim, TurnEnd::Completed(ref t) if t == "terminei"),
            "{fim:?}"
        );
        drop(sink);
        let codigos: Vec<String> = frames(sink_rx)
            .iter()
            .map(|f| f["code"].as_str().unwrap_or("?").to_string())
            .collect();
        assert_eq!(codigos, ["session_mismatch"]);
    }

    /// A auditoria do #1047 achou isto: mensagem lida **durante** o turno
    /// entrava na fila sem passar pela janela do rate limit, entao um cliente
    /// enfileirava turnos de graca. A janela e a mesma dos dois lados.
    #[tokio::test]
    async fn mensagem_durante_o_turno_conta_no_rate_limit() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"content":"mais uma"}"#, r#"{"type":"stop"}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        // Janela ja no teto, como se o cliente tivesse gasto a cota toda.
        let mut janela: std::collections::VecDeque<Instant> = std::iter::repeat_with(Instant::now)
            .take(super::WS_RATE_LIMIT_MAX as usize)
            .collect();

        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(String::new())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos_prontos(vec![]),
                task,
                true,
                None,
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("o stop tem de soltar o turno");

        assert!(matches!(fim, TurnEnd::Stopped { .. }), "{fim:?}");
        assert!(
            fila.is_empty(),
            "mensagem passou do teto e mesmo assim entrou na fila: {fila:?}"
        );
        drop(sink);
        let codigos: Vec<String> = frames(sink_rx)
            .iter()
            .filter_map(|f| f["code"].as_str().map(str::to_string))
            .collect();
        assert_eq!(codigos, ["rate_limited"]);
    }

    /// O outro guarda que faltava: o teto de tamanho do texto. Um cliente
    /// enfileirava ate 8 mensagens acima de `MAX_WS_TEXT_BYTES` porque o
    /// check so existia no loop principal.
    #[tokio::test]
    async fn mensagem_gigante_durante_o_turno_e_recusada() {
        let gigante = format!(
            r#"{{"content":"{}"}}"#,
            "a".repeat(super::MAX_WS_TEXT_BYTES + 1)
        );
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[&gigante]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(String::new())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos_prontos(vec![]),
                task,
                true,
                None,
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("a recusa tem de soltar o turno");

        assert!(matches!(fim, TurnEnd::ClientGone), "{fim:?}");
        assert!(fila.is_empty(), "mensagem gigante entrou na fila: {fila:?}");
        drop(sink);
        let codigos: Vec<String> = frames(sink_rx)
            .iter()
            .filter_map(|f| f["code"].as_str().map(str::to_string))
            .collect();
        assert_eq!(codigos, ["message_too_large"]);
    }

    /// Antes do #1047 o socket nao era lido durante o turno e a mensagem
    /// esperava no buffer do TCP. Agora que ele e lido, a mensagem tem de ir
    /// para a fila — descarta-la seria a regressao.
    #[tokio::test]
    async fn mensagem_durante_o_turno_entra_na_fila_em_vez_de_sumir() {
        let (mut sink, _sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"content":"a proxima pergunta"}"#, r#"{"type":"stop"}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();

        let eventos = eventos_prontos(vec![]);
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(String::new())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos,
                task,
                true,
                None,
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("o stop tem de soltar o turno");

        assert!(matches!(fim, TurnEnd::Stopped { .. }), "{fim:?}");
        assert_eq!(
            fila.into_iter().collect::<Vec<_>>(),
            [r#"{"content":"a proxima pergunta"}"#.to_string()],
        );
    }

    /// `stop` sem `session_id` vale para a sessao corrente; com `session_id`,
    /// so para aquela. O resto nao e `stop` e segue para a fila de turnos.
    #[test]
    fn parse_stop_distingue_alvo_e_nao_stop() {
        let stop = |raw| parse_stop(raw).map(|s| s.session_id);
        assert_eq!(stop(r#"{"type":"stop"}"#), Some(None));
        assert_eq!(
            stop(r#"{"type":"stop","session_id":"s1"}"#),
            Some(Some("s1".to_string()))
        );
        // String vazia e o mesmo que nao mandar.
        assert_eq!(stop(r#"{"type":"stop","session_id":""}"#), Some(None));
        assert_eq!(parse_stop(r#"{"content":"stop"}"#), None);
        assert_eq!(parse_stop(r#"{"type":"resume"}"#), None);
        assert_eq!(parse_stop("stop"), None);
        assert_eq!(parse_stop("{nao e json"), None);
    }

    // ─── #1754: turno de voz (turn_id, stop por turno, contexto) ─────────

    #[test]
    fn turn_id_valido_volta_e_invalido_e_marcado() {
        let msg = parse_user_message(r#"{"content":"oi","voice":true,"turn_id":"v-1_abc"}"#);
        assert!(msg.voice);
        assert_eq!(msg.turn_id.as_deref(), Some("v-1_abc"));
        assert!(!msg.turn_id_invalido);
        for ruim in [
            r#"{"content":"oi","turn_id":""}"#,
            r#"{"content":"oi","turn_id":"tem espaco"}"#,
            r#"{"content":"oi","turn_id":"<script>"}"#,
            r#"{"content":"oi","turn_id":7}"#,
        ] {
            let msg = parse_user_message(ruim);
            assert!(msg.turn_id_invalido, "{ruim}");
            assert_eq!(msg.turn_id, None, "{ruim}");
        }
        let longo = format!(r#"{{"content":"oi","turn_id":"{}"}}"#, "a".repeat(65));
        assert!(parse_user_message(&longo).turn_id_invalido);
        // Sem o campo, nada muda para o cliente antigo.
        let antigo = parse_user_message(r#"{"content":"oi"}"#);
        assert!(!antigo.turn_id_invalido && antigo.turn_id.is_none());
    }

    #[test]
    fn frames_levam_o_turn_id_so_quando_o_turno_tem_um() {
        let com: serde_json::Value = serde_json::from_str(&turn_event_frame(
            "s1",
            Some("v-2"),
            &TurnEvent::TextDelta("oi".into()),
        ))
        .expect("frame valido");
        assert_eq!(com["turn_id"], "v-2");
        let sem = turn_event_frame("s1", None, &TurnEvent::TextDelta("oi".into()));
        assert!(!sem.contains("turn_id"), "{sem}");
        let parou: serde_json::Value =
            serde_json::from_str(&stopped_frame("s1", Some("v-2"))).expect("frame valido");
        assert_eq!(parou["turn_id"], "v-2");
        assert_eq!(
            stopped_frame("s1", None),
            r#"{"session_id":"s1","type":"stopped"}"#,
            "sem turn_id o frame e o de antes"
        );
    }

    #[test]
    fn stop_com_turn_id_so_cancela_o_proprio_turno() {
        let stop =
            parse_stop(r#"{"type":"stop","turn_id":"v-1","heard_chars":12}"#).expect("e um stop");
        assert_eq!(stop.heard_chars, Some(12));
        assert_eq!(stop.turn_id_ecoado(), Some("v-1"));
        assert!(stop.cancela(Some("v-1")));
        assert!(!stop.cancela(Some("v-2")), "stop de turno velho");
        assert!(
            !stop.cancela(None),
            "turno sem id nao e alvo de stop com id"
        );
        // Sem turn_id: o contrato antigo, cancela o que estiver rodando.
        let antigo = parse_stop(r#"{"type":"stop"}"#).expect("e um stop");
        assert!(antigo.cancela(Some("v-9")) && antigo.cancela(None));
        // turn_id invalido nao cancela nada.
        let ruim = parse_stop(r#"{"type":"stop","turn_id":"a b"}"#).expect("e um stop");
        assert!(!ruim.cancela(Some("a b")) && !ruim.cancela(None));
        assert_eq!(ruim.turn_id_ecoado(), None);
        // heard_chars negativo ou nao numerico e ignorado.
        let sem_numero = parse_stop(r#"{"type":"stop","heard_chars":"10"}"#).expect("e um stop");
        assert_eq!(sem_numero.heard_chars, None);
    }

    #[test]
    fn interrupcao_separa_o_falado_do_gerado_sem_quebrar_unicode() {
        let parcial = "Olá, tudo bem? Hoje está ótimo 🦀.";
        let texto = super::interrupted_voice_text(parcial, Some(5));
        assert!(texto.contains("Spoken aloud:]\nOlá, "), "{texto}");
        assert!(texto.contains("not heard:]\ntudo bem?"), "{texto}");
        let nada = super::interrupted_voice_text(parcial, Some(0));
        assert!(nada.contains("before any of it was spoken"), "{nada}");
        let tudo = super::interrupted_voice_text(parcial, Some(10_000));
        assert!(tudo.contains("after this text was spoken"), "{tudo}");
        let sem_info = super::interrupted_voice_text(parcial, None);
        assert!(sem_info.contains("may not have been heard"), "{sem_info}");
        assert!(super::interrupted_voice_text("", Some(3)).contains("before a reply"));
    }

    #[test]
    fn interrupcao_tardia_vira_nota_uma_vez_e_so_do_ultimo_turno() {
        let mut voz = super::VozDaConexao {
            ultimo_turno: Some(("v-1".into(), "Primeira frase. Segunda frase.".into())),
            nota_pendente: None,
        };
        // Stop de outro turno: ignorado.
        voz.registrar_interrupcao_tardia(
            &parse_stop(r#"{"type":"stop","turn_id":"v-0","heard_chars":3}"#).expect("stop"),
        );
        assert!(voz.nota_pendente.is_none());
        // Sem heard_chars nao ha o que anotar.
        voz.registrar_interrupcao_tardia(
            &parse_stop(r#"{"type":"stop","turn_id":"v-1"}"#).expect("stop"),
        );
        assert!(voz.nota_pendente.is_none());
        voz.registrar_interrupcao_tardia(
            &parse_stop(r#"{"type":"stop","turn_id":"v-1","heard_chars":16}"#).expect("stop"),
        );
        let nota = voz.nota_pendente.clone().expect("nota registrada");
        assert!(nota.contains("first 16 of 30 characters"), "{nota}");
        assert!(nota.ends_with("Primeira frase. "), "{nota}");
        assert!(
            !nota.contains("Segunda"),
            "o nao falado nao e citado: {nota}"
        );
        // O mesmo turno nao gera nota de novo.
        voz.nota_pendente = None;
        voz.registrar_interrupcao_tardia(
            &parse_stop(r#"{"type":"stop","turn_id":"v-1","heard_chars":1}"#).expect("stop"),
        );
        assert!(voz.nota_pendente.is_none());
        // Tudo falado: nada a anotar.
        assert!(super::nota_de_interrupcao_tardia("abc", 3).is_none());
    }

    /// Barge-in durante a reproducao de um turno concluido manda um `stop`
    /// com o `turn_id` velho. Se ele chegar durante o turno seguinte, NAO pode
    /// cancelar o turno novo — so confirma o velho.
    #[tokio::test]
    async fn stop_de_turno_velho_nao_cancela_o_turno_atual() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"type":"stop","turn_id":"v-1","heard_chars":4}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();
        let mut voz = super::VozDaConexao {
            ultimo_turno: Some(("v-1".into(), "resposta antiga".into())),
            nota_pendente: None,
        };
        let eventos = eventos_prontos(vec![TurnEvent::TextDelta("nova".into())]);
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok("nova".to_string())
        });

        let fim = super::run_streaming_turn(
            &mut sink,
            &mut cliente,
            "s1",
            eventos,
            task,
            true,
            Some("v-2"),
            &mut pong,
            &mut fila,
            &mut janela,
            &mut voz,
        )
        .await;

        assert!(
            matches!(fim, TurnEnd::Completed(ref t) if t == "nova"),
            "{fim:?}"
        );
        assert!(
            voz.nota_pendente.is_some(),
            "a interrupcao do v-1 vira nota"
        );
        drop(sink);
        let frames = frames(sink_rx);
        let parou = frames
            .iter()
            .find(|f| f["type"] == "stopped")
            .expect("stop do turno velho e confirmado");
        assert_eq!(parou["turn_id"], "v-1");
        let delta = frames
            .iter()
            .find(|f| f["type"] == "delta")
            .expect("delta do turno novo");
        assert_eq!(delta["turn_id"], "v-2");
    }

    #[tokio::test]
    async fn stop_do_turno_atual_cancela_e_guarda_o_quanto_foi_ouvido() {
        let (mut sink, sink_rx) = futures::channel::mpsc::unbounded::<Message>();
        let mut cliente = cliente(&[r#"{"type":"stop","turn_id":"v-3","heard_chars":2}"#]);
        let mut fila = std::collections::VecDeque::new();
        let mut pong = Instant::now();
        let mut janela = std::collections::VecDeque::new();
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok("nunca".to_string())
        });

        let fim = tokio::time::timeout(
            Duration::from_secs(5),
            super::run_streaming_turn(
                &mut sink,
                &mut cliente,
                "s1",
                eventos_prontos(vec![TurnEvent::TextDelta("parcial".into())]),
                task,
                true,
                Some("v-3"),
                &mut pong,
                &mut fila,
                &mut janela,
                &mut super::VozDaConexao::default(),
            ),
        )
        .await
        .expect("o stop tem de soltar o turno");

        assert!(
            matches!(
                fim,
                TurnEnd::Stopped {
                    heard_chars: Some(2),
                    ..
                }
            ),
            "{fim:?}"
        );
        drop(sink);
        let frames = frames(sink_rx);
        let ultimo = frames.last().expect("frames");
        assert_eq!(ultimo["type"], "stopped");
        assert_eq!(ultimo["turn_id"], "v-3");
    }
}
