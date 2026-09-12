//! A spec declarativa de uma automação — TOML (ou JSON) versionável no repo
//! do usuário, o formato do exemplo da issue #1128:
//!
//! ```toml
//! [[automation]]
//! name = "ventilador-garagem-quente"
//! [[automation.trigger]]
//! type = "state_changed"
//! entity = "sensor.garagem_temperatura"
//! [[automation.condition]]
//! expr = "to.state > 32"
//! [[automation.action]]
//! type = "device_execute"
//! device = "fan.garagem"
//! capability = "power"
//! args = { on = true }
//! ```
//!
//! Validação no carregamento, fail-closed: nome único e com formato fixo,
//! pelo menos um gatilho e uma ação, toda expressão de condição parses
//! ([`crate::automations::expr`]), todo `expr` de cron parses (croner).
//! Regra quebrada não sobe — o erro nomeia arquivo e automação.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::expr::Expr;

/// Um gatilho. `state_changed` observa uma entidade no barramento; `cron`
/// dispara no relógio (expressão de 5 campos, avaliada no fuso do host —
/// uma casa funciona no fuso de quem mora nela).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerSpec {
    StateChanged {
        /// O id do dispositivo no registry (para o HA, a `entity_id`).
        entity: String,
    },
    Cron {
        /// Expressão cron de 5 campos (min hora dia mês dia-semana).
        expr: String,
    },
}

/// Uma condição — todas as listadas têm que passar (AND).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionSpec {
    pub expr: String,
}

/// Um limite de cadência por automação — proteção contra loop (automação
/// que dispara evento que dispara a si mesma).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RateLimitSpec {
    pub max_per_hour: u32,
}

/// A ação. Slice atual: só `device_execute` — as outras (`send_message`,
/// `tool`) são rejeitadas no parse com a mensagem dizendo que entram em
/// slice futuro, para a regra não subir meia-boca.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSpec {
    DeviceExecute {
        /// O id do dispositivo no registry.
        device: String,
        /// O nome da `Capability` a executar.
        capability: String,
        /// Argumentos validados pela mesma `validar_args` das tools.
        #[serde(default)]
        args: serde_json::Value,
    },
}

/// A automação declarada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationSpec {
    /// Nome único, `[a-z0-9-]` — vira chave no store e nas auditorias.
    pub name: String,
    /// Desligável sem apagar o arquivo.
    #[serde(default = "sim")]
    pub enabled: bool,
    #[serde(default)]
    pub trigger: Vec<TriggerSpec>,
    /// Todas as condições devem avaliar `true` (AND).
    #[serde(default)]
    pub condition: Vec<ConditionSpec>,
    #[serde(default)]
    pub action: Vec<ActionSpec>,
    /// Janela de debounce (segundos): eventos repetidos dentro da janela
    /// não re-disparam.
    #[serde(default)]
    pub debounce_secs: Option<u64>,
    /// Limite de execuções por hora.
    #[serde(default)]
    pub rate_limit: Option<RateLimitSpec>,
    /// Espera fixa entre condição satisfeita e ação (segundos).
    #[serde(default)]
    pub delay_secs: Option<u64>,
}

fn sim() -> bool {
    true
}

/// O envelope de um arquivo de specs: uma ou mais `[[automation]]`.
#[derive(Debug, Deserialize)]
struct ArquivoSpec {
    #[serde(default)]
    automation: Vec<AutomationSpec>,
}

/// O erro do carregamento — nomeia arquivo e automação, para o autor da
/// regra saber exatamente o que corrigir.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub struct ErroSpec {
    pub arquivo: String,
    pub automacao: Option<String>,
    pub mensagem: String,
}

impl std::fmt::Display for ErroSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.arquivo, self.mensagem)?;
        if let Some(nome) = &self.automacao {
            write!(f, " (automação '{nome}')")?;
        }
        Ok(())
    }
}

fn erro(arquivo: &str, automacao: Option<&str>, mensagem: impl Into<String>) -> ErroSpec {
    ErroSpec {
        arquivo: arquivo.to_string(),
        automacao: automacao.map(|s| s.to_string()),
        mensagem: mensagem.into(),
    }
}

impl AutomationSpec {
    /// Valida a automação inteira. Toda rejeição aqui é um erro do AUTOR
    /// da regra (o config do usuário), não do sistema.
    pub fn validar(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > 64 {
            return Err(format!("nome '{}' fora do formato (1-64 chars)", self.name));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || !self
                .name
                .starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err(format!(
                "nome '{}' fora do formato (lowercase, dígitos e hífen)",
                self.name
            ));
        }
        if self.trigger.is_empty() {
            return Err("sem gatilho — a automação nunca dispara".into());
        }
        if self.action.is_empty() {
            return Err("sem ação — a automação não faz nada".into());
        }
        for t in &self.trigger {
            match t {
                TriggerSpec::StateChanged { entity } => {
                    if entity.is_empty() {
                        return Err("gatilho state_changed sem entity".into());
                    }
                }
                TriggerSpec::Cron { expr } => {
                    if let Err(e) = crate::automations::cron::parse_cron(expr) {
                        return Err(format!("cron inválido '{expr}': {e}"));
                    }
                }
            }
        }
        for c in &self.condition {
            if let Err(e) = Expr::parse(&c.expr) {
                return Err(format!("condição inválida '{}': {e}", c.expr));
            }
        }
        if let Some(max) = self.rate_limit
            && max.max_per_hour == 0
        {
            return Err(
                "rate_limit.max_per_hour = 0 desligaria a automação — use enabled = false".into(),
            );
        }
        for a in &self.action {
            match a {
                ActionSpec::DeviceExecute {
                    device, capability, ..
                } => {
                    if device.is_empty() || capability.is_empty() {
                        return Err("device_execute exige device e capability não vazios".into());
                    }
                }
            }
        }
        Ok(())
    }
}

/// Carrega todas as specs de um diretório (`*.toml` e `*.json`, em ordem
/// alfabética). Nomes duplicados entre arquivos são erro — automação com
/// nome ambíguo não tem auditoria confiável.
pub fn carregar_dir(dir: &Path) -> Result<Vec<AutomationSpec>, ErroSpec> {
    let mut entradas: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| {
            erro(
                &dir.display().to_string(),
                None,
                format!("falha ao ler o diretório: {e}"),
            )
        })?
        .filter_map(|entrada| entrada.ok())
        .map(|entrada| entrada.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("toml") | Some("json")
            )
        })
        .collect();
    entradas.sort();
    let mut todas = Vec::new();
    for arquivo in &entradas {
        let nome = arquivo.display().to_string();
        let conteudo = std::fs::read_to_string(arquivo)
            .map_err(|e| erro(&nome, None, format!("falha ao ler: {e}")))?;
        let parsed: ArquivoSpec = match arquivo.extension().and_then(|e| e.to_str()) {
            Some("toml") => toml::from_str(&conteudo)
                .map_err(|e| erro(&nome, None, format!("TOML inválido: {e}")))?,
            _ => serde_json::from_str(&conteudo)
                .map_err(|e| erro(&nome, None, format!("JSON inválido: {e}")))?,
        };
        for spec in parsed.automation {
            spec.validar()
                .map_err(|m| erro(&nome, Some(&spec.name), m))?;
            if todas.iter().any(|j: &AutomationSpec| j.name == spec.name) {
                return Err(erro(
                    &nome,
                    Some(&spec.name),
                    "nome duplicado em outro arquivo do diretório",
                ));
            }
            todas.push(spec);
        }
    }
    Ok(todas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir_tmp(nome: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("garraia-spec-{nome}-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("cria dir");
        base
    }

    const EXEMPLO_ISSUE: &str = r#"
[[automation]]
name = "ventilador-garagem-quente"

[[automation.trigger]]
type = "state_changed"
entity = "sensor.garagem_temperatura"

[[automation.condition]]
expr = "to.state > 32"

[[automation.action]]
type = "device_execute"
device = "fan.garagem"
capability = "power"
args = { on = true }
"#;

    #[test]
    fn parse_da_espec_da_issue() {
        let spec: ArquivoSpec = toml::from_str(EXEMPLO_ISSUE).expect("TOML parse");
        assert_eq!(spec.automation.len(), 1);
        let a = &spec.automation[0];
        assert_eq!(a.name, "ventilador-garagem-quente");
        assert!(a.enabled);
        assert_eq!(
            a.trigger,
            vec![TriggerSpec::StateChanged {
                entity: "sensor.garagem_temperatura".into()
            }]
        );
        assert_eq!(a.condition.len(), 1);
        assert_eq!(
            a.action,
            vec![ActionSpec::DeviceExecute {
                device: "fan.garagem".into(),
                capability: "power".into(),
                args: serde_json::json!({ "on": true }),
            }]
        );
        a.validar().expect("spec da issue valida");
    }

    #[test]
    fn parse_json_equivalente() {
        let spec: ArquivoSpec = serde_json::from_str(
            r#"{"automation": [{"name": "luz-noite", "enabled": false,
                "trigger": [{"type": "cron", "expr": "0 19 * * *"}],
                "action": [{"type": "device_execute", "device": "light.sala", "capability": "power"}]}]}"#,
        )
        .expect("JSON parse");
        let a = &spec.automation[0];
        assert!(!a.enabled);
        a.validar().expect("valida");
    }

    #[test]
    fn validacao_rejeita_as_buracos() {
        let vazio = AutomationSpec {
            name: "sem-gatilho".into(),
            enabled: true,
            trigger: vec![],
            condition: vec![],
            action: vec![ActionSpec::DeviceExecute {
                device: "x".into(),
                capability: "power".into(),
                args: serde_json::Value::Null,
            }],
            debounce_secs: None,
            rate_limit: None,
            delay_secs: None,
        };
        assert!(vazio.validar().is_err());

        let nome_feio = AutomationSpec {
            name: "Luz Sala".into(),
            ..vazio.clone()
        };
        assert!(nome_feio.validar().is_err());

        let sem_acao = AutomationSpec {
            name: "so-gatilho".into(),
            trigger: vec![TriggerSpec::StateChanged {
                entity: "s.x".into(),
            }],
            action: vec![],
            ..vazio
        };
        assert!(sem_acao.validar().is_err());
    }

    #[test]
    fn condicao_invalida_rejeitada_na_carga() {
        let spec: ArquivoSpec = toml::from_str(
            r#"
[[automation]]
name = "condicao-quebrada"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.x"
[[automation.condition]]
expr = "to. > 32"
[[automation.action]]
type = "device_execute"
device = "light.y"
capability = "power"
"#,
        )
        .expect("TOML parse");
        let msg = spec.automation[0].validar().unwrap_err();
        assert!(msg.contains("condição inválida"), "{msg}");
    }

    #[test]
    fn acao_desconhecida_rejeitada_com_mensagem_direita() {
        // send_message entra em slice futuro — o parse tem que dizer isso.
        let resultado: Result<ArquivoSpec, _> = toml::from_str(
            r#"
[[automation]]
name = "mensagem"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.x"
[[automation.action]]
type = "send_message"
text = "olá"
"#,
        );
        let err = resultado.expect_err("send_message não é suportado ainda");
        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    #[test]
    fn cron_invalido_rejeitado_na_carga() {
        let spec: ArquivoSpec = toml::from_str(
            r#"
[[automation]]
name = "cron-quebrado"
[[automation.trigger]]
type = "cron"
expr = "nao-e-cron"
[[automation.action]]
type = "device_execute"
device = "light.y"
capability = "power"
"#,
        )
        .expect("TOML parse");
        let msg = spec.automation[0].validar().unwrap_err();
        assert!(msg.contains("cron inválido"), "{msg}");
    }

    #[test]
    fn rate_limit_zero_rejeitado() {
        let spec: ArquivoSpec = toml::from_str(
            r#"
[[automation]]
name = "zero"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.x"
[automation.rate_limit]
max_per_hour = 0
[[automation.action]]
type = "device_execute"
device = "light.y"
capability = "power"
"#,
        )
        .expect("TOML parse");
        let msg = spec.automation[0].validar().unwrap_err();
        assert!(msg.contains("max_per_hour = 0"), "{msg}");
    }

    #[test]
    fn carregar_dir_le_toml_e_json_e_rejeita_duplicado() {
        let dir = dir_tmp("dupe");
        std::fs::write(dir.join("a.toml"), EXEMPLO_ISSUE).unwrap();
        std::fs::write(
            dir.join("b.json"),
            r#"{"automation": [{"name": "ventilador-garagem-quente",
                "trigger": [{"type": "state_changed", "entity": "s.x"}],
                "action": [{"type": "device_execute", "device": "d", "capability": "power"}]}]}"#,
        )
        .unwrap();
        let err = carregar_dir(&dir).unwrap_err();
        assert!(err.to_string().contains("duplicado"), "{err}");

        std::fs::remove_file(dir.join("b.json")).unwrap();
        let specs = carregar_dir(&dir).expect("sem duplicado carrega");
        assert_eq!(specs.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn carregar_dir_arquivo_quebrado_nomeia_o_arquivo() {
        let dir = dir_tmp("quebrado");
        std::fs::write(dir.join("ruim.toml"), "[[automation]]\nname = 3\n").unwrap();
        let err = carregar_dir(&dir).unwrap_err();
        assert!(err.arquivo.ends_with("ruim.toml"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
