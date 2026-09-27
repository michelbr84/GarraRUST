use super::*;

/// Nenhuma label pode carregar id de sessao, de usuario ou conteudo — e a
/// explosao de cardinalidade que o docblock do `garraia-telemetry` descreve.
#[tokio::test]
pub(super) async fn nenhuma_label_carrega_identificador_ou_conteudo() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);
    rt.set_embedding_provider(Arc::new(ContandoEmbeddings(
        std::sync::atomic::AtomicUsize::new(0),
    )));

    let emitidas = metricas_emitidas(|| async {
        rt.remember_turn(
            "sessao-secreta-42",
            None,
            Some("usuario-secreto"),
            "minha senha do banco e 1234",
            "",
        )
        .await
        .expect("remember_turn");
    })
    .await;

    for m in &emitidas {
        for proibido in ["sessao-secreta", "usuario-secreto", "senha", "1234"] {
            assert!(!m.contains(proibido), "label vazou {proibido:?}: {m}");
        }
    }
    assert!(!emitidas.is_empty(), "o teste precisa ter emitido algo");
}

// ─── #952: ruido nao merece vetor ─────────────────────────────────────

/// Provider de embeddings que conta quantas vezes foi chamado.
///
/// O contador e metade do teste: a politica nao so evita gravar o vetor,
/// ela evita **pedir** o vetor. Numa conversa longa, uma ida ao provider
/// por "ok" nao e pouco.
pub(super) struct ContandoEmbeddings(pub(super) std::sync::atomic::AtomicUsize);

#[async_trait]
impl crate::embeddings::EmbeddingProvider for ContandoEmbeddings {
    fn provider_id(&self) -> &str {
        "contando"
    }
    fn model(&self) -> &str {
        "modelo-de-teste"
    }
    async fn embed_documents(&self, texts: &[String]) -> garraia_common::Result<Vec<Vec<f32>>> {
        self.0
            .fetch_add(texts.len(), std::sync::atomic::Ordering::SeqCst);
        Ok(texts.iter().map(|_| vec![0.25, 0.5, 0.75]).collect())
    }
    async fn embed_query(&self, _text: &str) -> garraia_common::Result<Vec<f32>> {
        Ok(vec![0.25, 0.5, 0.75])
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

pub(super) async fn runtime_com_memoria(
    policy: crate::memory_noise::NoisePolicy,
) -> (
    AgentRuntime,
    Arc<garraia_db::MemoryStore>,
    Arc<ContandoEmbeddings>,
) {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let embeddings = Arc::new(ContandoEmbeddings(std::sync::atomic::AtomicUsize::new(0)));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store.clone());
    rt.set_embedding_provider(embeddings.clone());
    rt.set_noise_policy(policy);
    (rt, store, embeddings)
}

pub(super) fn chamadas(e: &ContandoEmbeddings) -> usize {
    e.0.load(std::sync::atomic::Ordering::SeqCst)
}

/// O sintoma da issue: "oi" era gravado **com** vetor e disputava o
/// top-K com memoria de verdade.
#[tokio::test]
pub(super) async fn turno_de_ruido_e_gravado_sem_vetor_e_sem_ida_ao_provider() {
    let (rt, store, embeddings) =
        runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

    rt.remember_turn("s1", None, None, "oi", "bom dia")
        .await
        .expect("remember_turn");

    assert_eq!(chamadas(&embeddings), 0, "pediu vetor para ruido");
    let r = store.integrity_report().expect("report");
    assert_eq!(r.entries_with_embedding, 0);
    assert_eq!(r.entries_without_embedding, 2, "as duas seguem gravadas");
}

/// O outro lado da moeda, e o que impede o filtro de virar perda de
/// memoria: conteudo de verdade continua ganhando vetor.
#[tokio::test]
pub(super) async fn turno_de_verdade_continua_ganhando_vetor() {
    let (rt, store, embeddings) =
        runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

    rt.remember_turn(
        "s1",
        None,
        None,
        "meu nome e Michel e eu moro na Florida",
        "anotado: voce se chama Michel e mora na Florida",
    )
    .await
    .expect("remember_turn");

    assert_eq!(chamadas(&embeddings), 2);
    let r = store.integrity_report().expect("report");
    assert_eq!(r.entries_with_embedding, 2);
    assert_eq!(r.map_rows, 2, "as duas entraram no indice vetorial");
}

/// Um turno pode ter uma metade de ruido e outra de conteudo. Elas sao
/// decididas separadamente — a pergunta "ok" nao pode derrubar a resposta
/// que veio depois dela.
#[tokio::test]
pub(super) async fn as_duas_metades_do_turno_sao_decididas_em_separado() {
    let (rt, store, embeddings) =
        runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

    rt.remember_turn(
        "s1",
        None,
        None,
        "ok",
        "o gateway sobe na porta 3888 e le a config de ~/.garraia",
    )
    .await
    .expect("remember_turn");

    assert_eq!(chamadas(&embeddings), 1);
    let r = store.integrity_report().expect("report");
    assert_eq!(r.entries_with_embedding, 1);
    assert_eq!(r.entries_without_embedding, 1);
}

/// Desligar a politica devolve o comportamento anterior ao #952, inteiro.
#[tokio::test]
pub(super) async fn politica_desligada_embedda_ate_o_ruido() {
    let (rt, store, embeddings) =
        runtime_com_memoria(crate::memory_noise::NoisePolicy::disabled()).await;

    rt.remember_turn("s1", None, None, "oi", "bom dia")
        .await
        .expect("remember_turn");

    assert_eq!(chamadas(&embeddings), 2);
    assert_eq!(
        store
            .integrity_report()
            .expect("report")
            .entries_with_embedding,
        2
    );
}

/// A coluna `embedding_model` descreve o vetor. Uma entrada pulada por
/// ruido nao tem vetor, entao nao pode sair dizendo por qual modelo foi
/// indexada — foi essa mentira que o #948 corrigiu, e o filtro novo nao
/// pode reintroduzi-la.
#[tokio::test]
pub(super) async fn entrada_pulada_nao_finge_ter_modelo() {
    let (rt, store, _) = runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

    rt.remember_turn("s1", None, None, "ok", "valeu")
        .await
        .expect("remember_turn");

    for entrada in store.recent_entries(10).expect("recent") {
        assert!(entrada.embedding.is_none());
        assert!(
            entrada.embedding_model.is_none(),
            "linha sem vetor anunciando modelo: {:?}",
            entrada.embedding_model
        );
    }
}
