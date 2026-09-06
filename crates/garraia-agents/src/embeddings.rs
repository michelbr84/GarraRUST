use async_trait::async_trait;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

use crate::provider_resilience::RetryPolicy;

/// Quantos textos o provider do Ollama embeda em paralelo por lote.
///
/// O endpoint dele e um-texto-por-request. Em serie, reindexar as ~7k
/// entradas de uma base real (#953) seriam 7k idas e voltas encadeadas.
/// Quatro de cada vez corta o tempo sem inundar um servidor local, que
/// normalmente esta rodando um modelo so.
const OLLAMA_BATCH_CONCURRENCY: usize = 4;

/// Quantos caracteres do corpo de erro de um provider entram no log.
const MAX_ERROR_BODY_CHARS: usize = 200;

/// Prefixos de token que, se aparecerem numa palavra, condenam a palavra
/// inteira. Espelham os padroes de `garraia_security::redact_secrets`; aqui
/// sao aplicados sem regex para nao arrastar o crate de seguranca inteiro
/// para dentro do runtime de agentes.
const KEY_LIKE_PREFIXES: [&str; 4] = ["sk-", "xoxb-", "xoxp-", "xapp-"];

/// Prepara o corpo de uma resposta de erro para virar mensagem de log.
///
/// O corpo de erro de um provider **nao e conteudo confiavel**: a OpenAI ecoa
/// de volta a chave que voce mandou quando ela esta errada, e um endpoint
/// self-hosted pode devolver o request inteiro, headers inclusive. Enquanto o
/// runtime engolia esses erros com `.ok()` isso nunca aparecia; agora que eles
/// sao logados (#948), o corpo passa a ser tratado como potencialmente
/// sensivel — regra absoluta 6 do CLAUDE.md.
///
/// 401 e 403 perdem o corpo inteiro: e exatamente o caso em que ele fala sobre
/// a credencial. O status, que e o que o operador precisa para agir, continua
/// na mensagem.
fn sanitize_error_body(status: u16, body: &str) -> String {
    if status == 401 || status == 403 {
        return "<omitido: resposta de autenticacao pode ecoar a credencial>".to_string();
    }

    let scrubbed = redact_key_like_tokens(body);
    if scrubbed.chars().count() > MAX_ERROR_BODY_CHARS {
        let head: String = scrubbed.chars().take(MAX_ERROR_BODY_CHARS).collect();
        format!("{head}... <truncado>")
    } else {
        scrubbed
    }
}

/// Troca por `[REDACTED]` qualquer palavra que contenha um prefixo de chave
/// conhecido. Redige a palavra toda de proposito: em JSON a chave vem cercada
/// de aspas e virgulas, e cortar so o token deixaria pedaco para tras.
fn redact_key_like_tokens(input: &str) -> String {
    input
        .split(' ')
        .map(|word| {
            if KEY_LIKE_PREFIXES.iter().any(|prefix| word.contains(prefix)) {
                "[REDACTED]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn model(&self) -> &str;
    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
    async fn health_check(&self) -> Result<bool>;
}

/// Ollama embeddings provider (local, free).
pub struct OllamaEmbeddingProvider {
    client: reqwest::Client,
    model: String,
    base_url: String,
}

impl OllamaEmbeddingProvider {
    pub fn new(model: Option<String>, base_url: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            model: model.unwrap_or_else(|| "nomic-embed-text".to_string()),
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/api/embeddings", self.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // `try_join_all` preserva a ordem do lote — o chamador casa vetor com
        // texto por indice, entao embaralhar aqui corromperia a memoria em
        // silencio.
        let mut embeddings = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(OLLAMA_BATCH_CONCURRENCY) {
            let pending = chunk.iter().map(|text| self.embed_query(text));
            embeddings.extend(futures::future::try_join_all(pending).await?);
        }
        Ok(embeddings)
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let response = self
            .client
            .post(self.endpoint())
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": text
            }))
            .send()
            .await
            .map_err(|e| Error::Agent(format!("ollama embeddings request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body =
                sanitize_error_body(status.as_u16(), &response.text().await.unwrap_or_default());
            return Err(Error::Agent(format!(
                "ollama embeddings request failed: status={status}, body={body}"
            )));
        }

        let payload: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to decode ollama response: {}", e)))?;

        Ok(payload.embedding)
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.embed_query("health check").await.is_ok())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}

/// OpenAI-compatible embeddings provider (works with LM Studio, OpenAI, etc.).
pub struct OpenAiEmbeddingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiEmbeddingProvider {
    pub fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com".to_string()),
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/v1/embeddings",
            self.base_url.trim_end_matches('/').trim_end_matches("/v1")
        )
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut request = self.client.post(self.endpoint());
        // LM Studio e outros servidores OpenAI-compativeis locais nao pedem
        // credencial, e alguns recusam um `Authorization: Bearer ` vazio.
        // Sem chave, sem header — o bootstrap ja garante que so chega aqui
        // sem chave quem aponta para um endpoint proprio.
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }

        let response = request
            .json(&serde_json::json!({
                "model": self.model,
                "input": texts
            }))
            .send()
            .await
            .map_err(|e| Error::Agent(format!("openai embeddings request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body =
                sanitize_error_body(status.as_u16(), &response.text().await.unwrap_or_default());
            return Err(Error::Agent(format!(
                "openai embeddings request failed: status={status}, body={body}"
            )));
        }

        #[derive(Deserialize)]
        struct OpenAiEmbedData {
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct OpenAiEmbedResponse {
            data: Vec<OpenAiEmbedData>,
        }

        let payload: OpenAiEmbedResponse = response.json().await.map_err(|e| {
            Error::Agent(format!("failed to decode openai embeddings response: {e}"))
        })?;

        Ok(payload.data.into_iter().map(|d| d.embedding).collect())
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_documents(&[text.to_string()]).await?;
        results
            .pop()
            .ok_or_else(|| Error::Agent("empty embedding response".into()))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.embed_query("health check").await.is_ok())
    }
}

/// Cohere embeddings provider.
pub struct CohereEmbeddingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl CohereEmbeddingProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| "embed-english-v3.0".to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.cohere.com".to_string()),
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/embed", self.base_url.trim_end_matches('/'))
    }

    fn build_request_body(&self, texts: &[String], input_type: &str) -> CohereEmbedRequest {
        CohereEmbedRequest {
            model: self.model.clone(),
            texts: texts.to_vec(),
            input_type: input_type.to_string(),
            embedding_types: vec!["float".to_string()],
            truncate: "END".to_string(),
        }
    }

    async fn embed_with_input_type(
        &self,
        texts: &[String],
        input_type: &str,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(&self.build_request_body(texts, input_type))
            .send()
            .await
            .map_err(|e| Error::Agent(format!("cohere request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body =
                sanitize_error_body(status.as_u16(), &response.text().await.unwrap_or_default());
            return Err(Error::Agent(format!(
                "cohere embed request failed: status={status}, body={body}"
            )));
        }

        let payload: CohereEmbedResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to decode cohere response: {e}")))?;

        payload.into_float_embeddings()
    }
}

#[async_trait]
impl EmbeddingProvider for CohereEmbeddingProvider {
    fn provider_id(&self) -> &str {
        "cohere"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed_with_input_type(texts, "search_document").await
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text.to_string()];
        let mut embeddings = self.embed_with_input_type(&texts, "search_query").await?;
        embeddings
            .pop()
            .ok_or_else(|| Error::Agent("cohere returned no embeddings for query".into()))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.embed_query("health check").await.is_ok())
    }
}

#[derive(Debug, Clone, Serialize)]
struct CohereEmbedRequest {
    model: String,
    texts: Vec<String>,
    input_type: String,
    embedding_types: Vec<String>,
    truncate: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CohereEmbedResponse {
    embeddings: Option<CohereEmbeddings>,
}

#[derive(Debug, Clone, Deserialize)]
struct CohereEmbeddings {
    float: Option<Vec<Vec<f32>>>,
}

impl CohereEmbedResponse {
    fn into_float_embeddings(self) -> Result<Vec<Vec<f32>>> {
        self.embeddings
            .and_then(|e| e.float)
            .ok_or_else(|| Error::Agent("cohere response missing float embeddings".into()))
    }
}

/// Repeticao com backoff e validacao de dimensao em volta de qualquer
/// [`EmbeddingProvider`] (#962, #961).
///
/// Fica **fora** dos providers de proposito. Cada um fala um protocolo
/// diferente, mas a fragilidade e a mesma: uma falha de transporte de um
/// segundo — Ollama recarregando modelo, rate limit, conexao derrubada —
/// fazia a memoria daquele turno nascer sem vetor e ficar invisivel para
/// sempre para a busca semantica (#948). Envolver uma vez cobre os tres.
///
/// A validacao de dimensao roda **depois** do retry e nao e repetida: um
/// modelo que devolve 768 onde a config declara 1024 vai devolver 768 de
/// novo. Repetir so gastaria tempo antes de recusar.
pub struct ResilientEmbeddingProvider {
    inner: Arc<dyn EmbeddingProvider>,
    retry: RetryPolicy,
    expected_dimensions: Option<usize>,
}

impl ResilientEmbeddingProvider {
    pub fn new(inner: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            inner,
            retry: RetryPolicy::default(),
            expected_dimensions: None,
        }
    }

    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Dimensao declarada na config (`embeddings.<nome>.dimensions`). Sem
    /// ela nao ha o que validar — e ate hoje o campo era parseado e nunca
    /// consumido por ninguem (#961).
    pub fn with_expected_dimensions(mut self, dimensions: Option<usize>) -> Self {
        self.expected_dimensions = dimensions;
        self
    }

    fn validate(&self, vectors: &[Vec<f32>]) -> Result<()> {
        let Some(expected) = self.expected_dimensions else {
            return Ok(());
        };

        for vector in vectors {
            if vector.len() != expected {
                return Err(Error::Agent(format!(
                    "embeddings: provider '{}' (modelo '{}') devolveu {} dimensoes, \
                     a config declara {} — vetor recusado para nao corromper o indice: \
                     cada dimensao cria uma tabela vec_embeddings_N propria, e o recall \
                     passaria a procurar na tabela errada, perdendo em silencio tudo o \
                     que ja estava indexado",
                    self.inner.provider_id(),
                    self.inner.model(),
                    vector.len(),
                    expected
                )));
            }
        }
        Ok(())
    }

    async fn with_retry<T, F, Fut>(&self, operation: &str, mut call: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        loop {
            match call().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if !is_transient_embedding_error(&err) {
                        return Err(err);
                    }
                    if attempt >= self.retry.max_retries {
                        warn!(
                            provider = self.inner.provider_id(),
                            model = self.inner.model(),
                            operation,
                            attempts = attempt + 1,
                            "embeddings: falha transitoria persistiu depois de esgotar \
                             as tentativas: {err}"
                        );
                        return Err(err);
                    }

                    let delay = self.retry.delay_for_attempt(attempt);
                    warn!(
                        provider = self.inner.provider_id(),
                        model = self.inner.model(),
                        operation,
                        attempt = attempt + 1,
                        retry_in_ms = delay.as_millis() as u64,
                        "embeddings: falha transitoria, tentando de novo: {err}"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }
}

#[async_trait]
impl EmbeddingProvider for ResilientEmbeddingProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let vectors = self
            .with_retry("embed_documents", || self.inner.embed_documents(texts))
            .await?;
        self.validate(&vectors)?;
        Ok(vectors)
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let vector = self
            .with_retry("embed_query", || self.inner.embed_query(text))
            .await?;
        self.validate(std::slice::from_ref(&vector))?;
        Ok(vector)
    }

    async fn health_check(&self) -> Result<bool> {
        // Sem retry: um health check que insiste tres vezes com backoff
        // atrasa o boot e ainda por cima mente sobre o estado instantaneo
        // do provider.
        self.inner.health_check().await
    }
}

/// Extrai o `status=NNN` que os providers colocam na mensagem de erro.
fn extract_http_status(msg: &str) -> Option<u16> {
    // So o trecho ANTES do corpo: o corpo e texto do servidor e pode conter
    // "status=" tambem, o que faria a classificacao ler o numero errado
    // (achado da auditoria).
    let head = match msg.split_once("body=") {
        Some((before, _)) => before,
        None => msg,
    };
    let start = head.find("status=")? + "status=".len();
    let digits: String = head[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Vale a pena repetir esta falha?
///
/// A regra e conservadora nos dois sentidos: 4xx nunca se repete (chave
/// errada, modelo inexistente, payload invalido — repetir so queima tempo e
/// cota), e corpo que nao decodifica tambem nao, porque e contrato quebrado,
/// nao azar. O resto sem status HTTP e falha de transporte — timeout,
/// conexao recusada, DNS — que e exatamente o caso que o #962 relata.
fn is_transient_embedding_error(err: &Error) -> bool {
    let msg = err.to_string().to_lowercase();

    if msg.contains("failed to decode") || msg.contains("missing float embeddings") {
        return false;
    }

    match extract_http_status(&msg) {
        Some(429) => true,
        Some(status) if (500..600).contains(&status) => true,
        Some(_) => false,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CohereEmbedResponse, CohereEmbeddingProvider, EmbeddingProvider, Error,
        MAX_ERROR_BODY_CHARS, OllamaEmbeddingProvider, OpenAiEmbeddingProvider,
        ResilientEmbeddingProvider, Result, RetryPolicy, is_transient_embedding_error,
        sanitize_error_body,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Provider de mentira que falha as `fails_before_success` primeiras
    /// chamadas com um erro dado e depois responde.
    struct FlakyProvider {
        error: String,
        fails_before_success: usize,
        calls: AtomicUsize,
        vector: Vec<f32>,
    }

    impl FlakyProvider {
        fn new(error: &str, fails_before_success: usize, vector: Vec<f32>) -> Self {
            Self {
                error: error.to_string(),
                fails_before_success,
                calls: AtomicUsize::new(0),
                vector,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EmbeddingProvider for FlakyProvider {
        fn provider_id(&self) -> &str {
            "flaky"
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            let mut out = Vec::with_capacity(texts.len());
            for text in texts {
                out.push(self.embed_query(text).await?);
            }
            Ok(out)
        }

        async fn embed_query(&self, _text: &str) -> Result<Vec<f32>> {
            let seen = self.calls.fetch_add(1, Ordering::SeqCst);
            if seen < self.fails_before_success {
                Err(Error::Agent(self.error.clone()))
            } else {
                Ok(self.vector.clone())
            }
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    fn fast_retry() -> RetryPolicy {
        RetryPolicy {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(4),
            backoff_factor: 2.0,
        }
    }

    /// #962: falha transitoria (Ollama recarregando modelo) nao pode custar a
    /// memoria do turno. Duas falhas de transporte, terceira tentativa passa.
    #[tokio::test]
    async fn retries_transient_failure_until_it_succeeds() {
        let inner = Arc::new(FlakyProvider::new(
            "ollama embeddings request failed: operation timed out",
            2,
            vec![0.1, 0.2, 0.3],
        ));
        let provider =
            ResilientEmbeddingProvider::new(inner.clone()).with_retry_policy(fast_retry());

        let vector = provider.embed_query("oi").await.expect("deveria recuperar");

        assert_eq!(vector, vec![0.1, 0.2, 0.3]);
        assert_eq!(inner.calls(), 3, "duas falhas + a tentativa que deu certo");
    }

    /// Chave errada nao melhora repetindo — repetir so queima tempo e cota.
    #[tokio::test]
    async fn does_not_retry_client_error() {
        let inner = Arc::new(FlakyProvider::new(
            "openai embeddings request failed: status=401, body=invalid key",
            99,
            vec![0.0],
        ));
        let provider =
            ResilientEmbeddingProvider::new(inner.clone()).with_retry_policy(fast_retry());

        let erro = provider.embed_query("oi").await;

        assert!(erro.is_err());
        assert_eq!(inner.calls(), 1, "4xx e definitivo: uma tentativa so");
    }

    /// Esgotar as tentativas devolve o erro, nao um vetor vazio silencioso.
    #[tokio::test]
    async fn gives_up_after_max_retries() {
        let inner = Arc::new(FlakyProvider::new(
            "ollama embeddings request failed: connection refused",
            99,
            vec![0.0],
        ));
        let provider =
            ResilientEmbeddingProvider::new(inner.clone()).with_retry_policy(fast_retry());

        assert!(provider.embed_query("oi").await.is_err());
        assert_eq!(inner.calls(), 4, "1 tentativa + 3 repeticoes");
    }

    /// #961: dimensao divergente e recusada — nunca gravada. E nao e repetida:
    /// um modelo que devolve 768 vai devolver 768 de novo.
    #[tokio::test]
    async fn rejects_vector_with_unexpected_dimension() {
        let inner = Arc::new(FlakyProvider::new("nunca falha", 0, vec![0.1, 0.2, 0.3]));
        let provider = ResilientEmbeddingProvider::new(inner.clone())
            .with_retry_policy(fast_retry())
            .with_expected_dimensions(Some(1024));

        let erro = provider
            .embed_query("oi")
            .await
            .expect_err("3 dimensoes onde a config declara 1024");

        let msg = erro.to_string();
        assert!(msg.contains("1024"), "erro precisa dizer o esperado: {msg}");
        assert!(msg.contains('3'), "e o recebido: {msg}");
        assert_eq!(
            inner.calls(),
            1,
            "divergencia de dimensao nao e transitoria"
        );
    }

    /// Sem `dimensions` na config nao ha o que validar — o vetor passa.
    #[tokio::test]
    async fn passes_through_when_no_dimension_is_declared() {
        let inner = Arc::new(FlakyProvider::new("nunca falha", 0, vec![0.1, 0.2, 0.3]));
        let provider = ResilientEmbeddingProvider::new(inner)
            .with_retry_policy(fast_retry())
            .with_expected_dimensions(None);

        assert_eq!(
            provider.embed_query("oi").await.expect("deveria passar"),
            vec![0.1, 0.2, 0.3]
        );
    }

    /// O lote inteiro tambem e validado, nao so a consulta.
    #[tokio::test]
    async fn validates_every_vector_of_a_batch() {
        let inner = Arc::new(FlakyProvider::new("nunca falha", 0, vec![0.1, 0.2]));
        let provider = ResilientEmbeddingProvider::new(inner)
            .with_retry_policy(fast_retry())
            .with_expected_dimensions(Some(3));

        let textos = vec!["um".to_string(), "dois".to_string()];
        assert!(provider.embed_documents(&textos).await.is_err());
    }

    /// #962: o lote do Ollama passou a ser paralelo, e paralelismo que
    /// embaralha a saida corromperia a memoria em silencio — o chamador casa
    /// vetor com texto por indice. Seis textos com concorrencia 4 cruzam a
    /// fronteira do lote de proposito.
    #[tokio::test]
    async fn ollama_batch_preserves_input_order() {
        use axum::{Json, Router, routing::post};
        use serde_json::Value;

        async fn embed(Json(body): Json<Value>) -> Json<Value> {
            // O vetor devolvido identifica o texto que chegou, pelo tamanho.
            let marker = body["prompt"].as_str().unwrap_or_default().len() as f32;
            Json(serde_json::json!({ "embedding": [marker, 0.0, 0.0] }))
        }

        let app = Router::new().route("/api/embeddings", post(embed));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let provider = OllamaEmbeddingProvider::new(None, Some(format!("http://{addr}")));
        let texts: Vec<String> = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let vectors = provider.embed_documents(&texts).await.expect("lote");

        let markers: Vec<f32> = vectors.iter().map(|v| v[0]).collect();
        assert_eq!(
            markers,
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            "o lote paralelo devolveu os vetores fora da ordem dos textos"
        );
    }

    /// Regra absoluta 6. Enquanto o runtime engolia os erros de embedding
    /// com `.ok()` (#948) o corpo da resposta nunca chegava ao log; agora que
    /// chega, ele e conteudo nao confiavel — a OpenAI ecoa de volta a chave
    /// que voce mandou quando ela esta errada.
    #[test]
    fn auth_failure_bodies_never_reach_the_log() {
        let corpo_da_openai = r#"{"error":{"message":"Incorrect API key provided: \
            sk-proj-abcdefghijklmnopqrstuvwxyz0123456789. You can find your key at ..."}}"#;

        let sanitizado = sanitize_error_body(401, corpo_da_openai);
        assert!(!sanitizado.contains("sk-"), "chave vazou: {sanitizado}");
        assert!(sanitizado.contains("omitido"));

        // 403 tambem: a resposta fala sobre a credencial.
        assert!(!sanitize_error_body(403, corpo_da_openai).contains("sk-"));
    }

    /// Fora do caso de autenticacao o corpo ajuda a diagnosticar, entao ele
    /// fica — mas com os tokens de formato conhecido raspados.
    #[test]
    fn other_bodies_keep_diagnostics_but_lose_key_like_tokens() {
        let corpo = r#"{"error":"model not found","hint":"token sk-abcdefghijklmnop was used"}"#;
        let sanitizado = sanitize_error_body(500, corpo);

        assert!(sanitizado.contains("model not found"), "{sanitizado}");
        assert!(!sanitizado.contains("sk-abcdefghijklmnop"), "{sanitizado}");
        assert!(sanitizado.contains("[REDACTED]"), "{sanitizado}");
    }

    #[test]
    fn long_bodies_are_truncated() {
        let corpo = "x".repeat(MAX_ERROR_BODY_CHARS * 3);
        let sanitizado = sanitize_error_body(500, &corpo);

        assert!(sanitizado.contains("<truncado>"));
        assert!(sanitizado.chars().count() < corpo.chars().count());
    }

    /// O caminho inteiro: servidor devolve 401 com a chave no corpo, e a
    /// mensagem de erro que sai do provider — a mesma que o runtime loga —
    /// nao pode carregar a chave.
    #[tokio::test]
    async fn provider_error_message_from_a_401_carries_no_key() {
        use axum::http::StatusCode;
        use axum::{Router, routing::post};

        async fn unauthorized() -> (StatusCode, String) {
            (
                StatusCode::UNAUTHORIZED,
                r#"{"error":{"message":"Incorrect API key provided: sk-proj-supersecretvalue123456"}}"#
                    .to_string(),
            )
        }

        let app = Router::new().route("/v1/embeddings", post(unauthorized));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let provider = OpenAiEmbeddingProvider::new(
            "sk-chave-de-teste".into(),
            None,
            Some(format!("http://{addr}")),
        );

        let erro = provider
            .embed_query("oi")
            .await
            .expect_err("401 tem de virar erro");
        let msg = erro.to_string();

        assert!(msg.contains("401"), "o status precisa sobreviver: {msg}");
        assert!(
            !msg.contains("supersecretvalue"),
            "a chave do corpo vazou para a mensagem de erro: {msg}"
        );
        assert!(!msg.contains("sk-"), "resto de token na mensagem: {msg}");
    }

    /// A classificacao le o status do cabecalho da mensagem, nao do corpo —
    /// um corpo que contenha "status=200" nao pode confundi-la.
    #[test]
    fn status_is_read_from_the_header_not_from_the_body() {
        let err = Error::Agent(
            "openai embeddings request failed: status=503, body=upstream said status=200".into(),
        );
        assert!(
            is_transient_embedding_error(&err),
            "503 no cabecalho manda, nao o 200 do corpo"
        );

        let err = Error::Agent(
            "openai embeddings request failed: status=400, body=retry when status=503".into(),
        );
        assert!(
            !is_transient_embedding_error(&err),
            "400 no cabecalho manda, nao o 503 do corpo"
        );
    }

    #[test]
    fn classifies_which_failures_are_worth_retrying() {
        let transitorio = [
            "ollama embeddings request failed: operation timed out",
            "ollama embeddings request failed: connection refused",
            "openai embeddings request failed: status=429, body=slow down",
            "openai embeddings request failed: status=503, body=upstream",
            "cohere request failed: error sending request",
        ];
        for msg in transitorio {
            assert!(
                is_transient_embedding_error(&Error::Agent(msg.to_string())),
                "deveria repetir: {msg}"
            );
        }

        let definitivo = [
            "openai embeddings request failed: status=401, body=bad key",
            "openai embeddings request failed: status=404, body=no such model",
            "ollama embeddings request failed: status=400, body=bad payload",
            "failed to decode ollama response: missing field",
            "cohere response missing float embeddings",
        ];
        for msg in definitivo {
            assert!(
                !is_transient_embedding_error(&Error::Agent(msg.to_string())),
                "nao deveria repetir: {msg}"
            );
        }
    }

    #[test]
    fn builds_expected_request_shape() {
        let provider =
            CohereEmbeddingProvider::new("test-key", Some("embed-english-v3.0".into()), None);
        let body = provider.build_request_body(
            &["hello".to_string(), "world".to_string()],
            "search_document",
        );
        assert_eq!(body.model, "embed-english-v3.0");
        assert_eq!(body.input_type, "search_document");
        assert_eq!(body.embedding_types, vec!["float".to_string()]);
        assert_eq!(body.texts.len(), 2);
    }

    #[test]
    fn parses_float_embeddings_payload() {
        let payload: CohereEmbedResponse = serde_json::from_str(
            r#"{
                "embeddings": {
                    "float": [[0.1, 0.2, 0.3], [0.9, 0.1, 0.0]]
                }
            }"#,
        )
        .expect("json should parse");

        let vectors = payload
            .into_float_embeddings()
            .expect("should contain float embeddings");
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0], vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn endpoint_is_normalized_without_double_slashes() {
        let provider = CohereEmbeddingProvider::new(
            "test-key",
            Some("embed-english-v3.0".into()),
            Some("https://api.cohere.com/".into()),
        );
        assert_eq!(provider.endpoint(), "https://api.cohere.com/v1/embed");
    }
}
