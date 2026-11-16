//! #1912: turnos realmente sobrepostos no mesmo runtime, sem canais reais.
//! A barreira no provider só libera a resposta quando ambos os turnos já
//! resolveram a pendência. Não prova isolamento do transporte Telegram/MCP.

use super::aprovacao_entre_turnos::{CAMINHOS, Caminho, PedeAlvos, e_pedido, escopo};
use super::{AgentRuntime, ToolQuePedeConfirmacao};
use crate::exec_context::ExecContext;
use crate::providers::{
    ChatMessage, ChatRole, LlmProvider, LlmRequest, LlmResponse, MessagePart, StreamEvent,
};
use futures::Stream;
use garraia_common::Result;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Barrier;

struct ProviderComBarreira {
    inner: Arc<PedeAlvos>,
    ativa: AtomicBool,
    chegaram: AtomicUsize,
    barreira: Barrier,
}

impl ProviderComBarreira {
    async fn esperar(&self, request: &LlmRequest) {
        if self.ativa.load(Ordering::SeqCst)
            && matches!(
                request.messages.last(),
                Some(ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text(_),
                })
            )
        {
            self.chegaram.fetch_add(1, Ordering::SeqCst);
            self.barreira.wait().await;
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for ProviderComBarreira {
    fn provider_id(&self) -> &str {
        "aprovacao_concorrente"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.esperar(request).await;
        self.inner.complete(request).await
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        // No fallback, só complete espera; aguardar duas vezes esconderia
        // um deadlock ou imporia uma segunda rodada artificial da barreira.
        if self.inner.streaming {
            self.esperar(request).await;
        }
        self.inner.stream_complete(request).await
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

async fn turno(rt: &AgentRuntime, caminho: Caminho, sessao: &str, texto: &str) -> String {
    let exec = ExecContext {
        approval_scope: escopo("telegram", sessao, "dono"),
        ..ExecContext::default()
    };
    match caminho {
        Caminho::AgentConfig => rt
            .process_message_with_agent_config(
                sessao,
                texto,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                &exec,
            )
            .await
            .expect("turno batch"),
        Caminho::Streaming | Caminho::StreamingSemSuporte => {
            let (tx, mut rx) = tokio::sync::mpsc::channel(64);
            let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });
            let resposta = rt
                .process_message_streaming_with_events(
                    sessao,
                    texto,
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
                .expect("turno streaming/fallback");
            dreno.await.expect("dreno");
            resposta
        }
    }
}

async fn verificar_concorrencia(mesmo_escopo: bool) {
    for caminho in CAMINHOS {
        let rt = AgentRuntime::new();
        let (tool, rodou) = ToolQuePedeConfirmacao::nova();
        rt.register_tool(Box::new(tool));
        let provider = Arc::new(ProviderComBarreira {
            inner: PedeAlvos::novo(&["x", "x", "x", "x"], matches!(caminho, Caminho::Streaming)),
            ativa: AtomicBool::new(false),
            chegaram: AtomicUsize::new(0),
            barreira: Barrier::new(2),
        });
        rt.register_provider(provider.clone());
        let sessao_b = if mesmo_escopo { "chat-a" } else { "chat-b" };

        assert!(e_pedido(&turno(&rt, caminho, "chat-a", "apaga x").await));
        if !mesmo_escopo {
            assert!(e_pedido(&turno(&rt, caminho, sessao_b, "apaga x").await));
        }
        assert!(rodou.lock().expect("lock").is_empty());
        assert_eq!(rt.pending_approvals.len(), if mesmo_escopo { 1 } else { 2 });
        provider.ativa.store(true, Ordering::SeqCst);

        // Sem sleeps: nenhum turno recebe ToolUse antes de o outro chegar
        // ao provider. A resolução da aprovação precede a chamada ao modelo.
        let (a, b) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(
                turno(&rt, caminho, "chat-a", "sim"),
                turno(&rt, caminho, sessao_b, "sim"),
            )
        })
        .await
        .expect("os dois turnos devem atravessar a barreira sem deadlock");
        assert_eq!(provider.chegaram.load(Ordering::SeqCst), 2, "{caminho:?}");
        let pausados = usize::from(e_pedido(&a)) + usize::from(e_pedido(&b));
        if mesmo_escopo {
            assert_eq!(pausados, 1, "só um sim consome o pedido: {caminho:?}");
            assert_eq!(*rodou.lock().expect("lock"), ["x"], "{caminho:?}");
            assert_eq!(rt.pending_approvals.len(), 1, "perdedor pede de novo");
        } else {
            assert_eq!(pausados, 0, "escopos distintos não competem: {caminho:?}");
            assert_eq!(*rodou.lock().expect("lock"), ["x", "x"], "{caminho:?}");
            assert!(rt.pending_approvals.is_empty(), "{caminho:?}");
        }
    }
}

#[tokio::test]
async fn dois_sims_concorrentes_consumem_o_mesmo_pedido_uma_vez() {
    verificar_concorrencia(true).await;
}

#[tokio::test]
async fn aprovacoes_concorrentes_de_chats_distintos_nao_se_consumem() {
    verificar_concorrencia(false).await;
}
