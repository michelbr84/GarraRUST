use super::super::{AgentRuntime, TOOL_PROGRAM_NAME};
use super::{
    EcoInteiroTool, ToolQueMarca, inicios_casados_com_fins, turno_de_streaming_com_eventos,
};
use crate::exec_context::ExecContext;
use crate::providers::{
    ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, StreamEvent,
};
use crate::turn_events::TurnEvent;
use futures::Stream;
use garraia_common::Result;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Volta 0: o `tool_program` pelo stream. Volta 1: texto. O
/// `complete()` FALHA — se o turno caisse no batch, o teste
/// quebraria em vez de passar pelo ramo errado em silencio.
pub(super) struct ProgramaEmStreaming {
    pub(super) programa: serde_json::Value,
    pub(super) voltas: AtomicUsize,
    pub(super) resultados: std::sync::Mutex<Vec<String>>,
}

impl ProgramaEmStreaming {
    pub(super) fn novo(programa: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            programa,
            voltas: AtomicUsize::new(0),
            resultados: std::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl LlmProvider for ProgramaEmStreaming {
    fn provider_id(&self) -> &str {
        "programa_em_streaming"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Err(garraia_common::Error::Agent(
            "o teste exige o ramo de streaming; o batch nao pode rodar".into(),
        ))
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        for m in &request.messages {
            if let (ChatRole::User, MessagePart::Parts(blocos)) = (&m.role, &m.content) {
                for b in blocos {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        self.resultados.lock().expect("lock").push(content.clone());
                    }
                }
            }
        }
        let volta = self.voltas.fetch_add(1, Ordering::SeqCst);
        let eventos = if volta == 0 {
            let json = self.programa.to_string();
            let meio = json.len() / 2;
            vec![
                Ok(StreamEvent::ToolUseStart {
                    index: 0,
                    id: "programa-stream".to_string(),
                    name: TOOL_PROGRAM_NAME.to_string(),
                }),
                Ok(StreamEvent::InputJsonDelta(json[..meio].to_string())),
                Ok(StreamEvent::InputJsonDelta(json[meio..].to_string())),
                Ok(StreamEvent::ContentBlockStop { index: 0 }),
                Ok(StreamEvent::MessageStop),
            ]
        } else {
            vec![
                Ok(StreamEvent::TextDelta("concluido".to_string())),
                Ok(StreamEvent::MessageStop),
            ]
        };
        Ok(Box::pin(futures::stream::iter(eventos)))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Posicao de cada evento de tool, para afirmar "dentro do par".
pub(super) fn posicoes(eventos: &[TurnEvent], alvo: &str) -> (Vec<usize>, Vec<usize>) {
    let mut inicios = Vec::new();
    let mut fins = Vec::new();
    for (i, e) in eventos.iter().enumerate() {
        match e {
            TurnEvent::ToolStarted { name, .. } if name == alvo => inicios.push(i),
            TurnEvent::ToolFinished { name, .. } if name == alvo => fins.push(i),
            _ => {}
        }
    }
    (inicios, fins)
}

#[tokio::test]
pub(super) async fn tool_program_no_streaming_real_emite_eventos_por_passo_em_ordem() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let provider = ProgramaEmStreaming::novo(serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": 7 }, "as": "sete" },
            { "tool": "eco_inteiro", "args": { "n": "$sete" } }
        ]
    }));
    rt.register_provider(provider.clone());

    let (resultado, eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-sd-stream", &ExecContext::default()).await;
    let resposta = resultado.expect("turno");
    assert!(resposta.contains("concluido"), "{resposta}");
    assert_eq!(provider.voltas.load(Ordering::SeqCst), 2);

    assert_eq!(
        inicios_casados_com_fins(&eventos),
        [TOOL_PROGRAM_NAME, "eco_inteiro", "eco_inteiro"],
        "{eventos:?}"
    );
    let (ini_prog, fim_prog) = posicoes(&eventos, TOOL_PROGRAM_NAME);
    let (ini_passos, fim_passos) = posicoes(&eventos, "eco_inteiro");
    assert_eq!((ini_prog.len(), fim_prog.len()), (1, 1), "{eventos:?}");
    assert_eq!((ini_passos.len(), fim_passos.len()), (2, 2), "{eventos:?}");
    for p in ini_passos.iter().chain(&fim_passos) {
        assert!(
            ini_prog[0] < *p && *p < fim_prog[0],
            "evento de passo fora do par do programa: {eventos:?}"
        );
    }
    // Passo 0 inteiro antes do passo 1: nada intercalado.
    assert!(fim_passos[0] < ini_passos[1], "{eventos:?}");
    let sucessos: Vec<bool> = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished { success, .. } => Some(*success),
            _ => None,
        })
        .collect();
    assert_eq!(sucessos, [true, true, true], "{eventos:?}");

    // O `$sete` chegou substituido: os dois passos devolveram 7.
    let resultados = provider.resultados.lock().expect("lock").clone();
    let relatorio: serde_json::Value =
        serde_json::from_str(resultados.last().expect("resultado do programa"))
            .expect("relatorio e JSON");
    assert_eq!(relatorio["steps"][0]["output"], "7", "{relatorio}");
    assert_eq!(relatorio["steps"][1]["output"], "7", "{relatorio}");
}

/// A mesma coisa com um passo negado pelo gate: a contraparte, no
/// streaming de verdade, do T6.
#[tokio::test]
pub(super) async fn tool_program_no_streaming_real_com_passo_negado_casa_os_eventos() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let executou_negada = Arc::new(AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "negada",
        executou: Arc::clone(&executou_negada),
    }));
    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "so-eco",
        None,
        &serde_json::json!({ "allow": ["tool_program", "eco_inteiro"] }),
        &serde_json::json!({}),
    );
    let exec = ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    };
    let provider = ProgramaEmStreaming::novo(serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": 1 } },
            { "tool": "negada", "args": {} },
            { "tool": "eco_inteiro", "args": { "n": 2 } }
        ]
    }));
    rt.register_provider(provider.clone());

    let (resultado, eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-sd-stream-negada", &exec).await;
    let resposta = resultado.expect("turno");
    assert!(resposta.contains("concluido"), "{resposta}");
    assert!(!executou_negada.load(Ordering::SeqCst));

    assert_eq!(
        inicios_casados_com_fins(&eventos),
        [TOOL_PROGRAM_NAME, "eco_inteiro", "negada"],
        "{eventos:?}"
    );
    let (ini_prog, fim_prog) = posicoes(&eventos, TOOL_PROGRAM_NAME);
    let (ini_neg, fim_neg) = posicoes(&eventos, "negada");
    assert!(
        ini_prog[0] < ini_neg[0] && fim_neg[0] < fim_prog[0],
        "{eventos:?}"
    );
    let fim_da_negada = eventos.iter().find_map(|e| match e {
        TurnEvent::ToolFinished { name, success, .. } if name == "negada" => Some(*success),
        _ => None,
    });
    assert_eq!(fim_da_negada, Some(false), "{eventos:?}");
}
