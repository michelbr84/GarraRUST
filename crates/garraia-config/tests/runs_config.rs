//! #1227 (slice 5): a secao de topo `runs` e a validacao de
//! `runs.retention_days`.
//!
//! Arquivo proprio de teste de integracao para nao disputar o modulo de
//! testes de `model.rs`/`check.rs` com outras mudancas em paralelo.

use garraia_config::model::{RUNS_RETENTION_MAX_DAYS, RunsConfig};
use garraia_config::{AppConfig, ConfigLoader, Severity, run_check};

fn findings_de_runs(config: &AppConfig) -> Vec<(Severity, String)> {
    let dir = std::env::temp_dir().join(format!("garraia-runs-config-{}", std::process::id()));
    let loader = ConfigLoader::with_dir(&dir);
    run_check(&loader, config)
        .findings
        .into_iter()
        .filter(|f| f.field.starts_with("runs."))
        .map(|f| (f.severity, f.message))
        .collect()
}

/// Secao ausente = retencao desligada: uma atualizacao nao apaga nada.
#[test]
fn secao_ausente_e_retencao_desligada() {
    let config: AppConfig = serde_yaml::from_str("agent: {}\n").expect("yaml");
    assert_eq!(config.runs, RunsConfig { retention_days: 0 });
    assert_eq!(AppConfig::default().runs.retention_days, 0);
}

#[test]
fn secao_runs_e_lida_do_yaml() {
    let config: AppConfig = serde_yaml::from_str("runs:\n  retention_days: 30\n").expect("yaml");
    assert_eq!(config.runs.retention_days, 30);
}

/// Regressao do motivo de a chave ser de topo: `agents` e um mapa de agentes
/// nomeados, e a secao `runs` nao pode vazar para ele nem ser engolida.
#[test]
fn agentes_nomeados_continuam_intactos() {
    let yaml = "runs:\n  retention_days: 7\nagents:\n  revisor:\n    system_prompt: oi\n";
    let config: AppConfig = serde_yaml::from_str(yaml).expect("yaml");
    assert_eq!(config.runs.retention_days, 7);
    assert_eq!(config.agents.len(), 1);
    assert!(config.agents.contains_key("revisor"));
    assert!(!config.agents.contains_key("runs"));
}

#[test]
fn check_aceita_zero_e_a_faixa() {
    for dias in [0, 1, 30, RUNS_RETENTION_MAX_DAYS] {
        let mut config = AppConfig::default();
        config.runs.retention_days = dias;
        assert!(
            findings_de_runs(&config).is_empty(),
            "{dias} dias deveria ser aceito"
        );
    }
}

#[test]
fn check_recusa_acima_do_teto() {
    let mut config = AppConfig::default();
    config.runs.retention_days = RUNS_RETENTION_MAX_DAYS + 1;
    let f = findings_de_runs(&config);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].0, Severity::Error);
    assert!(f[0].1.contains("3651"), "{}", f[0].1);
}
