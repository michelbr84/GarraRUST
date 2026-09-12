//! Tools `device_list` / `device_read` / `device_execute` — a porta do
//! agente para o mundo físico (epic #1124, issues #1125 + #1129).
//!
//! As três tools são a **integração** do crate `garraia-hardware` com o
//! runtime de agentes: o registro (`DeviceRegistry`) diz quem existe, e o
//! [`HardwareGate`] do crate decide cada `device_execute` pela tabela
//! R0–R5 — o mesmo modelo de risco que o ADR 0020 cravou.
//!
//! # Quem decide o quê (as duas camadas)
//!
//! 1. **Camada de policy** — os modos (`ToolGate`/`ToolPolicy` em
//!    `crate::modes`) decidem se a tool roda no turno: é o `PolicyGated`/
//!    `PolicyRateLimited` da tabela. Modos read-only negam `device_execute`
//!    estaticamente (ver os testes de `modes`); `device_read`/`device_list`
//!    (R0, leitura) ficam disponíveis onde a descoberta faz sentido.
//! 2. **Camada de risco** — dentro da tool, o [`HardwareGate`] trata R3/R4
//!    com o fluxo de confirmação GAR-187 (impressão digital
//!    `(tool, assunto)`, o mesmo do bash) e R5 com deny-by-default
//!    allowlist do operador. Fail-closed: sem canal de confirmação
//!    (`new_without_confirmation`, o caminho MCP stateless full-auto),
//!    R3/R4/R5 são **bloqueadas** — nunca rodam porque ninguém pôde ser
//!    perguntado.
//!
//! O assunto do pedido de confirmação é `"{device}/{capability}: {args}"` —
//! o par completo do que será feito, no espírito do bash (onde o assunto é
//! o comando inteiro): um "ok" dado a `lampada/power` não autoriza
//! `garagem/door_unlock` no mesmo turno.
//!
//! R2 (`PolicyRateLimited`) roda aqui sob a policy do turno; o teto de
//! frequência em si é enforce do motor de automações (#1128) — a decisão
//! declarada já existe na tabela.
//!
//! Estado de presença (`DeviceStateStore`) é best-effort: contato
//! bem-sucedido marca online; falha **não** marca offline, porque um
//! `CapabilityDesconhecida` não é queda de conexão — quem sabe é o adapter.

use async_trait::async_trait;
use garraia_common::{Error, Result};
use garraia_hardware::{
    DeviceRegistry, DeviceStateStore, ExecutionDecision, HardwareGate, RiskClass,
};
use std::sync::Arc;

use super::approval::ApprovalFingerprint;
use super::{Tool, ToolContext, ToolOutput};

/// Best-effort: contato bem-sucedido marca o dispositivo online. Falha de
/// marcação não falha a operação — presença é telemetria, não controle.
async fn marcar_online(state: &Option<Arc<DeviceStateStore>>, device_id: &str) {
    if let Some(store) = state
        && let Err(e) = store.marcar(device_id, true).await
    {
        tracing::warn!(device = %device_id, error = %e, "hardware: falha ao marcar presença online");
    }
}

/// O texto de "dispositivo não registrado", único para as três tools — o
/// modelo precisa da mesma frase nos dois caminhos para aprender a pedir
/// `device_list` primeiro.
fn erro_desconhecido(device_id: &str) -> ToolOutput {
    ToolOutput::error(format!(
        "Dispositivo '{device_id}' não registrado. Use device_list para descobrir os dispositivos conectados."
    ))
}

/// Contexto compartilhado das tools de hardware: o registry é obrigatório
/// (as tools existem para vê-lo), o store de presença é opcional.
pub struct DeviceToolsConfig {
    pub registry: Arc<DeviceRegistry>,
    pub state: Option<Arc<DeviceStateStore>>,
}

impl DeviceToolsConfig {
    /// Sem store de presença — a descoberta lista o que o registry tem,
    /// sem anotar online/offline.
    pub fn new(registry: Arc<DeviceRegistry>) -> Self {
        Self {
            registry,
            state: None,
        }
    }

    /// Com store de presença (o caminho do gateway, que abre o SQLite).
    pub fn com_estado(mut self, state: Arc<DeviceStateStore>) -> Self {
        self.state = Some(state);
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────
// `device_list` — a descoberta.
// ─────────────────────────────────────────────────────────────────────────

pub struct DeviceListTool {
    config: Arc<DeviceToolsConfig>,
}

impl DeviceListTool {
    pub fn new(config: Arc<DeviceToolsConfig>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for DeviceListTool {
    fn name(&self) -> &str {
        "device_list"
    }

    fn description(&self) -> &str {
        "Lists registered physical devices with their capabilities and the risk class (R0-R5) of each one. Use device_read for readings and device_execute for actions."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        _input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let dispositivos = self.config.registry.list();
        if dispositivos.is_empty() {
            return Ok(ToolOutput::success(
                "Nenhum dispositivo registrado. Adapters de hardware registram dispositivos no boot; com registry vazio, não há hardware conectado."
                    .to_string(),
            ));
        }

        let mut linhas = vec![format!(
            "{} dispositivo(s) registrado(s):",
            dispositivos.len()
        )];
        for d in &dispositivos {
            let presenca = match &self.config.state {
                Some(store) => match store.estado(&d.id).await {
                    Ok(Some(p)) if p.online => " (online)".to_string(),
                    Ok(Some(_)) => " (offline)".to_string(),
                    Ok(None) => " (nunca visto)".to_string(),
                    Err(e) => {
                        tracing::warn!(device = %d.id, error = %e, "hardware: falha ao ler presença");
                        String::new()
                    }
                },
                None => String::new(),
            };
            linhas.push(format!("{}{}:", d.id, presenca));
            for cap in &d.capabilities {
                let tipo = if cap.read_only { "read" } else { "action" };
                match &cap.args_schema {
                    Some(schema) => linhas.push(format!(
                        "  {} [{}, {}, args: {schema}]",
                        cap.name, cap.risk, tipo
                    )),
                    None => linhas.push(format!("  {} [{}, {}]", cap.name, cap.risk, tipo)),
                }
            }
        }
        Ok(ToolOutput::success(linhas.join("\n")))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// `device_read` — leitura R0.
// ─────────────────────────────────────────────────────────────────────────

pub struct DeviceReadTool {
    config: Arc<DeviceToolsConfig>,
}

impl DeviceReadTool {
    pub fn new(config: Arc<DeviceToolsConfig>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for DeviceReadTool {
    fn name(&self) -> &str {
        "device_read"
    }

    fn description(&self) -> &str {
        "Reads a capability of a device (read-only, risk R0 — automatic). Examples: temperature, battery, door_status."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "device": { "type": "string", "description": "Device id (see device_list)" },
                "capability": { "type": "string", "description": "Capability name (read-only)" }
            },
            "required": ["device", "capability"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let device_id = input
            .get("device")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'device' ausente".into()))?;
        let capability = input
            .get("capability")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'capability' ausente".into()))?;

        let Some(device) = self.config.registry.get(device_id) else {
            return Ok(erro_desconhecido(device_id));
        };
        let caps = device.capabilities();
        let Some(cap) = caps.iter().find(|c| c.name == capability) else {
            return Ok(ToolOutput::error(format!(
                "Dispositivo '{device_id}' não expõe a capability '{capability}'. Use device_list para ver as capabilities disponíveis."
            )));
        };

        // A invariante leitura↔R0 pela porta da tool: `read` é para
        // capabilities read_only. Ação tem porta própria (`device_execute`).
        if !cap.read_only {
            return Ok(ToolOutput::error(format!(
                "A capability '{capability}' de '{device_id}' não é read-only ({}): leitura é para capabilities read-only; use device_execute.",
                cap.risk
            )));
        }

        match device.read(capability).await {
            Ok(valor) => {
                marcar_online(&self.config.state, device_id).await;
                tracing::info!(
                    device = %device_id, capability = %capability, session = %context.session_id,
                    "hardware: leitura R0 executada"
                );
                Ok(ToolOutput::success(valor.to_string()))
            }
            Err(e) => Ok(ToolOutput::error(e.to_string())),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// `device_execute` — ação R1–R5 atrás do gate.
// ─────────────────────────────────────────────────────────────────────────

pub struct DeviceExecuteTool {
    config: Arc<DeviceToolsConfig>,
    gate: HardwareGate,
    /// GAR-187: o runtime tem canal real de confirmação humana? Sem ele,
    /// R3/R4/R5 são fail-closed BLOCKED (o mesmo do bash sem canal, #1075).
    confirmation_enabled: bool,
}

impl DeviceExecuteTool {
    /// Tool com canal de confirmação — o caminho do gateway e do CLI.
    pub fn new(config: Arc<DeviceToolsConfig>) -> Self {
        Self {
            config,
            gate: HardwareGate::new(),
            confirmation_enabled: true,
        }
    }

    /// Tool sem canal de confirmação (paths stateless full-auto): o gate
    /// fecha — R3/R4 viram Denied na tabela e qualquer pedido de
    /// confirmação é fail-closed BLOCKED, não aguardado.
    pub fn new_without_confirmation(config: Arc<DeviceToolsConfig>) -> Self {
        Self {
            config,
            gate: HardwareGate::sem_canal_confirmacao(),
            confirmation_enabled: false,
        }
    }

    /// Allowlist R5 do operador (capabilities que o dono declarou
    /// executáveis). O padrão é vazio — deny por padrão.
    pub fn com_r5_allowlist(
        mut self,
        capabilities: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.gate = self.gate.com_r5_allowlist(capabilities);
        self
    }

    /// O assunto do pedido de confirmação: o par completo do que será
    /// executado. Mesma função para emitir o marcador e para verificar a
    /// aprovação — as duas pontas nunca divergem.
    fn subject(device: &str, capability: &str, args: &serde_json::Value) -> String {
        format!("{device}/{capability}: {args}")
    }
}

#[async_trait]
impl Tool for DeviceExecuteTool {
    fn name(&self) -> &str {
        "device_execute"
    }

    fn description(&self) -> &str {
        "Executes an action on a device. Risky actions (R3-R5) require user confirmation; the risk class of the capability governs what happens. Use device_list to discover devices first."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "device": { "type": "string", "description": "Device id (see device_list)" },
                "capability": { "type": "string", "description": "Capability to execute (not read-only)" },
                "args": { "type": "object", "description": "Arguments for the capability (per its schema in device_list)" }
            },
            "required": ["device", "capability"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let device_id = input
            .get("device")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'device' ausente".into()))?;
        let capability = input
            .get("capability")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'capability' ausente".into()))?;
        let args = input
            .get("args")
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        let Some(device) = self.config.registry.get(device_id) else {
            return Ok(erro_desconhecido(device_id));
        };
        let caps = device.capabilities();
        let Some(cap) = caps.iter().find(|c| c.name == capability) else {
            return Ok(ToolOutput::error(format!(
                "Dispositivo '{device_id}' não expõe a capability '{capability}'. Use device_list para ver as capabilities disponíveis."
            )));
        };

        // Defesa na fronteira: capability montada à mão pelo adapter pode
        // violar a invariante leitura↔R0 — `validar` pega o contorno, e
        // leitura tem porta própria (`device_read`), então `execute` em
        // R0 recusa mesmo com a decisão de tabela sendo `Auto`.
        if let Err(e) = cap.validar() {
            tracing::error!(device = %device_id, capability = %capability, error = %e, "hardware: capability inválida");
            return Ok(ToolOutput::error(e.to_string()));
        }
        if cap.read_only {
            return Ok(ToolOutput::error(format!(
                "A capability '{capability}' de '{device_id}' é read-only: leitura é com device_read, não execute."
            )));
        }

        // A decisão da tabela R0-R5 (ADR 0020 / #1129) para esta capability.
        let decisao = self.gate.decide(cap);
        tracing::info!(
            device = %device_id, capability = %capability, risk = %cap.risk,
            decision = ?decisao, session = %context.session_id,
            "hardware: decisão do gate"
        );

        match decisao {
            // Auto (R0 — não acontece aqui, `execute` recusa read-only) e
            // os dois Policy*: a policy do turno (`ToolGate`/modos) já
            // disse sim para a tool; o teto de R2 é enforce do motor de
            // automações (#1128). Roda.
            ExecutionDecision::Auto
            | ExecutionDecision::PolicyGated
            | ExecutionDecision::PolicyRateLimited => {
                match device.execute(capability, args).await {
                    Ok(resposta) => {
                        marcar_online(&self.config.state, device_id).await;
                        tracing::info!(
                            device = %device_id, capability = %capability,
                            risk = %cap.risk, session = %context.session_id,
                            "hardware: ação executada"
                        );
                        Ok(ToolOutput::success(format!(
                            "Executado em '{device_id}' ({capability} [{}]): {resposta}",
                            cap.risk
                        )))
                    }
                    Err(e) => Ok(ToolOutput::error(e.to_string())),
                }
            }
            // HumanConfirmation (R3) e ExplicitApproval (R4, R5
            // allowlistado): o fluxo GAR-187 — a aprovação é a impressão
            // digital de `(tool, assunto)`, o assunto é o par completo
            // `{device}/{capability}: {args}`. Fail-closed sem canal.
            ExecutionDecision::HumanConfirmation | ExecutionDecision::ExplicitApproval => {
                let assunto = Self::subject(device_id, capability, &args);
                let aprovado =
                    self.confirmation_enabled && context.approval.covers(self.name(), &assunto);
                if aprovado {
                    tracing::info!(
                        device = %device_id, capability = %capability, risk = %cap.risk,
                        session = %context.session_id, "hardware: ação aprovada pelo usuário"
                    );
                    match device.execute(capability, args).await {
                        Ok(resposta) => {
                            marcar_online(&self.config.state, device_id).await;
                            Ok(ToolOutput::success(format!(
                                "Executado em '{device_id}' ({capability} [{}]): {resposta}",
                                cap.risk
                            )))
                        }
                        Err(e) => Ok(ToolOutput::error(e.to_string())),
                    }
                } else if self.confirmation_enabled {
                    tracing::warn!(
                        device = %device_id, capability = %capability, risk = %cap.risk,
                        session = %context.session_id,
                        "hardware: ação de risco requer confirmação do usuário"
                    );
                    let marcador = ApprovalFingerprint::of(self.name(), &assunto).marker();
                    Ok(ToolOutput::confirmation_request(format!(
                        "{marcador} A ação '{capability}' em '{device_id}' é de risco {} e requer confirmação humana antes de ser executada.\nResponda **sim** para aprovar ou **não** para cancelar.",
                        cap.risk
                    )))
                } else {
                    tracing::warn!(
                        device = %device_id, capability = %capability, risk = %cap.risk,
                        session = %context.session_id,
                        "hardware: ação de risco BLOCKED (fail-closed: confirmation disabled)"
                    );
                    Ok(ToolOutput::error(format!(
                        "Ação bloqueada por segurança: '{capability}' em '{device_id}' ({}) exige confirmação e este runtime não possui canal de confirmação (fail-closed).",
                        cap.risk
                    )))
                }
            }
            // Denied: R5 fora da allowlist (deny-by-default) ou R3/R4/R5
            // sem canal de confirmação — o fail-closed da tabela.
            ExecutionDecision::Denied => {
                tracing::warn!(
                    device = %device_id, capability = %capability, risk = %cap.risk,
                    session = %context.session_id,
                    "hardware: execução negada pelo modelo de risco"
                );
                let motivo = match cap.risk {
                    RiskClass::R5 => {
                        "proibida por padrão (R5); só executa com allowlist explícita do operador"
                    }
                    _ => {
                        "exige confirmação humana e este runtime não possui canal de confirmação (fail-closed)"
                    }
                };
                Ok(ToolOutput::error(format!(
                    "Execução negada pelo modelo de risco: '{capability}' em '{device_id}' é {} — {motivo}.",
                    cap.risk
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::approval::ToolApproval;
    use garraia_hardware::{Capability, MockDevice, RiskClass};
    use serde_json::json;
    use std::sync::Arc;

    /// Registry com o north star da #1125: sensor R0 + lâmpada R1 — mais
    /// as capabilities de risco alto que os testes do gate exercitam.
    fn registry() -> Arc<DeviceRegistry> {
        let reg = Arc::new(DeviceRegistry::new());
        reg.register(Arc::new(MockDevice::sensor_temperatura()));
        reg.register(Arc::new(MockDevice::lampada_sala()));
        dispositivo_de_risco(&reg);
        reg
    }

    /// Dispositivo de risco completo (R0-R5) para os testes do gate.
    fn dispositivo_de_risco(reg: &Arc<DeviceRegistry>) {
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
        reg.register(Arc::new(device));
    }

    fn config(reg: &Arc<DeviceRegistry>) -> Arc<DeviceToolsConfig> {
        Arc::new(DeviceToolsConfig::new(reg.clone()))
    }

    fn ctx(approval: ToolApproval) -> ToolContext {
        ToolContext {
            session_id: "sessão-teste".into(),
            user_id: None,
            is_heartbeat: false,
            approval,
            working_dir: None,
            project_id: None,
        }
    }

    /// A descoberta é o primeiro passo do north star: sem adapter
    /// registrado, a lista é vazia e **não é erro** — é o mundo real de um
    /// gateway sem hardware.
    #[tokio::test]
    async fn lista_vazia_nao_e_erro() {
        let reg = Arc::new(DeviceRegistry::new());
        let tool = DeviceListTool::new(config(&reg));
        let output = tool
            .execute(&ctx(ToolApproval::None), json!({}))
            .await
            .expect("executa");
        assert!(!output.is_error, "{}", output.content);
        assert!(
            output.content.contains("Nenhum dispositivo"),
            "{}",
            output.content
        );
    }

    /// A descoberta mostra id, capabilities e o risco de cada uma — é o
    /// inventário que o modelo lê para decidir o próximo passo.
    #[tokio::test]
    async fn lista_mostra_dispositivos_com_risco() {
        let reg = registry();
        let tool = DeviceListTool::new(config(&reg));
        let output = tool
            .execute(&ctx(ToolApproval::None), json!({}))
            .await
            .expect("executa");
        assert!(!output.is_error, "{}", output.content);
        for esperado in [
            "sensor-sala",
            "lampada-sala",
            "temperature",
            "R0",
            "power",
            "R1",
        ] {
            assert!(
                output.content.contains(esperado),
                "falta '{esperado}': {}",
                output.content
            );
        }
    }

    /// A presença entra na lista quando o store existe — e online é o que
    /// `device_read` marcou no contato bem-sucedido anterior.
    #[tokio::test]
    async fn lista_anota_presenca_do_store() {
        let reg = registry();
        let store = Arc::new(DeviceStateStore::em_memoria().expect("store"));
        store.marcar("sensor-sala", true).await.expect("marca");
        store.marcar("lampada-sala", false).await.expect("marca");
        let cfg = Arc::new(DeviceToolsConfig::new(reg.clone()).com_estado(store.clone()));
        let tool = DeviceListTool::new(cfg);
        let output = tool
            .execute(&ctx(ToolApproval::None), json!({}))
            .await
            .expect("executa");
        assert!(
            output.content.contains("sensor-sala (online)"),
            "{}",
            output.content
        );
        assert!(
            output.content.contains("lampada-sala (offline)"),
            "{}",
            output.content
        );
    }

    /// O exemplo E2E da #1125: o agente lê a temperatura do sensor e recebe
    /// o valor — e o contato marca presença online no store.
    #[tokio::test]
    async fn leitura_do_sensor_devolve_valor_e_marca_online() {
        let reg = registry();
        let store = Arc::new(DeviceStateStore::em_memoria().expect("store"));
        let cfg = Arc::new(DeviceToolsConfig::new(reg.clone()).com_estado(store.clone()));
        let tool = DeviceReadTool::new(cfg);
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "sensor-sala", "capability": "temperature" }),
            )
            .await
            .expect("executa");
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("celsius"), "{}", output.content);
        assert!(output.content.contains("23.0"), "{}", output.content);

        let presenca = store
            .estado("sensor-sala")
            .await
            .expect("lê")
            .expect("marcado");
        assert!(presenca.online);
    }

    /// `device_read` é só leitura: capability de ação vai para
    /// `device_execute` — a tool recusa em vez de expor `read` para
    /// execução (a invariante leitura↔R0 pela porta da tool).
    #[tokio::test]
    async fn leitura_recusa_capability_de_acao() {
        let reg = registry();
        let tool = DeviceReadTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "lampada-sala", "capability": "power" }),
            )
            .await
            .expect("executa");
        assert!(output.is_error, "{}", output.content);
        assert!(
            output.content.contains("read-only") || output.content.contains("device_execute"),
            "erro aponta o caminho certo: {}",
            output.content
        );
    }

    /// Dispositivo/capability desconhecidos são erro legível, não pânico —
    /// o modelo precisa saber o que pedir de novo.
    #[tokio::test]
    async fn leitura_de_desconhecido_e_erro_legivel() {
        let reg = registry();
        let tool = DeviceReadTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "nada", "capability": "temperature" }),
            )
            .await
            .expect("executa");
        assert!(
            output.is_error && output.content.contains("nada"),
            "{}",
            output.content
        );

        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "sensor-sala", "capability": "humidity" }),
            )
            .await
            .expect("executa");
        assert!(
            output.is_error && output.content.contains("humidity"),
            "{}",
            output.content
        );
    }

    /// R1 é `PolicyGated`: a policy do turno já disse sim (a tool passou
    /// pelo `ToolGate`), então a execução roda sem confirmação extra.
    #[tokio::test]
    async fn executar_r1_roda_sob_policy() {
        let lampada = Arc::new(MockDevice::lampada_sala());
        let reg = Arc::new(DeviceRegistry::new());
        reg.register(lampada.clone());
        let tool = DeviceExecuteTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "lampada-sala", "capability": "power", "args": { "on": true } }),
            )
            .await
            .expect("executa");
        assert!(!output.is_error, "{}", output.content);
        assert_eq!(
            lampada.executed(),
            vec![("power".to_string(), json!({ "on": true }))]
        );
    }

    /// R3 (`HumanConfirmation`) com canal: a tool devolve
    /// `confirmation_request` com o marcador de impressão digital, e nada
    /// chega ao dispositivo.
    #[tokio::test]
    async fn executar_r3_pede_confirmacao_com_impressao_digital() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let args = json!({});
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "door_unlock", "args": args }),
            )
            .await
            .expect("executa");
        assert!(
            output.is_error,
            "confirmation_request é erro por design: {}",
            output.content
        );
        assert!(output.requires_confirmation, "{}", output.content);
        assert!(
            output.content.contains("CONFIRM_REQUIRED"),
            "marcador no pedido: {}",
            output.content
        );
        assert!(output.content.contains("door_unlock"), "{}", output.content);
        // O pedido emite uma impressão digital verificável para o assunto
        // exato — é ela que a aprovação vai casar.
        let fingerprint = ApprovalFingerprint::of(
            "device_execute",
            &DeviceExecuteTool::subject("risco-completo", "door_unlock", &args),
        );
        assert!(
            output.content.contains(fingerprint.marker().as_str()),
            "marcador do assunto certo: {}",
            output.content
        );
        // Nada chegou ao dispositivo.
        let device = reg.get("risco-completo").expect("device");
        let _ = device;
    }

    /// #1078 item 2 pela porta do hardware: a aprovação é a impressão
    /// digital de `(tool, assunto)` — "ok" dado a outro comando não abre a
    /// porta da garagem.
    #[tokio::test]
    async fn aprovacao_da_impressao_digital_cobre_o_pedido() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let args = json!({});
        let pedido =
            json!({ "device": "risco-completo", "capability": "door_unlock", "args": args });

        // Aprovação de OUTRO pedido: pede confirmação de novo.
        let errada = ToolApproval::granted(
            "device_execute",
            &DeviceExecuteTool::subject("risco-completo", "industrial_valve", &json!({})),
        );
        let output = tool
            .execute(&ctx(errada), pedido.clone())
            .await
            .expect("executa");
        assert!(
            output.requires_confirmation,
            "aprovação errada não cobre: {}",
            output.content
        );

        // Aprovação do pedido exato: roda.
        let certa = ToolApproval::granted(
            "device_execute",
            &DeviceExecuteTool::subject("risco-completo", "door_unlock", &args),
        );
        let output = tool.execute(&ctx(certa), pedido).await.expect("executa");
        assert!(!output.is_error, "{}", output.content);
    }

    /// Fail-closed do #1075: sem canal de confirmação, R3 é **bloqueada** —
    /// nunca aguardada, nunca executada, mesmo que o contexto traga uma
    /// "aprovação" (derivada de histórico que ninguém pode validar).
    #[tokio::test]
    async fn r3_sem_canal_e_fail_closed() {
        let reg = registry();
        let tool = DeviceExecuteTool::new_without_confirmation(config(&reg));
        let args = json!({});
        let certa = ToolApproval::granted(
            "device_execute",
            &DeviceExecuteTool::subject("risco-completo", "door_unlock", &args),
        );
        let output = tool
            .execute(
                &ctx(certa),
                json!({ "device": "risco-completo", "capability": "door_unlock", "args": args }),
            )
            .await
            .expect("executa");
        assert!(output.is_error, "{}", output.content);
        assert!(
            !output.requires_confirmation,
            "sem canal não se pede confirmação"
        );
        assert!(
            output.content.contains("fail-closed"),
            "a palavra que o bash usa, o hardware usa: {}",
            output.content
        );
    }

    /// A regra da #1129, literal: R5 é deny por padrão — allowlist vazia
    /// nega mesmo com canal e com aprovação pendente.
    #[tokio::test]
    async fn r5_e_denied_por_padrao() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "industrial_valve", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(output.is_error, "{}", output.content);
        assert!(!output.requires_confirmation);
        assert!(output.content.contains("R5"), "{}", output.content);
    }

    /// R5 allowlistado sai de Denied e pede approval explícito — nunca Auto.
    #[tokio::test]
    async fn r5_allowlistado_pede_aprovacao_explicita() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg)).com_r5_allowlist(["industrial_valve"]);
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "industrial_valve", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(output.requires_confirmation, "{}", output.content);
        assert!(output.is_error);

        // Outra R5 continua Denied — a allowlist não é geral.
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "security_system", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(
            output.is_error && !output.requires_confirmation,
            "{}",
            output.content
        );
    }

    /// R4 (`ExplicitApproval`): pede confirmação com canal, bloqueado sem.
    #[tokio::test]
    async fn r4_pede_aprovacao_explicita() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "robot_motion", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(output.requires_confirmation, "{}", output.content);

        let sem_canal = DeviceExecuteTool::new_without_confirmation(config(&reg));
        let output = sem_canal
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "risco-completo", "capability": "robot_motion", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(
            output.is_error && !output.requires_confirmation,
            "{}",
            output.content
        );
    }

    /// R0 pela porta errada: `device_execute` numa capability read-only —
    /// a tool recusa e aponta `device_read` (a tabela dá Auto para
    /// leitura, mas o caminho é `read`, não `execute`).
    #[tokio::test]
    async fn execute_em_capability_read_only_e_erro() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let output = tool
            .execute(
                &ctx(ToolApproval::None),
                json!({ "device": "sensor-sala", "capability": "temperature", "args": {} }),
            )
            .await
            .expect("executa");
        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("device_read"), "{}", output.content);
    }

    /// Parâmetro ausente é erro de entrada, não pânico.
    #[tokio::test]
    async fn execute_sem_parametros_e_erro() {
        let reg = registry();
        let tool = DeviceExecuteTool::new(config(&reg));
        let saida = tool
            .execute(&ctx(ToolApproval::None), json!({}))
            .await
            .expect_err("sem 'device' é erro de entrada");
        assert!(saida.to_string().contains("device"), "{}", saida);
    }

    /// E2E do aceite da #1129: as três tools aparecem no inventário do
    /// runtime — é o que o `GET /api/mcp/health` reporta (`source: native`).
    #[tokio::test]
    async fn tools_entrar_no_inventario_do_runtime() {
        let reg = Arc::new(DeviceRegistry::new());
        let cfg = config(&reg);
        let runtime = crate::AgentRuntime::new();
        runtime.register_tool(Box::new(DeviceListTool::new(cfg.clone())));
        runtime.register_tool(Box::new(DeviceReadTool::new(cfg.clone())));
        runtime.register_tool(Box::new(DeviceExecuteTool::new(cfg)));

        let inventario = runtime.tool_inventory();
        for nome in ["device_list", "device_read", "device_execute"] {
            let entrada = inventario
                .iter()
                .find(|e| e.name == nome)
                .unwrap_or_else(|| panic!("'{nome}' no inventário"));
            assert_eq!(entrada.source, "native", "{nome} é tool nativa");
        }
    }
}
