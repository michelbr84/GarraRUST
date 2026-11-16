//! A prova do dono do canal vale para o TURNO que a fez, e so para ele (#1910).
//!
//! A #1905 abriu o relatorio do `garra_status` para o dono pareado do bot em
//! chat privado do Telegram. A prova morava num campo MUTAVEL da sessao,
//! escrito pelo ponto de entrada e lido pela tool durante a execucao: dois
//! turnos sobrepostos na MESMA sessao (sessao compartilhada, Chat Sync) liam
//! o campo um do outro. O turno que nao provou nada podia herdar a marca do
//! dono — e o turno do dono podia perder a dele.
//!
//! Aqui a prova e um valor imutavel publicado num `task_local` APENAS em volta
//! do futuro que processa o turno do Telegram ([`no_turno_do_telegram`], texto,
//! streaming e voz). A tool consulta o escopo do PROPRIO futuro
//! ([`dono_provado_neste_turno`]) e confere que ele foi emitido para a mesma
//! instancia de `AppState`, a mesma sessao e o mesmo usuario do seu
//! `ToolContext`. Todo o resto nega (fail-closed):
//!
//! - fora de qualquer escopo (outro canal, web chat, teste, chamada direta);
//! - escopo de turno sem prova (nao-dono, grupo, allowlist sem dono) — e ele
//!   sombreia qualquer escopo externo;
//! - escopo de outra sessao, de outro usuario, de outro `AppState`, ou um
//!   `ToolContext` sem usuario;
//! - task filha (`tokio::spawn`): o `task_local` nao e herdado.
//!
//! A prova nao tem `Clone`, `Serialize` nem `Deserialize`: nao sai do
//! processo, nao vai para o historico nem para o banco, e nenhum texto do LLM,
//! de arquivo ou de MCP a fabrica — so o construtor deste modulo, a partir do
//! allowlist da instalacao e do `chat_id` que o Telegram entregou.
//!
//! O que esta prova NAO e: credencial de execucao. Ela so dispensa a retencao
//! do relatorio do `garra_status`. O teto de capacidades do turno
//! (`channels_view::teto_da_superficie`, alimentado por `sessao_do_operador`)
//! nao a consulta, e o canal remoto continua no piso `Read`. O restante da
//! #1910 (principal autenticado em `ExecContext`/`ToolContext` para conceder
//! execucao) segue aberto.

use std::future::Future;
use std::sync::{Arc, Weak};

use crate::state::AppState;

/// Fatos verificados de um turno do dono, emitidos para UMA instancia de
/// `AppState`, UMA sessao e UM usuario. Campos privados, sem `Clone` e sem
/// serde: so [`prova_do_telegram`] a constroi.
struct ProvaDoDono {
    /// `Weak` (e nao endereco cru) para a comparacao de identidade nao sofrer
    /// reuso de endereco: enquanto a prova existe, a alocacao tambem.
    estado: Weak<AppState>,
    session_id: String,
    user_id: String,
}

impl ProvaDoDono {
    fn vale_para(&self, state: &AppState, session_id: &str, user_id: &str) -> bool {
        std::ptr::eq(self.estado.as_ptr(), state)
            && self.session_id == session_id
            && self.user_id == user_id
    }
}

tokio::task_local! {
    /// `Some` so no futuro de um turno que provou o dono. `None` e um escopo
    /// de turno aberto SEM prova: nega e sombreia qualquer escopo externo.
    static PROVA_DO_TURNO: Option<ProvaDoDono>;
}

/// Chat privado do dono pareado (#1905). Grupo (id negativo) nao conta: o
/// mesmo chat carrega outros membros pareados e o relatorio aberto seria
/// postado para todos (#1347). No Telegram, o id de um chat privado e o id do
/// usuario — e e positivo. A comparacao e textual contra a forma canonica do
/// `chat_id`, entao `+7978617919` ou `07978617919` nao casam.
pub(crate) fn dono_em_chat_privado(is_owner: bool, chat_id: i64, user_id: &str) -> bool {
    is_owner && chat_id > 0 && chat_id.to_string() == user_id
}

/// Constroi a prova a partir de fatos verificados: o dono do allowlist da
/// PROPRIA instalacao (o mesmo gate que decide quem fala no canal) e o chat
/// privado. Lock envenenado, id vazio ou qualquer fato faltando: sem prova.
fn prova_do_telegram(
    state: &Arc<AppState>,
    session_id: &str,
    chat_id: i64,
    user_id: &str,
) -> Option<ProvaDoDono> {
    if session_id.is_empty() || user_id.is_empty() {
        return None;
    }
    let is_owner = state
        .allowlist
        .lock()
        .map(|l| l.is_owner(user_id))
        .unwrap_or(false);
    dono_em_chat_privado(is_owner, chat_id, user_id).then(|| ProvaDoDono {
        estado: Arc::downgrade(state),
        session_id: session_id.to_string(),
        user_id: user_id.to_string(),
    })
}

/// Roda `turno` (o processamento de UMA mensagem do Telegram: texto, streaming
/// ou voz) com a prova do dono deste turno — ou com a ausencia explicita dela.
/// O escopo termina com o futuro: nada fica gravado na sessao.
pub(crate) async fn no_turno_do_telegram<F: Future>(
    state: &Arc<AppState>,
    session_id: &str,
    chat_id: i64,
    user_id: &str,
    turno: F,
) -> F::Output {
    let prova = prova_do_telegram(state, session_id, chat_id, user_id);
    PROVA_DO_TURNO.scope(prova, turno).await
}

/// O turno em que esta chamada roda provou o dono para ESTE `AppState`, ESTA
/// sessao e ESTE usuario? Sem escopo, sem prova, sem usuario ou com qualquer
/// divergencia: `false`.
pub(crate) fn dono_provado_neste_turno(
    state: &AppState,
    session_id: &str,
    user_id: Option<&str>,
) -> bool {
    let Some(user_id) = user_id else {
        return false;
    };
    PROVA_DO_TURNO
        .try_with(|prova| {
            prova
                .as_ref()
                .is_some_and(|p| p.vale_para(state, session_id, user_id))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;
    use garraia_security::Allowlist;

    const DONO: &str = "7978617919";
    const DONO_CHAT: i64 = 7978617919;
    const OUTRO: &str = "555";
    const OUTRO_CHAT: i64 = 555;
    const GRUPO: i64 = -1001234567890;
    const SID: &str = "telegram-7978617919";

    /// Estado num tempdir (nada do config dir do desenvolvedor) com o dono
    /// pareado SO em memoria: `Allowlist::restricted` nao tem caminho, entao
    /// `claim_owner` nao grava arquivo nenhum.
    fn estado(dir: &std::path::Path) -> Arc<AppState> {
        let config = AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let st = Arc::new(AppState::with_config_dir(
            config,
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir,
        ));
        let mut lista = Allowlist::restricted(vec![OUTRO.to_string()]);
        assert!(lista.claim_owner(DONO));
        *st.allowlist.lock().expect("lock") = lista;
        st
    }

    // ── a regra pura (migrada de `bootstrap::telegram`, #1905) ──────────

    #[test]
    fn chat_privado_do_dono_prova() {
        assert!(dono_em_chat_privado(true, DONO_CHAT, DONO));
    }

    #[test]
    fn grupo_nao_prova_nem_para_o_dono() {
        assert!(!dono_em_chat_privado(true, GRUPO, DONO));
    }

    #[test]
    fn nao_dono_nunca_prova() {
        assert!(!dono_em_chat_privado(false, DONO_CHAT, DONO));
        assert!(!dono_em_chat_privado(false, GRUPO, OUTRO));
    }

    /// `chat == remetente > 0`, na forma canonica: id zero, negativo ou
    /// escrito de outro jeito nao casa.
    #[test]
    fn so_o_id_canonico_e_positivo_casa() {
        assert!(!dono_em_chat_privado(true, 0, "0"));
        assert!(!dono_em_chat_privado(true, -7978617919, "-7978617919"));
        assert!(!dono_em_chat_privado(true, DONO_CHAT, "+7978617919"));
        assert!(!dono_em_chat_privado(true, DONO_CHAT, "07978617919"));
        assert!(!dono_em_chat_privado(true, DONO_CHAT, ""));
    }

    // ── o escopo ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn fora_de_escopo_nao_ha_dono() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        assert!(!dono_provado_neste_turno(&st, SID, Some(DONO)));
    }

    /// O escopo do dono vale para a MESMA instancia, sessao e usuario — e
    /// termina com o turno: a sessao nao guarda nada.
    #[tokio::test]
    async fn escopo_do_dono_vale_so_para_estado_sessao_e_usuario_exatos() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        let dir2 = tempfile::tempdir().expect("tempdir");
        let outro_estado = estado(dir2.path());

        let vistos = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            (
                dono_provado_neste_turno(&st, SID, Some(DONO)),
                dono_provado_neste_turno(&st, "outra-sessao", Some(DONO)),
                dono_provado_neste_turno(&st, SID, Some(OUTRO)),
                dono_provado_neste_turno(&st, SID, None),
                dono_provado_neste_turno(&outro_estado, SID, Some(DONO)),
            )
        })
        .await;
        assert_eq!(vistos, (true, false, false, false, false));
        assert!(
            !dono_provado_neste_turno(&st, SID, Some(DONO)),
            "a prova sobreviveu ao fim do turno"
        );
    }

    /// Nao-dono, grupo, chat de outro, allowlist sem dono e ids vazios:
    /// o escopo abre, mas sem prova.
    #[tokio::test]
    async fn turno_sem_prova_nao_concede() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        let casos: [(&str, i64, &str); 5] = [
            (SID, OUTRO_CHAT, OUTRO), // nao-dono em chat privado
            (SID, GRUPO, DONO),       // dono em grupo
            (SID, OUTRO_CHAT, DONO),  // dono, mas o chat e de outro
            ("", DONO_CHAT, DONO),    // sessao vazia
            (SID, DONO_CHAT, ""),     // usuario vazio
        ];
        for (sid, chat, user) in casos {
            let visto = no_turno_do_telegram(&st, sid, chat, user, async {
                dono_provado_neste_turno(&st, sid, Some(user))
            })
            .await;
            assert!(!visto, "concedeu para {sid:?}/{chat}/{user:?}");
        }

        // Instalacao sem dono pareado: ninguem prova.
        *st.allowlist.lock().expect("lock") = Allowlist::restricted(vec![DONO.to_string()]);
        let visto = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            dono_provado_neste_turno(&st, SID, Some(DONO))
        })
        .await;
        assert!(!visto, "allowlist sem dono concedeu");
    }

    /// Um turno sem prova aninhado num escopo do dono NAO herda a prova de
    /// fora; quando ele termina, o de fora volta a valer.
    #[tokio::test]
    async fn escopo_sem_prova_sombreia_o_de_fora() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        let (dentro, depois) = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            let dentro = no_turno_do_telegram(&st, SID, OUTRO_CHAT, OUTRO, async {
                dono_provado_neste_turno(&st, SID, Some(DONO))
            })
            .await;
            (dentro, dono_provado_neste_turno(&st, SID, Some(DONO)))
        })
        .await;
        assert!(!dentro, "o turno sem prova herdou a prova de fora");
        assert!(depois, "o escopo do dono se perdeu depois do aninhado");
    }

    /// Task filha nao herda o `task_local`: fail-closed.
    #[tokio::test]
    async fn task_filha_nao_herda_a_prova() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        let na_filha = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            let st2 = Arc::clone(&st);
            tokio::spawn(async move { dono_provado_neste_turno(&st2, SID, Some(DONO)) })
                .await
                .expect("task filha")
        })
        .await;
        assert!(!na_filha, "a task filha herdou a prova do dono");
    }

    /// O defeito da #1910, no nivel do escopo: o turno do dono e um turno de
    /// outra origem (Telegram de nao-dono, ou web/Chat Sync sem escopo) na
    /// MESMA sessao, intercalados por uma barreira, nao trocam autoridade em
    /// nenhum dos dois sentidos.
    #[tokio::test]
    async fn turnos_intercalados_na_mesma_sessao_nao_trocam_autoridade() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        let barreira = tokio::sync::Barrier::new(3);

        let dono = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            let antes = dono_provado_neste_turno(&st, SID, Some(DONO));
            barreira.wait().await;
            tokio::task::yield_now().await;
            (antes, dono_provado_neste_turno(&st, SID, Some(DONO)))
        });
        let nao_dono = no_turno_do_telegram(&st, SID, OUTRO_CHAT, OUTRO, async {
            barreira.wait().await;
            (
                dono_provado_neste_turno(&st, SID, Some(OUTRO)),
                // Nem alegando ser o dono no `ToolContext`.
                dono_provado_neste_turno(&st, SID, Some(DONO)),
            )
        });
        let sem_escopo = async {
            barreira.wait().await;
            dono_provado_neste_turno(&st, SID, Some(DONO))
        };
        let (dono, nao_dono, sem_escopo) = tokio::join!(dono, nao_dono, sem_escopo);
        assert_eq!(dono, (true, true), "o dono perdeu a prova no meio do turno");
        assert_eq!(nao_dono, (false, false), "o nao-dono herdou a prova");
        assert!(!sem_escopo, "o turno sem escopo herdou a prova");
    }

    /// A prova nao vira teto: dentro do escopo do dono, `sessao_do_operador`
    /// continua `false` e o contexto do turno sai com o teto `Read` da
    /// superficie remota (#1905, B1).
    #[tokio::test]
    async fn prova_do_dono_nao_muda_o_teto_de_capacidades() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = estado(dir.path());
        st.hydrate_session_history(SID, Some("telegram"), Some(DONO))
            .await;
        let (provou, operador, teto) = no_turno_do_telegram(&st, SID, DONO_CHAT, DONO, async {
            let exec = st.exec_context_for(SID, None).await;
            (
                dono_provado_neste_turno(&st, SID, Some(DONO)),
                crate::channels_view::sessao_do_operador(&st, SID).await,
                exec.teto.is_some(),
            )
        })
        .await;
        assert!(provou, "o teste so vale com a prova ligada");
        assert!(!operador, "a prova do dono virou operador");
        assert!(teto, "turno do dono saiu sem teto de superficie");
    }

    // ── fiacao ───────────────────────────────────────────────────────────

    /// O escopo atravessa o `AgentRuntime` de producao ate o `execute` da
    /// tool, nos dois pontos de entrada que o Telegram usa (batch, que a voz
    /// tambem usa, e streaming). Se o runtime passar a despachar tools em
    /// task propria, o dono perde o relatorio — e este teste acusa.
    mod pelo_runtime {
        use super::*;
        use futures::Stream;
        use garraia_agents::{
            ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, StreamEvent,
        };
        use std::pin::Pin;
        use std::sync::Mutex;

        /// Pede `garra_status` na primeira rodada e guarda o relatorio.
        #[derive(Default)]
        struct PedeStatus {
            relatorio: Mutex<Option<String>>,
        }

        impl PedeStatus {
            /// `None` na primeira rodada (pede a tool); depois, o relatorio.
            fn rodada(&self, request: &LlmRequest) -> Option<String> {
                let relatorio = request.messages.last().and_then(|m| match &m.content {
                    MessagePart::Parts(p) => p.iter().find_map(|b| match b {
                        ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                        _ => None,
                    }),
                    _ => None,
                });
                if let Some(r) = &relatorio {
                    *self.relatorio.lock().expect("lock") = Some(r.clone());
                }
                relatorio
            }
        }

        #[async_trait::async_trait]
        impl LlmProvider for PedeStatus {
            fn provider_id(&self) -> &str {
                "pede_status"
            }
            async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
                let content = match self.rodada(request) {
                    Some(_) => ContentBlock::Text {
                        text: "ok".to_string(),
                    },
                    None => ContentBlock::ToolUse {
                        id: "status-1".to_string(),
                        name: "garra_status".to_string(),
                        input: serde_json::json!({}),
                    },
                };
                Ok(LlmResponse {
                    content: vec![content],
                    model: "m".to_string(),
                    usage: None,
                    stop_reason: None,
                })
            }
            async fn stream_complete(
                &self,
                request: &LlmRequest,
            ) -> garraia_common::Result<
                Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
            > {
                let eventos = match self.rodada(request) {
                    Some(_) => vec![
                        Ok(StreamEvent::TextDelta("ok".to_string())),
                        Ok(StreamEvent::MessageStop),
                    ],
                    None => vec![
                        Ok(StreamEvent::ToolUseStart {
                            index: 0,
                            id: "status-1".to_string(),
                            name: "garra_status".to_string(),
                        }),
                        Ok(StreamEvent::InputJsonDelta("{}".to_string())),
                        Ok(StreamEvent::ContentBlockStop { index: 0 }),
                        Ok(StreamEvent::MessageStop),
                    ],
                };
                Ok(Box::pin(futures::stream::iter(eventos)))
            }
            async fn health_check(&self) -> garraia_common::Result<bool> {
                Ok(true)
            }
        }

        fn estado_com_modelo(dir: &std::path::Path) -> (Arc<AppState>, Arc<PedeStatus>) {
            let config = AppConfig {
                data_dir: Some(dir.to_path_buf()),
                ..Default::default()
            };
            let agents = AgentRuntime::new();
            let modelo = Arc::new(PedeStatus::default());
            agents.register_provider(Arc::clone(&modelo) as Arc<dyn LlmProvider>);
            let st = Arc::new(AppState::with_config_dir(
                config,
                Arc::new(agents),
                ChannelRegistry::new(),
                dir,
            ));
            st.agents
                .register_tool(Box::new(crate::tools::GarraStatusTool::new(
                    &st,
                    crate::push_channels::PushMounted::default(),
                )));
            let mut lista = Allowlist::restricted(vec![OUTRO.to_string()]);
            assert!(lista.claim_owner(DONO));
            *st.allowlist.lock().expect("lock") = lista;
            (st, modelo)
        }

        /// Um turno como o `bootstrap::telegram` roda: hidrata, depois
        /// processa DENTRO de `no_turno_do_telegram`. Devolve o relatorio.
        async fn turno(
            st: &Arc<AppState>,
            modelo: &PedeStatus,
            user: &str,
            chat: i64,
            streaming: bool,
        ) -> serde_json::Value {
            *modelo.relatorio.lock().expect("lock") = None;
            st.hydrate_session_history(SID, Some("telegram"), Some(user))
                .await;
            let exec = st.exec_context_for(SID, None).await;
            let resposta = no_turno_do_telegram(st, SID, chat, user, async {
                if streaming {
                    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
                    let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });
                    let r = st
                        .agents
                        .process_message_streaming_with_agent_config(
                            SID,
                            "qual versao?",
                            &[],
                            tx,
                            None,
                            Some(user),
                            None,
                            None,
                            None,
                            None,
                            &exec,
                        )
                        .await;
                    let _ = dreno.await;
                    r
                } else {
                    st.agents
                        .process_message_with_agent_config(
                            SID,
                            "qual versao?",
                            &[],
                            None,
                            Some(user),
                            None,
                            None,
                            None,
                            None,
                            &exec,
                        )
                        .await
                }
            })
            .await;
            resposta.expect("turno");
            let relatorio = modelo
                .relatorio
                .lock()
                .expect("lock")
                .clone()
                .expect("o modelo nao recebeu o relatorio");
            serde_json::from_str(&relatorio).expect("json")
        }

        #[tokio::test]
        async fn dono_recebe_aberto_e_nao_dono_restrito_em_batch_e_streaming() {
            for streaming in [false, true] {
                let dir = tempfile::tempdir().expect("tempdir");
                let (st, modelo) = estado_com_modelo(dir.path());

                let dono = turno(&st, &modelo, DONO, DONO_CHAT, streaming).await;
                assert_eq!(
                    dono["withheld"],
                    serde_json::json!([]),
                    "streaming={streaming}: {dono}"
                );
                assert_eq!(dono["version"], env!("CARGO_PKG_VERSION"));

                let nao_dono = turno(&st, &modelo, OUTRO, OUTRO_CHAT, streaming).await;
                assert!(
                    nao_dono["providers"].is_null()
                        && nao_dono["withheld"] != serde_json::json!([]),
                    "streaming={streaming}: nao-dono recebeu o relatorio aberto: {nao_dono}"
                );

                let grupo = turno(&st, &modelo, DONO, GRUPO, streaming).await;
                assert!(
                    grupo["providers"].is_null(),
                    "streaming={streaming}: dono em grupo recebeu o relatorio aberto: {grupo}"
                );
            }
        }
    }

    /// Guarda de fiacao (varre o fonte): os tres processamentos do Telegram
    /// (texto batch, texto streaming e voz) rodam DENTRO de
    /// `no_turno_do_telegram`, e a marca mutavel da sessao nao volta.
    #[test]
    fn telegram_processa_todo_turno_dentro_do_escopo() {
        let fonte = include_str!("bootstrap/telegram.rs");
        let abre = "prova_do_dono::no_turno_do_telegram(";
        assert_eq!(
            fonte.matches(abre).count(),
            2,
            "esperava um escopo no handler de texto e um no de voz"
        );
        let processa = [
            ".process_message_with_agent_config(",
            ".process_message_streaming_with_agent_config(",
        ];
        let total: usize = processa.iter().map(|p| fonte.matches(p).count()).sum();
        assert_eq!(total, 3, "processamento novo no Telegram: ponha no escopo");
        // Cada processamento cai entre a abertura de um escopo e o
        // `persist_turn` que fecha o turno.
        for p in processa {
            for (pos, _) in fonte.match_indices(p) {
                let antes = &fonte[..pos];
                let abertura = antes.rfind(abre).unwrap_or_else(|| {
                    panic!("{p} em {pos} sem escopo antes");
                });
                assert!(
                    !antes[abertura..].contains(".persist_turn("),
                    "{p} em {pos} esta fora do escopo aberto em {abertura}"
                );
            }
        }
    }

    /// A autoridade nao volta a ser fato da sessao: o campo e o metodo da
    /// #1905 nao existem mais em `src/`.
    #[test]
    fn a_marca_mutavel_da_sessao_nao_volta() {
        let proibidos = [
            concat!("ultimo_turno", "_do_dono"),
            concat!("marcar_turno", "_do_dono"),
        ];
        let mut achados = Vec::new();
        fn varrer(dir: &std::path::Path, proibidos: &[&str], achados: &mut Vec<String>) {
            let Ok(entradas) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entradas.flatten() {
                let p = e.path();
                if p.is_dir() {
                    varrer(&p, proibidos, achados);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && let Ok(txt) = std::fs::read_to_string(&p)
                {
                    for termo in proibidos {
                        if txt.contains(termo) {
                            achados.push(format!("{}: {termo}", p.display()));
                        }
                    }
                }
            }
        }
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        varrer(&raiz, &proibidos, &mut achados);
        assert!(achados.is_empty(), "marca da sessao de volta: {achados:?}");
    }
}
