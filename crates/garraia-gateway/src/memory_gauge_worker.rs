//! Publica o tamanho da memoria no `/metrics`, periodicamente (#957).
//!
//! O #994 entregou quatro metricas de **instrumentacao**: elas contam o que
//! aconteceu quando aconteceu — uma chamada de embedding, um recall, um turno
//! ingerido. Faltavam as de **estado**: quantas entradas existem agora, e
//! quantas estao no indice. Estado nao tem evento, entao alguem precisa ir
//! olhar de tempos em tempos.
//!
//! # Por que um worker proprio, e nao um braco do de retencao
//!
//! Era o caminho obvio — ja existe um laco periodico tocando a memoria — e e
//! errado por dois motivos independentes.
//!
//! O primeiro **desliga a metrica para quase todo mundo**: o
//! `memory_retention_worker` so sobe quando `memory.retention.enabled` e
//! `true`, e ela nasce `false` de proposito, porque apaga dado. Os gauges
//! ficariam mortos em toda instalacao que nao ligou a retencao, que e a
//! maioria — e um painel que nao mostra nada e indistinguivel de um sistema
//! que nao tem nada.
//!
//! O segundo e cadencia: retencao roda a cada 24h por padrao. Um gauge com
//! 24h de resolucao nao mostra tendencia; mostra dois pontos por semana.
//!
//! # Este worker nao apaga nada
//!
//! E a diferenca que justifica ele nascer **ligado**. A retencao nasce
//! desligada porque apagar memoria de quem so atualizou a versao seria um
//! estrago. Aqui o pior caso e tres `count(*)` a cada poucos minutos.

use std::sync::Arc;
use std::time::Duration;

use garraia_db::MemoryProvider;
use tracing::{debug, warn};

/// De quanto em quanto tempo reler o tamanho.
///
/// Cinco minutos e o meio termo: um `scrape_interval` tipico do Prometheus e
/// de 15s a 1min, entao valores mais lentos que isso viram degraus visiveis no
/// grafico; mais rapido que isso paga `count(*)` sem ninguem olhar.
const INTERVALO_PADRAO: Duration = Duration::from_secs(300);

/// Sobe o laco que republica o tamanho da memoria.
///
/// Devolve o `JoinHandle` para quem chama decidir o ciclo de vida — o
/// `server.rs` faz `mem::forget`, como nos outros workers.
pub fn spawn_memory_gauge_worker(memory: Arc<dyn MemoryProvider>) -> tokio::task::JoinHandle<()> {
    spawn_com_intervalo(memory, INTERVALO_PADRAO)
}

fn spawn_com_intervalo(
    memory: Arc<dyn MemoryProvider>,
    intervalo: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(intervalo);
        // O primeiro tick do `interval` dispara imediatamente, o que e o que
        // se quer: o painel tem numero desde o boot, e nao so depois do
        // primeiro periodo.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            publicar_uma_vez(memory.as_ref()).await;
        }
    })
}

/// Uma leitura e a publicacao dela. Separado do laco para ser testavel sem
/// relogio.
pub async fn publicar_uma_vez(memory: &dyn MemoryProvider) {
    match memory.gauge_snapshot().await {
        Ok(g) => {
            garraia_common::metrics::set_memory_size(
                g.entries_with_embedding,
                g.entries_without_embedding,
                g.vector_index_rows,
            );
            debug!(
                com_vetor = g.entries_with_embedding,
                sem_vetor = g.entries_without_embedding,
                no_indice = g.vector_index_rows,
                "memory gauges atualizados"
            );
        }
        // Falhar a leitura **nao** derruba o laco: o proximo tick tenta de
        // novo. Um erro transitorio de SQLite nao pode custar a metrica para
        // sempre — e um gauge que para de atualizar em silencio e pior que um
        // que some, porque o Prometheus continua servindo o ultimo valor e o
        // painel mostra um numero velho como se fosse atual.
        Err(e) => {
            // O contador ao lado e o que torna a falha **alertavel**. Sem ele,
            // uma falha persistente congelaria os gauges sem sinal nenhum: o
            // Prometheus continua servindo o ultimo valor como atual, e o
            // painel mostra numero velho como se fosse novo.
            garraia_common::metrics::inc_memory_gauge_error();
            warn!("falha ao ler o tamanho da memoria para os gauges: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_db::MemoryStore;

    /// A publicacao le do store e nao entra em panico sem recorder instalado
    /// — que e o caso de todo teste e da CLI.
    #[tokio::test]
    async fn publicar_uma_vez_nao_entra_em_panico_sem_recorder() {
        let store = MemoryStore::in_memory().expect("store");
        publicar_uma_vez(&store).await;
    }

    /// Erro de leitura nao pode derrubar o laco. Aqui a leitura funciona; o
    /// que este teste ancora e que `publicar_uma_vez` **retorna** em vez de
    /// propagar — se alguem trocar por `?`, o laco passa a morrer no primeiro
    /// erro transitorio e o gauge congela sem aviso.
    #[tokio::test]
    async fn publicar_uma_vez_nao_propaga_erro() {
        let store = MemoryStore::in_memory().expect("store");
        let _: () = publicar_uma_vez(&store).await;
    }
}
