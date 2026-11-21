//! Testes unitarios de `evidence.rs` (ledger, divergencia, superficie).

use super::*;
use crate::turn_events::DesfechoDaChamada;
use std::time::Duration;

fn ev(id: &str, name: &str, success: bool) -> EvidenceRef {
    EvidenceRef {
        tool_call_id: id.into(),
        tool_name: name.into(),
        success,
        turno: 1,
        ordem: 1,
    }
}

#[test]
fn observado_exige_evidencia() {
    let c = Claim::observada("escrevi pagina.html", ev("c1", "file_write", true));
    assert_eq!(c.status, VerificationStatus::Observado);
    assert!(c.validar().is_ok());
    assert!(c.evidencia.as_ref().unwrap().success);
}

#[test]
fn ausencia_de_evidencia_nao_vira_falha_nem_ausencia() {
    // Sem registro, a classe é NaoVerificado — nunca "não aconteceu".
    let c = Claim::nao_verificada("não consigo confirmar se escrevi o arquivo");
    assert_eq!(c.status, VerificationStatus::NaoVerificado);
    assert!(c.evidencia.is_none());
    assert!(c.validar().is_ok());
    // E não é `Falhou` nem `Observado`.
    assert_ne!(c.status, VerificationStatus::Falhou);
    assert_ne!(c.status, VerificationStatus::Observado);
}

#[test]
fn falha_carrega_mecanismo_e_nao_e_sucesso() {
    let c = Claim::falhou("file_write bloqueado", ev("c2", "file_write", false));
    assert_eq!(c.status, VerificationStatus::Falhou);
    assert!(c.validar().is_ok());
    assert!(!c.evidencia.as_ref().unwrap().success);
}

#[test]
fn observado_sem_evidencia_e_invalido() {
    let mut c = Claim::inferida("qualquer");
    c.status = VerificationStatus::Observado; // tentativa inválida
    assert_eq!(c.validar(), Err(ClaimError::ObservadoSemEvidencia));
}

#[test]
fn inferido_e_relatado_nao_podem_carregar_evidencia() {
    let mut c = Claim::inferida("parece que");
    c.evidencia = Some(ev("c3", "file_read", true));
    assert_eq!(c.validar(), Err(ClaimError::EvidenciaEmClasseSemEvidencia));

    let mut r = Claim::relatada("o usuário disse");
    r.evidencia = Some(ev("c4", "file_read", true));
    assert_eq!(r.validar(), Err(ClaimError::EvidenciaEmClasseSemEvidencia));
}

#[test]
fn texto_do_modelo_nao_e_evidencia() {
    // TextDelta não prova ação; ToolStarted sozinho não prova resultado.
    assert!(evidence_from_event(&TurnEvent::TextDelta("x".into()), 1, 1).is_none());
    let started = TurnEvent::ToolStarted {
        id: "c5".into(),
        name: "file_write".into(),
        detail: String::new(),
        input: serde_json::Value::Null,
    };
    assert!(evidence_from_event(&started, 1, 1).is_none());

    let finished = TurnEvent::ToolFinished {
        id: "c5".into(),
        name: "file_write".into(),
        duration: Duration::from_millis(3),
        success: true,
        outcome: crate::turn_events::DesfechoDaChamada::Sucesso,
        summary: "ok".into(),
        output: String::new(),
    };
    let e = evidence_from_event(&finished, 1, 1).expect("ToolFinished produz evidência");
    assert_eq!(e.tool_call_id, "c5");
    assert!(e.success);
}

#[test]
fn chamada_cancelada_nao_vira_evidencia_de_falha() {
    // #1915: o turno caiu com a chamada em andamento. O resultado não foi
    // observado — ausência de evidência, não evidência de falha.
    let cancelada = TurnEvent::ToolFinished {
        id: "c9".into(),
        name: "bash".into(),
        duration: Duration::from_millis(40),
        success: false,
        outcome: crate::turn_events::DesfechoDaChamada::Cancelada,
        summary: "cancelada".into(),
        output: String::new(),
    };
    assert!(evidence_from_event(&cancelada, 2, 1).is_none());

    // Ledger montado de um fluxo real: início da cancelada, o fim
    // `Cancelada`, e uma segunda chamada que terminou com sucesso. Só a
    // segunda entra — a cancelada não deixa rastro nenhum.
    let fluxo = [
        TurnEvent::ToolStarted {
            id: "c9".into(),
            name: "bash".into(),
            detail: String::new(),
            input: serde_json::Value::Null,
        },
        cancelada,
        TurnEvent::ToolFinished {
            id: "c11".into(),
            name: "file_read".into(),
            duration: Duration::from_millis(2),
            success: true,
            outcome: DesfechoDaChamada::Sucesso,
            summary: "ok".into(),
            output: String::new(),
        },
    ];
    let ledger: Vec<EvidenceRef> = fluxo
        .iter()
        .enumerate()
        .filter_map(|(i, e)| evidence_from_event(e, 2, i as u32))
        .collect();
    assert_eq!(ledger.len(), 1, "{ledger:?}");
    assert_eq!(ledger[0].tool_call_id, "c11");
    assert!(ledger.iter().all(|e| e.tool_call_id != "c9"));

    // Sem entrada no ledger, a alegação de sucesso fica desconhecida em
    // vez de ser acusada de contradizer uma "falha" que não aconteceu —
    // e a alegação sobre a outra chamada segue sustentada.
    assert_eq!(
        detect_divergencia(&AlegacaoOperacional::sucesso("c9", "bash"), &ledger),
        Divergencia::SemEvidenciaNoLedger
    );
    assert_eq!(
        detect_divergencia(&AlegacaoOperacional::sucesso("c11", "file_read"), &ledger),
        Divergencia::Sustentada
    );

    // Os demais desfechos sem sucesso continuam sendo falha registrada.
    for outcome in crate::turn_events::DesfechoDaChamada::TODOS {
        if matches!(
            outcome,
            crate::turn_events::DesfechoDaChamada::Sucesso
                | crate::turn_events::DesfechoDaChamada::Cancelada
        ) {
            continue;
        }
        let ev = TurnEvent::ToolFinished {
            id: "c10".into(),
            name: "bash".into(),
            duration: Duration::from_millis(1),
            success: outcome.e_sucesso(),
            outcome,
            summary: String::new(),
            output: String::new(),
        };
        let e = evidence_from_event(&ev, 1, 1).expect("desfecho observado vira evidência");
        assert!(!e.success, "{outcome:?} não é sucesso");
    }
}

/// A evidência segue o `outcome`, não o `success` do evento: um evento
/// montado à mão com os dois em desacordo não consegue fabricar sucesso
/// (nem esconder um). Em produção eles não divergem — o `TurnSink` deriva
/// um do outro —, mas a fonte da evidência fica sendo uma só.
#[test]
fn evidencia_deriva_sucesso_do_outcome() {
    for outcome in DesfechoDaChamada::TODOS {
        let ev = TurnEvent::ToolFinished {
            id: "c12".into(),
            name: "bash".into(),
            duration: Duration::from_millis(1),
            success: !outcome.e_sucesso(),
            outcome,
            summary: String::new(),
            output: String::new(),
        };
        match evidence_from_event(&ev, 1, 1) {
            Some(e) => {
                assert!(outcome.resultado_observado(), "{outcome:?}");
                assert_eq!(e.success, outcome.e_sucesso(), "{outcome:?}");
            }
            None => assert!(!outcome.resultado_observado(), "{outcome:?}"),
        }
    }
}

#[test]
fn tabela_das_cinco_classes_tem_nome_estavel() {
    let esperado = [
        (VerificationStatus::Observado, "observado"),
        (VerificationStatus::Inferido, "inferido"),
        (VerificationStatus::Relatado, "relatado"),
        (VerificationStatus::NaoVerificado, "nao_verificado"),
        (VerificationStatus::Falhou, "falhou"),
    ];
    for (status, nome) in esperado {
        assert_eq!(status.as_str(), nome);
    }
}

// ─── Fatia 2: projeção de superfície ─────────────────────────────────

#[test]
fn surface_summary_projeta_classe_e_referencia() {
    let claims = vec![
        Claim::observada("escrevi pagina.html", ev("c1", "file_write", true)),
        Claim::nao_verificada("não sei se editei TESTE.md"),
        Claim::falhou("file_write bloqueado", ev("c2", "file_write", false)),
    ];
    let s = SurfaceSummary::de_claims(&claims);
    assert_eq!(s.len(), 3);
    let linhas = s.para_exibicao();
    // Observado: classe + ferramenta + id da execução.
    assert!(linhas[0].contains("[observado]"));
    assert!(linhas[0].contains("file_write"));
    assert!(linhas[0].contains("#c1"));
    // NaoVerificado: sem execução comprovada — nunca "não aconteceu".
    assert!(linhas[1].contains("[nao_verificado]"));
    assert!(linhas[1].contains("sem execucao comprovada"));
    // Falhou: classe + ferramenta + id, para o mecanismo ser rastreável.
    assert!(linhas[2].contains("[falhou]"));
    assert!(linhas[2].contains("#c2"));
}

#[test]
fn surface_summary_nao_expoem_payload_nem_caminho() {
    // O texto da afirmação é o que o modelo disse; o sumário não deve
    // acrescentar nada que não esteja na Claim.
    let c = Claim::observada("escrevi pagina.html", ev("c1", "file_write", true));
    let s = SurfaceSummary::de_claims(&[c]);
    let linha = &s.para_exibicao()[0];
    assert!(!linha.contains("/home/"));
    assert!(!linha.contains("output"));
    assert!(!linha.contains("input"));
}

#[test]
fn surface_summary_vazia_e_is_empty() {
    let s = SurfaceSummary::de_claims(&[]);
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert!(s.para_exibicao().is_empty());
}

// ─── Fatia 2: detecção de divergência ────────────────────────────────

#[test]
fn divergencia_sustentada_quando_ledger_confirma() {
    let alegacao = AlegacaoOperacional::sucesso("c1", "file_write");
    let ledger = vec![ev("c1", "file_write", true)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::Sustentada
    );
}

#[test]
fn divergencia_quando_modelo_afirma_sucesso_mas_ledger_falhou() {
    let alegacao = AlegacaoOperacional::sucesso("c1", "file_write");
    let ledger = vec![ev("c1", "file_write", false)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::AfirmaSucessoMasLedgerFalhou {
            tool_call_id: "c1".into(),
            tool_name: "file_write".into(),
        }
    );
}

#[test]
fn divergencia_sem_correspondencia_no_ledger_permanece_desconhecida() {
    let alegacao = AlegacaoOperacional::sucesso("c9", "file_write");
    let ledger = vec![ev("c1", "file_write", true)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::SemEvidenciaNoLedger
    );
}

#[test]
fn polaridade_nao_classificada_permanece_desconhecida() {
    let alegacao = AlegacaoOperacional::sem_polaridade("c1", "file_write");
    let ledger = vec![ev("c1", "file_write", true)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::SemEvidenciaNoLedger
    );
}

#[test]
fn alegacao_de_falha_bate_com_falha_do_ledger() {
    let alegacao = AlegacaoOperacional::falha("c1", "file_write");
    let ledger = vec![ev("c1", "file_write", false)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::Sustentada
    );
}

#[test]
fn divergencia_quando_modelo_afirma_falha_mas_ledger_confirma_sucesso() {
    let alegacao = AlegacaoOperacional::falha("c1", "file_write");
    let ledger = vec![ev("c1", "file_write", true)];
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::AfirmaFalhaMasLedgerSucesso {
            tool_call_id: "c1".into(),
            tool_name: "file_write".into(),
        }
    );
}
