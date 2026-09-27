use super::*;

// ─── #957: as metricas da memoria ─────────────────────────────────────

/// Nome + labels de tudo que foi emitido enquanto `f` rodava.
///
/// O recorder e **thread-local**, nao global, de proposito: o recorder
/// global do ecossistema `metrics` so pode ser instalado uma vez por
/// processo, e um teste que o instalasse quebraria todos os outros que
/// rodam em paralelo. `#[tokio::test]` usa o runtime `current_thread`,
/// entao o guard cobre o bloco inteiro sem risco de a task migrar de
/// thread no meio.
pub(super) async fn metricas_emitidas<F, Fut>(f: F) -> Vec<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let recorder = metrics_util::debugging::DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let guard = ::metrics::set_default_local_recorder(&recorder);
    f().await;
    drop(guard);

    let mut nomes: Vec<String> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(chave, _, _, _)| {
            let key = chave.key();
            let mut labels: Vec<String> = key
                .labels()
                .map(|l| format!("{}={}", l.key(), l.value()))
                .collect();
            labels.sort();
            if labels.is_empty() {
                key.name().to_string()
            } else {
                format!("{}{{{}}}", key.name(), labels.join(","))
            }
        })
        .collect();
    nomes.sort();
    nomes
}

/// O caso que a issue #957 descreve: falha de embedding era silenciosa. O
/// #948 tirou o silencio do log; isto tira do painel.
#[tokio::test]
pub(super) async fn falha_de_embedding_vira_contador() {
    struct SempreFalha;

    #[async_trait]
    impl crate::embeddings::EmbeddingProvider for SempreFalha {
        fn provider_id(&self) -> &str {
            "provider-de-teste"
        }
        fn model(&self) -> &str {
            "modelo"
        }
        async fn embed_documents(&self, _t: &[String]) -> garraia_common::Result<Vec<Vec<f32>>> {
            Err(garraia_common::Error::Agent("fora do ar".into()))
        }
        async fn embed_query(&self, _t: &str) -> garraia_common::Result<Vec<f32>> {
            Err(garraia_common::Error::Agent("fora do ar".into()))
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(false)
        }
    }

    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    rt.set_embedding_provider(Arc::new(SempreFalha));

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn("s1", None, None, "meu nome e Michel e moro na Florida", "")
            .await
            .expect("remember_turn");
    })
    .await;

    assert!(
        emitidas.iter().any(|m| m
            == "garraia_memory_embed_failures_total{operation=document,provider=provider-de-teste}"),
        "falha nao virou contador: {emitidas:?}"
    );
    assert!(
        emitidas
            .iter()
            .any(|m| m == "garraia_memory_ingested_total{outcome=failed}"),
        "desfecho `failed` nao foi contado: {emitidas:?}"
    );
    // A latencia da tentativa que falhou tambem entra: e o timeout que o
    // operador precisa ver. Antes da auditoria do #957 este ramo nao era
    // medido, e a p95 melhorava durante uma rajada de falha.
    assert!(
        emitidas.iter().any(|m| m
            == "garraia_memory_embed_latency_seconds{operation=document,provider=provider-de-teste}"),
        "a tentativa que falhou nao foi medida: {emitidas:?}"
    );
}

/// Os quatro desfechos precisam ser distinguiveis. `no_provider` e
/// `failed` sao a diferenca entre "ninguem configurou" e "configurou e
/// esta quebrado" — a pergunta que o operador faz primeiro.
#[tokio::test]
pub(super) async fn sem_provider_e_desfecho_proprio_nao_falha() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    // Sem `set_embedding_provider`.

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn("s1", None, None, "um fato de verdade para lembrar", "")
            .await
            .expect("remember_turn");
    })
    .await;

    assert!(
        emitidas
            .iter()
            .any(|m| m == "garraia_memory_ingested_total{outcome=no_provider}"),
        "{emitidas:?}"
    );
    assert!(
        !emitidas.iter().any(|m| m.contains("embed_failures")),
        "sem provider nao e falha do provider: {emitidas:?}"
    );
}

/// Ruido tem desfecho proprio (#952). Sem isso, o operador veria o total
/// de entradas sem vetor subir e nao teria como saber se e defeito ou
/// politica.
#[tokio::test]
pub(super) async fn ruido_tem_desfecho_proprio_e_nao_chama_o_provider() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let embeddings = Arc::new(ContandoEmbeddings(std::sync::atomic::AtomicUsize::new(0)));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    rt.set_embedding_provider(embeddings.clone());

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn("s1", None, None, "oi", "bom dia")
            .await
            .expect("remember_turn");
    })
    .await;

    assert!(
        emitidas
            .iter()
            .any(|m| m == "garraia_memory_ingested_total{outcome=noise}"),
        "{emitidas:?}"
    );
    assert_eq!(chamadas(&embeddings), 0, "ruido nao pode ir ao provider");
    assert!(
        !emitidas.iter().any(|m| m.contains("embed_latency")),
        "nao houve chamada, nao pode haver latencia: {emitidas:?}"
    );
}

/// O caminho feliz: latencia medida por provider e por operacao, e o
/// desfecho contado como `embedded`.
#[tokio::test]
pub(super) async fn sucesso_mede_latencia_por_provider_e_operacao() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    rt.set_embedding_provider(Arc::new(ContandoEmbeddings(
        std::sync::atomic::AtomicUsize::new(0),
    )));

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn("s1", None, None, "um fato de verdade para lembrar", "")
            .await
            .expect("remember_turn");
        rt.recall_context("quem sou eu", Some("s1"), None, 5)
            .await
            .expect("recall");
    })
    .await;

    assert!(
        emitidas
            .iter()
            .any(|m| m
                == "garraia_memory_embed_latency_seconds{operation=document,provider=contando}"),
        "{emitidas:?}"
    );
    assert!(
        emitidas
            .iter()
            .any(|m| m
                == "garraia_memory_embed_latency_seconds{operation=query,provider=contando}"),
        "a consulta do recall nao foi medida: {emitidas:?}"
    );
    assert!(
        emitidas
            .iter()
            .any(|m| m == "garraia_memory_recall_latency_seconds"),
        "{emitidas:?}"
    );
    assert!(
        emitidas
            .iter()
            .any(|m| m == "garraia_memory_ingested_total{outcome=embedded}"),
        "{emitidas:?}"
    );
}

/// Todo valor da label `provider` tem de vir do conjunto conhecido.
///
/// A primeira versao deste teste afirmava **ausencia** — que nenhuma label
/// contivesse certas strings proibidas —, e a auditoria do #957 mostrou
/// que isso passa vazio: um provider futuro que devolvesse id dinamico sem
/// nenhuma daquelas strings teria cardinalidade ilimitada e o teste
/// continuaria verde. Afirmar **presenca** num conjunto fechado e o que
/// realmente cobra a invariante.
#[tokio::test]
pub(super) async fn label_de_provider_vem_de_conjunto_fechado() {
    // Os tres de producao mais os de teste deste arquivo. Um provider novo
    // tem de entrar aqui de proposito, o que e o ponto: a lista e a
    // revisao.
    const CONHECIDOS: &[&str] = &["ollama", "openai", "cohere", "contando"];

    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    rt.set_embedding_provider(Arc::new(ContandoEmbeddings(
        std::sync::atomic::AtomicUsize::new(0),
    )));

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn("s1", None, None, "um fato de verdade para lembrar", "")
            .await
            .expect("remember_turn");
        rt.recall_context("quem sou eu", Some("s1"), None, 5)
            .await
            .expect("recall");
    })
    .await;

    let mut viu_provider = false;
    for m in &emitidas {
        let Some(inicio) = m.find("provider=") else {
            continue;
        };
        viu_provider = true;
        let resto = &m[inicio + "provider=".len()..];
        let valor = resto
            .split([',', '}'])
            .next()
            .expect("split sempre devolve ao menos um pedaco");
        assert!(
            CONHECIDOS.contains(&valor),
            "label `provider` fora do conjunto fechado: {valor:?} em {m}"
        );
    }
    assert!(
        viu_provider,
        "o teste precisa ter visto ao menos um provider"
    );
}
