//! Nível de aprovação do tier arriscado (#1858 fatia 2).
//!
//! # O que é
//!
//! O runtime tem três camadas de comando: o hard-deny (`safety_gate` —
//! inegociável, roda sempre), o tier arriscado (pede confirmação humana) e o
//! resto. Este knob declara o que o **operador** quer que aconteça com o tier
//! arriscado — e só com ele:
//!
//! - `Always` (default): o comportamento de sempre. Toda ação arriscada pede
//!   confirmação — é o que preserva quem nunca pediu nada.
//! - `NeverConfirm`: pula o **pedido** de confirmação quando existe canal de
//!   confirmação. É a `--yolo`/`--dangerously-skip-permissions` da #1858.
//!
//! # O que o nível NUNCA faz
//!
//! - **Nunca desarma o hard-deny**: o `safety_gate` roda antes de qualquer
//!   coisa e nenhum valor deste enum chega perto dele. `mkfs`/fork bomb
//!   continuam bloqueados com a flag ativa.
//! - **Nunca cria um canal de confirmação**: onde não há humano que possa
//!   aprovar (o caminho stateless do MCP, por exemplo), o fail-closed
//!   continua — um comando arriscado não passa a rodar só porque ninguém
//!   pode ser perguntado. O nível converte "ia perguntar" em "roda"; nunca
//!   converte "bloqueado por falta de canal" em "roda".
//! - **Nunca é o default e nunca persiste por si só**: entra no mundo por um
//!   ato explícito do operador (flag de CLI ou `agent.approval` no config).
//!
//! Por que vive em `garraia-common`: o config (`agent.approval`) e as tools
//! (`garraia-agents`) não dependem um do outro, e ambos precisam do mesmo
//! enum — é o vocabulário compartilhado, como `safety_gate`.

use serde::{Deserialize, Serialize};

/// O que o runtime faz com o tier arriscado de comando/ação.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalLevel {
    /// Default: toda ação arriscada pede confirmação humana. Comportamento
    /// idêntico ao de antes da #1858.
    #[default]
    Always,
    /// Pula o pedido de confirmação (quando há canal). O hard-deny e o
    /// fail-closed de canal ausente continuam valendo.
    NeverConfirm,
}

impl ApprovalLevel {
    /// Este nível dispensa o **pedido** de confirmação?
    ///
    /// O chamador ainda precisa provar que existe canal
    /// (`confirmation_enabled`) — `true` aqui nunca autoriza executar num
    /// runtime fail-closed.
    pub fn dispensa_confirmacao(self) -> bool {
        matches!(self, Self::NeverConfirm)
    }

    /// Rótulo estável para `/status`, logs e o arquivo de auditoria.
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::NeverConfirm => "never-confirm",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O default é o comportamento atual: quem nunca pediu nada não muda.
    #[test]
    fn default_e_always() {
        assert_eq!(ApprovalLevel::default(), ApprovalLevel::Always);
        assert!(!ApprovalLevel::Always.dispensa_confirmacao());
    }

    /// O yolo declara o que faz; o always nunca dispensa.
    #[test]
    fn never_confirm_dispensa_e_always_nao() {
        assert!(ApprovalLevel::NeverConfirm.dispensa_confirmacao());
        assert!(!ApprovalLevel::Always.dispensa_confirmacao());
    }

    /// Serde: kebab-case nos dois sentidos, e seção ausente => default.
    #[test]
    fn serde_kebab_case_e_default() {
        assert_eq!(
            serde_json::from_str::<ApprovalLevel>("\"never-confirm\"").expect("parse"),
            ApprovalLevel::NeverConfirm
        );
        assert_eq!(
            serde_json::to_string(&ApprovalLevel::NeverConfirm).expect("serializa"),
            "\"never-confirm\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalLevel::Always).expect("serializa"),
            "\"always\""
        );
    }

    /// Rótulo bate com a forma serializada — `/status` e o arquivo de
    /// auditoria não podem divergir do que o config aceita.
    #[test]
    fn rotulo_bate_com_a_forma_serializada() {
        for nivel in [ApprovalLevel::Always, ApprovalLevel::NeverConfirm] {
            let serializado = serde_json::to_string(&nivel).expect("serializa");
            assert_eq!(serializado, format!("\"{}\"", nivel.rotulo()));
        }
    }
}
