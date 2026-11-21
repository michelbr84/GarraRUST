//! #1915 (criterio 4): um turno descartado com a ferramenta em andamento
//! fecha a chamada com `DesfechoDaChamada::Cancelada`.
//!
//! Antes, cancelar o turno (Ctrl+C da `garra chat`, teto de parede, cliente
//! que desistiu) dropava o futuro e o `ToolStarted` ficava sem fim. Os testes
//! abaixo dropam o futuro do turno de verdade (`JoinHandle::abort`) com a
//! ferramenta pendurada e leem o canal DEPOIS, como a `garra chat` faz.

use super::super::{AgentRuntime, TOOL_PROGRAM_NAME};
use super::eventos_por_passo_no_streaming::ProgramaEmStreaming;
use super::{FerramentaLenta, inicios_casados_com_fins};
use crate::exec_context::ExecContext;
use crate::turn_events::{DesfechoDaChamada, TurnEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Roda o turno numa task, espera o `ToolStarted` de `alvo`, aborta a task
/// e devolve TODOS os eventos do canal, inclusive os que o drop emitiu.
async fn cancela_durante(rt: Arc<AgentRuntime>, alvo: &str) -> Vec<TurnEvent> {
    let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
    let turno = tokio::spawn(async move {
        rt.process_message_streaming_with_events(
            "sessao-cancelada",
            "roda",
            &[],
            tx,
            None,
            None,
            None,
            None,
            None,
            None,
            &ExecContext::default(),
        )
        .await
    });
    let mut eventos = Vec::new();
    loop {
        let e = tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv())
            .await
            .expect("o ToolStarted do alvo nao chegou")
            .expect("canal fechou antes do ToolStarted do alvo");
        let comecou = matches!(&e, TurnEvent::ToolStarted { name, .. } if name == alvo);
        eventos.push(e);
        if comecou {
            break;
        }
    }
    turno.abort();
    let _ = turno.await;
    // O sender morreu com o futuro: o canal drena e fecha.
    while let Some(e) = rx.recv().await {
        eventos.push(e);
    }
    eventos
}

fn fins(eventos: &[TurnEvent]) -> Vec<(String, DesfechoDaChamada, bool)> {
    eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished {
                name,
                outcome,
                success,
                ..
            } => Some((name.clone(), *outcome, *success)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn passo_pendurado_cancelado_fecha_passo_e_envelope_de_dentro_para_fora() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(FerramentaLenta));
    rt.register_provider(ProgramaEmStreaming::novo(serde_json::json!({
        "steps": [ { "tool": "lenta", "args": { "segundos": 3600 } } ]
    })));

    let eventos = cancela_durante(Arc::new(rt), "lenta").await;

    // Todo inicio tem o seu fim, empilhados: o passo fecha antes do envelope.
    let iniciados = inicios_casados_com_fins(&eventos);
    assert_eq!(
        iniciados,
        vec![TOOL_PROGRAM_NAME.to_string(), "lenta".to_string()]
    );
    assert_eq!(
        fins(&eventos),
        vec![
            ("lenta".to_string(), DesfechoDaChamada::Cancelada, false),
            (
                TOOL_PROGRAM_NAME.to_string(),
                DesfechoDaChamada::Cancelada,
                false
            ),
        ]
    );
    // O id casa com o do inicio.
    let ids_inicio: Vec<&str> = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolStarted { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let ids_fim: Vec<&str> = eventos
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids_fim,
        ids_inicio.iter().rev().copied().collect::<Vec<_>>()
    );
}

/// O caminho normal NAO emite `Cancelada`: um vigia esquecido armado num
/// dos `tool_finished` do despacho apareceria aqui como fim duplicado.
#[tokio::test]
async fn turno_que_termina_nao_emite_cancelada() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(FerramentaLenta));
    rt.register_provider(ProgramaEmStreaming::novo(serde_json::json!({
        "steps": [ { "tool": "lenta", "args": { "segundos": 0 } } ]
    })));

    let (resultado, eventos) =
        super::turno_de_streaming_com_eventos(&rt, "sessao-normal", &ExecContext::default()).await;
    resultado.expect("turno");
    inicios_casados_com_fins(&eventos);
    let desfechos: Vec<DesfechoDaChamada> = fins(&eventos).into_iter().map(|f| f.1).collect();
    assert_eq!(
        desfechos,
        vec![DesfechoDaChamada::Sucesso, DesfechoDaChamada::Sucesso]
    );
}
