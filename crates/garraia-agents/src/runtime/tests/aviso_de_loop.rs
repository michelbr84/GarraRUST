use super::super::AgentRuntime;
use super::{ToolQueConta, inicios_casados_com_fins, turno_de_streaming_com_eventos};
use crate::exec_context::ExecContext;
use crate::providers::{ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart};
use crate::turn_events::TurnEvent;
use garraia_common::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Pede `conta {x:1}` toda volta. Com `muda_apos_aviso`, ao ler o
/// aviso do detector no ultimo resultado, responde em texto — o
/// modelo que aprendeu com a observacao.
pub(super) struct RepeteConta {
    pub(super) voltas: AtomicUsize,
    pub(super) muda_apos_aviso: bool,
    pub(super) viu_aviso: std::sync::atomic::AtomicBool,
}

impl RepeteConta {
    pub(super) fn novo(muda_apos_aviso: bool) -> Arc<Self> {
        Arc::new(Self {
            voltas: AtomicUsize::new(0),
            muda_apos_aviso,
            viu_aviso: std::sync::atomic::AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl LlmProvider for RepeteConta {
    fn provider_id(&self) -> &str {
        "repete_conta"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.voltas.fetch_add(1, Ordering::SeqCst);
        let aviso = request.messages.last().is_some_and(|m| {
            matches!(m.role, ChatRole::User)
                && matches!(&m.content, MessagePart::Parts(p) if p.iter().any(|b|
                    matches!(b, ContentBlock::ToolResult { content, .. }
                        if content.contains("NAO foi executada"))))
        });
        if aviso {
            self.viu_aviso.store(true, Ordering::SeqCst);
        }
        let content = if aviso && self.muda_apos_aviso {
            vec![ContentBlock::Text {
                text: "mudei de abordagem".to_string(),
            }]
        } else {
            vec![ContentBlock::ToolUse {
                id: "t-conta".to_string(),
                name: "conta".to_string(),
                input: serde_json::json!({ "x": 1 }),
            }]
        };
        Ok(LlmResponse {
            content,
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

pub(super) fn runtime_com_conta(provider: Arc<RepeteConta>) -> (AgentRuntime, Arc<AtomicUsize>) {
    let rt = AgentRuntime::new();
    let vezes = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ToolQueConta {
        vezes: Arc::clone(&vezes),
    }));
    rt.register_provider(provider);
    (rt, vezes)
}

pub(super) async fn turno(rt: &AgentRuntime) -> Result<String> {
    rt.process_message_with_agent_config(
        "sessao-1295",
        "conta",
        &[],
        None,
        None,
        None,
        None,
        None,
        None,
        &ExecContext::default(),
    )
    .await
}

/// Item 1: a terceira chamada identica nao roda e vira aviso; a
/// quarta aborta com a mensagem do #1318. Execucoes reais: 2, como
/// antes do aviso existir.
#[tokio::test]
pub(super) async fn terceira_chamada_avisa_quarta_aborta_e_so_duas_rodam() {
    let provider = RepeteConta::novo(false);
    let (rt, vezes) = runtime_com_conta(provider.clone());

    let erro = turno(&rt).await.expect_err("repetir apos o aviso aborta");
    let msg = erro.to_string();
    assert!(msg.contains("tool loop detected: conta"), "{msg}");
    assert!(msg.contains("3 chamadas identicas"), "{msg}");
    assert!(
        provider.viu_aviso.load(Ordering::SeqCst),
        "o modelo recebeu a observacao corretiva antes do corte"
    );
    assert_eq!(provider.voltas.load(Ordering::SeqCst), 4);
    assert_eq!(vezes.load(Ordering::SeqCst), 2, "nunca mais de 2 execucoes");
}

/// Revisao da onda A: `conta{x:1}` tres vezes (a terceira vira
/// aviso), `conta{x:2}` e `conta{x:1}` de novo. A chamada diferente
/// no meio nao libera a avisada: o turno aborta na volta 5, e
/// `conta{x:1}` roda exatamente 2 vezes (mais 1 do `x:2`).
pub(super) struct RoteiroComOutraNoMeio {
    pub(super) voltas: AtomicUsize,
}

#[async_trait::async_trait]
impl LlmProvider for RoteiroComOutraNoMeio {
    fn provider_id(&self) -> &str {
        "roteiro_outra_no_meio"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        let volta = self.voltas.fetch_add(1, Ordering::SeqCst) + 1;
        let content = match volta {
            1..=3 | 5 => vec![ContentBlock::ToolUse {
                id: format!("t-{volta}"),
                name: "conta".to_string(),
                input: serde_json::json!({ "x": 1 }),
            }],
            4 => vec![ContentBlock::ToolUse {
                id: "t-4".to_string(),
                name: "conta".to_string(),
                input: serde_json::json!({ "x": 2 }),
            }],
            _ => vec![ContentBlock::Text {
                text: "fim".to_string(),
            }],
        };
        Ok(LlmResponse {
            content,
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

#[tokio::test]
pub(super) async fn chamada_avisada_aborta_mesmo_com_outra_chamada_no_meio() {
    let rt = AgentRuntime::new();
    let vezes = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ToolQueConta {
        vezes: Arc::clone(&vezes),
    }));
    let provider = Arc::new(RoteiroComOutraNoMeio {
        voltas: AtomicUsize::new(0),
    });
    rt.register_provider(provider.clone());

    let erro = turno(&rt)
        .await
        .expect_err("a avisada volta depois de outra chamada e aborta");
    let msg = erro.to_string();
    assert!(msg.contains("tool loop detected: conta"), "{msg}");
    assert!(msg.contains("depois do aviso"), "{msg}");
    assert_eq!(provider.voltas.load(Ordering::SeqCst), 5);
    assert_eq!(
        vezes.load(Ordering::SeqCst),
        3,
        "x:1 roda 2 vezes e x:2 uma; a avisada nunca volta a rodar"
    );
}

/// O modelo que muda de abordagem depois do aviso termina o turno.
#[tokio::test]
pub(super) async fn modelo_que_muda_de_abordagem_depois_do_aviso_termina_ok() {
    let provider = RepeteConta::novo(true);
    let (rt, vezes) = runtime_com_conta(provider.clone());

    let resposta = turno(&rt).await.expect("o aviso nao aborta");
    assert_eq!(resposta, "mudei de abordagem");
    assert_eq!(vezes.load(Ordering::SeqCst), 2);
    assert_eq!(provider.voltas.load(Ordering::SeqCst), 4);
}

/// Item 3: no streaming, a chamada barrada aparece no `/tool` — um
/// par `tool_started`/`tool_finished(success=false)` com o veredito,
/// tanto no aviso quanto no aborto. Usa o `EmLoop` (`file_read` com
/// o mesmo `path` toda volta, via `stream_complete` de verdade).
#[tokio::test]
pub(super) async fn chamada_barrada_por_loop_aparece_nos_eventos_de_tool() {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(super::EmLoop));
    let (resultado, eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-1295-eventos", &ExecContext::default()).await;
    let erro = resultado.expect_err("aborta na quarta");
    assert!(erro.to_string().contains("tool loop detected"), "{erro}");

    let iniciados = inicios_casados_com_fins(&eventos);
    assert_eq!(iniciados.len(), 4, "{eventos:?}");

    let fins: Vec<(&bool, &String, &String)> = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished {
                success,
                summary,
                output,
                ..
            } => Some((success, summary, output)),
            _ => None,
        })
        .collect();
    let (ok, resumo, saida) = fins[2];
    assert!(!ok);
    assert_eq!(resumo, "bloqueada: loop detectado (aviso ao modelo)");
    assert!(
        saida.contains("input repetido: /tmp/alvo-repetido"),
        "{saida}"
    );
    let (ok, resumo, saida) = fins[3];
    assert!(!ok);
    assert_eq!(resumo, "bloqueada: loop detectado (turno abortado)");
    assert!(
        saida.starts_with("tool loop detected: file_read"),
        "{saida}"
    );
}
