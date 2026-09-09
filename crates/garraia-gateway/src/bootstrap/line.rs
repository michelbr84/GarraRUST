use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_channels::line_channel::{LineChannel, LineConfig, LineOnMessageFn};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Constroi os canais LINE a partir da config (#1050).
///
/// Molde de forma: `bootstrap::whatsapp`. O LINE tambem e canal **push** —
/// nao ha conexao a manter, o LINE e que faz POST no `/webhooks/line` —, entao
/// o retorno e um `Vec<Arc<_>>` que vira estado da rota, e nao um
/// `Vec<Box<dyn Channel>>` que entra no `ChannelRegistry`.
///
/// Uma diferenca relevante em relacao ao whatsapp: `LineChannel::new` devolve
/// `Result` e recusa `channel_secret` em branco (#1051). Isso e proposital, e
/// e aqui que o efeito aparece — um canal sem segredo nao e "montado em modo
/// degradado", e simplesmente pulado, com o motivo no log. Sem segredo nao ha
/// como distinguir um webhook do LINE de um POST forjado por quem descobriu a
/// URL, e uma rota que aceita os dois e pior que rota nenhuma.
pub fn build_line_channels(config: &AppConfig, state: &SharedState) -> Vec<Arc<LineChannel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "line" || channel_config.enabled == Some(false) {
            continue;
        }

        let channel_access_token = channel_config
            .settings
            .get("channel_access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                resolve_api_key(
                    None,
                    "LINE_CHANNEL_ACCESS_TOKEN",
                    "LINE_CHANNEL_ACCESS_TOKEN",
                )
            });

        let Some(channel_access_token) = channel_access_token else {
            warn!(
                "line channel '{name}' has no channel_access_token, skipping \
                 (set channel_access_token in config or LINE_CHANNEL_ACCESS_TOKEN env var)"
            );
            continue;
        };

        let channel_secret = channel_config
            .settings
            .get("channel_secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| resolve_api_key(None, "LINE_CHANNEL_SECRET", "LINE_CHANNEL_SECRET"));

        let Some(channel_secret) = channel_secret else {
            warn!(
                "line channel '{name}' has no channel_secret, skipping \
                 (set channel_secret in config or LINE_CHANNEL_SECRET env var)"
            );
            continue;
        };

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));

        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);
        // Qual secao `[channels.<nome>]` este callback serve. Com dois canais
        // LINE configurados, um log sem isso nao diz em qual deles o usuario
        // bateu — e e justamente o caso em que a assinatura ja distinguiu os
        // dois. `Arc<str>` porque o callback e `Fn` e roda muitas vezes.
        let nome_do_canal: Arc<str> = Arc::from(name.as_str());

        let on_message: LineOnMessageFn = Arc::new(
            move |_reply_token: String,
                  user_id: String,
                  user_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                let nome_do_canal = Arc::clone(&nome_do_canal);
                Box::pin(async move {
                    // Allowlist / pairing check
                    {
                        // `expect`, e nao `unwrap` (regra 4 do CLAUDE.md): o
                        // unico modo de falha e o mutex envenenado por um
                        // panico em outra thread, e a mensagem diz isso. Os
                        // canais mais antigos (`slack`, `whatsapp`) ainda usam
                        // `unwrap` aqui; corrigi-los e outro PR.
                        let mut list = allowlist
                            .lock()
                            .expect("allowlist mutex envenenado por outra thread");
                        if list.needs_owner() {
                            list.claim_owner(&user_id);
                            info!(
                                canal = %nome_do_canal,
                                user_id = %user_id,
                                "line: primeiro usuario reivindicou o papel de owner"
                            );
                            return Ok(format!(
                                "Welcome, {}! You are now the owner of this GarraIA bot.\n\n\
                                 Send /pair to generate a code for adding other users.\n\
                                 Send /help for available commands.",
                                user_name
                            ));
                        }

                        if !list.is_allowed(&user_id) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing
                                    .lock()
                                    .expect("pairing mutex envenenado por outra thread")
                                    .claim(trimmed, &user_id);
                                if claimed.is_some() {
                                    list.add(&user_id);
                                    info!(
                                        canal = %nome_do_canal,
                                        user_id = %user_id,
                                        "line: usuario pareado por codigo"
                                    );
                                    return Ok(format!(
                                        "Welcome, {}! You now have access to this bot.",
                                        user_name
                                    ));
                                }
                            }

                            // O `user_id` do LINE (`U` + 32 hex) e um
                            // identificador persistente da conta, ou seja PII
                            // (regra 6). Vai como **campo estruturado**, nunca
                            // interpolado na frase: assim um filtro do pipeline
                            // de log consegue derrubar so o campo, sem ter de
                            // casar com o texto da mensagem. E vai uma vez so —
                            // no LINE o `user_name` E o proprio `user_id`
                            // (o webhook nao traz display name, ver
                            // `LineChannel::handle_incoming`), entao a forma
                            // dos outros canais imprimiria o mesmo
                            // identificador duas vezes.
                            warn!(
                                canal = %nome_do_canal,
                                user_id = %user_id,
                                "line: requisicao de usuario nao autorizado recusada"
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("line-{user_id}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("line"), Some(&user_id))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key();

                    // `exec_context_for`, e nao o wrapper `_with_context` (#988):
                    // o wrapper passa `ExecContext::default()`, e entao `/mode
                    // search` responde "modo definido" sem a politica de
                    // ferramenta valer. Prometer uma restricao que nao existe e
                    // pior que nao a oferecer.
                    let exec = state.exec_context_for(&session_id, Some(&user_id)).await;

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&user_id),
                                None,
                                None,
                                None,
                                None,
                                &exec,
                            )
                            .await
                    } else {
                        state
                            .agents
                            .process_message_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                continuity_key.as_deref(),
                                Some(&user_id),
                                None,
                                None,
                                None,
                                None,
                                &exec,
                            )
                            .await
                    }
                    .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(&session_id, Some("line"), Some(&user_id), &text, &response)
                        .await;

                    Ok(response)
                })
            },
        );

        let config = LineConfig {
            channel_access_token,
            channel_secret,
            name: name.clone(),
        };

        match LineChannel::new(config, on_message) {
            Ok(channel) => {
                channels.push(Arc::new(channel));
                info!("configured line channel: {name}");
            }
            // Hoje o unico erro possivel e o segredo em branco, ja barrado
            // acima quando ausente — mas `"   "` no TOML passa pelo `Option` e
            // e recusado aqui. O `match` fica pelo que vier depois: um
            // construtor que ganha validacao nova nao pode voltar a montar
            // canal quebrado por descuido do wiring.
            Err(e) => {
                warn!("line channel '{name}' rejeitado, pulando: {e}");
            }
        }
    }

    channels
}
