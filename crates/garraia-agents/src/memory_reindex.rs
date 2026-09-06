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
use crate::memory_noise::NoisePolicy;

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
    /// #952: a **mesma** politica de ruido da ingestao.
    ///
    /// Tem que ser a mesma, e nao um default local: uma entrada que a
    /// ingestao pulou por ser ruido fica com `embedding IS NULL`, que e
    /// exatamente o que este comando procura. Com politicas diferentes, o
    /// `garra memory reindex` reembeddaria "oi" e "bom dia" um por um,
    /// desfazendo o filtro e pagando provider por isso.
    pub noise: NoisePolicy,
}

impl Default for ReindexOptions {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_REINDEX_BATCH_SIZE,
            max_entries: None,
            noise: NoisePolicy::default(),
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
    /// #952: entradas que ficam sem vetor de proposito, por serem ruido.
    ///
    /// Elas continuam com `embedding IS NULL`, entao continuam contadas em
    /// `pending_before` e no `garra memory stats`. Este numero e o que
    /// explica por que aquele total nao vai a zero.
    pub skipped_noise: usize,
    /// Quantas entradas **seriam** reindexadas. So preenchido em `dry_run`.
    pub would_reindex: usize,
    /// `true` quando o provider falhou e a reindexacao parou no meio.
    pub stopped_early: bool,
    pub dry_run: bool,
}

/// Conta o que uma reindexacao faria, sem provider e sem gravar (#952).
///
/// Nao precisa de `EmbeddingProvider` **e a assinatura diz isso**: em dry-run
/// nenhum vetor e pedido, entao exigir um provider seria recusar a pergunta
/// "quanto tem para fazer?" justamente a quem ainda nao configurou um.
///
/// Antes do #952 esta resposta era so `entries_without_embedding`, e agora
/// esse numero passou a incluir entradas que ficam sem vetor **de proposito**.
/// Sem separar as duas coisas, o total nunca vai a zero e o operador nao tem
/// como saber se isso e saude ou defeito.
pub fn preview_reindex(store: &MemoryStore, options: &ReindexOptions) -> Result<ReindexReport> {
    let pending_before = store.integrity_report()?.entries_without_embedding;
    let mut report = ReindexReport {
        pending_before,
        index_repaired: 0,
        reindexed: 0,
        vanished: 0,
        skipped_noise: 0,
        would_reindex: 0,
        stopped_early: false,
        dry_run: true,
    };
    if pending_before == 0 {
        return Ok(report);
    }

    let batch_size = options.batch_size.max(1);
    // Em dry-run nada sai da fila, entao o cursor avanca sobre **tudo** que
    // foi olhado — senao a mesma pagina se repetiria para sempre.
    let mut cursor: Option<(chrono::DateTime<chrono::Utc>, String)> = None;
    let mut examinadas = 0usize;

    loop {
        let restante = match options.max_entries {
            Some(teto) => teto.saturating_sub(examinadas),
            None => batch_size,
        };
        if restante == 0 {
            break;
        }

        let lote = store.entries_missing_embeddings_after(
            batch_size.min(restante),
            cursor.as_ref().map(|(ts, id)| (*ts, id.as_str())),
        )?;
        let Some(ultima) = lote.last() else {
            break;
        };
        cursor = Some((ultima.created_at, ultima.id.clone()));

        for entrada in &lote {
            if options.noise.is_noise(&entrada.content) {
                report.skipped_noise += 1;
            } else {
                report.would_reindex += 1;
            }
            examinadas += 1;
        }
    }

    Ok(report)
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
        skipped_noise: 0,
        would_reindex: 0,
        stopped_early: false,
        dry_run: false,
    };

    // Reparo barato primeiro: reinserir no indice um vetor que ja esta na
    // coluna nao custa chamada de provider nenhuma, e sem isso o `reindex`
    // deixaria de fora justamente as entradas que o `remember_sync` gravou
    // enquanto o sqlite-vec estava fora — elas nao aparecem em
    // `entries_missing_embeddings`, que procura `embedding IS NULL`.
    //
    // A politica de ruido do #952 nao se aplica a este reparo, de proposito:
    // estas entradas **ja tem** o vetor na coluna, gravado antes de a politica
    // existir ou com ela desligada. Deixa-las fora do indice seria uma
    // limpeza retroativa disfarcada de reparo, e apagar passado e decisao do
    // operador (`garra memory compact`), nunca efeito colateral de reindexar.
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

    // Onde a fila parou. Chave da ultima linha vista, nao um `OFFSET`: o
    // `memory_retention_worker` do gateway apaga entradas em paralelo, e um
    // ponteiro numerico se desloca quando uma linha some antes dele — o lote
    // seguinte pularia uma entrada legitima, que ficaria sem vetor ate a
    // proxima reindexacao manual. Ver `entries_missing_embeddings_after`.
    let mut cursor: Option<(chrono::DateTime<chrono::Utc>, String)> = None;
    // Tudo que ja foi olhado — reindexado, sumido ou pulado. E o que o teto
    // de `--limit` limita: pular ruido tambem e trabalho, e sem contar aqui
    // um `--limit 10` numa base so de ruido varreria a base inteira.
    let mut examinadas = 0usize;

    loop {
        let restante = match options.max_entries {
            Some(teto) => teto.saturating_sub(examinadas),
            None => batch_size,
        };
        if restante == 0 {
            break;
        }

        let lote = store.entries_missing_embeddings_after(
            batch_size.min(restante),
            cursor.as_ref().map(|(ts, id)| (*ts, id.as_str())),
        )?;
        let Some(ultima) = lote.last() else {
            break;
        };
        // O cursor avanca sobre o lote **inteiro**, antes de qualquer
        // gravacao: toda linha daqui ja foi decidida, e nenhuma precisa ser
        // olhada de novo nesta execucao.
        cursor = Some((ultima.created_at, ultima.id.clone()));
        examinadas += lote.len();

        // `partition` preserva a ordem relativa dentro de cada metade, o que
        // mantem o pareamento texto <-> vetor mais abaixo.
        let (ruido, lote): (Vec<_>, Vec<_>) = lote
            .into_iter()
            .partition(|e| options.noise.is_noise(&e.content));
        report.skipped_noise += ruido.len();
        if lote.is_empty() {
            // Lote inteiro era ruido. Nao ha o que mandar ao provider, e o
            // cursor ja avancou: a proxima volta olha adiante.
            continue;
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

        for (entrada, vetor) in lote.iter().zip(vetores) {
            if store.set_embedding(&entrada.id, &vetor, &model)? {
                report.reindexed += 1;
            } else {
                report.vanished += 1;
            }
        }

        // O guard antigo ("se nao gravou nada, para") saiu junto com o
        // #952: um lote inteiro de ruido nao grava nada e nao e motivo para
        // parar. O progresso e garantido pelo cursor, que avanca sobre o lote
        // inteiro antes de qualquer decisao — nenhuma linha ja olhada volta.
    }

    info!(
        index_repaired = report.index_repaired,
        reindexed = report.reindexed,
        vanished = report.vanished,
        skipped_noise = report.skipped_noise,
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

    /// O ponto do #952 no reindex: o comando NAO pode desfazer o filtro da
    /// ingestao. Sem isto, `garra memory reindex` reembeddaria "oi" e "bom
    /// dia" um por um, pagando provider para reintroduzir o ruido.
    #[tokio::test]
    async fn reindex_nao_reembedda_o_ruido_que_a_ingestao_pulou() {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        for ruido in ["oi", "ok", "bom dia", "kkkk", "obrigado"] {
            store
                .remember(entrada(ruido, None))
                .await
                .expect("remember");
        }
        for real in ["meu nome e Michel e eu moro na Florida", "prefiro Postgres"] {
            store.remember(entrada(real, None)).await.expect("remember");
        }

        let provider = FakeProvider::new(3);
        let report = reindex_missing_embeddings(&store, &provider, ReindexOptions::default())
            .await
            .expect("reindex");

        assert_eq!(report.pending_before, 7);
        assert_eq!(report.reindexed, 2, "so as duas de verdade");
        assert_eq!(report.skipped_noise, 5);
        assert!(!report.stopped_early, "ruido nao e motivo para parar");

        // O ruido continua gravado e continua sem vetor — nada foi apagado.
        let depois = store.integrity_report().expect("report");
        assert_eq!(depois.entries_without_embedding, 5);
        assert_eq!(depois.entries_with_embedding, 2);
    }

    /// O caso que trava um loop ingenuo: entrada pulada continua com
    /// `embedding IS NULL`, ou seja, continua na fila. Sem o offset, o lote
    /// seguinte devolveria as mesmas linhas para sempre.
    #[tokio::test]
    async fn ruido_no_comeco_da_fila_nao_bloqueia_o_que_vem_depois() {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        // Um lote inteiro de ruido antes de qualquer conteudo real.
        for _ in 0..4 {
            store.remember(entrada("ok", None)).await.expect("remember");
        }
        store
            .remember(entrada("o deploy de sexta usa a imagem 1.2.3", None))
            .await
            .expect("remember");

        let report = reindex_missing_embeddings(
            &store,
            &FakeProvider::new(3),
            ReindexOptions {
                batch_size: 2,
                ..ReindexOptions::default()
            },
        )
        .await
        .expect("reindex");

        assert_eq!(report.skipped_noise, 4);
        assert_eq!(report.reindexed, 1, "a entrada real do fim da fila entrou");
    }

    /// Com a politica desligada o comando volta a ser o de antes do #952:
    /// reembedda tudo, inclusive o que hoje seria ruido. E a saida de quem
    /// discorda do filtro.
    #[tokio::test]
    async fn politica_desligada_reembedda_tudo() {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        for ruido in ["oi", "ok", "bom dia"] {
            store
                .remember(entrada(ruido, None))
                .await
                .expect("remember");
        }

        let report = reindex_missing_embeddings(
            &store,
            &FakeProvider::new(3),
            ReindexOptions {
                noise: NoisePolicy::disabled(),
                ..ReindexOptions::default()
            },
        )
        .await
        .expect("reindex");

        assert_eq!(report.skipped_noise, 0);
        assert_eq!(report.reindexed, 3);
    }

    /// O dry-run separa "vai ser reindexada" de "fica sem vetor de proposito".
    /// Sem essa separacao o total de `stats` nunca chega a zero e o operador
    /// nao tem como saber se isso e saude ou defeito.
    #[tokio::test]
    async fn dry_run_separa_o_que_seria_feito_do_que_e_ruido() {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        for ruido in ["oi", "valeu", "kkkkk"] {
            store
                .remember(entrada(ruido, None))
                .await
                .expect("remember");
        }
        for i in 0..2 {
            store
                .remember(entrada(&format!("fato de verdade numero {i}"), None))
                .await
                .expect("remember");
        }

        let report = preview_reindex(
            &store,
            &ReindexOptions {
                batch_size: 2,
                ..ReindexOptions::default()
            },
        )
        .expect("preview");

        assert!(report.dry_run);
        assert_eq!(report.pending_before, 5);
        assert_eq!(report.would_reindex, 2);
        assert_eq!(report.skipped_noise, 3);

        // Dry-run nao grava: a fila continua inteira.
        assert_eq!(
            store
                .integrity_report()
                .expect("r")
                .entries_without_embedding,
            5
        );
    }

    /// `--limit` conta o que foi **olhado**, nao so o que foi gravado: sem
    /// isso, um `--limit 10` numa base cheia de ruido varreria a base toda.
    #[tokio::test]
    async fn limite_conta_o_ruido_pulado_como_trabalho() {
        let store = MemoryStore::in_memory_with_vectors().expect("store");
        for _ in 0..20 {
            store.remember(entrada("ok", None)).await.expect("remember");
        }

        let report = reindex_missing_embeddings(
            &store,
            &FakeProvider::new(3),
            ReindexOptions {
                batch_size: 4,
                max_entries: Some(8),
                ..ReindexOptions::default()
            },
        )
        .await
        .expect("reindex");

        assert_eq!(
            report.skipped_noise, 8,
            "parou no teto, nao na base inteira"
        );
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

    /// Depois da auditoria do #952 o dry-run deixou de ser uma flag de
    /// `ReindexOptions`: `reindex_missing_embeddings` exigia um provider e o
    /// descartava, e a assinatura mentia para quem chamasse a API. Quem quer
    /// so contar chama `preview_reindex`, que nao pede provider nenhum.
    #[tokio::test]
    async fn dry_run_touches_nothing() {
        let store = store_com(3, 0).await;

        let report = preview_reindex(&store, &ReindexOptions::default()).expect("preview");

        assert_eq!(report.pending_before, 3);
        assert_eq!(report.reindexed, 0);
        assert_eq!(report.would_reindex, 3);
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
