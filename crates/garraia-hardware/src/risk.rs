//! O modelo de risco R0–R5 (#1129) — a parte não negociável do epic.
//!
//! Cada [`crate::Capability`] carrega um risk class, e o runtime de hardware
//! trata classes diferentes de formas diferentes. O objetivo do modelo:
//! *hardware nunca recebe autonomia irrestrita*.
//!
//! | Nível | Definição | Exemplos | Regra de execução |
//! |---|---|---|---|
//! | R0 | leitura | temperature, battery, door_status | automático (permitido) |
//! | R1 | ação reversível | light_on, volume, fan_speed | policy (ToolPolicy/modos) |
//! | R2 | pequena mudança física | cover_position, scene | policy + rate limit |
//! | R3 | acesso/segurança | door_unlock, garage_open | confirmação humana |
//! | R4 | risco físico | robot_motion, oven_on | policy + approval explícito |
//! | R5 | proibido por padrão | industrial actuator, security system | deny, só com explicit allowlist |
//!
//! A regra de execução **não** é um sistema paralelo ao que o repo já tem:
//! `PolicyGated` significa "a decisão passa pelo `ToolPolicy`/modos
//! existentes" (`garraia-agents::modes`), e `HumanConfirmation` /
//! `ExplicitApproval` significam o fluxo de confirmação do GAR-187
//! (`ToolApproval` por impressão digital), o mesmo do tier *risky* do bash.
//! Fail-closed por padrão: sem canal de confirmação, R3/R4 não rodam; R5 é
//! deny-by-default com allowlist explícita do operador.

use serde::{Deserialize, Serialize};

/// O quanto uma capability pode afetar o mundo físico.
///
/// A ordenação deriva do risco (`R0 < R1 < ... < R5`), e a serialização em
/// JSON usa a forma minúscula (`"r0"`) — é o que vai para o histórico de
/// conversa e para o inventário exposto ao agente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Leitura — sem efeito no mundo.
    R0,
    /// Ação reversível.
    R1,
    /// Pequena mudança física.
    R2,
    /// Acesso/segurança — destrancar porta, abrir portão.
    R3,
    /// Risco físico — braço robótico, forno.
    R4,
    /// Proibido por padrão — só com allowlist explícita do operador.
    R5,
}

/// A decisão que o [`crate::HardwareGate`] toma para uma capability.
///
/// As variantes nomeiam **quem decide**, não só sim/não — a integração
/// (`garraia-agents`) mapeia cada uma na máquina que já existe:
///
/// - `Auto`: roda;
/// - `PolicyGated`: só roda se o modo/`ToolPolicy` do turno permitir;
/// - `PolicyRateLimited`: idem, com teto de frequência (o teto é
///   declarado aqui e enforce no motor de automações, #1128);
/// - `HumanConfirmation`: fluxo GAR-187 — a tool devolve
///   `requires_confirmation` e aguarda a aprovação com impressão digital;
/// - `ExplicitApproval`: como o anterior, mas o operador precisa ter
///   allowlistado a capability explicitamente (R5 aprovado e R4 sempre);
/// - `Denied`: nunca roda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionDecision {
    Auto,
    PolicyGated,
    PolicyRateLimited,
    HumanConfirmation,
    ExplicitApproval,
    Denied,
}

impl RiskClass {
    /// A decisão da tabela R0–R5 para esta classe.
    ///
    /// Dois parâmetros mudam o desfecho sem mudar a tabela:
    ///
    /// - `canal_confirmacao`: existe canal real de confirmação humana?
    ///   Sem ele, R3/R4 são **negadas** (fail-closed — um pedido de
    ///   confirmação que ninguém pode responder não vira execução silenciosa);
    /// - `na_allowlist`: a capability está na allowlist de R5 do operador.
    pub fn decisao(self, canal_confirmacao: bool, na_allowlist: bool) -> ExecutionDecision {
        match self {
            Self::R0 => ExecutionDecision::Auto,
            Self::R1 => ExecutionDecision::PolicyGated,
            Self::R2 => ExecutionDecision::PolicyRateLimited,
            Self::R3 if canal_confirmacao => ExecutionDecision::HumanConfirmation,
            Self::R4 if canal_confirmacao => ExecutionDecision::ExplicitApproval,
            Self::R5 if na_allowlist && canal_confirmacao => ExecutionDecision::ExplicitApproval,
            // Fail-closed: R3/R4 sem canal de confirmação e R5 (allowlistado
            // ou não) sem canal, ou R5 fora da allowlist — nenhuma execução.
            Self::R3 | Self::R4 | Self::R5 => ExecutionDecision::Denied,
        }
    }

    /// Parse a partir de texto livre ("r0", "R3"), para config e logs.
    pub fn de_texto(s: &str) -> Option<Self> {
        let baixo = s.trim().to_ascii_lowercase();
        match baixo.as_str() {
            "r0" => Some(Self::R0),
            "r1" => Some(Self::R1),
            "r2" => Some(Self::R2),
            "r3" => Some(Self::R3),
            "r4" => Some(Self::R4),
            "r5" => Some(Self::R5),
            _ => None,
        }
    }

    /// Forma canônica de exibição ("R3").
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R0 => "R0",
            Self::R1 => "R1",
            Self::R2 => "R2",
            Self::R3 => "R3",
            Self::R4 => "R4",
            Self::R5 => "R5",
        }
    }
}

impl std::fmt::Display for RiskClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O aceite da #1129, literal: "tabela de decisão testada: R0 auto,
    /// R1 policy, R3 confirmation, R5 deny" — com os dois intermédios.
    #[test]
    fn tabela_de_decisao_r0_a_r5() {
        let casos = [
            (RiskClass::R0, ExecutionDecision::Auto),
            (RiskClass::R1, ExecutionDecision::PolicyGated),
            (RiskClass::R2, ExecutionDecision::PolicyRateLimited),
            (RiskClass::R3, ExecutionDecision::HumanConfirmation),
            (RiskClass::R4, ExecutionDecision::ExplicitApproval),
        ];
        for (classe, esperada) in casos {
            assert_eq!(
                classe.decisao(true, false),
                esperada,
                "{classe:?} com canal de confirmação"
            );
        }
        // R5 é deny por padrão — allowlist vazia, nada a fazer.
        assert_eq!(
            RiskClass::R5.decisao(true, false),
            ExecutionDecision::Denied,
            "R5 sem allowlist é deny, mesmo com canal de confirmação"
        );
    }

    /// R5 só deixa de ser Denied quando o OPERADOR o allowlistou — e
    /// mesmo aí não vira Auto: vira approval explícito.
    #[test]
    fn r5_allowlistado_vira_approval_explicito() {
        assert_eq!(
            RiskClass::R5.decisao(true, true),
            ExecutionDecision::ExplicitApproval
        );
    }

    /// Fail-closed: sem canal de confirmação real (ex.: path MCP stateless
    /// full-auto), R3/R4 não podem virar execução silenciosa — espelha o
    /// tier risky do bash sem canal (GAR-187/#1075).
    #[test]
    fn sem_canal_de_confirmacao_r3_e_r4_sao_negadas() {
        for classe in [RiskClass::R3, RiskClass::R4] {
            assert_eq!(
                classe.decisao(false, false),
                ExecutionDecision::Denied,
                "{classe:?} sem canal de confirmação é fail-closed"
            );
        }
        // R5 allowlistado sem canal também cai para Denied: approval
        // explícito que ninguém pode dar não é approval.
        assert_eq!(
            RiskClass::R5.decisao(false, true),
            ExecutionDecision::Denied
        );
        // R0/R1/R2 não dependem do canal.
        assert_eq!(RiskClass::R0.decisao(false, false), ExecutionDecision::Auto);
        assert_eq!(
            RiskClass::R1.decisao(false, false),
            ExecutionDecision::PolicyGated
        );
    }

    /// A ordenação derivada existe para que "R≥1" seja expressável
    /// (`> R0`) — é como o modo `ask` nega execução de risco.
    #[test]
    fn ordenacao_deriva_do_risco() {
        assert!(RiskClass::R0 < RiskClass::R1);
        assert!(RiskClass::R1 < RiskClass::R5);
        assert!(RiskClass::R5 > RiskClass::R4);
    }

    #[test]
    fn parse_e_display_ida_e_volta() {
        for classe in [
            RiskClass::R0,
            RiskClass::R1,
            RiskClass::R2,
            RiskClass::R3,
            RiskClass::R4,
            RiskClass::R5,
        ] {
            assert_eq!(RiskClass::de_texto(classe.as_str()), Some(classe));
            assert_eq!(
                RiskClass::de_texto(classe.as_str().to_lowercase().as_str()),
                Some(classe)
            );
        }
        assert_eq!(RiskClass::de_texto("R6"), None);
        assert_eq!(RiskClass::de_texto("risky"), None);
        assert_eq!(RiskClass::de_texto(""), None);
    }

    /// A forma JSON ("r3") é o contrato que viaja para o inventário do
    /// agente e para logs estruturados.
    #[test]
    fn serde_usa_a_forma_minuscula() {
        let json = serde_json::to_string(&RiskClass::R3).expect("serializa");
        assert_eq!(json, "\"r3\"");
        let volta: RiskClass = serde_json::from_str(&json).expect("desserializa");
        assert_eq!(volta, RiskClass::R3);
    }
}
