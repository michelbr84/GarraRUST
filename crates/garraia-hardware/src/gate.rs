//! O gate de execução de hardware — a tabela R0–R5 aplicada a uma capability.
//!
//! Reuso, não paralelo: o [`HardwareGate`] **decide**; quem **executa** é a
//! máquina que já existe no repo. `HumanConfirmation`/`ExplicitApproval`
//! viram o fluxo de confirmação do GAR-187 (`ToolApproval` por impressão
//! digital, o mesmo do tier *risky* do bash); `PolicyGated`/`PolicyRateLimited`
//! deixam a decisão com o `ToolPolicy`/modos do turno. O gate não guarda
//! estado — é uma tabela pura, e o teste é tabela-driven (#1129).

use crate::capability::Capability;
use crate::risk::{ExecutionDecision, RiskClass};
use std::collections::BTreeSet;

/// O gate de hardware.
///
/// Duas configurações, ambas fail-closed por padrão:
///
/// - `r5_allowlist`: capabilities R5 que o OPERADOR declarou executáveis.
///   Vazia por padrão — nenhum actuator industrial/security system roda
///   sem declaração explícita.
/// - `canal_confirmacao`: existe canal real de confirmação humana? Sem ele,
///   R3/R4 são negadas (o mesmo fail-closed do bash sem canal, #1075).
#[derive(Debug, Clone, Default)]
pub struct HardwareGate {
    r5_allowlist: BTreeSet<String>,
    canal_confirmacao: bool,
}

impl HardwareGate {
    /// O gate padrão: allowlist vazia, canal de confirmação **ligado**.
    ///
    /// Default ligado porque o runtime do gateway tem canal de confirmação
    /// (o fluxo GAR-187 do bash); os paths sem canal desligam
    /// explicitamente (`sem_canal_confirmacao`), que é o lado certo para
    /// errar.
    pub fn new() -> Self {
        Self {
            r5_allowlist: BTreeSet::new(),
            canal_confirmacao: true,
        }
    }

    /// O gate dos paths sem canal de confirmação real (MCP stateless
    /// full-auto, heartbeats): R3/R4/R5-allowlistado viram `Denied`.
    pub fn sem_canal_confirmacao() -> Self {
        Self {
            r5_allowlist: BTreeSet::new(),
            canal_confirmacao: false,
        }
    }

    /// Allowlist de capabilities R5, declarada pelo operador.
    pub fn com_r5_allowlist(
        mut self,
        capabilities: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        for cap in capabilities {
            self.r5_allowlist.insert(cap.as_ref().trim().to_string());
        }
        self
    }

    /// A decisão para uma capability.
    pub fn decide(&self, cap: &Capability) -> ExecutionDecision {
        self.decide_class(cap.risk, &cap.name)
    }

    /// A decisão para uma capability, sabendo se o Operador a allowlistou
    /// em R5 (a integração passa o conteúdo do gate).
    pub fn decide_class(&self, classe: RiskClass, capability_name: &str) -> ExecutionDecision {
        let na_allowlist = self.r5_allowlist.contains(capability_name);
        classe.decisao(self.canal_confirmacao, na_allowlist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tabela do #1129 via gate, para uma capability concreta — o mesmo
    /// aceite do risco, agora pela porta que as tools usam.
    #[test]
    fn gate_decide_pela_tabela() {
        let gate = HardwareGate::new();
        let casos = [
            (RiskClass::R0, "temperature", ExecutionDecision::Auto),
            (RiskClass::R1, "light_on", ExecutionDecision::PolicyGated),
            (RiskClass::R2, "scene", ExecutionDecision::PolicyRateLimited),
            (
                RiskClass::R3,
                "door_unlock",
                ExecutionDecision::HumanConfirmation,
            ),
            (
                RiskClass::R4,
                "robot_motion",
                ExecutionDecision::ExplicitApproval,
            ),
            (RiskClass::R5, "industrial_valve", ExecutionDecision::Denied),
        ];
        for (classe, nome, esperada) in casos {
            let cap = exemplar(classe, nome);
            assert_eq!(gate.decide(&cap), esperada, "{nome} ({classe})");
        }
    }

    /// R5 allowlistado sai de Denied — mas vira ExplicitApproval, nunca Auto.
    #[test]
    fn gate_honra_a_allowlist_r5() {
        let gate = HardwareGate::new().com_r5_allowlist(["industrial_valve"]);
        let cap = exemplar(RiskClass::R5, "industrial_valve");
        assert_eq!(gate.decide(&cap), ExecutionDecision::ExplicitApproval);
        // Outra capability R5 continua Denied.
        let outra = exemplar(RiskClass::R5, "security_system");
        assert_eq!(gate.decide(&outra), ExecutionDecision::Denied);
    }

    /// Sem canal de confirmação, o gate fecha: R3/R4 denied e R5-allowlistado
    /// também (approval que ninguém pode dar não é approval).
    #[test]
    fn gate_sem_canal_e_fail_closed() {
        let gate = HardwareGate::sem_canal_confirmacao().com_r5_allowlist(["industrial_valve"]);
        let casos = [
            (RiskClass::R3, "door_unlock", ExecutionDecision::Denied),
            (RiskClass::R4, "robot_motion", ExecutionDecision::Denied),
            (RiskClass::R5, "industrial_valve", ExecutionDecision::Denied),
            (RiskClass::R0, "temperature", ExecutionDecision::Auto),
            (RiskClass::R1, "light_on", ExecutionDecision::PolicyGated),
        ];
        for (classe, nome, esperada) in casos {
            let cap = exemplar(classe, nome);
            assert_eq!(gate.decide(&cap), esperada, "{nome} sem canal");
        }
    }

    fn exemplar(classe: RiskClass, nome: &str) -> Capability {
        if classe == RiskClass::R0 {
            Capability::leitura(nome, None)
        } else {
            Capability::acao(nome, classe, None).expect("cap válida")
        }
    }
}
