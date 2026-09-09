use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_channels::google_chat::{GoogleChatChannel, GoogleChatConfig, GoogleChatOnMessageFn};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Constroi os canais Google Chat a partir da config (#1050).
///
/// Canal **push**, como o WhatsApp e o LINE: o `Vec<Arc<_>>` vira estado da
/// rota `/webhooks/google-chat`, e nao entrada do `ChannelRegistry`.
///
/// Tres campos, e nenhum deles e opcional na pratica:
///
/// - `audience` — o que o token do webhook precisa declarar em `aud`. Sem
///   ela `GoogleChatChannel::new` recusa o canal, porque todo webhook do
///   Chat e assinado pela mesma chave do Google e o `aud` e a unica coisa
///   que distingue esta app das outras.
/// - `service_account_token` — o bearer com que o gateway **responde** pela
///   API REST. Sem ele o canal receberia e nao teria como falar, o que e
///   pior que nao existir: o usuario manda mensagem e o silencio parece bug
///   do bot.
/// - `space_allowlist` nao existe: o controle de quem pode falar e o
///   `Allowlist` compartilhado com os outros canais.
///
/// `service_account_key_path` esta na config e **nao e usado**: transformar
/// a chave da conta de servico num token exige o fluxo OAuth2 JWT bearer,
/// que ainda nao esta implementado. Ate la o operador fornece o token
/// pronto. O campo fica porque o dia em que o fluxo entrar ele e o caminho.
pub fn build_google_chat_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Arc<GoogleChatChannel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "google_chat" || channel_config.enabled == Some(false) {
            continue;
        }

        let audience = channel_config
            .settings
            .get("audience")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| resolve_api_key(None, "GOOGLE_CHAT_AUDIENCE", "GOOGLE_CHAT_AUDIENCE"));

        let Some(audience) = audience else {
            warn!(
                "google chat channel '{name}' has no audience, skipping \
                 (set audience in config or GOOGLE_CHAT_AUDIENCE env var; \
                  without it every Google Chat app's token would be accepted)"
            );
            continue;
        };

        let service_account_token = channel_config
            .settings
            .get("service_account_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                resolve_api_key(
                    None,
                    "GOOGLE_CHAT_SERVICE_ACCOUNT_TOKEN",
                    "GOOGLE_CHAT_SERVICE_ACCOUNT_TOKEN",
                )
            });

        let Some(service_account_token) = service_account_token else {
            warn!(
                "google chat channel '{name}' has no service_account_token, skipping \
                 (set service_account_token in config or \
                  GOOGLE_CHAT_SERVICE_ACCOUNT_TOKEN env var; without it the channel \
                  could receive messages but never answer)"
            );
            continue;
        };

        let webhook_url = channel_config
            .settings
            .get("webhook_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));

        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);
        let nome_do_canal: Arc<str> = Arc::from(name.as_str());

        let on_message: GoogleChatOnMessageFn = Arc::new(
            move |space_id: String,
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
                        // `expect`, e nao `unwrap` (regra 4): o unico modo de
                        // falha e o mutex envenenado por um panico em outra
                        // thread, e a mensagem diz isso.
                        let mut list = allowlist
                            .lock()
                            .expect("allowlist mutex envenenado por outra thread");
                        if list.needs_owner() {
                            list.claim_owner(&user_id);
                            // O `user_id` do Google Chat (`users/<numero>`) e
                            // identificador persistente da conta, ou seja PII
                            // (regra 6): vai como campo estruturado, nunca
                            // interpolado na frase, para um filtro do pipeline
                            // de log conseguir derruba-lo sozinho.
                            info!(
                                canal = %nome_do_canal,
                                user_id = %user_id,
                                "google chat: primeiro usuario reivindicou o papel de owner"
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
                                        "google chat: usuario pareado por codigo"
                                    );
                                    return Ok(format!(
                                        "Welcome, {}! You now have access to this bot.",
                                        user_name
                                    ));
                                }
                            }

                            warn!(
                                canal = %nome_do_canal,
                                user_id = %user_id,
                                "google chat: requisicao de usuario nao autorizado recusada"
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    // A sessao e por **space**, nao por usuario: um space e
                    // uma sala, e a conversa dentro dela e uma so — como o
                    // `slack-{channel_id}`. Chavear por usuario daria a cada
                    // participante um historico proprio da mesma conversa.
                    let session_id = format!("google-chat-{space_id}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("google_chat"), Some(&user_id))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key();

                    // `exec_context_for`, e nao o wrapper `_with_context`
                    // (#988): o wrapper passa `ExecContext::default()`, e
                    // entao `/mode search` responde "modo definido" sem a
                    // politica de ferramenta valer.
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
                        .persist_turn(
                            &session_id,
                            Some("google_chat"),
                            Some(&user_id),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let cfg = GoogleChatConfig {
            webhook_url,
            service_account_key_path: None,
            service_account_token,
            audience,
            name: name.clone(),
        };

        match GoogleChatChannel::new(cfg, on_message) {
            Ok(channel) => {
                channels.push(Arc::new(channel));
                info!("configured google chat channel: {name}");
            }
            // Hoje o unico erro e a audience em branco, ja barrada acima
            // quando ausente — mas `"   "` no TOML passa pelo `Option` e e
            // recusado aqui. O `match` fica pelo que vier depois: um
            // construtor que ganhe validacao nova nao pode voltar a montar
            // canal quebrado por descuido do wiring.
            Err(e) => {
                warn!("google chat channel '{name}' rejeitado, pulando: {e}");
            }
        }
    }

    channels
}
