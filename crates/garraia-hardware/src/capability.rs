//! `Capability` — o que um dispositivo sabe fazer, com o risco embutido.
//!
//! A capability é a unidade da descoberta: o agente enxerga
//! `LivingRoomLight ├ power ├ brightness └ color` e cada linha carrega o
//! seu risk class (#1129) e um schema leve de argumentos. O risco é
//! **dado no construtor** e validado como invariante — uma capability
//! read-only com risco R1 é um bug de adapter, e o crate a recusa na
//! entrada em vez de deixar a confusão chegar ao gate.

use crate::risk::RiskClass;
use serde::{Deserialize, Serialize};

/// Uma ação ou leitura que um [`crate::Device`] expõe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    /// Nome curto e estável ("power", "temperature", "door_unlock").
    pub name: String,
    /// O risco de executar esta capability (#1129).
    pub risk: RiskClass,
    /// Verdadeiro quando a capability não muda o mundo — e aí o risco é R0,
    /// por construção.
    pub read_only: bool,
    /// JSON Schema leve dos argumentos de `execute` (objeto JSON Schema
    /// truncado: só `type`/`properties`/`required` são garantidos). `None`
    /// para capabilities sem argumentos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_schema: Option<serde_json::Value>,
}

impl Capability {
    /// Uma capability de leitura (R0 por construção).
    pub fn leitura(name: impl Into<String>, args_schema: Option<serde_json::Value>) -> Self {
        Self {
            name: name.into(),
            risk: RiskClass::R0,
            read_only: true,
            args_schema,
        }
    }

    /// Uma capability de ação com risco declarado.
    ///
    /// `read_only` sai `false` por construção — contorno de leitura↔R0 só é
    /// alcançável montando a struct à mão (campos públicos), e `validar`
    /// é quem o pega. O construtor recusa (não pânico) risco R0, que é
    /// exclusivo de capabilities read_only (use [`Capability::leitura`]).
    pub fn acao(
        name: impl Into<String>,
        risk: RiskClass,
        args_schema: Option<serde_json::Value>,
    ) -> Result<Self, crate::error::HardwareError> {
        let name = name.into();
        if risk == RiskClass::R0 {
            return Err(crate::error::HardwareError::CapabilityInvalida {
                nome: name,
                classe: risk.as_str().to_string(),
                motivo:
                    "risco R0 é exclusivo de capabilities read_only (use `Capability::leitura`)"
                        .to_string(),
            });
        }
        Ok(Self {
            name,
            risk,
            read_only: false,
            args_schema,
        })
    }

    /// Checa a correspondência leitura↔R0 numa capability já construída.
    ///
    /// Os construtores garantem a invariante, mas os campos são públicos
    /// (é o contrato que viaja em JSON) — adapters que montam capabilities
    /// à mão podem violá-la sem perceber. `validar` é o teste que a tool
    /// e o registry podem rodar na fronteira, e é o erro que o agente vê.
    pub fn validar(&self) -> Result<(), crate::error::HardwareError> {
        let coerente = match self.risk {
            RiskClass::R0 => self.read_only,
            _ => !self.read_only,
        };
        if coerente {
            return Ok(());
        }
        Err(crate::error::HardwareError::CapabilityInvalida {
            nome: self.name.clone(),
            classe: self.risk.as_str().to_string(),
            motivo: if self.read_only {
                "read_only exige risco R0".to_string()
            } else {
                "risco R0 exige read_only".to_string()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O par leitura↔R0 é a invariante que o gate confia: o construtor
    /// garante, então `gate.decide` não re-checa.
    #[test]
    fn leitura_e_sempre_r0() {
        let cap = Capability::leitura("temperature", None);
        assert_eq!(cap.risk, RiskClass::R0);
        assert!(cap.read_only);
        assert_eq!(cap.name, "temperature");
    }

    #[test]
    fn acao_com_risco_declarado() {
        let cap = Capability::acao("door_unlock", RiskClass::R3, None).expect("válido");
        assert!(!cap.read_only);
        assert_eq!(cap.risk, RiskClass::R3);
    }

    #[test]
    fn acao_recusa_r0_sem_read_only() {
        let err = Capability::acao("power", RiskClass::R0, None)
            .expect_err("R0 sem read_only é inválido");
        assert!(err.to_string().contains("R0"), "erro cita o risco: {err}");
    }

    /// A capability viaja para o inventário do agente em JSON.
    /// O teste de fronteira que o construtor torna redundante para quem
    /// usa `leitura`/`acao` — mas que pega o adapter que montou a
    /// capability à mão (campos públicos = contrato JSON).
    #[test]
    fn validar_aceita_construtores_e_pegam_contorno() {
        // Construtores sempre passam.
        assert!(Capability::leitura("temperature", None).validar().is_ok());
        assert!(
            Capability::acao("power", RiskClass::R1, None)
                .expect("válido")
                .validar()
                .is_ok()
        );

        // Contorno: read_only com R3 (construído à mão, sem construtor).
        let mut cap = Capability::leitura("door_unlock", None);
        cap.risk = RiskClass::R3;
        let err = cap.validar().expect_err("read_only com R3 é inválido");
        assert!(err.to_string().contains("R3"), "erro cita o risco: {err}");

        // Contorno inverso: R0 sem read_only.
        let mut cap = Capability::leitura("temperature", None);
        cap.read_only = false;
        let err = cap.validar().expect_err("R0 sem read_only é inválido");
        assert!(err.to_string().contains("R0"), "erro cita o risco: {err}");
    }

    #[test]
    fn serde_ida_e_volta() {
        let cap = Capability::acao(
            "light_on",
            RiskClass::R1,
            Some(serde_json::json!({
                "type": "object",
                "properties": { "on": { "type": "boolean" } },
                "required": ["on"]
            })),
        )
        .expect("válido");
        let json = serde_json::to_string(&cap).expect("serializa");
        let volta: Capability = serde_json::from_str(&json).expect("desserializa");
        assert_eq!(volta, cap);
        // Cap sem schema não polui o JSON.
        let sem_schema = Capability::leitura("temperature", None);
        let json = serde_json::to_string(&sem_schema).expect("serializa");
        assert!(
            !json.contains("args_schema"),
            "sem schema não serializa o campo: {json}"
        );
    }
}
