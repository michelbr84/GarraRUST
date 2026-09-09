//! Ligação do bridge OpenClaw (#1050).
//!
//! O `OpenClawClient` existe e é completo — tem loop de reconexão com backoff,
//! conversão nos dois sentidos e um `status()` que os handlers de
//! `/api/openclaw/*` já sabem ler. O que faltava era alguém construí-lo:
//! `state.openclaw_client` era `None` **constante**, então as quatro rotas
//! caíam todas no ramo "não configurado" e a env `OPENCLAW_ENABLED` que o
//! doc-comment do módulo prometia não era lida em lugar nenhum.
//!
//! Ele não é um `Channel` como os outros — não tem `connect()`, e o
//! `ChannelRegistry` não o comporta. É um cliente que já dispara o próprio
//! loop no `new()` e devolve um `Receiver` de mensagens; quem liga o bridge
//! precisa consumir esse receiver e devolver a resposta pelo `send_reply`.
//! É o que este módulo faz.

use garraia_channels::OpenClawConfig;
use garraia_config::AppConfig;
use tracing::{info, warn};

/// Lê a configuração do bridge de `[channels.<nome>]` com
/// `channel_type = "openclaw"`.
///
/// Não há seção `openclaw` dedicada em `garraia-config`: os canais são
/// configurados pelo `settings: HashMap<String, Value>` genérico do
/// `ChannelConfig`, e este segue o mesmo caminho. `enabled = false` e o
/// `enabled` interno do próprio `OpenClawConfig` são respeitados — o de fora
/// desliga o canal como em qualquer outro, o de dentro é o que o
/// `OpenClawConfig` já documentava.
///
/// Mais de uma seção `openclaw` não faz sentido (o bridge é um daemon só),
/// então a primeira vence e as demais viram aviso.
pub fn build_openclaw_config(config: &AppConfig) -> Option<OpenClawConfig> {
    let mut escolhida: Option<(String, OpenClawConfig)> = None;

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "openclaw" || channel_config.enabled == Some(false) {
            continue;
        }

        let settings = serde_json::Value::Object(
            channel_config
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );

        let parsed: OpenClawConfig = match serde_json::from_value(settings) {
            Ok(c) => c,
            Err(e) => {
                warn!("openclaw channel '{name}' has invalid settings, skipping: {e}");
                continue;
            }
        };

        if !parsed.enabled {
            info!("openclaw channel '{name}' is present but `enabled = false`, skipping");
            continue;
        }

        match &escolhida {
            None => escolhida = Some((name.clone(), parsed)),
            Some((primeira, _)) => warn!(
                "openclaw channel '{name}' ignored: the bridge is a single daemon and \
                 '{primeira}' was configured first"
            ),
        }
    }

    if let Some((name, cfg)) = escolhida {
        info!(
            "configured openclaw bridge: {name} -> {} ({} platform(s))",
            cfg.ws_url,
            if cfg.channels.is_empty() {
                "all".to_string()
            } else {
                cfg.channels.len().to_string()
            }
        );
        Some(cfg)
    } else {
        None
    }
}

/// Consome as mensagens que o bridge entrega e devolve a resposta do agente.
///
/// O `OpenClawClient::new` ja disparou o loop de conexao; este laco e a outra
/// ponta. Sem ele o bridge conectaria, receberia e jogaria fora.
///
/// Segue o mesmo contrato dos outros canais: sanitiza, recusa injecao de
/// prompt, hidrata o historico da sessao, usa `exec_context_for` (e nao o
/// wrapper `_with_context`, que passa `ExecContext::default()` e faria
/// `/mode` nao valer aqui — a assimetria do #988), e persiste o turno.
///
/// O `session_id` vem do proprio `from_openclaw_message`, que ja o monta como
/// `openclaw-<plataforma>-<canal>`. A resposta reusa a `metadata` da mensagem
/// de entrada, que e como o `to_openclaw_message` sabe para qual plataforma e
/// para qual conversa devolver.
pub fn spawn_openclaw_router(
    state: crate::state::SharedState,
    client: std::sync::Arc<garraia_channels::OpenClawClient>,
    mut rx: tokio::sync::mpsc::Receiver<garraia_common::Message>,
) {
    tokio::spawn(async move {
        while let Some(entrada) = rx.recv().await {
            let garraia_common::MessageContent::Text(bruto) = &entrada.content else {
                // Imagem, audio, arquivo: o bridge entrega, o agente ainda nao
                // consome. Descartar em silencio seria pior que dizer.
                info!(
                    session = %entrada.session_id,
                    "openclaw: mensagem sem texto ignorada (tipo ainda nao suportado)"
                );
                continue;
            };

            let session_id = entrada.session_id.to_string();
            let user_id = entrada.user_id.to_string();

            let texto = garraia_security::InputValidator::sanitize(bruto);
            if garraia_security::InputValidator::check_prompt_injection(&texto) {
                warn!(session = %session_id, "openclaw: prompt injection recusada");
                continue;
            }

            state
                .hydrate_session_history(&session_id, Some("openclaw"), Some(&user_id))
                .await;
            let history = state.session_history(&session_id);
            let continuity_key = state.continuity_key();
            let exec = state.exec_context_for(&session_id, Some(&user_id)).await;

            let resposta = state
                .agents
                .process_message_with_agent_config(
                    &session_id,
                    &texto,
                    &history,
                    continuity_key.as_deref(),
                    Some(&user_id),
                    None,
                    None,
                    None,
                    None,
                    &exec,
                )
                .await;

            let resposta = match resposta {
                Ok(r) => r,
                Err(e) => {
                    warn!(session = %session_id, "openclaw: agent error: {e}");
                    continue;
                }
            };

            state
                .persist_turn(
                    &session_id,
                    Some("openclaw"),
                    Some(&user_id),
                    &texto,
                    &resposta,
                )
                .await;

            let mut saida = entrada.clone();
            saida.direction = garraia_common::MessageDirection::Outgoing;
            saida.content = garraia_common::MessageContent::Text(resposta);
            if let Err(e) = client.send_reply(&saida).await {
                warn!(session = %session_id, "openclaw: falha ao devolver a resposta: {e}");
            }
        }

        info!("openclaw: o bridge fechou o canal de entrada; roteador encerrado");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::ChannelConfig;

    fn com_canais(entradas: Vec<(&str, &str, Option<bool>, serde_json::Value)>) -> AppConfig {
        let mut config = AppConfig::default();
        for (nome, tipo, enabled, settings) in entradas {
            let settings = match settings {
                serde_json::Value::Object(m) => m.into_iter().collect(),
                _ => Default::default(),
            };
            config.channels.insert(
                nome.to_string(),
                ChannelConfig {
                    channel_type: tipo.to_string(),
                    enabled,
                    settings,
                },
            );
        }
        config
    }

    #[test]
    fn sem_secao_openclaw_o_bridge_fica_desligado() {
        assert!(build_openclaw_config(&AppConfig::default()).is_none());
        // Uma secao de outro canal nao liga o bridge por engano.
        let c = com_canais(vec![(
            "slack-1",
            "slack",
            None,
            serde_json::json!({"enabled": true}),
        )]);
        assert!(build_openclaw_config(&c).is_none());
    }

    #[test]
    fn le_ws_url_e_plataformas_das_settings() {
        let c = com_canais(vec![(
            "oc",
            "openclaw",
            None,
            serde_json::json!({
                "enabled": true,
                "ws_url": "ws://127.0.0.1:19999",
                "channels": ["whatsapp", "signal"],
            }),
        )]);
        let cfg = build_openclaw_config(&c).expect("bridge configurado");
        assert_eq!(cfg.ws_url, "ws://127.0.0.1:19999");
        assert_eq!(cfg.channels, vec!["whatsapp", "signal"]);
    }

    /// O default do `ws_url` continua valendo quando a chave nao vem — e o
    /// que faz `[channels.oc] channel_type = "openclaw"` + `enabled = true`
    /// bastar para um daemon local.
    #[test]
    fn ws_url_tem_default() {
        let c = com_canais(vec![(
            "oc",
            "openclaw",
            None,
            serde_json::json!({"enabled": true}),
        )]);
        let cfg = build_openclaw_config(&c).expect("bridge configurado");
        assert_eq!(cfg.ws_url, "ws://127.0.0.1:18789");
        assert!(cfg.channels.is_empty());
    }

    /// Dois desligamentos independentes, os dois respeitados: o `enabled` do
    /// `ChannelConfig`, que vale para qualquer canal, e o do proprio
    /// `OpenClawConfig`, que o struct ja documentava e ninguem lia.
    #[test]
    fn os_dois_enabled_desligam() {
        let de_fora = com_canais(vec![(
            "oc",
            "openclaw",
            Some(false),
            serde_json::json!({"enabled": true}),
        )]);
        assert!(build_openclaw_config(&de_fora).is_none());

        let de_dentro = com_canais(vec![(
            "oc",
            "openclaw",
            None,
            serde_json::json!({"enabled": false}),
        )]);
        assert!(build_openclaw_config(&de_dentro).is_none());

        // E o default do `enabled` interno e `false`: nao basta existir.
        let sem_nada = com_canais(vec![("oc", "openclaw", None, serde_json::json!({}))]);
        assert!(build_openclaw_config(&sem_nada).is_none());
    }

    #[test]
    fn settings_invalidas_nao_derrubam_o_boot() {
        let c = com_canais(vec![(
            "oc",
            "openclaw",
            None,
            serde_json::json!({"enabled": true, "reconnect_interval_secs": "nao e numero"}),
        )]);
        assert!(build_openclaw_config(&c).is_none());
    }
}
