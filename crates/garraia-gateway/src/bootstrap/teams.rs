use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_channels::teams::{TeamsChannel, TeamsConfig, TeamsOnMessageFn};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Constroi os canais Teams a partir da config (#1050).
///
/// Canal **push**, como o WhatsApp, o LINE e o Google Chat: o `Vec<Arc<_>>`
/// vira estado da rota `/webhooks/teams`, e nao entrada do `ChannelRegistry`.
///
/// Tres campos, todos obrigatorios:
///
/// - `app_id` — o Microsoft App ID, que e a **audiencia** esperada no token
///   do webhook. Sem ele `TeamsChannel::new` recusa o canal: todos os tokens
///   do Bot Framework sao assinados pelas mesmas chaves, e o `aud` e a unica
///   coisa que distingue este bot dos outros.
/// - `app_secret` e `tenant_id` — o par com que o gateway obtem, do Azure AD,
///   o token com que **responde**. Sem eles o canal receberia e nao teria
///   como falar.
///
/// A autenticacao de saida (`authenticate`) nao roda aqui: ela precisa de
/// `&mut self` e de rede, e o boot nao deve depender de nenhum dos dois. O
/// canal a faz sob demanda.
pub fn build_teams_channels(config: &AppConfig, state: &SharedState) -> Vec<Arc<TeamsChannel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "teams" || channel_config.enabled == Some(false) {
            continue;
        }

        let app_id = channel_config
            .settings
            .get("app_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| resolve_api_key(None, "TEAMS_APP_ID", "TEAMS_APP_ID"));

        let Some(app_id) = app_id else {
            warn!(
                "teams channel '{name}' has no app_id, skipping \
                 (set app_id in config or TEAMS_APP_ID env var; without it a token \
                  issued for any other Bot Framework bot would be accepted)"
            );
            continue;
        };

        let app_secret = channel_config
            .settings
            .get("app_secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| resolve_api_key(None, "TEAMS_APP_SECRET", "TEAMS_APP_SECRET"));

        let Some(app_secret) = app_secret else {
            warn!(
                "teams channel '{name}' has no app_secret, skipping \
                 (set app_secret in config or TEAMS_APP_SECRET env var; without it \
                  the channel could receive messages but never answer)"
            );
            continue;
        };

        let tenant_id = channel_config
            .settings
            .get("tenant_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| resolve_api_key(None, "TEAMS_TENANT_ID", "TEAMS_TENANT_ID"));

        let Some(tenant_id) = tenant_id else {
            warn!(
                "teams channel '{name}' has no tenant_id, skipping \
                 (set tenant_id in config or TEAMS_TENANT_ID env var)"
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
        let nome_do_canal: Arc<str> = Arc::from(name.as_str());

        let on_message: TeamsOnMessageFn = Arc::new(
            move |conversation_id: String,
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
                            // O `user_id` do Teams (`users/<numero>`) e
                            // identificador persistente da conta, ou seja PII
                            // (regra 6): vai como campo estruturado, nunca
                            // interpolado na frase, para um filtro do pipeline
                            // de log conseguir derruba-lo sozinho.
                            info!(
                                canal = %nome_do_canal,
                                user_id = %user_id,
                                "teams: primeiro usuario reivindicou o papel de owner"
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
                                        "teams: usuario pareado por codigo"
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
                                "teams: requisicao de usuario nao autorizado recusada"
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    // A sessao e por **conversa**, nao por usuario: uma
                    // conversa do Teams e uma sala (canal ou chat), e o
                    // historico dela e um so — como o `slack-{channel_id}`.
                    // Chavear por usuario daria a cada participante um
                    // historico proprio da mesma conversa.
                    let session_id = format!("teams-{conversation_id}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("teams"), Some(&user_id))
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
                        .persist_turn(&session_id, Some("teams"), Some(&user_id), &text, &response)
                        .await;

                    Ok(response)
                })
            },
        );

        let cfg = TeamsConfig {
            app_id,
            app_secret,
            tenant_id,
            name: name.clone(),
        };

        match TeamsChannel::new(cfg, on_message) {
            Ok(channel) => {
                channels.push(Arc::new(channel));
                info!("configured teams channel: {name}");
            }
            // Hoje o unico erro e o `app_id` em branco, ja barrado acima
            // quando ausente — mas `"   "` no TOML passa pelo `Option` e e
            // recusado aqui. O `match` fica pelo que vier depois: um
            // construtor que ganhe validacao nova nao pode voltar a montar
            // canal quebrado por descuido do wiring.
            Err(e) => {
                warn!("teams channel '{name}' rejeitado, pulando: {e}");
            }
        }
    }

    channels
}
