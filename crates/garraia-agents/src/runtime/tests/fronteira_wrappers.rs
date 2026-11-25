//! Fronteira de autorizacao dos wrappers SEM contexto (follow-up do #1666).
//!
//! O defeito: os wrappers de conveniencia do runtime (`process_message`,
//! `process_message_with_context`, `process_heartbeat`) constroem o contexto
//! internamente — antes `ExecContext::default()`, sem teto e com portao
//! aberto — e qualquer caminho de turno que passasse por eles (o bot do
//! workspace, as tarefas agendadas) rodava ferramentas destrutivas sem prova
//! de operador. Aqui se prova, pelo caminho de PRODUCAO do runtime (provider
//! roteirizado + ferramenta que conta execucoes):
//!
//! - o wrapper sem contexto nega a ferramenta destrutiva (o comportamento
//!   corrigido; no codigo anterior a contagem era 1 e o teste reprova);
//! - o agendamento (heartbeat) sem contexto tambem nao eleva privilegio;
//! - trocar de modo nao amplia o que o teto da superficie fecha;
//! - a sobrescrita do teto do principal (o defeito do WhatsApp vinculado)
//!   nao remove o teto da superficie, que e camada propria;
//! - o fluxo legitimo do operador, com contexto EXPLICITO, continua rodando.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

/// Pede `alvo` uma unica volta e encerra com texto.
pub(super) struct PedeUmaVez {
    pub(super) alvo: &'static str,
}

#[async_trait::async_trait]
impl LlmProvider for PedeUmaVez {
    fn provider_id(&self) -> &str {
        "pede_uma_vez"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let ja_houve_volta = request.messages.last().is_some_and(|m| match &m.content {
            MessagePart::Parts(blocos) => blocos
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. })),
            MessagePart::Text(_) => false,
        });
        let content = if ja_houve_volta {
            vec![ContentBlock::Text {
                text: "encerrado".to_string(),
            }]
        } else {
            vec![ContentBlock::ToolUse {
                id: "chamada-1".to_string(),
                name: self.alvo.to_string(),
                input: serde_json::json!({}),
            }]
        };
        Ok(LlmResponse {
            content,
            model: "modelo-de-teste".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Ferramenta destrutiva de mentira (`bash`, `ProcessExecute` em
/// `capacidades_nativas`) que conta execucoes.
struct ContaExecucoes {
    executou: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for ContaExecucoes {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "stub destrutivo que conta execucoes"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.fetch_add(1, SeqCst);
        Ok(ToolOutput::success("rodou"))
    }
}

pub(super) fn runtime_com_tool_destrutiva() -> (AgentRuntime, Arc<AtomicUsize>) {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(PedeUmaVez { alvo: "bash" }));
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ContaExecucoes {
        executou: Arc::clone(&executou),
    }));
    (rt, executou)
}

/// Comportamento CORRIGIDO: `process_message` (wrapper sem contexto) nega a
/// ferramenta destrutiva. No codigo anterior da correcao este teste reprova —
/// o wrapper construia `ExecContext::default()`, o portao ficava aberto e a
/// ferramenta rodava (contagem 1).
#[tokio::test]
async fn wrapper_sem_contexto_barra_ferramenta_destrutiva() {
    let (rt, executou) = runtime_com_tool_destrutiva();

    let resposta = rt
        .process_message("sessao-fronteira-1", "executa", &[])
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        0,
        "sem prova de operador, a ferramenta destrutiva nao pode rodar"
    );
    assert!(
        !resposta.contains("rodou"),
        "a resposta nao pode conter saida da ferramenta: {resposta}"
    );
}

/// Agendamento nao eleva privilegio: o heartbeat sem contexto tambem roda sob
/// o teto de leitura.
#[tokio::test]
async fn heartbeat_agendado_sem_contexto_nao_eleva_privilegio() {
    let (rt, executou) = runtime_com_tool_destrutiva();

    let resposta = rt
        .process_heartbeat("sessao-fronteira-2", "executa", &[], None, None)
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        0,
        "tarefa agendada sem contexto provado nao alcanca ferramenta destrutiva"
    );
    assert!(!resposta.contains("rodou"), "sem saida da ferramenta");
}

/// Trocar de modo nao amplia: `/mode code` sobre um contexto sem prova de
/// operador continua barrado.
#[tokio::test]
async fn modo_escolhido_nao_amplia_o_teto_da_superficie() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let mut exec = ExecContext::sem_prova_de_operador();
    exec.agent_mode = Some("code".to_string());

    rt.process_message_with_exec("sessao-fronteira-3", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        0,
        "modo escolhido nao pode ampliar o teto da superficie"
    );
}

/// O defeito do WhatsApp vinculado: `exec.teto = teto_do_principal(Dono)` (= `None`)
/// removia o unico teto do turno. Agora o teto da superficie e camada propria
/// que a atribuicao nao toca.
#[tokio::test]
async fn sobrescrita_do_teto_do_principal_nao_remove_o_da_superficie() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let mut exec = ExecContext::sem_prova_de_operador();
    exec.teto = None; // Dono: sem teto de PRINCIPAL

    rt.process_message_with_exec("sessao-fronteira-4", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        0,
        "a camada da superficie sobrevive a sobrescrita do teto do principal"
    );
}

/// Fluxo legitimo do operador: com contexto EXPLICITO (confianca local
/// declarada), a ferramenta continua rodando.
#[tokio::test]
async fn operador_com_contexto_explicito_continua_rodando() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let exec = ExecContext::default().operador_local();

    rt.process_message_with_exec("sessao-fronteira-5", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        1,
        "o operador, com confianca declarada, nao perde o fluxo legitimo"
    );
}

/// O heartbeat do operador (contexto explicito) tambem continua funcional.
#[tokio::test]
async fn heartbeat_do_operador_com_contexto_explicito_continua_funcional() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let exec = ExecContext::default().operador_local();

    rt.process_heartbeat_with_exec("sessao-fronteira-6", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(
        executou.load(SeqCst),
        1,
        "agendamento do operador com contexto explicito continua rodando"
    );
}
