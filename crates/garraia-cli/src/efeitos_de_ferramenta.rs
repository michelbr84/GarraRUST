//! Efeitos de ferramenta observados nesta sessão da CLI (#1709/#1712).
//!
//! A trilha de ferramentas (`crate::tool_history`) é alimentada pelos eventos do
//! **executor** — `ToolStarted`/`ToolFinished` do runtime, nunca pelo texto do
//! modelo. Até aqui ela servia o histórico entre turnos (#1708); esta camada a
//! projeta como **evidência auditável** consumida pelo autodiagnóstico da CLI
//! (`cli_inspect`), respondendo "esta ferramenta rodou neste turno?" com a
//! referência da execução em vez de "não consigo saber".
//!
//! A projeção é a primeira consumidora de produção de
//! [`garraia_agents::evidence`]: cada registro vira uma [`Claim`] com classe de
//! confiança explícita. As regras que importam aqui:
//!
//! - sucesso registrado → [`VerificationStatus::Observado`](garraia_agents::evidence::VerificationStatus::Observado)
//!   **com** [`EvidenceRef`] (id da chamada + ferramenta + turno);
//! - falha/bloqueio registrado → `falhou`, nunca sucesso;
//! - chamada interrompida ou ainda em execução → `nao_verificado`: ausência de
//!   resultado **não** é evidência de que não aconteceu;
//! - nada de `input`, `output`, `detail` ou caminho: a projeção carrega só
//!   ferramenta, id da chamada, classe, turno e ordem.

use garraia_agents::evidence::{Claim, EvidenceRef, SurfaceSummary};
use serde::Serialize;

use crate::tool_history::{Status, ToolRecord};

/// Quantas execuções a projeção mostra. O total continua reportado ao lado,
/// então o corte é visível para quem lê — não um truncamento silencioso.
pub const MAX_EFEITOS: usize = 20;

/// Uma linha da projeção: a classe de evidência de uma execução observada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinhaDeEfeito {
    /// Nome estável da classe (`observado`, `falhou`, `nao_verificado`).
    pub classe: &'static str,
    /// O que se afirma sobre a execução, em uma linha.
    pub texto: String,
    /// Ferramenta que executou, quando a evidência existe.
    pub ferramenta: Option<String>,
    /// Id da chamada do provedor (`tool_use_id`) quando a evidência existe.
    pub execucao: Option<String>,
    /// Turno em que a execução foi observada.
    pub turno: u32,
    /// Ordem da chamada dentro do turno.
    pub ordem: u32,
}

/// A projeção pronta para o autodiagnóstico.
///
/// `disponivel == false` é o estado fail-closed de quando nenhum turno com
/// ferramenta foi observado nesta sessão: não há evidência nenhuma, e a
/// resposta correta é "não foi observado", nunca uma lista vazia que pareça
/// "nenhuma ferramenta rodou".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjecaoDeEfeitos {
    pub disponivel: bool,
    /// Motivo estável quando `disponivel == false`.
    pub motivo: Option<&'static str>,
    /// Execuções guardadas nesta sessão (inclui as que não couberam na página).
    pub total: usize,
    /// Quantas linhas esta projeção traz.
    pub exibidas: usize,
    pub ultimo_turno: Option<u32>,
    pub linhas: Vec<LinhaDeEfeito>,
}

/// Snapshot compartilhado da trilha, atualizado no fim de cada turno.
///
/// Guarda só as execuções mais recentes (`MAX_EFEITOS`), como a própria trilha
/// faz: o autodiagnóstico responde sobre o que ainda é consultável, e `total`
/// diz quantas foram registradas.
#[derive(Debug, Default)]
pub struct SnapshotDeEfeitos {
    registros: Vec<ToolRecord>,
}

impl SnapshotDeEfeitos {
    /// Absorve os registros de um turno fechado (`ToolTrail::fim_do_turno`).
    ///
    /// Turno sem ferramenta não conta como observação: nada muda, para o
    /// snapshot não fingir que houve execução.
    pub fn atualizar(&mut self, registros: &[ToolRecord]) {
        if registros.is_empty() {
            return;
        }
        self.registros.extend_from_slice(registros);
        if self.registros.len() > MAX_EFEITOS {
            let excesso = self.registros.len() - MAX_EFEITOS;
            self.registros.drain(0..excesso);
        }
    }

    /// Último turno com ferramenta observada.
    pub fn ultimo_turno(&self) -> Option<u32> {
        self.registros.last().map(|r| r.turno)
    }

    /// A projeção para o autodiagnóstico.
    pub fn projecao(&self) -> ProjecaoDeEfeitos {
        if self.registros.is_empty() {
            return ProjecaoDeEfeitos {
                disponivel: false,
                motivo: Some("no_tool_turn_observed_in_this_session"),
                total: 0,
                exibidas: 0,
                ultimo_turno: None,
                linhas: Vec::new(),
            };
        }
        let claims = claims_dos_registros(&self.registros);
        let sumario = SurfaceSummary::de_claims(&claims);
        let linhas = sumario
            .linhas()
            .iter()
            .zip(self.registros.iter())
            .map(|(linha, registro)| LinhaDeEfeito {
                classe: linha.classe,
                texto: linha.texto.clone(),
                ferramenta: linha.ferramenta.clone(),
                execucao: linha.execucao.clone(),
                turno: registro.turno,
                ordem: registro.ordem,
            })
            .collect::<Vec<_>>();
        ProjecaoDeEfeitos {
            disponivel: true,
            motivo: None,
            total: self.registros.len(),
            exibidas: linhas.len(),
            ultimo_turno: self.ultimo_turno(),
            linhas,
        }
    }
}

/// Converte registros do executor em [`Claim`] com proveniência.
///
/// É a única tradução entre a trilha e a camada de evidência; quem consome lê a
/// classe daqui. Nenhum texto carrega payload da ferramenta.
pub fn claims_dos_registros(registros: &[ToolRecord]) -> Vec<Claim> {
    registros.iter().map(claim_do_registro).collect()
}

fn claim_do_registro(registro: &ToolRecord) -> Claim {
    let evidencia = EvidenceRef {
        tool_call_id: registro.call_id.clone(),
        tool_name: registro.name.clone(),
        // Só `Ok` é sucesso registrado. Interrompida/em execução não viram
        // evidência de resultado nenhum — por isso não usam `EvidenceRef`.
        success: registro.status == Status::Ok,
        turno: registro.turno,
        ordem: registro.ordem,
    };
    let nome = registro.name.as_str();
    match registro.status {
        Status::Ok => Claim::observada(
            format!("{nome}: execucao concluida no turno {}", registro.turno),
            evidencia,
        ),
        Status::Error => Claim::falhou(
            format!(
                "{nome}: execucao falhou ou foi bloqueada no turno {}",
                registro.turno
            ),
            evidencia,
        ),
        Status::Interrupted => Claim::nao_verificada(format!(
            "{nome}: interrompida no turno {} — sem resultado registrado",
            registro.turno
        )),
        Status::Running => Claim::nao_verificada(format!(
            "{nome}: em execucao no turno {} — resultado ainda nao observado",
            registro.turno
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registro(id: &str, nome: &str, status: Status, turno: u32, ordem: u32) -> ToolRecord {
        ToolRecord {
            call_id: id.to_string(),
            name: nome.to_string(),
            input: json!({ "path": "/home/alguem/segredo.txt" }),
            detail: "caminho /home/alguem/segredo.txt".to_string(),
            status,
            summary: "escrita concluida".to_string(),
            output: "conteudo-sensivel-do-arquivo".to_string(),
            turno,
            ordem,
        }
    }

    fn snapshot(registros: Vec<ToolRecord>) -> SnapshotDeEfeitos {
        let mut s = SnapshotDeEfeitos::default();
        s.atualizar(&registros);
        s
    }

    #[test]
    fn projecao_vazia_e_fail_closed_em_vez_de_lista_vazia() {
        let projecao = SnapshotDeEfeitos::default().projecao();
        assert!(!projecao.disponivel);
        assert_eq!(
            projecao.motivo,
            Some("no_tool_turn_observed_in_this_session")
        );
        assert_eq!(projecao.total, 0);
        assert!(projecao.linhas.is_empty());
    }

    #[test]
    fn execucao_ok_vira_observado_com_referencia_da_chamada() {
        let s = snapshot(vec![registro("c1", "file_write", Status::Ok, 1, 1)]);
        let projecao = s.projecao();
        assert!(projecao.disponivel);
        let linha = &projecao.linhas[0];
        assert_eq!(linha.classe, "observado");
        assert_eq!(linha.ferramenta.as_deref(), Some("file_write"));
        assert_eq!(linha.execucao.as_deref(), Some("c1"));
        assert_eq!(linha.turno, 1);
        assert_eq!(linha.ordem, 1);
    }

    #[test]
    fn execucao_com_falha_vira_falhou_e_nunca_sucesso() {
        let s = snapshot(vec![registro("c2", "bash", Status::Error, 2, 1)]);
        let projecao = s.projecao();
        assert_eq!(projecao.linhas[0].classe, "falhou");
        assert_eq!(projecao.linhas[0].execucao.as_deref(), Some("c2"));
    }

    #[test]
    fn interrompida_nao_e_observado_nem_falhou() {
        let s = snapshot(vec![registro("c3", "bash", Status::Interrupted, 3, 1)]);
        let projecao = s.projecao();
        let linha = &projecao.linhas[0];
        assert_eq!(linha.classe, "nao_verificado");
        // Sem resultado registrado não há `EvidenceRef`: a linha não pode
        // alegar execução comprovada.
        assert_eq!(linha.execucao, None);
        assert_eq!(linha.ferramenta, None);
    }

    #[test]
    fn em_execucao_nao_e_observado() {
        let s = snapshot(vec![registro("c4", "bash", Status::Running, 4, 1)]);
        assert_eq!(s.projecao().linhas[0].classe, "nao_verificado");
    }

    #[test]
    fn snapshot_guarda_so_as_ultimas_execucoes_e_reporta_o_total() {
        let mut s = SnapshotDeEfeitos::default();
        let primeiro: Vec<ToolRecord> = (1..=15)
            .map(|i| registro(&format!("t1-{i}"), "file_read", Status::Ok, 1, i))
            .collect();
        s.atualizar(&primeiro);
        let segundo: Vec<ToolRecord> = (1..=15)
            .map(|i| registro(&format!("t2-{i}"), "file_read", Status::Ok, 2, i))
            .collect();
        s.atualizar(&segundo);

        let projecao = s.projecao();
        assert_eq!(projecao.total, MAX_EFEITOS);
        assert_eq!(projecao.exibidas, MAX_EFEITOS);
        assert_eq!(projecao.ultimo_turno, Some(2));
        // As mais antigas saíram: com 15 + 15 registros, as últimas 20 ficam
        // (t1-11..t1-15 e depois o turno 2 inteiro).
        assert_eq!(projecao.linhas[0].execucao.as_deref(), Some("t1-11"));
        assert_eq!(projecao.linhas[4].execucao.as_deref(), Some("t1-15"));
        assert_eq!(projecao.linhas[5].execucao.as_deref(), Some("t2-1"));
        assert_eq!(
            projecao.linhas[MAX_EFEITOS - 1].execucao.as_deref(),
            Some("t2-15")
        );
    }

    #[test]
    fn turno_sem_ferramenta_nao_conta_como_observacao() {
        let mut s = SnapshotDeEfeitos::default();
        s.atualizar(&[]);
        assert!(s.ultimo_turno().is_none());
        assert!(!s.projecao().disponivel);
    }

    #[test]
    fn projecao_nao_carrega_payload_da_ferramenta() {
        let s = snapshot(vec![registro("c5", "file_write", Status::Ok, 1, 1)]);
        let projecao = s.projecao();
        let texto = serde_json::to_string(&projecao.linhas).unwrap();
        assert!(!texto.contains("segredo.txt"));
        assert!(!texto.contains("conteudo-sensivel"));
        assert!(!texto.contains("caminho"));
    }

    #[test]
    fn todas_as_linhas_tem_classe_do_vocabulario_estavel() {
        let s = snapshot(vec![
            registro("a", "file_write", Status::Ok, 1, 1),
            registro("b", "bash", Status::Error, 1, 2),
            registro("c", "bash", Status::Interrupted, 1, 3),
        ]);
        let validos = ["observado", "falhou", "nao_verificado"];
        for linha in s.projecao().linhas {
            assert!(validos.contains(&linha.classe), "classe {}", linha.classe);
        }
    }
}
