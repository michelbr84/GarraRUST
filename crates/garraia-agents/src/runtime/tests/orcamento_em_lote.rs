//! Lote paralelo de chamadas acima do teto do orcamento.
//!
//! O modelo pode pedir varias ferramentas numa resposta so. O orcamento so
//! era conferido no topo do laco do turno, antes de cada chamada ao modelo,
//! entao um lote de 15 rodava inteiro contra um teto de 10. Sem folga na
//! tarefa — o modo `search`, piso do WhatsApp pessoal, tem 10 chamadas por
//! tarefa e portanto 10 por volta —, o turno ainda caia em `execution budget
//! exceeded` depois de tudo executado, sem o modelo ver os resultados, e o
//! canal respondia "tente de novo em instantes". Visto num pod com a v0.4.6
//! em 2026-09-28: quatro turnos, com 11, 13 (5 + 8), 15 e 16 chamadas.

use super::super::AgentRuntime;
use super::{ToolQueConta, inicios_casados_com_fins, turno_de_streaming_com_eventos};
use crate::exec_context::ExecContext;
use crate::modes::{AgentMode, ModeProfile};
use crate::providers::{
    ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, StreamEvent,
};
use crate::turn_events::TurnEvent;
use futures::Stream;
use garraia_common::Result;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// O texto que o modelo de mentira manda quando responde.
const RESPOSTA: &str = "respondi com o que tinha";

/// O texto que acompanha o lote quando o modelo insiste escrevendo algo.
const TEXTO_JUNTO_DO_LOTE: &str = "ainda falta conferir o resto";

/// Trecho fixo da recusa de uma chamada que passaria do teto.
const NAO_EXECUTADA: &str = "NAO executada";

/// Trecho fixo da nota que vai no `system` do pedido da volta final.
const NOTA_DA_VOLTA_FINAL: &str = "orcamento de chamadas de ferramenta deste pedido acabou";

/// Trecho fixo da mensagem que fecha o turno quando o modelo, na volta
/// final, so pede ferramenta e nao escreve nada.
const MENSAGEM_FIXA: &str = "Tente pedir uma parte por vez";

/// O que o modelo faz depois de pedir o lote na primeira volta.
#[derive(Clone, Copy)]
pub(super) enum Depois {
    /// Responde em texto.
    Responde,
    /// Pede o lote de novo, sem texto nenhum.
    Insiste,
    /// Pede o lote de novo, com um texto junto.
    InsisteComTexto,
}

/// Volta 0: um lote de `tamanho` chamadas a `conta`, cada uma com argumento
/// proprio (com argumentos iguais o detector de loop entraria antes). Nas
/// voltas seguintes, faz o que `depois` manda. Guarda o `system` e os
/// `ToolResult` da ultima mensagem de cada pedido.
///
/// Com `streaming`, o lote chega pelo stream e o `complete()` falha: um
/// turno que caisse no batch quebraria o teste em vez de passar pelo ramo
/// errado em silencio. Sem `streaming` e o contrario: o `stream_complete`
/// falha como o padrao do trait, e o turno de streaming roda o ramo batch.
pub(super) struct Lote {
    tamanho: usize,
    depois: Depois,
    streaming: bool,
    voltas: AtomicUsize,
    sistemas: Mutex<Vec<String>>,
    resultados: Mutex<Vec<Vec<String>>>,
}

impl Lote {
    pub(super) fn novo(tamanho: usize, depois: Depois, streaming: bool) -> Arc<Self> {
        Arc::new(Self {
            tamanho,
            depois,
            streaming,
            voltas: AtomicUsize::new(0),
            sistemas: Mutex::new(Vec::new()),
            resultados: Mutex::new(Vec::new()),
        })
    }

    fn voltas(&self) -> usize {
        self.voltas.load(Ordering::SeqCst)
    }

    fn sistema(&self, volta: usize) -> String {
        self.sistemas.lock().expect("lock")[volta].clone()
    }

    fn resultados(&self, volta: usize) -> Vec<String> {
        self.resultados.lock().expect("lock")[volta].clone()
    }

    /// Registra o pedido e devolve o numero da volta, a partir de 0.
    fn registrar(&self, request: &LlmRequest) -> usize {
        self.sistemas
            .lock()
            .expect("lock")
            .push(request.system.clone().unwrap_or_default());
        let ultimos = match request.messages.last() {
            Some(m) if matches!(m.role, ChatRole::User) => match &m.content {
                MessagePart::Parts(blocos) => blocos
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        self.resultados.lock().expect("lock").push(ultimos);
        self.voltas.fetch_add(1, Ordering::SeqCst)
    }

    fn pede_o_lote(&self, volta: usize) -> bool {
        volta == 0 || !matches!(self.depois, Depois::Responde)
    }

    fn texto(&self, volta: usize) -> Option<&'static str> {
        match self.depois {
            Depois::Responde if volta > 0 => Some(RESPOSTA),
            Depois::InsisteComTexto if volta > 0 => Some(TEXTO_JUNTO_DO_LOTE),
            _ => None,
        }
    }

    fn chamadas(&self, volta: usize) -> Vec<(String, serde_json::Value)> {
        (0..self.tamanho)
            .map(|i| (format!("v{volta}-t{i}"), serde_json::json!({ "n": i })))
            .collect()
    }
}

#[async_trait::async_trait]
impl LlmProvider for Lote {
    fn provider_id(&self) -> &str {
        "lote"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        if self.streaming {
            return Err(garraia_common::Error::Agent(
                "o teste exige o ramo de streaming; o batch nao pode rodar".into(),
            ));
        }
        let volta = self.registrar(request);
        let mut content = Vec::new();
        if let Some(texto) = self.texto(volta) {
            content.push(ContentBlock::Text {
                text: texto.to_string(),
            });
        }
        if self.pede_o_lote(volta) {
            content.extend(self.chamadas(volta).into_iter().map(|(id, input)| {
                ContentBlock::ToolUse {
                    id,
                    name: "conta".to_string(),
                    input,
                }
            }));
        }
        Ok(LlmResponse {
            content,
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        if !self.streaming {
            return Err(garraia_common::Error::Agent(
                "provedor lote nao suporta streaming".into(),
            ));
        }
        let volta = self.registrar(request);
        let mut eventos = Vec::new();
        if let Some(texto) = self.texto(volta) {
            eventos.push(Ok(StreamEvent::TextDelta(texto.to_string())));
        }
        if self.pede_o_lote(volta) {
            for (index, (id, input)) in self.chamadas(volta).into_iter().enumerate() {
                eventos.push(Ok(StreamEvent::ToolUseStart {
                    index,
                    id,
                    name: "conta".to_string(),
                }));
                eventos.push(Ok(StreamEvent::InputJsonDelta(input.to_string())));
                eventos.push(Ok(StreamEvent::ContentBlockStop { index }));
            }
        }
        eventos.push(Ok(StreamEvent::MessageStop));
        Ok(Box::pin(futures::stream::iter(eventos)))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

fn runtime_com(provider: Arc<Lote>) -> (AgentRuntime, Arc<AtomicUsize>) {
    let rt = AgentRuntime::new();
    let vezes = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ToolQueConta {
        vezes: Arc::clone(&vezes),
    }));
    rt.register_provider(provider);
    (rt, vezes)
}

/// O perfil de verdade do `search` — 10 chamadas por tarefa e, por isso,
/// 10 por volta —, com a `conta` liberada na whitelist.
fn modo_search() -> ExecContext {
    let mut perfil = ModeProfile::from_mode(AgentMode::Search);
    perfil.tool_policy.allowed.push("conta".to_string());
    ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    }
}

async fn turno(rt: &AgentRuntime, exec: &ExecContext) -> Result<String> {
    rt.process_message_with_agent_config(
        "sessao-lote",
        "testa todas as ferramentas",
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
}

fn recusadas(resultados: &[String]) -> Vec<&String> {
    resultados
        .iter()
        .filter(|r| r.contains(NAO_EXECUTADA))
        .collect()
}

/// O caso do pod: `search` e um lote de 15. So 10 rodam; as 5 de cima voltam
/// ao modelo como nao executadas; e, como a tarefa esgotou, o modelo ganha
/// uma volta final, avisada no `system`. O turno fecha com a resposta dele,
/// e nao com `execution budget exceeded`.
#[tokio::test]
pub(super) async fn lote_acima_do_teto_roda_so_o_teto_e_o_modelo_responde_na_volta_final() {
    let provider = Lote::novo(15, Depois::Responde, false);
    let (rt, vezes) = runtime_com(provider.clone());

    let resposta = turno(&rt, &modo_search())
        .await
        .expect("orcamento esgotado nao derruba o turno");

    assert_eq!(resposta, RESPOSTA);
    assert_eq!(vezes.load(Ordering::SeqCst), 10, "nunca mais que o teto");
    assert_eq!(provider.voltas(), 2, "o lote e a volta final, nada mais");
    let resultados = provider.resultados(1);
    assert_eq!(resultados.len(), 15, "todo tool_use tem o seu resultado");
    let recusas = recusadas(&resultados);
    assert_eq!(recusas.len(), 5, "{resultados:?}");
    assert!(
        recusas.iter().all(|r| r.contains("tarefa")),
        "sem folga na tarefa, a recusa diz isso: {recusas:?}"
    );
    assert!(!provider.sistema(0).contains(NOTA_DA_VOLTA_FINAL));
    assert!(
        provider.sistema(1).contains(NOTA_DA_VOLTA_FINAL),
        "{}",
        provider.sistema(1)
    );
}

/// Com folga na tarefa (o padrao: 10 por volta, 50 por tarefa), o lote
/// tambem para no teto da volta. As 5 de cima voltam dizendo que o modelo
/// pode pedir de novo na proxima volta, e nao ha nota de volta final.
#[tokio::test]
pub(super) async fn com_folga_na_tarefa_o_lote_para_no_teto_da_volta_e_o_turno_segue() {
    let provider = Lote::novo(15, Depois::Responde, false);
    let (rt, vezes) = runtime_com(provider.clone());

    let resposta = turno(&rt, &ExecContext::default()).await.expect("turno");

    assert_eq!(resposta, RESPOSTA);
    assert_eq!(vezes.load(Ordering::SeqCst), 10, "o teto e por volta");
    let resultados = provider.resultados(1);
    let recusas = recusadas(&resultados);
    assert_eq!(recusas.len(), 5, "{resultados:?}");
    assert!(
        recusas.iter().all(|r| r.contains("proxima volta")),
        "{recusas:?}"
    );
    assert!(
        !provider.sistema(1).contains(NOTA_DA_VOLTA_FINAL),
        "com folga na tarefa nao ha volta final"
    );
}

/// Na volta final nada roda, mesmo que o modelo ignore o aviso e peca o
/// lote de novo sem escrever nada. O turno fecha com a mensagem fixa, que
/// diz o limite, e o provider e chamado so duas vezes.
#[tokio::test]
pub(super) async fn na_volta_final_nada_roda_mesmo_se_o_modelo_insistir() {
    let provider = Lote::novo(15, Depois::Insiste, false);
    let (rt, vezes) = runtime_com(provider.clone());

    let resposta = turno(&rt, &modo_search())
        .await
        .expect("orcamento esgotado nao derruba o turno");

    assert!(resposta.contains(MENSAGEM_FIXA), "{resposta}");
    assert!(
        resposta.contains("10"),
        "a mensagem diz o limite: {resposta}"
    );
    assert_eq!(vezes.load(Ordering::SeqCst), 10);
    assert_eq!(provider.voltas(), 2, "nao existe terceira volta");
}

/// Se o modelo escreve algo junto do lote insistente, o turno fecha com o
/// texto dele — e o lote, de novo, nao roda.
#[tokio::test]
pub(super) async fn na_volta_final_o_texto_do_modelo_fecha_o_turno() {
    let provider = Lote::novo(15, Depois::InsisteComTexto, false);
    let (rt, vezes) = runtime_com(provider.clone());

    let resposta = turno(&rt, &modo_search())
        .await
        .expect("orcamento esgotado nao derruba o turno");

    assert_eq!(resposta, TEXTO_JUNTO_DO_LOTE);
    assert_eq!(vezes.load(Ordering::SeqCst), 10);
    assert_eq!(provider.voltas(), 2);
}

/// O mesmo pelo ramo de stream do turno de streaming: 10 rodam, todo inicio
/// tem o seu fim (as 5 recusadas fecham com `success: false`) e o turno
/// termina com a resposta da volta final.
#[tokio::test]
pub(super) async fn no_stream_o_lote_para_no_teto_e_todo_inicio_tem_fim() {
    let provider = Lote::novo(15, Depois::Responde, true);
    let (rt, vezes) = runtime_com(provider.clone());

    let (resultado, eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-lote-stream", &modo_search()).await;
    let resposta = resultado.expect("orcamento esgotado nao derruba o turno");

    assert!(resposta.contains(RESPOSTA), "{resposta}");
    assert_eq!(vezes.load(Ordering::SeqCst), 10);
    assert_eq!(inicios_casados_com_fins(&eventos).len(), 15, "{eventos:?}");
    let falhas = eventos
        .iter()
        .filter(|e| matches!(e, TurnEvent::ToolFinished { success: false, .. }))
        .count();
    assert_eq!(falhas, 5, "{eventos:?}");
    // #1915: as 5 recusadas dizem POR QUE — orcamento esgotado, nao erro da
    // ferramenta (que nem rodou) nem recusa de politica.
    let desfechos: Vec<crate::turn_events::DesfechoDaChamada> = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .collect();
    use crate::turn_events::DesfechoDaChamada as D;
    assert_eq!(
        desfechos
            .iter()
            .filter(|d| **d == D::OrcamentoEsgotado)
            .count(),
        5,
        "{eventos:?}"
    );
    assert_eq!(
        desfechos.iter().filter(|d| **d == D::Sucesso).count(),
        10,
        "as 10 que couberam no teto rodaram: {eventos:?}"
    );
    assert!(provider.sistema(1).contains(NOTA_DA_VOLTA_FINAL));
}

/// Pelo ramo de stream, com o modelo insistindo na volta final: nada roda,
/// e a mensagem fixa chega ao canal como texto. O turno nao vira "vazio"
/// (#1048) so porque o modelo nao escreveu — nao ha redo em batch.
#[tokio::test]
pub(super) async fn no_stream_a_volta_final_insistente_fecha_com_a_mensagem_fixa() {
    let provider = Lote::novo(15, Depois::Insiste, true);
    let (rt, vezes) = runtime_com(provider.clone());

    let (resultado, eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-lote-stream-insiste", &modo_search()).await;
    let resposta = resultado.expect("orcamento esgotado nao derruba o turno");

    assert!(resposta.contains(MENSAGEM_FIXA), "{resposta}");
    assert_eq!(vezes.load(Ordering::SeqCst), 10);
    assert_eq!(provider.voltas(), 2, "sem redo e sem terceira volta");
    let texto: String = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texto.contains(MENSAGEM_FIXA),
        "a mensagem vai ao canal: {texto:?}"
    );
}

/// Pelo ramo batch do turno de streaming (provider sem stream), com o
/// modelo insistindo: nada roda, e o turno fecha com a mensagem fixa em vez
/// do erro de turno vazio do #1048.
#[tokio::test]
pub(super) async fn no_batch_do_streaming_a_volta_final_insistente_fecha_com_a_mensagem_fixa() {
    let provider = Lote::novo(15, Depois::Insiste, false);
    let (rt, vezes) = runtime_com(provider.clone());

    let (resultado, _eventos) =
        turno_de_streaming_com_eventos(&rt, "sessao-lote-batch-insiste", &modo_search()).await;
    let resposta = resultado.expect("orcamento esgotado nao derruba o turno");

    assert!(resposta.contains(MENSAGEM_FIXA), "{resposta}");
    assert_eq!(vezes.load(Ordering::SeqCst), 10);
    assert_eq!(provider.voltas(), 2);
}
