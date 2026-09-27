//! Politica de retencao (#1436): a memoria do agente e o ledger de runs,
//! lado a lado e **distintas**.
//!
//! Sao duas politicas, com chaves, unidades e riscos diferentes:
//!
//! | | memoria (`memory.retention`) | ledger (`runs.retention_days`) |
//! |---|---|---|
//! | o que sai | entrada nao fixada mais velha que `max_age_days`, ou vencida | run **terminal** mais velho que N dias |
//! | desligada | `enabled: false` (default) | `0` (default) |
//! | varredura automatica | a cada `interval_hours` | a cada 24 h, fixo |
//! | nunca sai | entrada fixada | run `running` |
//!
//! Este modulo e o **dono das faixas**. O `garraia config check` chama
//! [`memory_findings`] e [`run_ledger_findings`]; o `PATCH
//! /admin/api/retention` aplica um [`RetentionPatch`] e recusa o que esses
//! mesmos achados chamam de `Error`. Uma regra, dois consumidores: o Web
//! Console nao consegue gravar o que a CLI recusaria.

use serde::{Deserialize, Serialize};

use crate::check::{Finding, Severity};
use crate::model::{
    AppConfig, MemoryConfig, RETENTION_INTERVAL_MAX_HOURS, RETENTION_INTERVAL_MIN_HOURS,
    RETENTION_MAX_AGE_MAX_DAYS, RETENTION_MAX_AGE_MIN_DAYS, RUNS_RETENTION_MAX_DAYS, RunsConfig,
};

/// O que o `PATCH /admin/api/retention` aceita. Secao ou campo ausente =
/// nao mexe. Campo desconhecido e erro de desserializacao, nunca ignorado:
/// um `max_age` digitado errado nao pode virar "salvo" sem ter mudado nada.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPatch {
    pub memory: Option<MemoryRetentionPatch>,
    pub run_ledger: Option<RunLedgerRetentionPatch>,
}

/// `memory.retention`, campo a campo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRetentionPatch {
    pub enabled: Option<bool>,
    pub max_age_days: Option<u32>,
    pub interval_hours: Option<u32>,
}

/// `runs.retention_days`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunLedgerRetentionPatch {
    pub retention_days: Option<u32>,
}

/// Uma mudanca aplicada — o que a resposta mostra e o audit grava.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetentionChange {
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

impl std::fmt::Display for RetentionChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} -> {}", self.field, self.before, self.after)
    }
}

/// A politica da memoria como o console e a CLI a mostram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemoryRetentionPolicy {
    /// Liga a varredura **automatica**. A limpeza manual (console ou
    /// `garraia memory compact`) existe com ela ligada ou nao.
    pub enabled: bool,
    pub max_age_days: u32,
    pub interval_hours: u32,
}

impl MemoryRetentionPolicy {
    pub fn of(config: &AppConfig) -> Self {
        let r = &config.memory.retention;
        Self {
            enabled: r.enabled,
            max_age_days: r.max_age_days,
            interval_hours: r.interval_hours,
        }
    }
}

/// A politica do ledger como o console e a CLI a mostram. `enabled` e
/// derivado: `retention_days > 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunLedgerRetentionPolicy {
    pub retention_days: u32,
    pub enabled: bool,
}

impl RunLedgerRetentionPolicy {
    pub fn of(config: &AppConfig) -> Self {
        let dias = config.runs.retention_days;
        Self {
            retention_days: dias,
            enabled: dias > 0,
        }
    }
}

/// Uma faixa fechada `[min, max]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Range {
    pub min: u32,
    pub max: u32,
}

/// As faixas que o formulario mostra — as mesmas que os achados cobram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RetentionLimits {
    pub memory_max_age_days: Range,
    pub memory_interval_hours: Range,
    /// `0` = nunca apaga; `1..=max` = janela em dias.
    pub run_ledger_retention_days: Range,
}

pub fn limits() -> RetentionLimits {
    RetentionLimits {
        memory_max_age_days: Range {
            min: RETENTION_MAX_AGE_MIN_DAYS,
            max: RETENTION_MAX_AGE_MAX_DAYS,
        },
        memory_interval_hours: Range {
            min: RETENTION_INTERVAL_MIN_HOURS,
            max: RETENTION_INTERVAL_MAX_HOURS,
        },
        run_ledger_retention_days: Range {
            min: 0,
            max: RUNS_RETENTION_MAX_DAYS,
        },
    }
}

/// Aplica o patch em `config` e devolve so o que de fato mudou. Valor igual
/// ao atual nao e mudanca. Nao valida: quem chama roda [`findings`] depois
/// e recusa `Error`.
pub fn apply_patch(config: &mut AppConfig, patch: &RetentionPatch) -> Vec<RetentionChange> {
    fn trocar<T: PartialEq + ToString + Copy>(
        mudancas: &mut Vec<RetentionChange>,
        field: &'static str,
        atual: &mut T,
        novo: Option<T>,
    ) {
        if let Some(novo) = novo
            && novo != *atual
        {
            mudancas.push(RetentionChange {
                field,
                before: atual.to_string(),
                after: novo.to_string(),
            });
            *atual = novo;
        }
    }

    let mut mudancas = Vec::new();
    if let Some(m) = &patch.memory {
        let r = &mut config.memory.retention;
        trocar(
            &mut mudancas,
            "memory.retention.enabled",
            &mut r.enabled,
            m.enabled,
        );
        trocar(
            &mut mudancas,
            "memory.retention.max_age_days",
            &mut r.max_age_days,
            m.max_age_days,
        );
        trocar(
            &mut mudancas,
            "memory.retention.interval_hours",
            &mut r.interval_hours,
            m.interval_hours,
        );
    }
    if let Some(l) = &patch.run_ledger {
        trocar(
            &mut mudancas,
            "runs.retention_days",
            &mut config.runs.retention_days,
            l.retention_days,
        );
    }
    mudancas
}

/// Os achados de retencao da config inteira: memoria e depois ledger.
pub fn findings(config: &AppConfig) -> Vec<Finding> {
    let mut out = memory_findings(&config.memory);
    out.extend(run_ledger_findings(&config.runs));
    out
}

/// `true` quando algum achado e `Error` — o que recusa um `PATCH`.
pub fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}

fn erro(field: &str, message: String) -> Finding {
    Finding {
        severity: Severity::Error,
        field: field.to_owned(),
        message,
    }
}

fn aviso(field: &str, message: String) -> Finding {
    Finding {
        severity: Severity::Warning,
        field: field.to_owned(),
        message,
    }
}

/// Politica de retencao da memoria do agente (#956, #959).
///
/// A retencao apaga dado do usuario, entao a validacao e mais dura do que o
/// normal: uma faixa errada aqui nao produz um erro em runtime, produz uma
/// varredura que apaga o que nao devia — ou que nunca roda e deixa o operador
/// achando que roda.
pub fn memory_findings(memory: &MemoryConfig) -> Vec<Finding> {
    let r = &memory.retention;
    let mut out = Vec::new();

    if r.max_age_days < RETENTION_MAX_AGE_MIN_DAYS || r.max_age_days > RETENTION_MAX_AGE_MAX_DAYS {
        out.push(erro(
            "memory.retention.max_age_days",
            format!(
                "memory.retention.max_age_days ({}) must be in [{RETENTION_MAX_AGE_MIN_DAYS}, {RETENTION_MAX_AGE_MAX_DAYS}] days",
                r.max_age_days
            ),
        ));
    }

    if r.interval_hours < RETENTION_INTERVAL_MIN_HOURS
        || r.interval_hours > RETENTION_INTERVAL_MAX_HOURS
    {
        out.push(erro(
            "memory.retention.interval_hours",
            format!(
                "memory.retention.interval_hours ({}) must be in [{RETENTION_INTERVAL_MIN_HOURS}, {RETENTION_INTERVAL_MAX_HOURS}] hours",
                r.interval_hours
            ),
        ));
    }

    // Politica ligada com a memoria desligada nao apaga nada — mas quem
    // escreveu a config acha que apaga.
    if r.enabled && !memory.enabled {
        out.push(aviso(
            "memory.retention.enabled",
            "memory.retention.enabled=true but memory.enabled=false; the retention sweep never runs"
                .into(),
        ));
    }

    // Uma varredura mais rara que a propria janela deixa dado vencido vivo por
    // ate um intervalo inteiro depois do prazo. Nao e erro, e surpresa.
    if r.enabled && u64::from(r.interval_hours) > u64::from(r.max_age_days) * 24 {
        out.push(aviso(
            "memory.retention.interval_hours",
            format!(
                "memory.retention.interval_hours ({}) is longer than max_age_days ({} days = {} hours); \
                 entries can outlive the window by a full interval",
                r.interval_hours,
                r.max_age_days,
                u64::from(r.max_age_days) * 24
            ),
        ));
    }
    out
}

/// Retencao do ledger `agent_runs` (#1227 slice 5).
///
/// `0` e o default e quer dizer "nunca apaga" — valido, e sem finding: um
/// Warning aqui faria o `config check --strict` de toda instalacao default
/// sair nao-zero. O sinal de ledger crescendo sem teto e o aviso de boot do
/// gateway. Acima do teto e Error: o numero deixou de ser politica.
pub fn run_ledger_findings(runs: &RunsConfig) -> Vec<Finding> {
    if runs.retention_days > RUNS_RETENTION_MAX_DAYS {
        return vec![erro(
            "runs.retention_days",
            format!(
                "runs.retention_days ({}) must be 0 (never delete) or in [1, {RUNS_RETENTION_MAX_DAYS}] days",
                runs.retention_days
            ),
        )];
    }
    Vec::new()
}

/// A janela, em dias, que uma limpeza **manual** da memoria usa:
/// `memory.retention.max_age_days`, na faixa. `Err` diz por que nao ha.
pub fn memory_cleanup_days(config: &AppConfig) -> Result<u32, String> {
    let dias = config.memory.retention.max_age_days;
    if (RETENTION_MAX_AGE_MIN_DAYS..=RETENTION_MAX_AGE_MAX_DAYS).contains(&dias) {
        Ok(dias)
    } else {
        Err(format!(
            "memory.retention.max_age_days ({dias}) is outside [{RETENTION_MAX_AGE_MIN_DAYS}, {RETENTION_MAX_AGE_MAX_DAYS}]; fix the policy before cleaning up"
        ))
    }
}

/// A janela, em dias, que uma limpeza **manual** do ledger usa:
/// `runs.retention_days`. `0` quer dizer "nunca apaga" e nao e janela.
pub fn run_ledger_cleanup_days(config: &AppConfig) -> Result<u32, String> {
    match config.runs.retention_days {
        0 => Err(
            "runs.retention_days is 0 (keep every run): set a retention window before cleaning up"
                .into(),
        ),
        dias if dias <= RUNS_RETENTION_MAX_DAYS => Ok(dias),
        dias => Err(format!(
            "runs.retention_days ({dias}) is above {RUNS_RETENTION_MAX_DAYS}; fix the policy before cleaning up"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigLoader;
    use crate::model::{
        RETENTION_INTERVAL_MAX_HOURS, RETENTION_INTERVAL_MIN_HOURS, RETENTION_MAX_AGE_MAX_DAYS,
        RETENTION_MAX_AGE_MIN_DAYS, RUNS_RETENTION_MAX_DAYS,
    };

    fn campos(achados: &[Finding], severidade: Severity) -> Vec<String> {
        achados
            .iter()
            .filter(|f| f.severity == severidade)
            .map(|f| f.field.clone())
            .collect()
    }

    /// Instalacao nova: nada e apagado sem o operador pedir, e a config
    /// default nao reclama de si mesma.
    #[test]
    fn defaults_sao_conservadores_e_sem_achado() {
        let c = AppConfig::default();
        assert_eq!(
            MemoryRetentionPolicy::of(&c),
            MemoryRetentionPolicy {
                enabled: false,
                max_age_days: 90,
                interval_hours: 24
            }
        );
        assert_eq!(
            RunLedgerRetentionPolicy::of(&c),
            RunLedgerRetentionPolicy {
                retention_days: 0,
                enabled: false
            }
        );
        assert!(findings(&c).is_empty(), "{:?}", findings(&c));
    }

    #[test]
    fn limites_sao_as_faixas_do_modelo() {
        let l = limits();
        assert_eq!(
            l.memory_max_age_days,
            Range {
                min: RETENTION_MAX_AGE_MIN_DAYS,
                max: RETENTION_MAX_AGE_MAX_DAYS
            }
        );
        assert_eq!(
            l.memory_interval_hours,
            Range {
                min: RETENTION_INTERVAL_MIN_HOURS,
                max: RETENTION_INTERVAL_MAX_HOURS
            }
        );
        assert_eq!(
            l.run_ledger_retention_days,
            Range {
                min: 0,
                max: RUNS_RETENTION_MAX_DAYS
            }
        );
    }

    #[test]
    fn memoria_fora_da_faixa_e_error() {
        for dias in [0, RETENTION_MAX_AGE_MAX_DAYS + 1] {
            let mut m = MemoryConfig::default();
            m.retention.max_age_days = dias;
            assert_eq!(
                campos(&memory_findings(&m), Severity::Error),
                vec!["memory.retention.max_age_days".to_string()],
                "max_age_days={dias}"
            );
        }
        for horas in [0, RETENTION_INTERVAL_MAX_HOURS + 1] {
            let mut m = MemoryConfig::default();
            m.retention.interval_hours = horas;
            assert_eq!(
                campos(&memory_findings(&m), Severity::Error),
                vec!["memory.retention.interval_hours".to_string()],
                "interval_hours={horas}"
            );
        }
        let mut m = MemoryConfig::default();
        m.retention.max_age_days = RETENTION_MAX_AGE_MAX_DAYS;
        m.retention.interval_hours = RETENTION_INTERVAL_MIN_HOURS;
        assert!(memory_findings(&m).is_empty(), "os limites sao aceitos");
    }

    /// Aviso descreve surpresa, nao perda: nao recusa um `PATCH`.
    #[test]
    fn avisos_da_memoria_nao_sao_erro() {
        let mut m = MemoryConfig {
            enabled: false,
            ..MemoryConfig::default()
        };
        m.retention.enabled = true;
        let a = memory_findings(&m);
        assert_eq!(
            campos(&a, Severity::Warning),
            vec!["memory.retention.enabled".to_string()]
        );
        assert!(!has_errors(&a));

        let mut m = MemoryConfig::default();
        m.retention.enabled = true;
        m.retention.max_age_days = 1;
        m.retention.interval_hours = 48;
        let a = memory_findings(&m);
        assert_eq!(
            campos(&a, Severity::Warning),
            vec!["memory.retention.interval_hours".to_string()]
        );
        assert!(!has_errors(&a));
    }

    #[test]
    fn ledger_aceita_zero_e_a_faixa_e_recusa_acima_do_teto() {
        for dias in [0, 1, 30, RUNS_RETENTION_MAX_DAYS] {
            let runs = RunsConfig {
                retention_days: dias,
            };
            assert!(run_ledger_findings(&runs).is_empty(), "{dias}");
        }
        let runs = RunsConfig {
            retention_days: RUNS_RETENTION_MAX_DAYS + 1,
        };
        let a = run_ledger_findings(&runs);
        assert_eq!(
            campos(&a, Severity::Error),
            vec!["runs.retention_days".to_string()]
        );
        assert!(a[0].message.contains("3651"), "{}", a[0].message);
    }

    /// O patch mexe so no que veio, e memoria e ledger nao se contaminam.
    #[test]
    fn patch_muda_so_o_que_veio_e_distingue_memoria_de_ledger() {
        let mut c = AppConfig::default();
        let mudancas = apply_patch(
            &mut c,
            &RetentionPatch {
                memory: Some(MemoryRetentionPatch {
                    max_age_days: Some(30),
                    ..Default::default()
                }),
                run_ledger: None,
            },
        );
        assert_eq!(
            mudancas,
            vec![RetentionChange {
                field: "memory.retention.max_age_days",
                before: "90".into(),
                after: "30".into(),
            }]
        );
        assert_eq!(c.memory.retention.max_age_days, 30);
        assert!(!c.memory.retention.enabled, "enabled nao veio, nao muda");
        assert_eq!(c.memory.retention.interval_hours, 24);
        assert_eq!(c.runs.retention_days, 0, "o ledger nao e a memoria");

        let mudancas = apply_patch(
            &mut c,
            &RetentionPatch {
                memory: None,
                run_ledger: Some(RunLedgerRetentionPatch {
                    retention_days: Some(14),
                }),
            },
        );
        assert_eq!(mudancas.len(), 1, "{mudancas:?}");
        assert_eq!(mudancas[0].to_string(), "runs.retention_days: 0 -> 14");
        assert_eq!(c.runs.retention_days, 14);
        assert_eq!(
            c.memory.retention.max_age_days, 30,
            "a memoria nao e o ledger"
        );
    }

    #[test]
    fn patch_igual_ao_atual_nao_e_mudanca() {
        let mut c = AppConfig::default();
        let patch = RetentionPatch {
            memory: Some(MemoryRetentionPatch {
                enabled: Some(false),
                max_age_days: Some(90),
                interval_hours: Some(24),
            }),
            run_ledger: Some(RunLedgerRetentionPatch {
                retention_days: Some(0),
            }),
        };
        assert!(apply_patch(&mut c, &patch).is_empty());
    }

    #[test]
    fn patch_com_tudo_lista_as_quatro_mudancas_na_ordem() {
        let mut c = AppConfig::default();
        let patch = RetentionPatch {
            memory: Some(MemoryRetentionPatch {
                enabled: Some(true),
                max_age_days: Some(30),
                interval_hours: Some(12),
            }),
            run_ledger: Some(RunLedgerRetentionPatch {
                retention_days: Some(7),
            }),
        };
        let campos: Vec<&str> = apply_patch(&mut c, &patch)
            .iter()
            .map(|m| m.field)
            .collect();
        assert_eq!(
            campos,
            vec![
                "memory.retention.enabled",
                "memory.retention.max_age_days",
                "memory.retention.interval_hours",
                "runs.retention_days",
            ]
        );
    }

    /// Campo desconhecido e recusado em qualquer nivel — nunca ignorado.
    #[test]
    fn patch_recusa_campo_desconhecido() {
        for bruto in [
            serde_json::json!({ "memory": { "max_age": 3 } }),
            serde_json::json!({ "runs": { "retention_days": 3 } }),
            serde_json::json!({ "run_ledger": { "days": 3 } }),
        ] {
            assert!(
                serde_json::from_value::<RetentionPatch>(bruto.clone()).is_err(),
                "{bruto}"
            );
        }
        // Negativo e fora de u32 tambem nao desserializam.
        assert!(
            serde_json::from_value::<RetentionPatch>(
                serde_json::json!({ "memory": { "max_age_days": -1 } })
            )
            .is_err()
        );
        let vazio: RetentionPatch = serde_json::from_value(serde_json::json!({})).expect("vazio");
        assert_eq!(vazio, RetentionPatch::default());
    }

    /// A limpeza manual usa a janela salva; o ledger com `0` nao tem janela.
    #[test]
    fn janela_da_limpeza_manual() {
        let mut c = AppConfig::default();
        assert_eq!(memory_cleanup_days(&c), Ok(90));
        c.memory.retention.max_age_days = 0;
        assert!(memory_cleanup_days(&c).is_err());
        c.memory.retention.max_age_days = RETENTION_MAX_AGE_MAX_DAYS + 1;
        assert!(memory_cleanup_days(&c).is_err());

        let erro = run_ledger_cleanup_days(&c).expect_err("0 e nunca apaga");
        assert!(erro.contains("runs.retention_days"), "{erro}");
        c.runs.retention_days = 30;
        assert_eq!(run_ledger_cleanup_days(&c), Ok(30));
        c.runs.retention_days = RUNS_RETENTION_MAX_DAYS + 1;
        assert!(run_ledger_cleanup_days(&c).is_err());
    }

    /// Uma regra, dois consumidores: o achado que o `config check` emite e
    /// exatamente o que este modulo emite.
    #[test]
    fn config_check_usa_as_mesmas_regras() {
        let mut c = AppConfig::default();
        c.memory.retention.max_age_days = 0;
        c.runs.retention_days = RUNS_RETENTION_MAX_DAYS + 1;
        let dir = std::env::temp_dir().join(format!("garraia-retention-{}", std::process::id()));
        let check = crate::run_check(&ConfigLoader::with_dir(&dir), &c);
        let do_check: Vec<(Severity, String, String)> = check
            .findings
            .iter()
            .filter(|f| f.field.starts_with("memory.retention") || f.field.starts_with("runs."))
            .map(|f| (f.severity, f.field.clone(), f.message.clone()))
            .collect();
        let daqui: Vec<(Severity, String, String)> = findings(&c)
            .iter()
            .map(|f| (f.severity, f.field.clone(), f.message.clone()))
            .collect();
        assert_eq!(do_check, daqui);
        assert_eq!(daqui.len(), 2, "{daqui:?}");
    }
}
