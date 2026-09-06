//! Metricas da memoria semantica (#957).
//!
//! # Por que aqui, e nao em `garraia-telemetry`
//!
//! Quem **emite** estas metricas e o `garraia-agents` (e, num slice futuro, o
//! `garraia-db`). Nenhum dos dois pode depender do `garraia-telemetry`: aquele
//! crate arrasta OpenTelemetry, OTLP, tonic, axum e tower, e o `garraia-cli`
//! linka o `garraia-agents` — a CLI passaria a carregar um stack de servidor
//! para nao usar nada dele.
//!
//! A saida e o padrao do proprio ecossistema `metrics`: **biblioteca emite,
//! binario instala o recorder**. As macros do facade resolvem para um ponteiro
//! global; sem recorder instalado — que e o caso da CLI — cada chamada e um
//! no-op barato. O gateway instala o recorder Prometheus no boot
//! (`garraia_telemetry::init_metrics`) e so entao as medicoes viram numero.
//!
//! O `garraia-common` e o crate que gateway, agents, db, config e security ja
//! compartilham. E o mesmo argumento que esta escrito no `Cargo.toml` deste
//! crate para o guard de SSRF: melhor um lugar do que tres copias.
//!
//! # Cardinalidade
//!
//! Toda label aqui vem de conjunto **fechado**, e isso foi verificado, nao
//! presumido: `provider_id()` devolve literal fixo nos tres providers reais —
//! `"ollama"`, `"openai"`, `"cohere"` (`garraia-agents/src/embeddings.rs`) — e
//! o `ResilientEmbeddingProvider` delega para o de dentro. `operation` e
//! `outcome` sao enums que viram `&'static str`. Nao ha caminho por onde um id
//! de sessao, de usuario ou um texto de conteudo chegue a uma label, o que
//! seria a explosao de cardinalidade que o docblock de
//! `garraia-telemetry::metrics` descreve.
//!
//! **`model()` fica de fora de proposito**, e essa e a decisao que mais
//! importa aqui. Ao contrario do `provider_id()`, o nome do modelo vem da
//! config do usuario: `memory.embedding_provider` aponta para uma entrada de
//! `embeddings:` onde o `model` e string livre. Usa-lo como label daria ao
//! operador — sem querer — uma serie nova por valor digitado, e um erro de
//! digitacao repetido viraria lixo permanente no Prometheus. Quem precisa
//! saber o modelo tem o log, que nao paga cardinalidade.
//!
//! # Nome
//!
//! Prefixo `garraia_`, nao `garra_`. A issue #957 propoe `garra_memory_*`, mas
//! as metricas que ja existem sao `garraia_requests_total`,
//! `garraia_http_latency_seconds`, `garraia_errors_total` e
//! `garraia_active_sessions`. Duas familias de prefixo no mesmo `/metrics`
//! quebrariam todo dashboard que agrupa por `garraia_.*`.

use metrics::{counter, gauge, histogram};

/// Latencia de uma chamada ao provider de embeddings, em segundos.
pub const MEMORY_EMBED_LATENCY_SECONDS: &str = "garraia_memory_embed_latency_seconds";

/// Falhas ao pedir embedding. Era o buraco do #948: a falha ficou visivel no
/// log, mas log nao alerta nem vira tendencia.
pub const MEMORY_EMBED_FAILURES_TOTAL: &str = "garraia_memory_embed_failures_total";

/// Latencia do recall (busca na memoria), em segundos.
pub const MEMORY_RECALL_LATENCY_SECONDS: &str = "garraia_memory_recall_latency_seconds";

/// Turnos que entraram na memoria, por desfecho.
pub const MEMORY_INGESTED_TOTAL: &str = "garraia_memory_ingested_total";

/// Quantas entradas a memoria tem, separadas por ter vetor ou nao.
///
/// **Gauge**, e nao counter, porque o numero sobe e desce: a retencao apaga, o
/// `compact` apaga, o `reindex` move entradas de um lado para o outro.
pub const MEMORY_ENTRIES: &str = "garraia_memory_entries";

/// Leituras do tamanho da memoria que falharam.
///
/// Existe por causa do pior modo de falha de um gauge: quando a leitura para de
/// funcionar, o valor **nao some** — o Prometheus continua servindo o ultimo
/// como se fosse atual, e quem olha o painel nao distingue "banco inacessivel"
/// de "a base tem mesmo esse tamanho". Um contador ao lado torna a falha
/// alertavel, e e a unica forma de o silencio virar sinal. Apontado pela
/// auditoria do #957.
pub const MEMORY_GAUGE_ERRORS_TOTAL: &str = "garraia_memory_gauge_errors_total";

/// Linhas no indice vetorial (`vec_id_map`).
///
/// Lado a lado com o `MEMORY_ENTRIES{has_embedding="true"}`, os dois deviam
/// ser iguais. **A distancia entre eles e o sinal**: entrada com vetor na
/// coluna mas fora do indice nao aparece na busca semantica, e era invisivel
/// ate o `garra memory stats` (#960) — que so mostra quando alguem pergunta.
/// Aqui vira tendencia, que e o que faz alguem perguntar.
pub const MEMORY_VECTOR_INDEX_SIZE: &str = "garraia_memory_vector_index_size";

/// Qual das duas chamadas do provider foi medida.
///
/// Elas tem perfis diferentes e nao podem ser somadas: `embed_documents`
/// recebe lote e roda na ingestao; `embed_query` e uma so e esta no caminho
/// critico da resposta ao usuario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedOp {
    Document,
    Query,
}

impl EmbedOp {
    pub fn as_label(self) -> &'static str {
        match self {
            EmbedOp::Document => "document",
            EmbedOp::Query => "query",
        }
    }
}

/// O que aconteceu com um turno na ingestao.
///
/// E a metrica que a issue #957 pede sem saber o nome: "quantas entradas tem
/// embedding" vira uma pergunta respondivel no tempo, e nao so no instante,
/// quando se conta o **desfecho de cada ingestao**. `Noise` so existe por
/// causa do filtro do #952 — sem ele, o operador veria o total de entradas sem
/// vetor subir e nao teria como saber se e defeito ou politica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    /// Recebeu vetor e entrou no indice.
    Embedded,
    /// A politica de ruido (#952) recusou. Nao e falha.
    Noise,
    /// Nao ha provider de embeddings configurado.
    NoProvider,
    /// O provider foi chamado e falhou. A entrada foi gravada sem vetor.
    Failed,
}

impl IngestOutcome {
    pub fn as_label(self) -> &'static str {
        match self {
            IngestOutcome::Embedded => "embedded",
            IngestOutcome::Noise => "noise",
            IngestOutcome::NoProvider => "no_provider",
            IngestOutcome::Failed => "failed",
        }
    }
}

/// Mede uma chamada bem-sucedida ao provider de embeddings.
pub fn record_embed_latency(provider: &str, op: EmbedOp, seconds: f64) {
    histogram!(
        MEMORY_EMBED_LATENCY_SECONDS,
        "provider" => provider.to_string(),
        "operation" => op.as_label(),
    )
    .record(seconds);
}

/// Conta uma falha do provider de embeddings.
pub fn inc_embed_failure(provider: &str, op: EmbedOp) {
    counter!(
        MEMORY_EMBED_FAILURES_TOTAL,
        "provider" => provider.to_string(),
        "operation" => op.as_label(),
    )
    .increment(1);
}

/// Mede um recall inteiro — semantico ou textual, com ou sem provider.
///
/// Sem label de caminho de proposito: o que o operador quer saber e quanto o
/// usuario esperou, e essa e a mesma pergunta nos dois casos. Separar por
/// caminho aqui esconderia a piora que interessa (o recall degradar por cair
/// no textual) atras de duas series que, cada uma, parecem saudaveis.
pub fn record_recall_latency(seconds: f64) {
    histogram!(MEMORY_RECALL_LATENCY_SECONDS).record(seconds);
}

/// Conta um turno ingerido, por desfecho.
pub fn inc_ingested(outcome: IngestOutcome) {
    counter!(MEMORY_INGESTED_TOTAL, "outcome" => outcome.as_label()).increment(1);
}

/// Publica o tamanho da memoria (#957).
///
/// Os tres numeros saem **da mesma leitura**, de proposito: publicados em
/// momentos diferentes, um painel que compare "com vetor" contra "no indice"
/// veria diferenca que e so defasagem entre as coletas, e nao o defeito real
/// que essa comparacao existe para achar.
pub fn inc_memory_gauge_error() {
    counter!(MEMORY_GAUGE_ERRORS_TOTAL).increment(1);
}

pub fn set_memory_size(com_vetor: usize, sem_vetor: usize, linhas_no_indice: usize) {
    gauge!(MEMORY_ENTRIES, "has_embedding" => "true").set(com_vetor as f64);
    gauge!(MEMORY_ENTRIES, "has_embedding" => "false").set(sem_vetor as f64);
    gauge!(MEMORY_VECTOR_INDEX_SIZE).set(linhas_no_indice as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As labels sao `&'static str` vindas de enum, o que e a garantia
    /// estrutural contra explosao de cardinalidade: nao ha como um id de
    /// sessao virar label sem alguem mudar a assinatura.
    #[test]
    fn labels_de_enum_sao_conjunto_fechado() {
        assert_eq!(EmbedOp::Document.as_label(), "document");
        assert_eq!(EmbedOp::Query.as_label(), "query");
        for (outcome, esperado) in [
            (IngestOutcome::Embedded, "embedded"),
            (IngestOutcome::Noise, "noise"),
            (IngestOutcome::NoProvider, "no_provider"),
            (IngestOutcome::Failed, "failed"),
        ] {
            assert_eq!(outcome.as_label(), esperado);
        }
    }

    /// O `provider` e label; o `model` nao pode ser. `provider_id()` e literal
    /// fixo nos tres providers reais, mas o nome do modelo vem da config do
    /// usuario — vira serie nova por valor digitado. Este teste nao consegue
    /// afirmar a ausencia sozinho; ele ancora a intencao, e a assinatura dos
    /// helpers e o que a cobra: nenhum deles aceita o modelo.
    #[test]
    fn os_helpers_nao_aceitam_o_nome_do_modelo() {
        // Se alguem acrescentar um parametro de modelo, este teste para de
        // compilar — que e o sinal desejado.
        let _: fn(&str, EmbedOp, f64) = record_embed_latency;
        let _: fn(&str, EmbedOp) = inc_embed_failure;
        let _: fn(f64) = record_recall_latency;
        let _: fn(IngestOutcome) = inc_ingested;
    }

    /// Os tres numeros do tamanho saem da mesma leitura.
    ///
    /// Publicados em momentos diferentes, um painel que compare "com vetor"
    /// contra "no indice" veria defasagem entre coletas e leria como defeito —
    /// justamente a comparacao que essas duas metricas existem para permitir.
    /// Este teste nao consegue afirmar simultaneidade; ele ancora a assinatura,
    /// que e o que a garante: **uma** funcao recebe os tres.
    #[test]
    fn o_tamanho_e_publicado_de_uma_leitura_so() {
        let _: fn(usize, usize, usize) = set_memory_size;
    }

    /// Prefixo `garraia_`, alinhado com `garraia_requests_total` e as outras
    /// tres que ja existem. A issue #957 propoe `garra_`; seguir a issue ao pe
    /// da letra quebraria todo dashboard que agrupa por `garraia_.*`.
    #[test]
    fn todo_nome_usa_o_prefixo_do_projeto() {
        for nome in [
            MEMORY_EMBED_LATENCY_SECONDS,
            MEMORY_EMBED_FAILURES_TOTAL,
            MEMORY_RECALL_LATENCY_SECONDS,
            MEMORY_INGESTED_TOTAL,
            MEMORY_ENTRIES,
            MEMORY_VECTOR_INDEX_SIZE,
            MEMORY_GAUGE_ERRORS_TOTAL,
        ] {
            assert!(
                nome.starts_with("garraia_memory_"),
                "prefixo errado: {nome}"
            );
        }
    }

    /// Convencao do Prometheus: contador termina em `_total`, histograma de
    /// tempo termina em `_seconds`. Nao e frescura — e o que faz
    /// `rate()`/`histogram_quantile()` serem escritos sem adivinhacao.
    #[test]
    fn sufixos_seguem_a_convencao_do_prometheus() {
        assert!(MEMORY_EMBED_FAILURES_TOTAL.ends_with("_total"));
        assert!(MEMORY_INGESTED_TOTAL.ends_with("_total"));
        assert!(MEMORY_EMBED_LATENCY_SECONDS.ends_with("_seconds"));
        assert!(MEMORY_RECALL_LATENCY_SECONDS.ends_with("_seconds"));

        // Gauge nao leva sufixo: `_total` mentiria (o numero desce) e
        // `_seconds` nao e unidade de contagem. A convencao do Prometheus e
        // justamente essa ausencia.
        assert!(!MEMORY_ENTRIES.ends_with("_total"));
        assert!(!MEMORY_VECTOR_INDEX_SIZE.ends_with("_total"));
        assert!(MEMORY_GAUGE_ERRORS_TOTAL.ends_with("_total"));
    }

    /// Sem recorder instalado — o caso da CLI — emitir nao pode entrar em
    /// panico nem custar nada visivel. E a premissa que permite instrumentar
    /// `garraia-agents` sem arrastar o stack de telemetria para o binario.
    #[test]
    fn emitir_sem_recorder_e_no_op() {
        record_embed_latency("ollama", EmbedOp::Document, 0.42);
        inc_embed_failure("ollama", EmbedOp::Query);
        record_recall_latency(0.01);
        inc_ingested(IngestOutcome::Noise);
        set_memory_size(10, 2, 10);
        inc_memory_gauge_error();
    }
}
