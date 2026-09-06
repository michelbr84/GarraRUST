//! Reindexacao da memoria semantica (#953).
//!
//! Mora **aqui** e nao no `MemoryStore`, que e onde a issue propunha, por uma
//! razao de camada: o `EmbeddingProvider` e deste crate, e `garraia-agents` ja
//! depende de `garraia-db`. Pedir ao store que receba um provider exigiria a
//! dependencia inversa, e o ciclo nao fecha. As primitivas ficaram no store
//! (`entries_missing_embeddings`, `set_embedding`); o que precisa dos dois
//! lados fica aqui.

use garraia_common::Result;
use garraia_db::MemoryStore;
use tracing::{info, warn};

use crate::embeddings::EmbeddingProvider;

/// Quantas entradas por ida ao provider.
///
/// O lote do Ollama ja e paralelo internamente (#962), entao este numero e
/// sobre o tamanho da transacao logica, nao sobre concorrencia: pequeno o
/// bastante para que uma interrupcao no meio de uma base de milhares perca
/// pouco trabalho, grande o bastante para nao virar uma ida por entrada.
pub const DEFAULT_REINDEX_BATCH_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub struct ReindexOptions {
    pub batch_size: usize,
    /// Teto de entradas a processar. `None` = ate acabar a fila.
    pub max_entries: Option<usize>,
    /// So conta o que seria feito, sem chamar o provider nem gravar.
    pub dry_run: bool,
}

impl Default for ReindexOptions {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_REINDEX_BATCH_SIZE,
            max_entries: None,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexReport {
    /// Entradas sem vetor antes de comecar.
    pub pending_before: usize,
    /// Entradas que ja tinham vetor na coluna e faltavam no indice, e foram
    /// reinseridas sem custo de provider. E o estado que o `remember_sync`
    /// produz quando o sqlite-vec falha na ingestao.
    pub index_repaired: usize,
    pub reindexed: usize,
    /// Entradas que sumiram entre listar e gravar — nao e erro.
    pub vanished: usize,
    /// `true` quando o provider falhou e a reindexacao parou no meio.
    pub stopped_early: bool,
    pub dry_run: bool,
}

/// Reprocessa entradas gravadas sem vetor.
///
/// Para no primeiro lote que falha, de proposito: uma falha de provider quase
/// sempre significa que ele esta fora, e insistir lote apos lote so demora
/// para dar a mesma noticia. O relatorio diz quanto foi feito, e rodar de novo
/// depois continua de onde parou — a fila e derivada do banco, nao de estado
/// em memoria.
pub async fn reindex_missing_embeddings(
    store: &MemoryStore,
    provider: &dyn EmbeddingProvider,
    options: ReindexOptions,
) -> Result<ReindexReport> {
    let pending_before = store.integrity_report()?.entries_without_embedding;
    let model = provider.model().to_string();

    let mut report = ReindexReport {
        pending_before,
        index_repaired: 0,
        reindexed: 0,
        vanished: 0,
        stopped_early: false,
        dry_run: options.dry_run,
    };

    if options.dry_run {
        return Ok(report);
    }

    // Reparo barato primeiro: reinserir no indice um vetor que ja esta na
    // coluna nao custa chamada de provider nenhuma, e sem isso o `reindex`
    // deixaria de fora justamente as entradas que o `remember_sync` gravou
    // enquanto o sqlite-vec estava fora — elas nao aparecem em
    // `entries_missing_embeddings`, que procura `embedding IS NULL`.
    report.index_repaired =
        store.reindex_missing_index_rows(options.max_entries.unwrap_or(usize::MAX))?;
    if report.index_repaired > 0 {
        info!(
            reparadas = report.index_repaired,
            "vetores que ja estavam na coluna voltaram para o indice"
        );
    }

    if pending_before == 0 {
        return Ok(report);
    }

    let batch_size = options.batch_size.max(1);

    loop {
        let restante = match options.max_entries {
            Some(teto) => teto.saturating_sub(report.reindexed + report.vanished),
            None => batch_size,
        };
        if restante == 0 {
            break;
        }

        let lote = store.entries_missing_embeddings(batch_size.min(restante))?;
        if lote.is_empty() {
            break;
        }

        let textos: Vec<String> = lote.iter().map(|e| e.content.clone()).collect();
        let vetores = match provider.embed_documents(&textos).await {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    provider = provider.provider_id(),
                    model = model.as_str(),
                    reindexed = report.reindexed,
                    "reindexacao interrompida: o provider de embeddings falhou. \
                     Rode de novo quando ele voltar — a fila continua de onde parou: {e}"
                );
                report.stopped_early = true;
                break;
            }
        };

        // Um provider que devolve menos vetores do que textos quebraria o
        // pareamento por indice e gravaria o vetor errado na entrada errada.
        if vetores.len() != lote.len() {
            warn!(
                provider = provider.provider_id(),
                esperados = lote.len(),
                recebidos = vetores.len(),
                "reindexacao interrompida: o provider devolveu um numero de vetores \
                 diferente do numero de textos, e parear por indice gravaria vetor \
                 na entrada errada"
            );
            report.stopped_early = true;
            break;
        }

        let mut gravou_algo = false;
        for (entrada, vetor) in lote.iter().zip(vetores) {
            if store.set_embedding(&entrada.id, &vetor, &model)? {
                report.reindexed += 1;
                gravou_algo = true;
            } else {
                report.vanished += 1;
            }
        }

        // Sem isto, um lote que nao grava nada devolveria as mesmas linhas
        // para sempre — a fila vem do banco, e o que nao foi gravado continua
        // nela.
        if !gravou_algo {
            report.stopped_early = true;
            break;
        }
    }

    info!(
        index_repaired = report.index_repaired,
        reindexed = report.reindexed,
        vanished = report.vanished,
        pending_before = report.pending_before,
        "reindexacao da memoria concluida"
    );

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::EmbeddingProvider;
    use async_trait::async_trait;
    use garraia_common::Error;
    use garraia_db::{MemoryRole, NewMemoryEntry};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProvider {
        dims: usize,
        fail_after: Option<usize>,
        calls: AtomicUsize,
    }

    impl FakeProvider {
        fn new(dims: usize) -> Self {
            Self {
                dims,
                fail_after: None,
                calls: AtomicUsize::new(0),
            }
        }

        fn failing_after(dims: usize, batches: usize) -> Self {
            Self {
                dims,
                fail_after: Some(batches),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl EmbeddingProvider for FakeProvider {
        fn provider_id(&self) -> &str {
            "fake"
        }

        fn model(&self) -> &str {
            "fake-model"
        }

        async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(limite) = self.fail_after
                && n >= limite
            {
                return Err(Error::Agent("provider fora do ar".into()));
            }
            Ok(texts.iter().map(|_| vec![0.5; self.dims]).collect())
        }

        async fn embed_query(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![0.5; self.dims])
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    async fn store_com(sem_vetor: usize, com_vetor: usize) -> MemoryStore {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        for i in 0..sem_vetor {
            store
                .remember(entrada(&format!("sem vetor {i}"), None))
                .await
                .expect("remember");
        }
        for i in 0..com_vetor {
            store
                .remember(entrada(
                    &format!("com vetor {i}"),
                    Some(vec![0.1, 0.2, 0.3]),
                ))
                .await
                .expect("remember");
        }
        store
    }

    fn entrada(content: &str, embedding: Option<Vec<f32>>) -> NewMemoryEntry {
        NewMemoryEntry {
            tenant_id: "default".to_string(),
            session_id: "s".to_string(),
            channel_id: None,
            user_id: None,
            continuity_key: None,
            role: MemoryRole::User,
            content: content.to_string(),
            embedding_model: embedding.as_ref().map(|_| "antigo".to_string()),
            embedding,
            metadata: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn reindexes_only_the_entries_without_a_vector() {
        let store = store_com(5, 3).await;
        let provider = FakeProvider::new(3);

        let report = reindex_missing_embeddings(&store, &provider, ReindexOptions::default())
            .await
            .expect("reindex");

        assert_eq!(report.pending_before, 5);
        assert_eq!(report.reindexed, 5);
        assert!(!report.stopped_early);

        let depois = store.integrity_report().expect("report");
        assert_eq!(depois.entries_without_embedding, 0);
        assert_eq!(depois.entries_with_embedding, 8, "as 3 antigas seguem la");
    }

    /// A entrada reindexada passa a ser encontravel pela busca semantica —
    /// que e o ponto inteiro do #953.
    #[tokio::test]
    async fn reindexed_entries_enter_the_vector_index() {
        let store = store_com(4, 0).await;
        assert_eq!(store.integrity_report().expect("r").map_rows, 0);

        reindex_missing_embeddings(&store, &FakeProvider::new(3), ReindexOptions::default())
            .await
            .expect("reindex");

        let depois = store.integrity_report().expect("report");
        assert_eq!(depois.map_rows, 4, "as quatro entraram no indice");
        assert!(depois.orphan_map_entries.is_empty());
    }

    #[tokio::test]
    async fn dry_run_touches_nothing() {
        let store = store_com(3, 0).await;
        let options = ReindexOptions {
            dry_run: true,
            ..ReindexOptions::default()
        };

        let report = reindex_missing_embeddings(&store, &FakeProvider::new(3), options)
            .await
            .expect("reindex");

        assert_eq!(report.pending_before, 3);
        assert_eq!(report.reindexed, 0);
        assert!(report.dry_run);
        assert_eq!(
            store
                .integrity_report()
                .expect("r")
                .entries_without_embedding,
            3,
            "dry-run nao pode gravar"
        );
    }

    /// Provider que cai no meio nao pode fazer a reindexacao girar em falso:
    /// ela para, relata o que fez, e rodar de novo continua de onde parou.
    #[tokio::test]
    async fn stops_when_the_provider_fails_and_keeps_what_was_done() {
        let store = store_com(10, 0).await;
        let provider = FakeProvider::failing_after(3, 1);
        let options = ReindexOptions {
            batch_size: 4,
            ..ReindexOptions::default()
        };

        let report = reindex_missing_embeddings(&store, &provider, options.clone())
            .await
            .expect("reindex");

        assert_eq!(report.reindexed, 4, "o primeiro lote passou");
        assert!(report.stopped_early);
        assert_eq!(
            store
                .integrity_report()
                .expect("r")
                .entries_without_embedding,
            6
        );

        // Provider de volta: a fila continua de onde parou, sem estado em memoria.
        let report = reindex_missing_embeddings(&store, &FakeProvider::new(3), options)
            .await
            .expect("reindex");
        assert_eq!(report.pending_before, 6);
        assert_eq!(report.reindexed, 6);
        assert_eq!(
            store
                .integrity_report()
                .expect("r")
                .entries_without_embedding,
            0
        );
    }

    #[tokio::test]
    async fn max_entries_caps_the_work() {
        let store = store_com(10, 0).await;
        let options = ReindexOptions {
            batch_size: 3,
            max_entries: Some(5),
            ..ReindexOptions::default()
        };

        let report = reindex_missing_embeddings(&store, &FakeProvider::new(3), options)
            .await
            .expect("reindex");

        assert_eq!(report.reindexed, 5, "para no teto, nao no fim da fila");
        assert_eq!(
            store
                .integrity_report()
                .expect("r")
                .entries_without_embedding,
            5
        );
    }

    #[tokio::test]
    async fn empty_queue_is_a_no_op() {
        let store = store_com(0, 2).await;

        let report =
            reindex_missing_embeddings(&store, &FakeProvider::new(3), ReindexOptions::default())
                .await
                .expect("reindex");

        assert_eq!(report.pending_before, 0);
        assert_eq!(report.reindexed, 0);
        assert!(!report.stopped_early);
    }
}
