//! `MockDevice` — o dispositivo in-memory dos testes e fixtures.
//!
//! Feature `mock-device` (default-on, mesmo padrão do `testing-provider`
//! do garraia-embeddings). Determinístico: `execute` grava no estado e
//! devolve o que foi pedido, `read` devolve o estado — nada de relógio,
//! nada de rede. Registra as chamadas (`reads`/`executed`) para os testes
//! afirmarem sobre elas.
//!
//! Fábricas nomeadas cobrem o north star do epic: `sensor_temperatura` (o
//! exemplo E2E da #1125) e `lampada_sala` (R1). Capabilities R2–R5 entram
//! via `com_cap` nos testes da tabela de decisão.

use crate::capability::Capability;
use crate::device::Device;
use crate::risk::RiskClass;
use crate::{HardwareError, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

/// Dispositivo in-memory, determinístico, observável.
#[derive(Default)]
pub struct MockDevice {
    id: String,
    caps: Vec<Capability>,
    /// Estado atual por capability name.
    state: Mutex<HashMap<String, Value>>,
    /// Log de leituras, em ordem.
    reads: Mutex<Vec<String>>,
    /// Log de execuções `(capability, args)`, em ordem.
    executes: Mutex<Vec<(String, Value)>>,
}

impl MockDevice {
    /// Dispositivo com id e capabilities dadas.
    pub fn new(id: impl Into<String>, caps: Vec<Capability>) -> Self {
        Self {
            id: id.into(),
            caps,
            ..Default::default()
        }
    }

    /// O sensor do exemplo E2E da #1125: `TemperatureSensor └ temperature`.
    /// Estado inicial determinístico.
    pub fn sensor_temperatura() -> Self {
        Self::new(
            "sensor-sala",
            vec![Capability::leitura("temperature", None)],
        )
        .com_estado("temperature", serde_json::json!({ "celsius": 23.0 }))
    }

    /// A lampada R1 do north star: `power` + `brightness`.
    pub fn lampada_sala() -> Self {
        Self::new(
            "lampada-sala",
            vec![
                Capability::acao("power", RiskClass::R1, None).expect("power é R1"),
                Capability::acao(
                    "brightness",
                    RiskClass::R1,
                    Some(serde_json::json!({
                        "type": "object",
                        "properties": { "percent": { "type": "integer", "minimum": 0, "maximum": 100 } },
                        "required": ["percent"]
                    })),
                )
                .expect("brightness é R1"),
            ],
        )
    }

    /// Builder: acrescenta uma capability.
    pub fn com_cap(mut self, cap: Capability) -> Self {
        self.caps.push(cap);
        self
    }

    /// Builder: pré-define o estado de uma capability (o que `read` devolve).
    ///
    /// `self` por valor e sem `mut`: o estado vive atrás de `Mutex`, então a
    /// mutação acontece pelo guard e não por `&mut self`.
    pub fn com_estado(self, capability: &str, valor: Value) -> Self {
        self.state
            .lock()
            .expect("mutex do mock")
            .insert(capability.to_string(), valor);
        self
    }

    /// Capabilities registradas no log de leituras.
    pub fn reads(&self) -> Vec<String> {
        self.reads.lock().expect("mutex do mock").clone()
    }

    /// Execuções registradas `(capability, args)`.
    pub fn executed(&self) -> Vec<(String, Value)> {
        self.executes.lock().expect("mutex do mock").clone()
    }

    fn capability(&self, name: &str) -> Result<&Capability> {
        self.caps.iter().find(|c| c.name == name).ok_or_else(|| {
            HardwareError::CapabilityDesconhecida {
                dispositivo: self.id.clone(),
                capability: name.to_string(),
            }
        })
    }
}

#[async_trait]
impl Device for MockDevice {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.caps.clone()
    }

    async fn read(&self, capability: &str) -> Result<Value> {
        self.capability(capability)?;
        self.reads
            .lock()
            .expect("mutex do mock")
            .push(capability.to_string());
        Ok(self
            .state
            .lock()
            .expect("mutex do mock")
            .get(capability)
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn execute(&self, capability: &str, args: Value) -> Result<Value> {
        self.capability(capability)?;
        self.executes
            .lock()
            .expect("mutex do mock")
            .push((capability.to_string(), args.clone()));
        self.state
            .lock()
            .expect("mutex do mock")
            .insert(capability.to_string(), args.clone());
        Ok(json!({ "device": self.id, "capability": capability, "ok": true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::resumo;

    /// O exemplo E2E da #1125: o agente lista `TemperatureSensor` e lê
    /// `temperature` — determinístico, sem hardware.
    #[tokio::test]
    async fn sensor_de_temperatura_lista_e_le() {
        let sensor = MockDevice::sensor_temperatura();
        let resumo = resumo(&sensor);
        assert_eq!(resumo.id, "sensor-sala");
        assert_eq!(resumo.capabilities.len(), 1);
        assert_eq!(resumo.capabilities[0].name, "temperature");
        assert_eq!(resumo.capabilities[0].risk, RiskClass::R0);

        let valor = sensor.read("temperature").await.expect("lê temperatura");
        assert_eq!(valor, serde_json::json!({ "celsius": 23.0 }));
        assert_eq!(sensor.reads(), vec!["temperature".to_string()]);
    }

    /// `execute` muda o estado e registra — é como os testes afirmam que a
    /// ação chegou ao dispositivo.
    #[tokio::test]
    async fn execute_grava_estado_e_log() {
        let lampada = MockDevice::lampada_sala();
        let saida = lampada
            .execute("power", serde_json::json!({ "on": true }))
            .await
            .expect("executa power");
        assert_eq!(saida["ok"], json!(true));

        // O novo estado é o que `read` passa a devolver.
        let estado = lampada.read("power").await.expect("lê power");
        assert_eq!(estado, serde_json::json!({ "on": true }));
        assert_eq!(
            lampada.executed(),
            vec![("power".to_string(), serde_json::json!({ "on": true }))]
        );
    }

    /// Capability fora da lista do dispositivo é erro tipado, não pânico.
    #[tokio::test]
    async fn capability_desconhecida_e_erro() {
        let sensor = MockDevice::sensor_temperatura();
        let err = sensor
            .read("humidity")
            .await
            .expect_err("humidity não existe");
        assert!(err.to_string().contains("sensor-sala"), "{err}");
        assert!(err.to_string().contains("humidity"), "{err}");
    }

    /// O mock cobre a tabela toda: fixtures R2–R5 para os testes do gate.
    #[test]
    fn fixtures_cobrem_r0_a_r5() {
        let device = MockDevice::new(
            "risco-completo",
            vec![
                Capability::leitura("door_status", None),
                Capability::acao("fan_speed", RiskClass::R1, None).expect("R1"),
                Capability::acao("cover_position", RiskClass::R2, None).expect("R2"),
                Capability::acao("door_unlock", RiskClass::R3, None).expect("R3"),
                Capability::acao("robot_motion", RiskClass::R4, None).expect("R4"),
                Capability::acao("industrial_valve", RiskClass::R5, None).expect("R5"),
            ],
        );
        let riscos: Vec<&str> = device
            .capabilities()
            .iter()
            .map(|c| c.risk.as_str())
            .collect();
        assert_eq!(riscos, vec!["R0", "R1", "R2", "R3", "R4", "R5"]);
    }
}
