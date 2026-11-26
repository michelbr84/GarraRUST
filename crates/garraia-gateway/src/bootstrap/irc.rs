//! Ligação do canal IRC (#1050).
//!
//! O `IrcChannel` tem `impl Channel`, conexão TCP com TLS opcional, JOIN nos
//! canais configurados e parsing de PRIVMSG desde antes desta issue. Faltava
//! o call-site.
//!
//! Molde: `bootstrap/slack.rs`. O callback do IRC tem quase a mesma forma —
//! `(conversa, nick, user_name, text, delta_tx)` —, com a conversa
//! ([`garraia_channels::ConversaIrc`]) no lugar do nome do canal: no privado o
//! alvo do `PRIVMSG` é o nick do bot, e a sessão sai de quem mandou
//! ([`sessao_da_conversa`], #2011).
//!
//! **Sem token.** No IRC a identidade é o nick, que qualquer um pode
//! reivindicar num servidor sem NickServ. Por isso a allowlist com pareamento
//! não é conveniência aqui: é o único controle de acesso que existe neste
//! canal, e vale a pena dizer isso em voz alta. A sessão privada é por nick, então
//! quem tomar o nick de alguém também vê o histórico privado daquele nick — o
//! mesmo modelo de confiança da allowlist.

use std::sync::Arc;

use garraia_agents::ChatMessage;
use garraia_config::AppConfig;
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::channel_gates;

/// Constrói os canais IRC a partir da config. Chamado depois de o estado
/// virar `Arc`, para o callback poder capturar um `SharedState`.
pub fn build_irc_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "irc" || channel_config.enabled == Some(false) {
            continue;
        }

        let server = channel_config
            .settings
            .get("server")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("IRC_SERVER").ok());

        let Some(server) = server else {
            warn!(
                "irc channel '{name}' has no server, skipping \
                 (set server in config or the IRC_SERVER env var)"
            );
            continue;
        };

        let salas: Vec<String> = channel_config
            .settings
            .get("channels")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        if salas.is_empty() {
            warn!(
                "irc channel '{name}' has no channels to join, skipping \
                 (set `channels = [\"#sala\"]` under the channel settings)"
            );
            continue;
        }

        let use_tls = channel_config
            .settings
            .get("use_tls")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // A porta default segue o `use_tls`: 6697 com TLS, 6667 sem. O default
        // do `IrcConfig` e 6667 fixo, o que daria uma conexao em claro numa
        // porta de TLS se o operador ligasse `use_tls` sem dizer a porta.
        let port = channel_config
            .settings
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(if use_tls { 6697 } else { 6667 });

        let nick = channel_config
            .settings
            .get("nick")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "garrabot".to_string());

        let on_message = callback_irc(state);

        let channel = garraia_channels::IrcChannel::new(
            garraia_channels::IrcConfig {
                server,
                port,
                nick,
                channels: salas,
                use_tls,
            },
            on_message,
        );
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured irc channel: {name}");
    }

    channels
}

/// O id da sessao de uma conversa do IRC (#2011).
///
/// - **Canal:** `irc-<canal>`, como sempre foi; quem esta no canal divide a
///   conversa, como num grupo de qualquer outro canal.
/// - **Privado:** `irc-@<nick normalizado>`, uma sessao por pessoa. Antes era
///   `irc-<nick do bot>` para todo mundo, e o modelo recebia as mensagens
///   privadas de uma pessoa no turno de outra. O `@` nao aparece em nick nem
///   nos prefixos de canal que o `garraia-channels` aceita, entao o id nunca
///   colide com o de um canal nem com o `irc-<nick do bot>` antigo, que fica
///   como esta e nao recebe turno novo.
pub(crate) fn sessao_da_conversa(conversa: &garraia_channels::ConversaIrc) -> String {
    match conversa {
        garraia_channels::ConversaIrc::Canal(canal) => format!("irc-{canal}"),
        garraia_channels::ConversaIrc::Privado(nick) => format!("irc-@{nick}"),
    }
}

/// O callback de cada `PRIVMSG` recebido: allowlist, sessao e turno.
fn callback_irc(state: &SharedState) -> garraia_channels::IrcOnMessageFn {
    // #1189: gates compartilhados com o AppState -- instancias
    // proprias fazem o `/pair` nunca casar com o `claim()`.
    let (allowlist, pairing) = channel_gates(state);

    let state_for_cb = Arc::clone(state);
    let allowlist_for_cb = Arc::clone(&allowlist);
    let pairing_for_cb = Arc::clone(&pairing);

    Arc::new(
        move |conversa: garraia_channels::ConversaIrc,
              nick: String,
              user_name: String,
              text: String,
              delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
            let state = Arc::clone(&state_for_cb);
            let allowlist = Arc::clone(&allowlist_for_cb);
            let pairing = Arc::clone(&pairing_for_cb);
            Box::pin(async move {
                // No IRC o nick e a unica identidade que existe, e num
                // servidor sem NickServ qualquer um pode toma-lo. A
                // allowlist e o unico controle de acesso deste canal.
                {
                    let mut list = allowlist.lock().unwrap();
                    if list.needs_owner() {
                        list.claim_owner(&nick);
                        info!("irc: auto-paired owner {nick}");
                        return Ok(format!(
                            "Welcome, {user_name}! You are now the owner of this GarraIA \
                             bot. Use /pair to generate a code for adding other users."
                        ));
                    }

                    if !list.is_allowed(&nick) {
                        let trimmed = text.trim();
                        if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                            let claimed = pairing.lock().unwrap().claim(trimmed, &nick);
                            if claimed.is_some() {
                                list.add(&nick);
                                info!("irc: paired user {nick} via code");
                                return Ok(format!(
                                    "Welcome, {user_name}! You now have access to this bot."
                                ));
                            }
                        }

                        let onde = match &conversa {
                            garraia_channels::ConversaIrc::Canal(canal) => canal.as_str(),
                            garraia_channels::ConversaIrc::Privado(_) => "privado",
                        };
                        warn!("irc: unauthorized nick {nick} in {onde}");
                        return Err("__blocked__".to_string());
                    }
                }

                let session_id = sessao_da_conversa(&conversa);

                let text = garraia_security::InputValidator::sanitize(&text);
                if garraia_security::InputValidator::check_prompt_injection(&text) {
                    return Err("input rejected: potential prompt injection detected".to_string());
                }

                state
                    .hydrate_session_history(&session_id, Some("irc"), Some(&nick))
                    .await;
                let history: Vec<ChatMessage> = state.session_history(&session_id);
                let continuity_key = state.continuity_key();
                // `exec_context_for`, nunca o wrapper `_with_context` (#988).
                let exec = crate::approval_scope::com_escopo(
                    state
                        .exec_context_for_msg(&session_id, Some(&nick), Some(&text))
                        .await,
                    "irc",
                    &session_id,
                    &nick,
                );

                let response = if let Some(delta_sender) = delta_tx {
                    state
                        .agents
                        .process_message_streaming_with_agent_config(
                            &session_id,
                            &text,
                            &history,
                            delta_sender,
                            continuity_key.as_deref(),
                            Some(&nick),
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
                            Some(&nick),
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
                    .persist_turn(&session_id, Some("irc"), Some(&nick), &text, &response)
                    .await;

                Ok(response)
            })
        },
    )
}

#[cfg(test)]
mod tests {
    //! #2011: a conversa privada de cada pessoa e uma sessao. Os testes passam
    //! pelo protocolo — um servidor IRC falso em 127.0.0.1, o `IrcChannel` de
    //! verdade e o callback de producao — e olham o que o modelo recebeu.

    use super::*;
    use std::sync::Mutex;

    use garraia_agents::AgentRuntime;
    use garraia_agents::providers::{ContentBlock, LlmProvider, LlmRequest, LlmResponse};
    use garraia_channels::{Channel, ChannelRegistry};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// Modelo que guarda as mensagens de cada pedido que recebeu.
    #[derive(Default)]
    struct ModeloQueGuarda {
        pedidos: Mutex<Vec<String>>,
    }

    impl ModeloQueGuarda {
        fn pedidos(&self) -> Vec<String> {
            self.pedidos.lock().map(|v| v.clone()).unwrap_or_default()
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ModeloQueGuarda {
        fn provider_id(&self) -> &str {
            "stub"
        }
        fn configured_model(&self) -> Option<&str> {
            Some("stub-1")
        }
        async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
            if let Ok(mut v) = self.pedidos.lock() {
                v.push(format!("{:?}", request.messages));
            }
            Ok(LlmResponse {
                content: vec![ContentBlock::Text { text: "ok".into() }],
                model: "stub-1".into(),
                usage: None,
                stop_reason: Some("end_turn".into()),
            })
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(true)
        }
    }

    /// Estado com o modelo que guarda e a allowlist com `permitidos` (o
    /// primeiro e o dono, para nenhum turno virar o boas-vindas do pareamento).
    ///
    /// A allowlist e a do proprio `AppState` (o gate que o `channel_gates`
    /// entrega ao canal), preenchida pela API dela: construir outra aqui cairia
    /// em `config::tests::nenhum_canal_constroi_gate_proprio`, que varre o
    /// fonte deste arquivo. O `with_config_dir` aponta o `allowlist.json` para
    /// o tempdir do teste.
    fn estado(dir: &tempfile::TempDir, permitidos: &[&str]) -> (SharedState, Arc<ModeloQueGuarda>) {
        let config = AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let agents = AgentRuntime::new();
        let modelo = Arc::new(ModeloQueGuarda::default());
        agents.register_provider(Arc::clone(&modelo) as Arc<dyn LlmProvider>);
        let state: SharedState = Arc::new(crate::state::AppState::with_config_dir(
            config,
            Arc::new(agents),
            ChannelRegistry::new(),
            dir.path(),
        ));
        {
            let mut lista = state.allowlist.lock().expect("lock");
            assert!(lista.needs_owner(), "allowlist nova, restrita e sem dono");
            assert!(lista.claim_owner(permitidos[0]));
            for nick in permitidos {
                lista.add(*nick);
            }
        }
        (state, modelo)
    }

    /// Sobe o canal com o callback de producao contra um servidor falso e
    /// devolve o lado do servidor.
    async fn conecta(state: &SharedState) -> (garraia_channels::IrcChannel, TcpStream) {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = garraia_channels::IrcChannel::new(
            garraia_channels::IrcConfig {
                server: "127.0.0.1".into(),
                port: porta,
                nick: "garrabot".into(),
                channels: vec![],
                use_tls: false,
            },
            callback_irc(state),
        );
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let servidor = conexao.await.expect("join").expect("accept");
        (canal, servidor)
    }

    async fn escreve(servidor: &mut TcpStream, linha: &str) {
        servidor
            .write_all(format!("{linha}\r\n").as_bytes())
            .await
            .expect("escreve");
    }

    /// Manda `linha` como o servidor e espera a resposta do bot chegar a
    /// `destino`. Um turno de cada vez: os turnos do canal rodam em tarefas
    /// proprias, e a ordem do historico so e deterministica assim.
    async fn turno(servidor: &mut TcpStream, linha: &str, destino: &str) {
        escreve(servidor, linha).await;
        let esperado = format!("PRIVMSG {destino} :");
        let mut recebido = Vec::new();
        let mut buf = [0u8; 4096];
        let prazo = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while !String::from_utf8_lossy(&recebido).contains(&esperado) {
            let n = tokio::time::timeout_at(prazo, servidor.read(&mut buf))
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "sem resposta para {destino}: {:?}",
                        String::from_utf8_lossy(&recebido)
                    )
                })
                .expect("leitura do socket");
            assert!(n > 0, "conexao fechada antes da resposta");
            recebido.extend_from_slice(&buf[..n]);
        }
    }

    /// O vazamento da #2011: o modelo recebia as mensagens privadas da Ana no
    /// turno do Bruno, e a sessao `irc-<nick do bot>` juntava todo mundo.
    #[tokio::test]
    async fn conversas_privadas_de_pessoas_diferentes_nao_se_misturam() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["ana", "bruno"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        turno(
            &mut servidor,
            ":ana!a@h PRIVMSG garrabot :segredo-da-ana",
            "ana",
        )
        .await;
        turno(&mut servidor, ":bruno!b@h PRIVMSG garrabot :oi", "bruno").await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 2, "{pedidos:?}");
        assert!(pedidos[0].contains("segredo-da-ana"));
        assert!(
            !pedidos[1].contains("segredo-da-ana"),
            "o turno do Bruno viu a conversa privada da Ana: {}",
            pedidos[1]
        );
        assert!(
            !state.sessions.contains_key("irc-garrabot"),
            "a sessao antiga `irc-<nick do bot>` recebeu turno novo"
        );
        assert!(state.sessions.contains_key("irc-@ana"));
        assert!(state.sessions.contains_key("irc-@bruno"));
        canal.disconnect().await.expect("desconecta");
    }

    #[test]
    fn sessao_da_conversa_separa_canal_de_privado() {
        use garraia_channels::ConversaIrc::{Canal, Privado};
        assert_eq!(sessao_da_conversa(&Canal("#garra".into())), "irc-#garra");
        assert_eq!(sessao_da_conversa(&Canal("&local".into())), "irc-&local");
        assert_eq!(sessao_da_conversa(&Privado("ana".into())), "irc-@ana");
        // O id privado nunca e o de um canal nem o `irc-<nick do bot>` antigo.
        assert_ne!(
            sessao_da_conversa(&Privado("garrabot".into())),
            "irc-garrabot"
        );
        // O prefixo do canal continua `irc-` (channels_view::PREFIXOS_DE_SESSAO).
        assert!(sessao_da_conversa(&Privado("ana".into())).starts_with("irc-"));
    }

    /// A mesma pessoa continua a mesma conversa, com o nick na caixa que for:
    /// `Ana` e `ana` sao o mesmo nick no servidor, e com `CASEMAPPING=rfc1459`
    /// `[ana]` e `{ana}` tambem.
    #[tokio::test]
    async fn a_mesma_pessoa_continua_a_conversa_mesmo_trocando_a_caixa() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["Ana", "ana", "[bia]", "{bia}"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CHANTYPES=#& CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        turno(
            &mut servidor,
            ":Ana!a@h PRIVMSG garrabot :lembra-do-x",
            "Ana",
        )
        .await;
        turno(&mut servidor, ":ana!a@h PRIVMSG garrabot :e ai", "ana").await;
        turno(
            &mut servidor,
            ":[bia]!b@h PRIVMSG garrabot :lembra-do-y",
            "[bia]",
        )
        .await;
        turno(&mut servidor, ":{bia}!b@h PRIVMSG garrabot :e ai", "{bia}").await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 4, "{pedidos:?}");
        assert!(pedidos[1].contains("lembra-do-x"), "{}", pedidos[1]);
        assert!(pedidos[3].contains("lembra-do-y"), "{}", pedidos[3]);
        assert!(!pedidos[3].contains("lembra-do-x"), "{}", pedidos[3]);
        canal.disconnect().await.expect("desconecta");
    }

    /// Com `CASEMAPPING=ascii`, `[a]` e `{a}` sao duas pessoas no servidor, e
    /// duas conversas aqui.
    #[tokio::test]
    async fn casemapping_ascii_separa_colchete_de_chave() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["[a]", "{a}"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=ascii :are supported by this server",
        )
        .await;

        turno(
            &mut servidor,
            ":[a]!a@h PRIVMSG garrabot :segredo-do-colchete",
            "[a]",
        )
        .await;
        turno(&mut servidor, ":{a}!a@h PRIVMSG garrabot :oi", "{a}").await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 2, "{pedidos:?}");
        assert!(
            !pedidos[1].contains("segredo-do-colchete"),
            "{}",
            pedidos[1]
        );
        canal.disconnect().await.expect("desconecta");
    }

    /// No canal todo mundo divide a conversa, por desenho, e a sessao
    /// continua `irc-<canal>` — o id das sessoes de canal nao muda.
    #[tokio::test]
    async fn canal_continua_na_sessao_do_canal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["ana", "bruno"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        turno(
            &mut servidor,
            ":ana!a@h PRIVMSG #garra :fala-no-canal",
            "#garra",
        )
        .await;
        turno(&mut servidor, ":bruno!b@h PRIVMSG #garra :oi", "#garra").await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 2, "{pedidos:?}");
        assert!(pedidos[1].contains("fala-no-canal"), "{}", pedidos[1]);
        assert!(state.sessions.contains_key("irc-#garra"));
        canal.disconnect().await.expect("desconecta");
    }

    /// `&` tambem e prefixo de canal (`CHANTYPES` padrao `#&`): a mensagem no
    /// `&local` e respondida no canal, nao no privado de quem falou.
    #[tokio::test]
    async fn canal_com_e_comercial_e_respondido_no_canal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, _modelo) = estado(&dir, &["ana"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        turno(&mut servidor, ":ana!a@h PRIVMSG &local :oi", "&local").await;

        assert!(state.sessions.contains_key("irc-&local"));
        canal.disconnect().await.expect("desconecta");
    }
}
