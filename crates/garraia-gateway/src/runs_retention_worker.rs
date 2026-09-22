//! Varredura de retencao do ledger de runs de agente (#1227, slice 5).
//!
//! O ledger `agent_runs` cresce uma linha por run agendado (o scheduler e o
//! produtor de producao desde o slice 1) e nada o encurtava. Este worker
//! apaga runs **terminais** mais velhos que `runs.retention_days`.
//!
//! # Desligado por padrao, e o boot avisa
//!
//! `runs.retention_days: 0` (o default) quer dizer "nunca apaga". O ledger e
//! auditoria: ligar a varredura por default numa atualizacao apagaria o
//! historico de quem so quis atualizar a versao — o mesmo raciocinio da
//! `memory.retention` (`crate::memory_retention_worker`). Desligado, o
//! gateway loga **uma vez** quantos runs existem e como ligar.
//!
//! # O que a varredura nunca apaga
//!
//! Run `running`, qualquer que seja a idade — a regra e do SQL
//! (`SessionStore::prune_agent_runs`), nao deste modulo. Residuo de queda
//! vira `interrupted` na subida seguinte e so entao envelhece.
//!
//! # O que o log carrega
//!
//! Contagem e dias. **Nunca** `goal` nem trecho de resultado: o `goal` e o
//! payload de uma tarefa agendada, que pode ter vindo de qualquer usuario de
//! canal (CLAUDE.md §6). Ha teste varrendo este fonte atras disso.
//!
//! # Forma
//!
//! [`cutoff_for`] nao le relogio (recebe o `now`) e [`run_retention_tick`] so
//! orquestra store + log. O laco e um `tokio::time::interval` de 24 h com
//! `MissedTickBehavior::Delay`; o primeiro tick dispara no boot.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use garraia_db::SessionStore;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// De quanto em quanto tempo a varredura roda. Fixo: a unidade da politica
/// e o dia, e varrer mais vezes so apagaria o mesmo dia em pedacos.
pub const INTERVALO: Duration = Duration::from_secs(24 * 3600);

/// Configuracao da varredura, ja resolvida a partir do `AppConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunsRetentionConfig {
    pub interval: Duration,
    pub retention_days: u32,
}

impl RunsRetentionConfig {
    /// Le `runs.retention_days` — `None` quando e `0` (desligado).
    pub fn from_app_config(config: &garraia_config::AppConfig) -> Option<Self> {
        let dias = config.runs.retention_days;
        if dias == 0 {
            return None;
        }
        Some(Self {
            interval: INTERVALO,
            retention_days: dias,
        })
    }
}

/// Instante de corte a partir de `now`, sem relogio proprio.
///
/// `None` quando a subtracao nao cabe no calendario — e ai a varredura
/// **nao roda**: o lado seguro de uma operacao que apaga dado.
pub fn cutoff_for(now: DateTime<Utc>, retention_days: u32) -> Option<DateTime<Utc>> {
    now.checked_sub_signed(chrono::Duration::try_days(i64::from(retention_days))?)
}

/// Uma varredura. Devolve quantos runs sairam, ou `None` quando nao rodou
/// (corte impossivel) ou falhou (fail-soft: o gateway segue, o ledger so
/// fica maior do que a politica pediu).
pub async fn run_retention_tick(
    store: &Arc<Mutex<SessionStore>>,
    now: DateTime<Utc>,
    retention_days: u32,
) -> Option<usize> {
    let cutoff = cutoff_for(now, retention_days)?;
    let resultado = store.lock().await.prune_agent_runs(cutoff);
    match resultado {
        Ok(apagados) => {
            if apagados > 0 {
                info!(
                    apagados,
                    retention_days, "retencao do ledger de runs: runs terminais antigos apagados"
                );
            } else {
                debug!(
                    retention_days,
                    "retencao do ledger de runs: nada a apagar neste tick"
                );
            }
            Some(apagados)
        }
        Err(e) => {
            warn!(erro = %e, "retencao do ledger de runs falhou neste tick");
            None
        }
    }
}

/// Sobe a varredura em segundo plano. O primeiro tick do `interval` dispara
/// imediatamente, entao a primeira varredura acontece no boot.
pub fn spawn_runs_retention_worker(
    store: Arc<Mutex<SessionStore>>,
    config: RunsRetentionConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(config.interval);
        // Gateway que ficou suspenso nao acorda disparando todas as
        // varreduras perdidas de uma vez.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            run_retention_tick(&store, Utc::now(), config.retention_days).await;
        }
    })
}

/// Ponto de entrada do boot (`server.rs`): sobe o worker quando a politica
/// esta ligada, ou avisa uma vez, com a contagem, quando nao esta. Sem
/// `sessions.db` aberto nao ha ledger, e nada acontece.
///
/// O aviso roda numa task propria para o boot nao esperar o lock do store.
pub fn iniciar(config: &garraia_config::AppConfig, store: Option<Arc<Mutex<SessionStore>>>) {
    let Some(store) = store else {
        debug!("retencao do ledger de runs: sem sessions.db, nada a fazer");
        return;
    };
    match RunsRetentionConfig::from_app_config(config) {
        Some(retencao) => {
            let dias = retencao.retention_days;
            let handle = spawn_runs_retention_worker(store, retencao);
            // Detach: a varredura vive enquanto o processo viver, como a da
            // memoria.
            std::mem::forget(handle);
            info!(
                retention_days = dias,
                "runs_retention_worker spawned: apaga runs terminais mais velhos que a janela, a cada 24 horas"
            );
        }
        None => {
            tokio::spawn(async move {
                let total = store.lock().await.count_agent_runs();
                match total {
                    Ok(total) => info!(
                        total,
                        "retencao do ledger de runs desligada (runs.retention_days=0): o ledger \
                         cresce sem teto. Ligue com `runs: {{ retention_days: N }}` no config.yml."
                    ),
                    Err(e) => debug!(erro = %e, "retencao do ledger de runs: contagem falhou"),
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_com_runs() -> Arc<Mutex<SessionStore>> {
        let st = SessionStore::in_memory().expect("store");
        st.start_agent_run("feito", None, "objetivo feito", None)
            .expect("abre");
        st.finish_agent_run("feito", garraia_db::RunStatus::Done, Some("ok"), None)
            .expect("fecha");
        st.start_agent_run("vivo", None, "objetivo vivo", None)
            .expect("abre");
        Arc::new(Mutex::new(st))
    }

    #[test]
    fn zero_nao_produz_worker() {
        let mut config = garraia_config::AppConfig::default();
        assert!(
            RunsRetentionConfig::from_app_config(&config).is_none(),
            "a retencao nasce desligada"
        );
        config.runs.retention_days = 30;
        let r = RunsRetentionConfig::from_app_config(&config).expect("ligada");
        assert_eq!(r.retention_days, 30);
        assert_eq!(r.interval, INTERVALO);
    }

    #[test]
    fn cutoff_e_puro_e_recusa_absurdo() {
        let agora = DateTime::parse_from_rfc3339("2026-09-21T12:00:00Z")
            .expect("data")
            .with_timezone(&Utc);
        let corte = cutoff_for(agora, 30).expect("30 dias cabem");
        assert_eq!(corte.to_rfc3339(), "2026-08-22T12:00:00+00:00");
        assert!(cutoff_for(agora, u32::MAX).is_none());
    }

    /// Com o presente adiantado, o run terminal sai e o `running` fica.
    #[tokio::test]
    async fn tick_apaga_terminal_velho_e_poupa_running() {
        let store = store_com_runs();
        let futuro = Utc::now() + chrono::Duration::days(400);
        let apagados = run_retention_tick(&store, futuro, 30).await.expect("tick");
        assert_eq!(apagados, 1);
        let restantes = store
            .lock()
            .await
            .list_recent_agent_runs(10)
            .expect("lista");
        assert_eq!(restantes.len(), 1);
        assert_eq!(restantes[0].id, "vivo");
    }

    #[tokio::test]
    async fn tick_sem_nada_velho_nao_apaga() {
        let store = store_com_runs();
        assert_eq!(run_retention_tick(&store, Utc::now(), 30).await, Some(0));
        assert_eq!(store.lock().await.count_agent_runs().expect("conta"), 2);
    }

    /// Corte impossivel nao apaga nada — nem chega ao banco.
    #[tokio::test]
    async fn tick_com_corte_impossivel_nao_roda() {
        let store = store_com_runs();
        assert_eq!(run_retention_tick(&store, Utc::now(), u32::MAX).await, None);
        assert_eq!(store.lock().await.count_agent_runs().expect("conta"), 2);
    }

    /// O log deste modulo leva contagem e dias, nunca conteudo do ledger.
    /// `concat!` impede o teste de casar consigo mesmo.
    #[test]
    fn log_nunca_carrega_conteudo_do_ledger() {
        let src = include_str!("runs_retention_worker.rs");
        let fim = src
            .find(concat!("#[cfg(", "test)]"))
            .expect("modulo de teste");
        let producao = &src[..fim];
        for agulha in [
            concat!("go", "al ="),
            concat!("go", "al,"),
            concat!(".go", "al"),
            concat!("snip", "pet ="),
            concat!("_snip", "pet"),
        ] {
            assert!(
                !producao.contains(agulha),
                "o worker nao pode logar conteudo do ledger ({agulha})"
            );
        }
    }
}
