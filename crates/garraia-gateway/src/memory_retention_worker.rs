//! Varredura periodica de retencao da memoria do agente (#956, #959).
//!
//! O `MemoryStore::compact()` existe desde sempre e **nunca rodou em
//! producao**: os unicos chamadores eram os testes e, desde o #950, a CLI.
//! A memoria de longo prazo crescia sem teto, o recall degradava (mais
//! candidatos, mais ruido) e o backup ficava maior a cada dia.
//!
//! # Desligada por padrao, e o boot avisa
//!
//! Ligar a varredura por default numa atualizacao apagaria memoria de quem so
//! quis atualizar a versao. Entao `memory.retention.enabled` nasce `false`, e
//! quando esta assim o gateway loga **uma vez** quantas entradas existem e
//! como ligar — o operador ganha o sinal sem pagar com dado.
//!
//! # O que a varredura nunca apaga
//!
//! Entrada fixada (`pinned_at`). A politica nao sabe o que importa; o pin e
//! como o operador conta para ela.
//!
//! # Forma
//!
//! O tick e puro no que da: [`cutoff_for`] nao le relogio (recebe o `now`) e
//! [`run_retention_tick`] so orquestra store + relatorio. O laco em si e um
//! `tokio::time::interval` — a mesma forma do `uploads_worker` do plan 0047.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use garraia_db::{CompactionReport, MemoryProvider};
use tracing::{debug, info, warn};

/// Configuracao da varredura, ja resolvida a partir do `AppConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRetentionConfig {
    pub interval: Duration,
    pub max_age_days: u32,
}

impl MemoryRetentionConfig {
    /// Le a secao `memory.retention` — `None` quando a politica esta desligada
    /// ou a memoria inteira esta.
    pub fn from_app_config(config: &garraia_config::AppConfig) -> Option<Self> {
        let r = &config.memory.retention;
        if !config.memory.enabled || !r.enabled {
            return None;
        }
        Some(Self {
            interval: Duration::from_secs(u64::from(r.interval_hours) * 3600),
            max_age_days: r.max_age_days,
        })
    }
}

/// Instante de corte a partir de `now` — sem relogio proprio, para o teste
/// poder fixar o presente.
///
/// `None` quando a subtracao nao cabe no calendario. Nesse caso a varredura
/// **nao roda**: um corte impossivel e defeito de configuracao, e o lado
/// seguro de uma operacao que apaga dado e nao apagar nada.
pub fn cutoff_for(now: DateTime<Utc>, max_age_days: u32) -> Option<DateTime<Utc>> {
    now.checked_sub_signed(chrono::Duration::try_days(i64::from(max_age_days))?)
}

/// Uma varredura. Devolve o relatorio da compactacao.
pub async fn run_retention_tick(
    memory: &Arc<dyn MemoryProvider>,
    now: DateTime<Utc>,
    max_age_days: u32,
) -> Option<CompactionReport> {
    let cutoff = cutoff_for(now, max_age_days)?;
    match memory.compact(cutoff).await {
        Ok(report) => {
            if report.deleted_entries > 0 {
                info!(
                    deleted = report.deleted_entries,
                    max_age_days, "retencao da memoria: entradas antigas ou vencidas apagadas"
                );
            } else {
                debug!(
                    max_age_days,
                    "retencao da memoria: nada a apagar neste tick"
                );
            }
            Some(report)
        }
        Err(e) => {
            // Falhar uma varredura nao derruba o gateway nem para as
            // proximas: a memoria segue utilizavel, so maior do que a
            // politica pediu.
            warn!("retencao da memoria falhou neste tick: {e}");
            None
        }
    }
}

/// Sobe a varredura em segundo plano. O primeiro tick do `interval` dispara
/// imediatamente, entao a primeira varredura acontece no boot.
pub fn spawn_memory_retention_worker(
    memory: Arc<dyn MemoryProvider>,
    config: MemoryRetentionConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(config.interval);
        // Um gateway que ficou suspenso nao deve acordar e disparar todas as
        // varreduras perdidas de uma vez.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            run_retention_tick(&memory, Utc::now(), config.max_age_days).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use garraia_db::{MemoryRole, MemoryStore, NewMemoryEntry};

    fn entrada(conteudo: &str) -> NewMemoryEntry {
        NewMemoryEntry {
            tenant_id: "default".to_string(),
            session_id: "s1".to_string(),
            channel_id: None,
            user_id: None,
            continuity_key: None,
            role: MemoryRole::User,
            content: conteudo.to_string(),
            embedding: None,
            embedding_model: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn cutoff_anda_para_tras_o_numero_de_dias() {
        let agora = Utc::now();
        let corte = cutoff_for(agora, 90).expect("90 dias cabem");
        assert_eq!((agora - corte).num_days(), 90);
    }

    /// Duas guardas em serie contra um `max_age_days` absurdo: o `try_days`
    /// recusa a duracao antes de o `Duration::days` entrar em panico, e o
    /// `checked_sub_signed` recusa a data resultante quando ela sai do
    /// calendario. Nos dois casos a varredura simplesmente nao roda — o lado
    /// seguro de uma operacao que apaga dado.
    #[test]
    fn cutoff_recusa_valor_absurdo_em_vez_de_entrar_em_panico() {
        assert!(cutoff_for(Utc::now(), u32::MAX).is_none());
    }

    #[tokio::test]
    async fn tick_apaga_o_que_passou_da_janela_e_poupa_o_fixado() {
        let store = MemoryStore::in_memory().expect("store");
        store
            .remember(entrada("descartavel"))
            .await
            .expect("insere");
        let fixada = store.remember(entrada("fixada")).await.expect("insere");
        assert!(store.set_pinned(&fixada, true).expect("fixa"));

        let memory: Arc<dyn MemoryProvider> = Arc::new(store);

        // Em vez de envelhecer as linhas, adianta o relogio: `run_retention_tick`
        // recebe o `now`, entao um presente 200 dias a frente poe as duas
        // entradas fora da janela de 90 sem tocar no banco.
        let futuro = Utc::now() + ChronoDuration::days(200);
        let report = run_retention_tick(&memory, futuro, 90).await.expect("tick");

        assert_eq!(
            report.deleted_entries, 1,
            "a fixada tem de sobreviver mesmo fora da janela"
        );
    }

    #[tokio::test]
    async fn tick_sem_nada_velho_nao_apaga() {
        let store = MemoryStore::in_memory().expect("store");
        store.remember(entrada("recente")).await.expect("insere");

        let memory: Arc<dyn MemoryProvider> = Arc::new(store);
        let report = run_retention_tick(&memory, Utc::now(), 90)
            .await
            .expect("tick");
        assert_eq!(report.deleted_entries, 0);
    }

    #[test]
    fn config_desligada_nao_produz_worker() {
        let mut config = garraia_config::AppConfig::default();
        assert!(
            MemoryRetentionConfig::from_app_config(&config).is_none(),
            "a retencao nasce desligada"
        );

        config.memory.retention.enabled = true;
        let resolvida = MemoryRetentionConfig::from_app_config(&config).expect("ligada");
        assert_eq!(resolvida.max_age_days, 90);
        assert_eq!(resolvida.interval, Duration::from_secs(24 * 3600));

        // Politica ligada com a memoria desligada nao roda.
        config.memory.enabled = false;
        assert!(MemoryRetentionConfig::from_app_config(&config).is_none());
    }
}
