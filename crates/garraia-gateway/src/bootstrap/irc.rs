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
//! mesmo modelo de confiança da allowlist. E é por nick **em cada conexão** da
//! config: um nick só identifica alguém dentro da própria rede.
//!
//! Na allowlist, as entradas do IRC são `irc:<nick>`, e o nick é comparado
//! pelo `CASEMAPPING` do servidor, como na sessão (#2015). São as únicas que
//! valem aqui: uma entrada sem prefixo pode ser o id de alguém de outro canal
//! ([`admitido`], GHSA-g6c8-mx6x-5g92).

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

        let on_message = callback_irc(state, name);

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
/// - **Privado:** `irc-@<nick normalizado>@<conexao>`, uma sessao por pessoa
///   em cada conexao IRC da config. Antes era `irc-<nick do bot>` para todo
///   mundo, e o modelo recebia as mensagens privadas de uma pessoa no turno de
///   outra. A `conexao` e o nome da entrada `type = "irc"` na config: o nick so
///   identifica alguem dentro da propria rede, e cada conexao aponta para uma.
///
/// O id privado e injetivo e nao colide com nenhum outro: o nick nunca tem
/// `@` (o `garraia-channels` o corta no primeiro `!` ou `@` do prefixo), entao
/// o primeiro `@` depois de `irc-@` separa o nick da conexao, por mais que o
/// nome da conexao tenha `@`. Nenhum canal comeca com `@` (os prefixos aceitos
/// sao `#&!+`), e o `irc-<nick do bot>` antigo e o `irc-@<nick>` sem conexao
/// ficam como estao e nao recebem turno novo.
pub(crate) fn sessao_da_conversa(
    conexao: &str,
    conversa: &garraia_channels::ConversaIrc,
) -> String {
    match conversa {
        garraia_channels::ConversaIrc::Canal(canal) => format!("irc-{canal}"),
        garraia_channels::ConversaIrc::Privado(nick) => {
            // A injetividade do id depende disto, que e garantido no
            // `garraia-channels` (`parse_privmsg`); a assercao pega nos testes
            // uma mudanca la que deixe `@` chegar ate aqui.
            debug_assert!(!nick.contains('@'), "nick com `@` quebra o id privado");
            format!("irc-@{nick}@{conexao}")
        }
    }
}

/// Prefixo das entradas do IRC na allowlist (#2015).
///
/// A allowlist e uma so para todos os canais (`channel_gates`, #1189), e os
/// ids dos outros sao comparados por igualdade exata (os do Slack, por
/// exemplo, tem maiusculas). O prefixo diz quais entradas sao nicks de IRC,
/// as unicas comparadas pelo `CASEMAPPING` do servidor ([`admitido`]).
const PREFIXO_DA_ALLOWLIST: &str = "irc:";

/// A entrada da allowlist de quem mandou: `irc:<nick como veio>`.
///
/// O nick fica como veio, e nao normalizado, porque a comparacao normaliza os
/// dois lados pelo `CASEMAPPING` da hora. Guardar a forma normalizada perderia
/// informacao: `[bia]`, gravada como `{bia}` sob `rfc1459`, casaria com outra
/// pessoa chamada `{bia}` se o servidor passasse a anunciar `ascii`.
fn entrada_da_allowlist(remetente: &garraia_channels::NickIrc) -> String {
    format!("{PREFIXO_DA_ALLOWLIST}{}", remetente.como_veio())
}

/// Se a allowlist admite `remetente` no IRC (#2015, GHSA-g6c8-mx6x-5g92).
///
/// So as entradas `irc:` valem aqui, e o nick delas e comparado pelo
/// `CASEMAPPING` anunciado quando a mensagem chegou. A equivalencia nunca e
/// maior que a do servidor agora, entao separar demais so bloqueia e nunca
/// admite outra pessoa.
///
/// Uma entrada sem prefixo nunca admite ninguem no IRC, nem por igualdade
/// exata. Os ids dos outros canais ficam sem prefixo nesta mesma allowlist, e
/// no IRC o nick e escolhido por quem conecta: o id de um usuario do Slack
/// (`U01ABCDEF`) e um nick valido, e quem entrasse com ele passaria como
/// aquela pessoa. Pelo mesmo motivo nenhuma entrada sem prefixo vira `irc:`
/// sozinha. Quem foi pareado pelo IRC antes da #2015 precisa de um codigo
/// novo de `/pair`.
fn admitido(lista: &garraia_security::Allowlist, remetente: &garraia_channels::NickIrc) -> bool {
    lista.is_allowed_where(|entrada| {
        entrada
            .strip_prefix(PREFIXO_DA_ALLOWLIST)
            .is_some_and(|nick| remetente.mesmo_nick(nick))
    })
}

/// O callback de cada `PRIVMSG` recebido: allowlist, sessao e turno.
/// `conexao` e o nome da entrada IRC na config, que entra no id da sessao
/// privada ([`sessao_da_conversa`]).
fn callback_irc(state: &SharedState, conexao: &str) -> garraia_channels::IrcOnMessageFn {
    // #1189: gates compartilhados com o AppState -- instancias
    // proprias fazem o `/pair` nunca casar com o `claim()`.
    let (allowlist, pairing) = channel_gates(state);

    let state_for_cb = Arc::clone(state);
    let allowlist_for_cb = Arc::clone(&allowlist);
    let pairing_for_cb = Arc::clone(&pairing);
    let conexao_for_cb: Arc<str> = Arc::from(conexao);

    Arc::new(
        move |conversa: garraia_channels::ConversaIrc,
              remetente: garraia_channels::NickIrc,
              user_name: String,
              text: String,
              delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
            let state = Arc::clone(&state_for_cb);
            let allowlist = Arc::clone(&allowlist_for_cb);
            let pairing = Arc::clone(&pairing_for_cb);
            let conexao = Arc::clone(&conexao_for_cb);
            Box::pin(async move {
                // No IRC o nick e a unica identidade que existe, e num
                // servidor sem NickServ qualquer um pode toma-lo. A
                // allowlist e o unico controle de acesso deste canal.
                {
                    let mut list = allowlist.lock().unwrap();
                    if list.needs_owner() {
                        list.claim_owner(entrada_da_allowlist(&remetente));
                        info!("irc: auto-paired owner {remetente}");
                        // Nada de "use /pair": o IRC nao trata comandos, e o
                        // `/pair` dos outros canais so atende o dono daquele
                        // canal (`is_owner` com o id de la), nao este nick.
                        // Para um dono do IRC, a unica porta e o arquivo
                        // (docs/hardening-gateway.md, GHSA-g6c8-mx6x-5g92).
                        return Ok(format!(
                            "Welcome, {user_name}! You are now the owner of this GarraIA \
                             bot. IRC has no /pair: to let someone else in, stop the \
                             gateway and add \"irc:<their nick>\" to users in \
                             allowlist.json."
                        ));
                    }

                    // So entradas `irc:` admitem aqui ([`admitido`]); uma sem
                    // prefixo pode ser o id de alguem de outro canal.
                    if !admitido(&list, &remetente) {
                        let trimmed = text.trim();
                        if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                            // Os erros de codigo contam por pessoa do
                            // servidor (o nick normalizado), e com o
                            // prefixo, separados dos ids dos outros canais.
                            let quem = format!("{PREFIXO_DA_ALLOWLIST}{}", remetente.normalizado());
                            let claimed = pairing.lock().unwrap().claim(trimmed, &quem);
                            if claimed.is_some() {
                                list.add(entrada_da_allowlist(&remetente));
                                info!("irc: paired user {remetente} via code");
                                return Ok(format!(
                                    "Welcome, {user_name}! You now have access to this bot."
                                ));
                            }
                        }

                        let onde = match &conversa {
                            garraia_channels::ConversaIrc::Canal(canal) => canal.as_str(),
                            garraia_channels::ConversaIrc::Privado(_) => "privado",
                        };
                        warn!("irc: unauthorized nick {remetente} in {onde}");
                        return Err("__blocked__".to_string());
                    }
                }

                // Daqui em diante, o nick como veio: o historico e o escopo de
                // aprovacao seguem como antes da #2015 (no escopo, o "sim" de
                // `Ana` nao aprova um pedido de `ana`, e a pessoa e perguntada
                // de novo: fail-closed).
                let nick = remetente.como_veio().to_string();

                let session_id = sessao_da_conversa(&conexao, &conversa);

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

    /// O nome da conexao IRC na config dos testes que usam [`conecta`].
    const CONEXAO: &str = "teste";

    /// Estado com o modelo que guarda e a allowlist com as `entradas` como
    /// ficam no `allowlist.json` (`irc:<nick>`, ou sem prefixo, como as
    /// antigas e as dos outros canais). A primeira e o dono, para nenhum turno
    /// virar o boas-vindas do pareamento; sem entradas, a allowlist fica sem
    /// dono.
    ///
    /// A allowlist e a do proprio `AppState` (o gate que o `channel_gates`
    /// entrega ao canal), preenchida pela API dela: construir outra aqui cairia
    /// em `config::tests::nenhum_canal_constroi_gate_proprio`, que varre o
    /// fonte deste arquivo. O `with_config_dir` aponta o `allowlist.json` para
    /// o tempdir do teste.
    fn estado(dir: &tempfile::TempDir, entradas: &[&str]) -> (SharedState, Arc<ModeloQueGuarda>) {
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
            if let Some(dono) = entradas.first() {
                assert!(lista.claim_owner(*dono));
            }
            for entrada in entradas {
                lista.add(*entrada);
            }
        }
        (state, modelo)
    }

    /// As entradas da allowlist, em ordem, para comparar.
    fn entradas(state: &SharedState) -> Vec<String> {
        let mut v: Vec<String> = state
            .allowlist
            .lock()
            .expect("lock")
            .list_users()
            .into_iter()
            .map(str::to_string)
            .collect();
        v.sort();
        v
    }

    /// Sobe o canal com o callback de producao contra um servidor falso e
    /// devolve o lado do servidor. A conexao se chama [`CONEXAO`] na config.
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
            callback_irc(state, CONEXAO),
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

    /// Manda `linha`, de alguem que a allowlist deve barrar, e confirma que o
    /// texto dela (`marca`) nao chegou ao modelo. Barrado e silencio, entao a
    /// prova e uma sonda do dono (`irc:dono`) logo depois: o runtime do teste
    /// tem uma thread so, e as tarefas de cada `PRIVMSG` rodam na ordem em
    /// que o laco de leitura as criou, a da `linha` antes da sonda.
    async fn barrado(servidor: &mut TcpStream, modelo: &ModeloQueGuarda, linha: &str, marca: &str) {
        escreve(servidor, linha).await;
        turno(servidor, ":dono!d@h PRIVMSG garrabot :sonda", "dono").await;
        let pedidos = modelo.pedidos();
        assert!(
            !pedidos.iter().any(|p| p.contains(marca)),
            "a allowlist deixou passar `{linha}`: {pedidos:?}"
        );
    }

    /// O vazamento da #2011: o modelo recebia as mensagens privadas da Ana no
    /// turno do Bruno, e a sessao `irc-<nick do bot>` juntava todo mundo.
    #[tokio::test]
    async fn conversas_privadas_de_pessoas_diferentes_nao_se_misturam() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:ana", "irc:bruno"]);
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
        assert!(state.sessions.contains_key("irc-@ana@teste"));
        assert!(state.sessions.contains_key("irc-@bruno@teste"));
        canal.disconnect().await.expect("desconecta");
    }

    #[test]
    fn sessao_da_conversa_separa_canal_de_privado() {
        use garraia_channels::ConversaIrc::{Canal, Privado};
        let sessao = sessao_da_conversa;
        assert_eq!(sessao("libera", &Canal("#garra".into())), "irc-#garra");
        assert_eq!(sessao("libera", &Canal("&local".into())), "irc-&local");
        assert_eq!(sessao("libera", &Privado("ana".into())), "irc-@ana@libera");
        // O id privado nunca e o de um canal, o `irc-<nick do bot>` antigo nem
        // o `irc-@<nick>` sem conexao.
        assert_ne!(
            sessao("libera", &Privado("garrabot".into())),
            "irc-garrabot"
        );
        assert_ne!(sessao("libera", &Privado("ana".into())), "irc-@ana");
        // O prefixo do canal continua `irc-` (channels_view::PREFIXOS_DE_SESSAO).
        assert!(sessao("libera", &Privado("ana".into())).starts_with("irc-"));
    }

    /// O mesmo nick em conexoes diferentes e outra pessoa, e o id privado e
    /// injetivo mesmo com `@` no nome da conexao: o nick nunca tem `@`, entao
    /// o primeiro `@` depois de `irc-@` separa os dois.
    #[test]
    fn sessao_privada_diz_de_qual_conexao_e() {
        use garraia_channels::ConversaIrc::Privado;
        let sessao = sessao_da_conversa;
        assert_ne!(
            sessao("libera", &Privado("ana".into())),
            sessao("oftc", &Privado("ana".into()))
        );
        // ("ana", "b@c") e ("ana@b", "c") dariam o mesmo texto, mas `ana@b`
        // nao chega como nick: o `garraia-channels` corta o prefixo no `@`.
        assert_eq!(sessao("b@c", &Privado("ana".into())), "irc-@ana@b@c");
    }

    /// A mesma pessoa continua a mesma conversa, com o nick na caixa que for:
    /// `Ana` e `ana` sao o mesmo nick no servidor, e com `CASEMAPPING=rfc1459`
    /// `[ana]` e `{ana}` tambem.
    #[tokio::test]
    async fn a_mesma_pessoa_continua_a_conversa_mesmo_trocando_a_caixa() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:Ana", "irc:ana", "irc:[bia]", "irc:{bia}"]);
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
        let (state, modelo) = estado(&dir, &["irc:[a]", "irc:{a}"]);
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
        let (state, modelo) = estado(&dir, &["irc:ana", "irc:bruno"]);
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
        let (state, _modelo) = estado(&dir, &["irc:ana"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        turno(&mut servidor, ":ana!a@h PRIVMSG &local :oi", "&local").await;

        assert!(state.sessions.contains_key("irc-&local"));
        canal.disconnect().await.expect("desconecta");
    }

    /// Uma conexao IRC da config apontando para um servidor falso nesta porta.
    fn conexao_irc(porta: u16) -> garraia_config::ChannelConfig {
        garraia_config::ChannelConfig {
            channel_type: "irc".into(),
            enabled: None,
            settings: std::collections::HashMap::from([
                ("server".to_string(), serde_json::json!("127.0.0.1")),
                ("port".to_string(), serde_json::json!(porta)),
                ("channels".to_string(), serde_json::json!(["#garra"])),
            ]),
        }
    }

    /// Duas conexoes IRC na config, cada uma num servidor: o nick so identifica
    /// alguem dentro da propria rede, entao a conversa privada de cada conexao
    /// e uma sessao. Os canais saem do `build_irc_channels`, como no boot, para
    /// o nome da conexao vir da config.
    #[tokio::test]
    async fn cada_conexao_tem_as_proprias_conversas_privadas() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:ana"]);

        let rede_a = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let rede_b = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let config = AppConfig {
            channels: std::collections::HashMap::from([
                (
                    "rede-a".to_string(),
                    conexao_irc(rede_a.local_addr().expect("addr").port()),
                ),
                (
                    "rede-b".to_string(),
                    conexao_irc(rede_b.local_addr().expect("addr").port()),
                ),
            ]),
            ..Default::default()
        };

        let mut canais = build_irc_channels(&config, &state);
        assert_eq!(canais.len(), 2, "uma conexao por entrada `type = irc`");
        let aceita_a = tokio::spawn(async move { rede_a.accept().await.map(|(s, _)| s) });
        let aceita_b = tokio::spawn(async move { rede_b.accept().await.map(|(s, _)| s) });
        for canal in &mut canais {
            canal.connect().await.expect("conecta");
        }
        let mut servidor_a = aceita_a.await.expect("join").expect("accept");
        let mut servidor_b = aceita_b.await.expect("join").expect("accept");

        turno(
            &mut servidor_a,
            ":ana!a@h PRIVMSG garrabot :segredo-da-rede-a",
            "ana",
        )
        .await;
        turno(&mut servidor_b, ":ana!a@h PRIVMSG garrabot :oi", "ana").await;
        turno(&mut servidor_a, ":ana!a@h PRIVMSG garrabot :e ai", "ana").await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 3, "{pedidos:?}");
        assert!(
            !pedidos[1].contains("segredo-da-rede-a"),
            "a conversa privada da conexao `rede-b` recebeu a da `rede-a`: {}",
            pedidos[1]
        );
        assert!(
            pedidos[2].contains("segredo-da-rede-a"),
            "a conversa da `rede-a` continua: {}",
            pedidos[2]
        );
        assert!(state.sessions.contains_key("irc-@ana@rede-a"));
        assert!(state.sessions.contains_key("irc-@ana@rede-b"));
        assert!(!state.sessions.contains_key("irc-@ana"));
        for canal in &mut canais {
            canal.disconnect().await.expect("desconecta");
        }
    }

    // #2015: a allowlist reconhece a mesma pessoa que a sessao reconhece. As
    // entradas do IRC sao `irc:<nick como veio>`, comparadas pelo
    // `CASEMAPPING` que o servidor anuncia na hora.

    /// `CASEMAPPING=rfc1459`: `Ana` admitida faz `ana` passar, e `[bia]`
    /// admitida faz `{bia}` passar. Antes, as duas eram barradas em silencio.
    #[tokio::test]
    async fn allowlist_admite_a_mesma_pessoa_em_outra_grafia() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono", "irc:Ana", "irc:[bia]"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        turno(&mut servidor, ":ana!a@h PRIVMSG garrabot :oi-da-ana", "ana").await;
        turno(
            &mut servidor,
            ":{bia}!b@h PRIVMSG garrabot :oi-da-bia",
            "{bia}",
        )
        .await;

        let pedidos = modelo.pedidos();
        assert_eq!(pedidos.len(), 2, "{pedidos:?}");
        assert!(pedidos[0].contains("oi-da-ana"), "{}", pedidos[0]);
        assert!(pedidos[1].contains("oi-da-bia"), "{}", pedidos[1]);
        canal.disconnect().await.expect("desconecta");
    }

    /// Sem anuncio (fica `ascii`) e com `CASEMAPPING=ascii`, `[bia]` e `{bia}`
    /// sao duas pessoas no servidor: a entrada de uma nao admite a outra.
    #[tokio::test]
    async fn sem_rfc1459_colchete_e_chave_sao_pessoas_diferentes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono", "irc:[bia]"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        barrado(
            &mut servidor,
            &modelo,
            ":{bia}!b@h PRIVMSG garrabot :marca-sem-anuncio",
            "marca-sem-anuncio",
        )
        .await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=ascii :are supported by this server",
        )
        .await;
        barrado(
            &mut servidor,
            &modelo,
            ":{bia}!b@h PRIVMSG garrabot :marca-ascii",
            "marca-ascii",
        )
        .await;
        // A caixa continua valendo em `ascii`.
        turno(&mut servidor, ":[BIA]!b@h PRIVMSG garrabot :oi", "[BIA]").await;
        canal.disconnect().await.expect("desconecta");
    }

    /// A entrada guarda o nick como veio, e a comparacao usa o `CASEMAPPING`
    /// da hora: `[bia]`, pareada sob `rfc1459`, nao admite `{bia}` quando o
    /// servidor passa a anunciar `ascii`. Gravar a forma normalizada (`{bia}`)
    /// admitiria, em `ascii`, outra pessoa.
    #[tokio::test]
    async fn entrada_gravada_sob_rfc1459_nao_vale_para_outra_grafia_em_ascii() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        let codigo = state.pairing.lock().expect("lock").generate("irc");
        turno(
            &mut servidor,
            &format!(":[bia]!b@h PRIVMSG garrabot :{codigo}"),
            "[bia]",
        )
        .await;
        assert_eq!(entradas(&state), ["irc:[bia]", "irc:dono"]);
        turno(&mut servidor, ":{bia}!b@h PRIVMSG garrabot :oi", "{bia}").await;

        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=ascii :are supported by this server",
        )
        .await;
        barrado(
            &mut servidor,
            &modelo,
            ":{bia}!b@h PRIVMSG garrabot :marca-chave",
            "marca-chave",
        )
        .await;
        turno(&mut servidor, ":[bia]!b@h PRIVMSG garrabot :oi", "[bia]").await;
        canal.disconnect().await.expect("desconecta");
    }

    /// So entradas `irc:` sao comparadas pelo `CASEMAPPING`. Uma entrada sem
    /// prefixo pode ser de qualquer canal (os ids do Slack tem maiusculas, por
    /// exemplo) e nao vira `irc:` por outra grafia; e uma entrada `irc:` nao
    /// admite ninguem dos outros canais, que consultam a allowlist por
    /// igualdade exata.
    #[tokio::test]
    async fn so_entradas_do_irc_sao_comparadas_pelo_casemapping() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono", "Ana", "irc:Bruno"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        barrado(
            &mut servidor,
            &modelo,
            ":ana!a@h PRIVMSG garrabot :marca-da-ana",
            "marca-da-ana",
        )
        .await;
        assert!(
            !entradas(&state).iter().any(|e| e == "irc:ana"),
            "uma entrada sem prefixo nao pode virar `irc:` por outra grafia"
        );

        {
            let lista = state.allowlist.lock().expect("lock");
            for de_outro_canal in ["Bruno", "bruno", "dono"] {
                assert!(
                    !lista.is_allowed(de_outro_canal),
                    "a entrada `irc:` admitiu `{de_outro_canal}` fora do IRC"
                );
            }
        }
        canal.disconnect().await.expect("desconecta");
    }

    /// Os erros de codigo do `/pair` contam por pessoa do servidor: trocar so
    /// a caixa do nick nao zera a contagem. Cinco erros em grafias diferentes
    /// travam o nick, e o codigo certo depois nao entra.
    #[tokio::test]
    async fn erros_de_codigo_contam_pelo_nick_normalizado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, _modelo) = estado(&dir, &["irc:dono"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        let codigo = state.pairing.lock().expect("lock").generate("irc");
        let errado = if codigo == "123456" {
            "654321"
        } else {
            "123456"
        };
        for grafia in ["Eve", "EVE", "eve", "eVe", "evE"] {
            escreve(
                &mut servidor,
                &format!(":{grafia}!e@h PRIVMSG garrabot :{errado}"),
            )
            .await;
        }
        escreve(
            &mut servidor,
            &format!(":Eve!e@h PRIVMSG garrabot :{codigo}"),
        )
        .await;
        // A sonda do dono vem depois das seis, na mesma ordem.
        turno(&mut servidor, ":dono!d@h PRIVMSG garrabot :sonda", "dono").await;

        assert_eq!(
            entradas(&state),
            ["irc:dono"],
            "o codigo certo entrou depois de cinco erros do mesmo nick"
        );
        canal.disconnect().await.expect("desconecta");
    }

    // GHSA-g6c8-mx6x-5g92: os ids dos outros canais ficam sem prefixo na
    // mesma allowlist, e no IRC o nick e escolhido por quem conecta. Uma
    // entrada sem prefixo nunca admite ninguem no IRC: so as `irc:` valem aqui.

    /// Um nick igual ao id de alguem admitido por outro canal (o id do Slack e
    /// um nick valido em qualquer servidor) e barrado, em qualquer grafia, e
    /// nao ganha entrada `irc:`. O id continua valendo no canal de origem.
    #[tokio::test]
    async fn nick_igual_ao_id_de_outro_canal_e_barrado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono", "U01ABCDEF"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        for (grafia, marca) in [("U01ABCDEF", "marca-exata"), ("u01abcdef", "marca-caixa")] {
            barrado(
                &mut servidor,
                &modelo,
                &format!(":{grafia}!x@h PRIVMSG garrabot :{marca}"),
                marca,
            )
            .await;
        }
        assert_eq!(
            entradas(&state),
            ["U01ABCDEF", "irc:dono"],
            "o id de outro canal ganhou entrada `irc:`"
        );
        assert!(
            state
                .allowlist
                .lock()
                .expect("lock")
                .is_allowed("U01ABCDEF"),
            "o id continua valendo no canal de origem"
        );
        canal.disconnect().await.expect("desconecta");
    }

    /// O dono pareado por outro canal tambem: o id dele, usado como nick, nao
    /// passa no IRC, e o dono continua o mesmo.
    #[tokio::test]
    async fn id_do_dono_de_outro_canal_nao_passa_no_irc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["U01DONO", "irc:dono"]);
        let (mut canal, mut servidor) = conecta(&state).await;

        barrado(
            &mut servidor,
            &modelo,
            ":U01DONO!x@h PRIVMSG garrabot :marca-do-dono",
            "marca-do-dono",
        )
        .await;
        assert_eq!(entradas(&state), ["U01DONO", "irc:dono"]);
        assert_eq!(
            state.allowlist.lock().expect("lock").owner(),
            Some("U01DONO")
        );
        canal.disconnect().await.expect("desconecta");
    }

    /// Entrada antiga do IRC, sem prefixo (de antes da #2015): sem prefixo nao
    /// da para saber de que canal ela e, entao ela nao vale no IRC, nem exata.
    /// A pessoa volta com um codigo novo de `/pair`, que grava a forma `irc:`,
    /// e dai em diante passa em qualquer grafia.
    #[tokio::test]
    async fn entrada_sem_prefixo_nao_vale_no_irc_e_o_pareamento_devolve_o_acesso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &["irc:dono", "Ana"]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        barrado(
            &mut servidor,
            &modelo,
            ":Ana!a@h PRIVMSG garrabot :marca-exata",
            "marca-exata",
        )
        .await;
        assert_eq!(
            entradas(&state),
            ["Ana", "irc:dono"],
            "a entrada sem prefixo ganhou a forma `irc:`"
        );

        let codigo = state.pairing.lock().expect("lock").generate("irc");
        turno(
            &mut servidor,
            &format!(":Ana!a@h PRIVMSG garrabot :{codigo}"),
            "Ana",
        )
        .await;
        assert_eq!(entradas(&state), ["Ana", "irc:Ana", "irc:dono"]);
        turno(&mut servidor, ":ana!a@h PRIVMSG garrabot :de-novo", "ana").await;
        assert!(
            modelo.pedidos().iter().any(|p| p.contains("de-novo")),
            "{:?}",
            modelo.pedidos()
        );
        canal.disconnect().await.expect("desconecta");
    }

    /// O dono pareado automaticamente tambem e `irc:<nick>`, e volta em outra
    /// grafia: o pareamento automatico ja nao vale para ele.
    #[tokio::test]
    async fn dono_volta_em_outra_grafia() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (state, modelo) = estado(&dir, &[]);
        let (mut canal, mut servidor) = conecta(&state).await;
        escreve(
            &mut servidor,
            ":srv 005 garrabot CASEMAPPING=rfc1459 :are supported by this server",
        )
        .await;

        turno(&mut servidor, ":Ana!a@h PRIVMSG garrabot :oi", "Ana").await;
        assert_eq!(
            state.allowlist.lock().expect("lock").owner(),
            Some("irc:Ana")
        );
        turno(&mut servidor, ":ana!a@h PRIVMSG garrabot :voltei", "ana").await;
        assert!(
            modelo.pedidos().iter().any(|p| p.contains("voltei")),
            "{:?}",
            modelo.pedidos()
        );
        canal.disconnect().await.expect("desconecta");
    }
}
