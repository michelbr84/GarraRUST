//! Avaliação longitudinal determinística da detecção de alegações operacionais (#1709).
//!
//! Corpus sintético fixo: testa a consistência entre a polaridade da alegação e o
//! resultado confiável do executor. Não mede a frequência de erros de um LLM real.

use super::*;

#[derive(Clone, Copy)]
enum ResultadoAlegado {
    Sucesso,
    Falha,
    NaoVerificado,
}

#[derive(Clone, Copy)]
enum ClasseCaso {
    Contraditorio,
    Coerente,
    SemEvidencia,
}

struct Caso {
    nome: &'static str,
    alegado: ResultadoAlegado,
    id_alegacao: &'static str,
    ferramenta_alegacao: &'static str,
    ledger: Option<(&'static str, &'static str, bool)>,
    classe: ClasseCaso,
}

fn corpus() -> [Caso; 7] {
    [
        Caso {
            nome: "escrita confirmada",
            alegado: ResultadoAlegado::Sucesso,
            id_alegacao: "c1",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c1", "file_write", true)),
            classe: ClasseCaso::Coerente,
        },
        Caso {
            nome: "escrita negada relatada como falha",
            alegado: ResultadoAlegado::Falha,
            id_alegacao: "c2",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c2", "file_write", false)),
            classe: ClasseCaso::Coerente,
        },
        Caso {
            nome: "falha relatada como sucesso",
            alegado: ResultadoAlegado::Sucesso,
            id_alegacao: "c3",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c3", "file_write", false)),
            classe: ClasseCaso::Contraditorio,
        },
        Caso {
            nome: "sucesso relatado como falha",
            alegado: ResultadoAlegado::Falha,
            id_alegacao: "c4",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c4", "file_write", true)),
            classe: ClasseCaso::Contraditorio,
        },
        Caso {
            nome: "id de chamada ausente do ledger",
            alegado: ResultadoAlegado::Sucesso,
            id_alegacao: "c5",
            ferramenta_alegacao: "file_write",
            ledger: Some(("outra-chamada", "file_write", true)),
            classe: ClasseCaso::SemEvidencia,
        },
        Caso {
            nome: "alegacao sem evidencia",
            alegado: ResultadoAlegado::NaoVerificado,
            id_alegacao: "",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c6", "file_write", true)),
            classe: ClasseCaso::SemEvidencia,
        },
        Caso {
            nome: "id igual com ferramenta diferente",
            alegado: ResultadoAlegado::Sucesso,
            id_alegacao: "c7",
            ferramenta_alegacao: "file_write",
            ledger: Some(("c7", "file_read", true)),
            classe: ClasseCaso::SemEvidencia,
        },
    ]
}

fn avaliar(caso: &Caso) -> Divergencia {
    let evidencia = EvidenceRef {
        tool_call_id: caso.id_alegacao.to_string(),
        tool_name: caso.ferramenta_alegacao.to_string(),
        success: matches!(caso.alegado, ResultadoAlegado::Sucesso),
        turno: 1,
        ordem: 1,
    };
    let claim = match caso.alegado {
        ResultadoAlegado::Sucesso => Claim::observada("operacao concluida", evidencia),
        ResultadoAlegado::Falha => Claim::falhou("operacao falhou", evidencia),
        ResultadoAlegado::NaoVerificado => Claim::nao_verificada("nao consigo confirmar"),
    };
    let ledger = caso
        .ledger
        .map(|(id, nome, sucesso)| EvidenceRef {
            tool_call_id: id.to_string(),
            tool_name: nome.to_string(),
            success: sucesso,
            turno: 1,
            ordem: 1,
        })
        .into_iter()
        .collect::<Vec<_>>();
    detect_divergencia(&claim, &ledger)
}

#[test]
fn baseline_longitudinal_do_corpus_fixo_de_alegacoes() {
    let casos = corpus();
    assert_eq!(casos.len(), 7, "o corpus v1 so muda com revisao explicita");
    let mut contradicoes = 0;
    let mut detectadas = 0;
    let mut coerentes = 0;
    let mut falsos_alertas = Vec::new();
    let mut sem_evidencia = 0;
    let mut preservadas_como_desconhecidas = 0;

    for caso in &casos {
        let resultado = avaliar(caso);
        match caso.classe {
            ClasseCaso::Contraditorio => {
                contradicoes += 1;
                if matches!(
                    resultado,
                    Divergencia::AfirmaSucessoMasLedgerFalhou { .. }
                        | Divergencia::AfirmaFalhaMasLedgerSucesso { .. }
                ) {
                    detectadas += 1;
                } else {
                    panic!("contradicao nao detectada: {} -> {resultado:?}", caso.nome);
                }
            }
            ClasseCaso::Coerente => {
                coerentes += 1;
                if resultado != Divergencia::Sustentada {
                    falsos_alertas.push(caso.nome);
                }
            }
            ClasseCaso::SemEvidencia => {
                sem_evidencia += 1;
                if resultado == Divergencia::SemEvidenciaNoLedger {
                    preservadas_como_desconhecidas += 1;
                } else {
                    panic!(
                        "ausencia de evidencia classificada incorretamente em {}: {resultado:?}",
                        caso.nome
                    );
                }
            }
        }
    }

    println!(
        "corpus={} contradicoes={}/{} coerentes_sem_falso_alerta={}/{} desconhecidas_preservadas={}/{}",
        casos.len(),
        detectadas,
        contradicoes,
        coerentes - falsos_alertas.len(),
        coerentes,
        preservadas_como_desconhecidas,
        sem_evidencia
    );
    assert_eq!(contradicoes, 2);
    assert_eq!(coerentes, 2);
    assert_eq!(sem_evidencia, 3);
    // A linha de base do corpus v1 deve detectar todas as polaridades incompatíveis.
    assert_eq!(detectadas, 2);
    assert!(
        falsos_alertas.is_empty(),
        "falsos alertas nas alegacoes coerentes: {falsos_alertas:?}"
    );
    assert_eq!(preservadas_como_desconhecidas, 3);
}
