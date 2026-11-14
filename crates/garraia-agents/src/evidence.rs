//! Conclusões auditáveis sobre efeitos de ferramenta: proveniência e calibração (#1709).
//!
//! A #1708 garante que a **trilha do executor** (o que realmente rodou) sobreviva
//! entre turnos. Esta camada resolve a segunda lacuna de produto: o agente mistura
//! sistematicamente evidência direta, inferência, dado de terceiro e ausência de
//! evidência — e transforma "não tenho como confirmar" em "não aconteceu".
//!
//! Aqui mora a estrutura reutilizável `Claim + EvidenceRef + VerificationStatus`,
//! alimentada por registros **gerados pelo executor** (o [`TurnEvent`] do runtime),
//! nunca pela resposta do modelo. A regra central, que os testes fixam:
//!
//! - **ausência de evidência ≠ evidência de ausência** — sem registro, a classe é
//!   [`VerificationStatus::NaoVerificado`], nunca `Falhou` nem "não ocorreu";
//! - [`VerificationStatus::Observado`] exige um [`EvidenceRef`] apontando para a
//!   execução que o comprova (id de chamada + resultado), não um resumo textual;
//! - execução que falhou ou foi bloqueada vira [`VerificationStatus::Falhou`],
//!   com o mecanismo, e nunca é relatada como sucesso.
//!
//! A camada de apresentação (CLI, canais) consome estes tipos para emitir um
//! sumário curto com proveniência; o detalhe fica sob demanda. Nada aqui executa
//! comando, muta estado ou decide permissão — só descreve **o que se sabe** sobre
//! uma ação, e de onde veio o saber.

use crate::turn_events::TurnEvent;

/// A classe de confiança de uma afirmação sobre um efeito/estado observável.
///
/// As cinco classes espelham a semântica mínima de evidência da #1709. Não há
/// percentual de confiança: confiança calibrada não existe aqui, e um número
/// arbitrário mente sobre a certeza.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    /// Confirmado por resultado de ferramenta do executor (com [`EvidenceRef`]).
    Observado,
    /// Interpretação de uma ou mais observações; traz ressalva explícita.
    Inferido,
    /// Informação recebida como entrada (usuário/terceiro), sem confirmação
    /// independente.
    Relatado,
    /// Não há dado suficiente. NÃO equivale a "não ocorreu".
    NaoVerificado,
    /// A execução falhou ou foi bloqueada (com erro/mecanismo correspondente).
    Falhou,
}

impl VerificationStatus {
    /// Nome estável, para sumário e serialização.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observado => "observado",
            Self::Inferido => "inferido",
            Self::Relatado => "relatado",
            Self::NaoVerificado => "nao_verificado",
            Self::Falhou => "falhou",
        }
    }
}

/// Referência verificável a uma execução do runtime.
///
/// É o que distingue "observado" de "eu acho": aponta para o registro do
/// executor (id da chamada + resultado), não para o texto que o modelo produziu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    /// Id da chamada de ferramenta (`tool_use_id`) que o provedor emitiu.
    pub tool_call_id: String,
    /// Nome registrado da ferramenta (`bash`, `file_write`, …).
    pub tool_name: String,
    /// `true` quando o executor registrou sucesso; `false` em falha/bloqueio.
    pub success: bool,
    /// Turno em que a execução ocorreu.
    pub turno: u32,
    /// Ordem da chamada dentro do turno.
    pub ordem: u32,
}

/// Uma afirmação sobre um efeito/estado, com a sua proveniência.
///
/// O `texto` é o que se afirma; `evidencia` é o que a sustenta (ou `None`);
/// `status` é a classe de confiança. O construtor [`Claim::observada`] é o único
/// caminho para `Observado`, e ele exige evidência — não há como declarar
/// "observado" sem apontar para a execução.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub texto: String,
    pub status: VerificationStatus,
    pub evidencia: Option<EvidenceRef>,
}

/// Erro de construção de uma [`Claim`] com semântica inválida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// Tentativa de declarar `Observado` sem [`EvidenceRef`].
    ObservadoSemEvidencia,
    /// Tentativa de declarar `Falhou` sem [`EvidenceRef`] (precisa do mecanismo).
    FalhouSemEvidencia,
    /// `EvidenceRef` presente numa classe que não a usa (ex.: `NaoVerificado`).
    EvidenciaEmClasseSemEvidencia,
}

impl Claim {
    /// Uma afirmação **confirmada** por uma execução do executor.
    ///
    /// É o único caminho para [`VerificationStatus::Observado`].
    pub fn observada(texto: impl Into<String>, evidencia: EvidenceRef) -> Self {
        Self {
            texto: texto.into(),
            status: VerificationStatus::Observado,
            evidencia: Some(evidencia),
        }
    }

    /// Uma afirmação sobre uma execução que **falhou ou foi bloqueada**.
    ///
    /// Carrega a evidência (o mecanismo/erro) para nunca ser relatada como
    /// sucesso.
    pub fn falhou(texto: impl Into<String>, evidencia: EvidenceRef) -> Self {
        Self {
            texto: texto.into(),
            status: VerificationStatus::Falhou,
            evidencia: Some(evidencia),
        }
    }

    /// Uma **inferência** a partir de observações; sem evidência direta única.
    pub fn inferida(texto: impl Into<String>) -> Self {
        Self {
            texto: texto.into(),
            status: VerificationStatus::Inferido,
            evidencia: None,
        }
    }

    /// Informação **relatada** por terceiro/usuário, sem confirmação independente.
    pub fn relatada(texto: impl Into<String>) -> Self {
        Self {
            texto: texto.into(),
            status: VerificationStatus::Relatado,
            evidencia: None,
        }
    }

    /// **Não há dado suficiente.** Nunca deve ser lido como "não ocorreu".
    pub fn nao_verificada(texto: impl Into<String>) -> Self {
        Self {
            texto: texto.into(),
            status: VerificationStatus::NaoVerificado,
            evidencia: None,
        }
    }

    /// Valida a coerência entre `status` e `evidencia`.
    ///
    /// É o guarda da semântica: `Observado`/`Falhou` exigem evidência; as demais
    /// classes não podem carregar uma.
    pub fn validar(&self) -> Result<(), ClaimError> {
        match (self.status, self.evidencia.is_some()) {
            (VerificationStatus::Observado, false) => Err(ClaimError::ObservadoSemEvidencia),
            (VerificationStatus::Falhou, false) => Err(ClaimError::FalhouSemEvidencia),
            (VerificationStatus::Observado, true) | (VerificationStatus::Falhou, true) => Ok(()),
            (VerificationStatus::Inferido, true)
            | (VerificationStatus::Relatado, true)
            | (VerificationStatus::NaoVerificado, true) => {
                Err(ClaimError::EvidenciaEmClasseSemEvidencia)
            }
            (VerificationStatus::Inferido, false)
            | (VerificationStatus::Relatado, false)
            | (VerificationStatus::NaoVerificado, false) => Ok(()),
        }
    }
}

/// Uma linha de [`EvidenceRef`] derivada de um [`TurnEvent`] do executor.
///
/// Só eventos de ferramenta produzem evidência. [`TurnEvent::TextDelta`] não
/// prova ação nenhuma (é o que o modelo disse) e devolve `None` — da mesma
/// forma, `ToolStarted` sozinho é início, não resultado: a evidência de
/// sucesso/falha nasce do `ToolFinished`.
pub fn evidence_from_event(event: &TurnEvent, turno: u32, ordem: u32) -> Option<EvidenceRef> {
    match event {
        TurnEvent::ToolFinished {
            id, name, success, ..
        } => Some(EvidenceRef {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            success: *success,
            turno,
            ordem,
        }),
        // Início sem fim ainda não prova resultado; texto não prova ação.
        TurnEvent::ToolStarted { .. } | TurnEvent::TextDelta(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            summary: "ok".into(),
            output: String::new(),
        };
        let e = evidence_from_event(&finished, 1, 1).expect("ToolFinished produz evidência");
        assert_eq!(e.tool_call_id, "c5");
        assert!(e.success);
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
}
