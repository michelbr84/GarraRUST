use super::super::AgentRuntime;
use super::ToolQuePedeConfirmacao;
use crate::exec_context::ExecContext;
use crate::providers::{ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart};
use garraia_common::Result;
use std::sync::{Arc, Mutex};

use super::aprovacao_entre_turnos::*;

/// Pede `precisa_confirmar {alvo: z}` enquanto houver menos de dois
/// resultados de tool depois da ultima mensagem humana.
pub(super) struct PedeDuasVezes;

#[async_trait::async_trait]
impl LlmProvider for PedeDuasVezes {
    fn provider_id(&self) -> &str {
        "pede_duas_vezes"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let ultima_humana = request
            .messages
            .iter()
            .rposition(|m| {
                matches!(m.role, ChatRole::User) && matches!(m.content, MessagePart::Text(_))
            })
            .unwrap_or(0);
        let resultados = request.messages[ultima_humana..]
            .iter()
            .filter(|m| {
                matches!(&m.content, MessagePart::Parts(p)
                    if p.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })))
            })
            .count();
        let content = if resultados < 2 {
            vec![ContentBlock::ToolUse {
                id: format!("t-{resultados}"),
                name: "precisa_confirmar".to_string(),
                input: serde_json::json!({ "alvo": "z" }),
            }]
        } else {
            vec![ContentBlock::Text {
                text: "concluido".to_string(),
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

/// Revisao da onda A: a aprovacao retomada vale para UM turno (o
/// registro e consumido na leitura), nao para uma execucao. Dentro do
/// turno aprovado, duas chamadas identicas ao pedido aprovado rodam
/// as duas — o mesmo contrato do caminho pelo historico. Este teste
/// prende o que o fragmento do changelog promete; se a aprovacao
/// virar uso unico, o teste e o fragmento mudam juntos.
#[tokio::test]
pub(super) async fn aprovacao_retomada_vale_para_o_turno_e_nao_para_uma_execucao() {
    let rt = AgentRuntime::new();
    let (tool, rodou) = ToolQuePedeConfirmacao::nova();
    rt.register_tool(Box::new(tool));
    rt.register_provider(Arc::new(PedeDuasVezes));
    let exec = ExecContext {
        approval_scope: escopo("web", "s1", "a"),
        ..ExecContext::default()
    };
    let turno = |texto: &'static str| {
        let rt = &rt;
        let exec = &exec;
        async move {
            rt.process_message_with_agent_config(
                "s1",
                texto,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno")
        }
    };
    assert!(e_pedido(&turno("apaga z").await));
    assert!(rodou.lock().expect("lock").is_empty());
    let r = turno("sim").await;
    assert_eq!(r, "concluido");
    assert_eq!(
        *rodou.lock().expect("lock"),
        vec!["z".to_string(), "z".to_string()],
        "as duas chamadas do turno aprovado rodam"
    );
    // O registro foi consumido: outro "sim" nao aprova nada.
    assert!(rt.pending_approvals.is_empty());
}

/// Pede `bash {command: printenv PATH}` enquanto nao houver resultado
/// de tool depois da ultima mensagem humana, e guarda o conteudo de
/// todo resultado que viu.
pub(super) struct PedeBash {
    pub(super) vistos: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl LlmProvider for PedeBash {
    fn provider_id(&self) -> &str {
        "pede_bash"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let ultima_humana = request
            .messages
            .iter()
            .rposition(|m| {
                matches!(m.role, ChatRole::User) && matches!(m.content, MessagePart::Text(_))
            })
            .unwrap_or(0);
        let mut depois = Vec::new();
        for m in &request.messages[ultima_humana..] {
            if let MessagePart::Parts(p) = &m.content {
                for b in p {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        depois.push(content.clone());
                    }
                }
            }
        }
        let content = if depois.is_empty() {
            vec![ContentBlock::ToolUse {
                id: "t-bash".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({ "command": "printenv PATH" }),
            }]
        } else {
            self.vistos.lock().expect("lock").extend(depois);
            vec![ContentBlock::Text {
                text: "concluido".to_string(),
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

/// **W3 da v0.4.5 com a ferramenta de verdade.** O usuario do
/// WhatsApp via `[CONFIRM_REQUIRED:6b2e7f7e9f135cbc] O comando a
/// seguir requer confirmacao...`: o marcador interno subia como
/// resposta do turno. Agora o texto que sai diz o comando e como
/// aprovar, sem o marcador — e o "sim" seguinte continua retomando,
/// porque a aprovacao nunca dependeu do marcador no texto.
#[tokio::test]
pub(super) async fn pedido_do_bash_chega_sem_marcador_e_o_sim_ainda_retoma() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(crate::tools::BashTool::new_with_confirmation(
        None,
    )));
    let provider = Arc::new(PedeBash {
        vistos: Mutex::new(Vec::new()),
    });
    rt.register_provider(Arc::clone(&provider) as Arc<dyn LlmProvider>);
    let exec = ExecContext {
        approval_scope: escopo("whatsapp_linked", "s1", "5511888880000"),
        ..ExecContext::default()
    };
    let turno = |texto: &'static str| {
        let rt = &rt;
        let exec = &exec;
        async move {
            rt.process_message_with_agent_config(
                "s1",
                texto,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno")
        }
    };

    let pedido = turno("mostra o PATH").await;
    assert!(
        !pedido.contains(crate::tools::approval::MARKER_PREFIX),
        "o marcador chegou ao humano: {pedido}"
    );
    assert!(
        pedido.starts_with("O comando a seguir requer confirma"),
        "o texto comeca pela frase, sem o marcador na frente: {pedido}"
    );
    assert!(pedido.contains("printenv PATH"), "{pedido}");
    assert!(
        pedido.contains("Responda **sim**"),
        "o texto diz como aprovar: {pedido}"
    );
    assert!(!rt.pending_approvals.is_empty(), "a pausa foi registrada");

    // Digitar o marcador nunca aprovou: ele nem chega ao humano, e o
    // que aprova e a palavra.
    let resposta = turno("sim").await;
    assert_eq!(resposta, "concluido");
    let vistos = provider.vistos.lock().expect("lock").clone();
    let caminho = std::env::var("PATH").unwrap_or_default();
    assert!(
        vistos
            .iter()
            .any(|r| !caminho.is_empty() && r.contains(caminho.as_str())),
        "o comando aprovado rodou e o modelo viu a saida: {vistos:?}"
    );
    assert!(rt.pending_approvals.is_empty(), "o pedido foi consumido");
}

/// **W3 no sink de eventos.** O mesmo despacho que monta a resposta
/// pausada manda, ANTES dela, o `tool_finished` da ferramenta aos
/// sinks `Events` — a linha que a `garraia chat` desenha e o resumo
/// que o `/ws` envia. O resumo e a saida capturada eram tirados do
/// conteudo cru, e o humano via `[CONFIRM_REQUIRED:<hex>] O comando a
/// seguir...` ali, mesmo com a resposta ja limpa. O registro da
/// pausa continua saindo do conteudo cru.
#[tokio::test]
pub(super) async fn a_linha_da_ferramenta_no_streaming_tambem_sai_sem_marcador() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(crate::tools::BashTool::new_with_confirmation(
        None,
    )));
    let provider = Arc::new(PedeBash {
        vistos: Mutex::new(Vec::new()),
    });
    rt.register_provider(Arc::clone(&provider) as Arc<dyn LlmProvider>);
    let exec = ExecContext {
        approval_scope: escopo("web", "s-w3", "u-1"),
        ..ExecContext::default()
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::turn_events::TurnEvent>(64);
    let coleta = tokio::spawn(async move {
        let mut eventos = Vec::new();
        while let Some(e) = rx.recv().await {
            eventos.push(e);
        }
        eventos
    });
    let resposta = rt
        .process_message_streaming_with_events(
            "s-w3",
            "mostra o PATH",
            &[],
            tx,
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect("turno");
    let eventos = coleta.await.expect("coleta");

    let fins: Vec<(&str, &str)> = eventos
        .iter()
        .filter_map(|e| match e {
            crate::turn_events::TurnEvent::ToolFinished {
                name,
                summary,
                output,
                ..
            } if name == "bash" => Some((summary.as_str(), output.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(fins.len(), 1, "um fim para a chamada do bash: {eventos:?}");
    let (resumo, saida) = fins[0];
    for (onde, texto) in [("summary", resumo), ("output", saida)] {
        assert!(
            !texto.contains(crate::tools::approval::MARKER_PREFIX),
            "o marcador chegou ao {onde} do tool_finished: {texto}"
        );
        assert!(
            texto.starts_with("O comando a seguir requer confirma"),
            "o {onde} e o pedido, sem o marcador na frente: {texto}"
        );
    }
    assert!(saida.contains("printenv PATH"), "{saida}");
    assert!(
        !resposta.contains(crate::tools::approval::MARKER_PREFIX),
        "{resposta}"
    );
    assert!(
        !rt.pending_approvals.is_empty(),
        "a pausa foi registrada com a impressao digital do conteudo cru"
    );
}
