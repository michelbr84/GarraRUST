//! De onde saem o endpoint e a credencial de um provider de LLM na CLI.
//!
//! **Invariante:** a `base_url` e a `api_key` de um provider saem da MESMA
//! entrada `llm.<nome>` do `config.yml`. Nunca a chave de uma entrada com o
//! endereco de outra, e nunca a chave de uma entrada com o host padrao do
//! tipo quando a entrada aponta para outro lugar.
//!
//! Antes deste modulo cada caminho da CLI montava o provider do seu jeito, e
//! tres deles misturavam as duas coisas:
//!
//! * `garraia ask -p openai` (e o `garra_ask`/`garra_agent` do MCP com
//!   `provider=openai`) lia `llm.openai.api_key` e jogava fora
//!   `llm.openai.base_url`: a chave de um endpoint OpenAI-compativel proprio ia
//!   para `https://api.openai.com`. O smoke de instalacao limpa da v0.4.4 pegou
//!   isso — `ask_explicit.out` mostra a chamada saindo para api.openai.com;
//! * o mesmo valia para `anthropic` e `openrouter` explicitos, e para os tres
//!   no encadeamento de autodeteccao (`agent.default_provider` ausente);
//! * o caminho do `agent.default_provider` lia a `base_url` da entrada padrao
//!   mas a chave de `llm.<tipo>` — com `default_provider: lmstudio` e um
//!   `llm.openai` ao lado, a chave do `llm.openai` ia para o LM Studio.
//!
//! Agora todos passam por [`bind_named`] / [`bind_entry`] (ou
//! [`bind_autodetect`], na cadeia sem `agent.default_provider`) e depois por
//! [`build_provider`]. Dentro de uma entrada o `api_key` dela vence a
//! variavel de ambiente do tipo — uma `OPENAI_API_KEY` velha num `.env` nao
//! troca a chave que o operador escreveu para aquela entrada.
//!
//! **A variavel de ambiente do tipo so vai para o host padrao do tipo.** Ela
//! preenche uma entrada sem chave apenas quando o endpoint dessa entrada E o
//! host padrao (sem `base_url`, ou com uma `base_url` igual a ele, como o
//! `https://openrouter.ai/api/v1` que o `garraia init` grava). Uma entrada
//! que aponta para outro lugar e nao traz chave fica SEM credencial: o
//! OpenAI-compativel manda o marcador [`KEYLESS_PLACEHOLDER`], o Anthropic e
//! o OpenRouter falham fechado. A `OPENAI_API_KEY` (as vezes carregada de um
//! `.env` do diretorio corrente pelo `dotenvy`) e a credencial da API da
//! OpenAI; manda-la para um proxy ou para um LM Studio de terceiro so porque
//! a entrada esqueceu o `api_key` seria o mesmo vazamento em outra porta.
//! Isso e mais estrito que o gateway (`garraia_config::provider_keys`), que
//! resolve config > ambiente sem olhar a `base_url`.
//!
//! O cofre de credenciais nao entra aqui: a CLI nunca o leu, e le-lo seria
//! outra mudanca de comportamento, fora do escopo desta correcao.

use std::fmt;
use std::sync::Arc;

use anyhow::{Result, bail};
use garraia_agents::{
    AnthropicProvider, LlamaCppProvider, LlmProvider, OllamaProvider, OpenAiProvider,
};
use garraia_config::provider_keys::provider_key_env;
use garraia_config::{AppConfig, LlmProviderConfig};

/// Endpoint padrao do OpenRouter, o mesmo do braco `openrouter` do gateway.
pub(crate) const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Endpoint padrao do `OpenAiProvider` sem `base_url`
/// (`garraia-agents/src/openai.rs`, `DEFAULT_BASE_URL`).
const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// Endpoint padrao do `AnthropicProvider` sem `base_url`
/// (`garraia-agents/src/anthropic.rs`, `DEFAULT_BASE_URL`).
const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Para onde um provider do tipo `kind` vai quando a entrada nao traz
/// `base_url` — o unico host que a variavel de ambiente do tipo alcanca.
fn default_endpoint(kind: &str) -> Option<&'static str> {
    match kind {
        "openai" => Some(OPENAI_DEFAULT_BASE_URL),
        "anthropic" => Some(ANTHROPIC_DEFAULT_BASE_URL),
        "openrouter" => Some(OPENROUTER_BASE_URL),
        _ => None,
    }
}

/// A variavel de ambiente do tipo (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
/// `OPENROUTER_API_KEY`) pode ser usada por uma entrada com esta `base_url`?
///
/// So quando o endpoint da entrada e o host padrao do tipo: sem `base_url`,
/// ou com uma `base_url` que e o proprio host padrao na forma canonica (o
/// `garraia init` grava `https://openrouter.ai/api/v1` no `llm.openrouter` e
/// deixa a chave na env — esse e o caminho de instalacao mais comum). Uma
/// `base_url` que nao se reconhece como o host padrao — outro host, `http://`,
/// userinfo, porta — conta como "outro lugar": fail closed.
pub(crate) fn env_credential_allowed(kind: &str, base_url: Option<&str>) -> bool {
    match non_empty(base_url) {
        None => true,
        Some(url) => default_endpoint(kind).is_some_and(|default| {
            canonical_endpoint(&url).eq_ignore_ascii_case(canonical_endpoint(default))
        }),
    }
}

/// A credencial do ambiente para o tipo, ja com a regra "vazia conta como
/// ausente".
fn env_credential(kind: &str, env: Env<'_>) -> Option<String> {
    provider_key_env(kind)
        .and_then(env)
        .filter(|v| !v.is_empty())
}

/// Variavel de ambiente da credencial de um `--url` avulso.
///
/// E a unica variavel lida para um endereco que nenhuma entrada do config
/// descreve. `OPENAI_API_KEY` e `GARRAIA_EMBEDDING_API_KEY` ja foram lidas
/// aqui e deixaram de ser: sao credenciais de outros endpoints (a API da
/// OpenAI e o provider de embeddings), e mandar qualquer uma delas para um
/// host digitado na linha de comando e exatamente o vazamento que este
/// modulo existe para impedir.
pub(crate) const AD_HOC_URL_KEY_ENV: &str = "LLM_API_KEY";

/// Chave usada quando um endpoint OpenAI-compativel local nao exige
/// autenticacao. O cliente HTTP nao aceita chave vazia.
pub(crate) const KEYLESS_PLACEHOLDER: &str = "not-needed";

/// Leitura de variavel de ambiente, injetavel para os testes nao mexerem no
/// ambiente do processo (que e global e compartilhado entre testes). `Sync`
/// porque atravessa `.await` no `detect_provider`, que roda dentro do
/// handler do MCP (futuro `Send`).
pub(crate) type Env<'a> = &'a (dyn Fn(&str) -> Option<String> + Sync);

/// A leitura de verdade: variavel ausente ou vazia conta como ausente, como
/// no `get_api_key` antigo e no `resolve_api_key` do `garraia-config`.
pub(crate) fn process_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

/// Tipos de provider que a CLI sabe construir. Uma entrada `llm:` de outro
/// tipo (os OpenAI-compativeis que so o gateway conhece, como `sansa`) e
/// recusada com mensagem clara em [`build_provider`] — nunca adivinhada.
pub(crate) fn is_buildable_kind(kind: &str) -> bool {
    matches!(
        kind,
        "ollama" | "llamacpp" | "anthropic" | "openai" | "openrouter"
    ) || (cfg!(feature = "dev-echo-provider") && kind == "echo")
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Endpoint + credencial + modelo de UM provider, todos da mesma origem.
///
/// Os campos sao privados para ninguem montar um vinculo misturando fontes;
/// a unica forma de obter um e por [`bind_entry`] / [`bind_named`].
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProviderBinding {
    /// A chave `llm.<entrada>` de onde vieram endpoint e credencial, ou
    /// `None` quando nenhuma entrada descreve o provider — ai o endpoint e o
    /// padrao do tipo e a credencial so pode vir do ambiente.
    entry: Option<String>,
    kind: String,
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
}

/// `Debug` a mao: a credencial nunca aparece, nem num `{:?}` de teste que
/// falhou (regra 6 do projeto). So se diz se ela existe.
impl fmt::Debug for ProviderBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderBinding")
            .field("entry", &self.entry)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key_set", &self.api_key.is_some())
            .field("model", &self.model)
            .finish()
    }
}

impl ProviderBinding {
    /// De qual entrada o vinculo saiu — usado pelos testes para afirmar que
    /// nenhum caminho vincula a entrada errada.
    #[cfg(test)]
    pub(crate) fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }

    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }

    #[cfg(test)]
    pub(crate) fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub(crate) fn has_credential(&self) -> bool {
        self.api_key.is_some()
    }

    pub(crate) fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Credencial exposta so para os testes afirmarem QUAL chave foi
    /// vinculada; o codigo de producao a entrega direto ao construtor.
    #[cfg(test)]
    pub(crate) fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// Onde o operador conserta a falta de credencial, para a mensagem de
    /// erro apontar o lugar certo.
    fn origin(&self) -> String {
        match &self.entry {
            Some(key) => format!("llm.{key}"),
            None => "config.yml (nenhuma entrada em llm:)".to_string(),
        }
    }

    fn missing_key_error(&self) -> anyhow::Error {
        let var = provider_key_env(&self.kind).unwrap_or("a variavel do provider");
        match &self.entry {
            // A entrada aponta para outro lugar: exportar a variavel nao
            // resolveria, e a mensagem nao pode sugerir que resolve.
            Some(key) if !env_credential_allowed(&self.kind, self.base_url.as_deref()) => {
                anyhow::anyhow!(
                    "llm.{key} has no api_key; its base_url is not the {} default host, \
                     so {var} is never sent to it",
                    self.kind
                )
            }
            Some(key) => anyhow::anyhow!("{var} not set and llm.{key} has no api_key"),
            None => anyhow::anyhow!("{var} not set and not found in config"),
        }
    }

    /// Endpoint padrao do tipo, credencial so do ambiente — o vinculo de
    /// quando nenhuma entrada descreve o tipo.
    fn env_only(kind: &str, env: Env<'_>) -> Self {
        ProviderBinding {
            entry: None,
            kind: kind.to_string(),
            base_url: None,
            api_key: env_credential(kind, env),
            model: None,
        }
    }
}

/// Vincula a entrada `llm.<key>` inteira: tipo, endpoint, credencial e
/// modelo saem todos dela. A credencial cai para a variavel de ambiente do
/// tipo apenas quando a entrada nao traz uma E o endpoint da entrada e o host
/// padrao do tipo ([`env_credential_allowed`]) — nunca para a `base_url`
/// propria da entrada.
pub(crate) fn bind_entry(key: &str, cfg: &LlmProviderConfig, env: Env<'_>) -> ProviderBinding {
    let kind = cfg.provider.trim().to_string();
    let base_url = non_empty(cfg.base_url.as_deref());
    let api_key = non_empty(cfg.api_key.as_deref()).or_else(|| {
        if env_credential_allowed(&kind, base_url.as_deref()) {
            env_credential(&kind, env)
        } else {
            None
        }
    });
    ProviderBinding {
        entry: Some(key.to_string()),
        kind,
        base_url,
        api_key,
        model: non_empty(cfg.model.as_deref()),
    }
}

/// Resolve o vinculo para um nome explicito (`--provider <nome>`, o
/// `provider` do `garra_ask`/`garra_agent`, um candidato da autodeteccao).
///
/// 1. `llm.<nome>` existe → a entrada inteira, com o tipo que ela declara.
///    E assim que um alias (`llm.lmstudio`, provider `openai`) funciona.
/// 2. `<nome>` e um tipo e `llm.main` e desse tipo → `llm.main` inteira. E o
///    fallback legado do `get_api_key`, que antes pegava so a CHAVE da
///    `llm.main` e a mandava para o host padrao, largando a `base_url` dela.
/// 3. `<nome>` e um tipo sem entrada → endpoint padrao do tipo, credencial
///    so do ambiente. Nenhuma chave de config de outra entrada entra aqui.
///
/// `None` quando o nome nao e entrada nem tipo construivel.
pub(crate) fn bind_named(config: &AppConfig, name: &str, env: Env<'_>) -> Option<ProviderBinding> {
    if let Some(cfg) = config.llm.get(name) {
        let vinculo = bind_entry(name, cfg, env);
        // A regra do passo 3 da autodeteccao, no caminho explicito (achado da
        // verificacao da #1370): `llm.openrouter { provider: openai }` sem
        // chave propria nao descreve o OpenRouter, e `-p openrouter` com
        // `OPENROUTER_API_KEY` exportada voltava sem credencial nenhuma. So
        // quando a entrada nao tem credencial e a variavel do tipo pedido
        // existe; a chave vai para o host padrao DESSE tipo, nunca para a
        // `base_url` da entrada, e um alias sem chave (`llm.lmstudio`) segue
        // com a entrada inteira.
        if vinculo.kind != name && is_buildable_kind(name) && !vinculo.has_credential() {
            let fallback = ProviderBinding::env_only(name, env);
            if fallback.has_credential() {
                return Some(fallback);
            }
        }
        return Some(vinculo);
    }
    if !is_buildable_kind(name) {
        return None;
    }
    if let Some(cfg) = config.llm.get("main")
        && cfg.provider.trim() == name
    {
        return Some(bind_entry("main", cfg, env));
    }
    Some(ProviderBinding::env_only(name, env))
}

/// O candidato `kind` da cadeia de autodeteccao (sem
/// `agent.default_provider`), ou `None` se ele nao tem credencial.
///
/// 1. [`bind_named`] devolveu um vinculo DESTE tipo (`llm.<kind>` do tipo,
///    `llm.main` do tipo, ou so o ambiente): ele, se tiver credencial. Uma
///    `llm.<kind>` do tipo que nao traz chave e aponta para outro lugar nao
///    vira "host padrao com a env": o operador disse onde o tipo mora.
/// 2. `llm.<kind>` declara OUTRO `provider:` construivel e traz o PROPRIO
///    `api_key` (ex.: `llm.openrouter { provider: openai, base_url:
///    https://openrouter.ai/api/v1, api_key }`): essa entrada inteira, com o
///    tipo que ela declara — a chave dela vai para o endpoint dela. Antes do
///    `provider_binding` a CLI achava essa chave pelo nome da entrada e a
///    autodeteccao escolhia o OpenRouter; descartar o candidato foi uma
///    regressao.
/// 3. Senao, o host padrao do tipo com a variavel de ambiente do tipo: uma
///    entrada de outro tipo com o nome do candidato nao descreve o candidato,
///    entao ela nao o apaga — `OPENROUTER_API_KEY` exportada continua valendo.
pub(crate) fn bind_autodetect(
    config: &AppConfig,
    kind: &str,
    env: Env<'_>,
) -> Option<ProviderBinding> {
    let named = bind_named(config, kind, env)?;
    if named.kind == kind {
        return named.has_credential().then_some(named);
    }
    let entry_has_own_key = config
        .llm
        .get(kind)
        .is_some_and(|cfg| non_empty(cfg.api_key.as_deref()).is_some());
    if entry_has_own_key && is_buildable_kind(&named.kind) {
        return Some(named);
    }
    let fallback = ProviderBinding::env_only(kind, env);
    fallback.has_credential().then_some(fallback)
}

/// Monta o provider de um vinculo. Nao faz I/O.
///
/// * `provider_id` renomeia um provider OpenAI-compativel (o `AgentRuntime`
///   procura provider por nome — GAR-582); `openrouter` sem nome explicito
///   continua registrado como `"openrouter"`.
/// * `url_override` e o `--url` da linha de comando e so vale para o
///   `llamacpp`, que nao tem credencial: aplicado a um tipo com chave, ele
///   mandaria a chave da entrada para um endereco avulso.
pub(crate) fn build_provider(
    binding: &ProviderBinding,
    model: &str,
    provider_id: Option<&str>,
    url_override: Option<&str>,
) -> Result<Arc<dyn LlmProvider>> {
    let model = Some(model.to_string());
    let base_url = binding.base_url.clone();
    match binding.kind.as_str() {
        "ollama" => Ok(Arc::new(OllamaProvider::new(model, base_url))),
        "llamacpp" => {
            let base = non_empty(url_override).or(base_url);
            Ok(Arc::new(LlamaCppProvider::new(model, base, None)))
        }
        "anthropic" => {
            let Some(key) = binding.api_key.as_deref() else {
                return Err(binding.missing_key_error());
            };
            Ok(Arc::new(AnthropicProvider::new(key, model, base_url)))
        }
        "openai" => {
            // Backend OpenAI-compativel local (LM Studio, vLLM) costuma nao
            // exigir chave: com `base_url` propria, sem chave, vai o marcador
            // — nunca a `OPENAI_API_KEY`, que `bind_entry` so poe no vinculo
            // quando o destino e a API da OpenAI. Esse destino (sem
            // `base_url`, ou com ela apontando para o proprio host padrao)
            // exige chave: sem ela, erro claro em vez de um 401 no primeiro
            // pedido.
            let custom_endpoint = !env_credential_allowed("openai", base_url.as_deref());
            let key = match binding.api_key.as_deref() {
                Some(k) => k.to_string(),
                None if custom_endpoint => KEYLESS_PLACEHOLDER.to_string(),
                None => return Err(binding.missing_key_error()),
            };
            let mut provider = OpenAiProvider::new(key, model, base_url);
            if let Some(id) = provider_id {
                provider = provider.with_name(id);
            }
            Ok(Arc::new(provider))
        }
        "openrouter" => {
            let Some(key) = binding.api_key.as_deref() else {
                return Err(binding.missing_key_error());
            };
            let base = base_url.unwrap_or_else(|| OPENROUTER_BASE_URL.to_string());
            let provider = OpenAiProvider::new(key, model, Some(base))
                .with_name(provider_id.unwrap_or("openrouter"));
            Ok(Arc::new(provider))
        }
        // Dev/CI only: o EchoProvider keyless (feature `dev-echo-provider`).
        #[cfg(feature = "dev-echo-provider")]
        "echo" => Ok(Arc::new(garraia_agents::EchoProvider::new(model))),
        other => bail!(
            "{} declara o tipo de provider '{other}', que a CLI nao sabe construir. \
             Tipos aceitos: ollama, llamacpp, anthropic, openai, openrouter",
            binding.origin()
        ),
    }
}

/// Forma canonica de um endpoint para comparar enderecos: sem espacos, sem
/// barra final e sem o `/v1` final — o mesmo corte que o `OpenAiProvider`
/// faz antes de acrescentar `/v1/chat/completions`, entao `http://h:1/v1` e
/// `http://h:1/` chegam ao mesmo lugar e contam como o mesmo endpoint.
fn canonical_endpoint(url: &str) -> &str {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed)
}

/// Credencial para um `--url` avulso.
///
/// 1. `LLM_API_KEY` — a variavel feita para isso;
/// 2. o `api_key` de uma entrada `llm:` cuja `base_url` e esse mesmo
///    endereco — e a credencial daquele host, entao manda-la para ele nao
///    cruza endpoint. Entradas percorridas em ordem de chave, deterministico;
/// 3. o marcador de "sem chave".
///
/// Nunca a `OPENAI_API_KEY` nem a `GARRAIA_EMBEDDING_API_KEY`.
pub(crate) fn credential_for_ad_hoc_url(config: &AppConfig, url: &str, env: Env<'_>) -> String {
    if let Some(key) = env(AD_HOC_URL_KEY_ENV).filter(|v| !v.is_empty()) {
        return key;
    }
    let target = canonical_endpoint(url);
    let mut keys: Vec<&String> = config.llm.keys().collect();
    keys.sort();
    for key in keys {
        let cfg = &config.llm[key];
        if let Some(base) = cfg.base_url.as_deref()
            && canonical_endpoint(base) == target
            && let Some(api_key) = non_empty(cfg.api_key.as_deref())
        {
            return api_key;
        }
    }
    KEYLESS_PLACEHOLDER.to_string()
}

/// Endpoint HTTP falso para os testes de roteamento: responde como OpenAI,
/// Anthropic e Ollama (com e sem streaming) e guarda cada pedido recebido,
/// para o teste afirmar QUEM recebeu o pedido e COM QUAL credencial.
#[cfg(test)]
pub(crate) mod mock_endpoint {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// Texto que so o endpoint falso devolve. Uma resposta com ele prova que
    /// o pedido chegou aqui, e nao ao host padrao do provider.
    pub(crate) const SENTINEL: &str = "resposta-do-endpoint-configurado";

    struct Responder;

    impl Respond for Responder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let path = request.url.path();
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null);
            let stream = body.get("stream").and_then(serde_json::Value::as_bool) == Some(true);
            if path.ends_with("/chat/completions") {
                if stream {
                    let chunk = serde_json::json!({
                        "id": "c", "object": "chat.completion.chunk", "model": "m",
                        "choices": [{"index": 0, "delta": {"role": "assistant", "content": SENTINEL}, "finish_reason": null}]
                    });
                    let end = serde_json::json!({
                        "id": "c", "object": "chat.completion.chunk", "model": "m",
                        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                    });
                    let sse = format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n");
                    return ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse);
                }
                return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "c", "object": "chat.completion", "model": "m",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": SENTINEL}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                }));
            }
            if path.ends_with("/v1/messages") {
                if stream {
                    let events = [
                        (
                            "message_start",
                            serde_json::json!({"type": "message_start", "message": {"id": "m", "type": "message", "role": "assistant", "model": "m", "content": [], "usage": {"input_tokens": 1, "output_tokens": 0}}}),
                        ),
                        (
                            "content_block_start",
                            serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
                        ),
                        (
                            "content_block_delta",
                            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": SENTINEL}}),
                        ),
                        (
                            "content_block_stop",
                            serde_json::json!({"type": "content_block_stop", "index": 0}),
                        ),
                        (
                            "message_delta",
                            serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"input_tokens": 1, "output_tokens": 1}}),
                        ),
                        ("message_stop", serde_json::json!({"type": "message_stop"})),
                    ];
                    let sse: String = events
                        .iter()
                        .map(|(ev, data)| format!("event: {ev}\ndata: {data}\n\n"))
                        .collect();
                    return ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse);
                }
                return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "m", "type": "message", "role": "assistant", "model": "m",
                    "content": [{"type": "text", "text": SENTINEL}],
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }));
            }
            if path.ends_with("/api/chat") {
                let done = serde_json::json!({
                    "model": "m", "message": {"role": "assistant", "content": SENTINEL}, "done": true
                });
                return ResponseTemplate::new(200)
                    .insert_header("content-type", "application/x-ndjson")
                    .set_body_string(format!("{done}\n"));
            }
            if path.ends_with("/models") {
                return ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "m"}]}));
            }
            if path.ends_with("/api/tags") {
                return ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models": [{"name": "m"}]}));
            }
            ResponseTemplate::new(404)
        }
    }

    /// Um endpoint falso em `127.0.0.1:<porta livre>`.
    pub(crate) struct MockEndpoint {
        server: MockServer,
    }

    impl MockEndpoint {
        pub(crate) async fn start() -> Self {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path_regex(".*"))
                .respond_with(Responder)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(".*"))
                .respond_with(Responder)
                .mount(&server)
                .await;
            Self { server }
        }

        /// `http://127.0.0.1:<porta>` — sem barra final.
        pub(crate) fn uri(&self) -> String {
            self.server.uri()
        }

        /// Caminhos dos pedidos recebidos, em ordem.
        pub(crate) async fn paths(&self) -> Vec<String> {
            self.server
                .received_requests()
                .await
                .unwrap_or_default()
                .iter()
                .map(|r| r.url.path().to_string())
                .collect()
        }

        /// A credencial de cada pedido recebido: o token do
        /// `Authorization: Bearer` ou o `x-api-key` (Anthropic). Pedido sem
        /// nenhum dos dois aparece como string vazia.
        pub(crate) async fn credentials(&self) -> Vec<String> {
            self.server
                .received_requests()
                .await
                .unwrap_or_default()
                .iter()
                .map(|r| {
                    let header = |name: &str| {
                        r.headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string)
                    };
                    header("authorization")
                        .map(|v| v.trim_start_matches("Bearer ").to_string())
                        .or_else(|| header("x-api-key"))
                        .unwrap_or_default()
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tudo puro: o ambiente entra por closure ([`Env`]), nunca por
    //! `std::env::set_var`.

    use super::mock_endpoint::{MockEndpoint, SENTINEL};
    use super::*;
    use garraia_agents::{ChatMessage, ChatRole, LlmRequest, MessagePart};
    use std::collections::HashMap;

    fn entry(provider: &str, key: Option<&str>, base: Option<&str>) -> LlmProviderConfig {
        LlmProviderConfig {
            provider: provider.to_string(),
            model: Some("m".to_string()),
            api_key: key.map(str::to_string),
            base_url: base.map(str::to_string),
            extra: HashMap::new(),
        }
    }

    fn config(entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = AppConfig::default();
        for (k, v) in entries {
            cfg.llm.insert((*k).to_string(), v.clone());
        }
        cfg
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn request() -> LlmRequest {
        LlmRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("oi".to_string()),
            }],
            system: None,
            max_tokens: Some(16),
            temperature: None,
            tools: Vec::new(),
        }
    }

    /// Faz UMA chamada de verdade e devolve o texto. Se o provider tivesse ido
    /// ao host padrao, a resposta nao traria o [`SENTINEL`] — so o endpoint
    /// falso o conhece.
    async fn call(provider: &Arc<dyn LlmProvider>) -> String {
        let response = provider
            .complete(&request())
            .await
            .unwrap_or_else(|e| panic!("chamada ao provider falhou: {e}"));
        response
            .content
            .iter()
            .filter_map(|b| match b {
                garraia_agents::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    // ── bind_entry / bind_named ─────────────────────────────────────────

    #[test]
    fn entry_key_beats_env_and_env_fills_only_a_missing_key() {
        let env = |var: &str| (var == "OPENAI_API_KEY").then(|| "do-ambiente".to_string());
        let com_chave = bind_entry("openai", &entry("openai", Some("da-entrada"), None), &env);
        assert_eq!(com_chave.api_key(), Some("da-entrada"));
        let sem_chave = bind_entry("openai", &entry("openai", None, None), &env);
        assert_eq!(sem_chave.api_key(), Some("do-ambiente"));
        // Chave vazia no config conta como ausente.
        let vazia = bind_entry("openai", &entry("openai", Some("  "), None), &no_env);
        assert_eq!(vazia.api_key(), None);
    }

    #[test]
    fn alias_entry_binds_with_its_declared_kind() {
        let cfg = config(&[(
            "lmstudio",
            entry("openai", Some("k-lm"), Some("http://127.0.0.1:1234/v1")),
        )]);
        let b = bind_named(&cfg, "lmstudio", &no_env).expect("alias configurado");
        assert_eq!(b.entry(), Some("lmstudio"));
        assert_eq!(b.kind(), "openai");
        assert_eq!(b.base_url(), Some("http://127.0.0.1:1234/v1"));
        assert_eq!(b.api_key(), Some("k-lm"));
    }

    #[test]
    fn main_entry_is_bound_whole_never_just_its_key() {
        let cfg = config(&[(
            "main",
            entry("anthropic", Some("k-main"), Some("https://proxy.interno")),
        )]);
        let b = bind_named(&cfg, "anthropic", &no_env).expect("main e do tipo");
        assert_eq!(b.entry(), Some("main"));
        assert_eq!(b.api_key(), Some("k-main"));
        assert_eq!(b.base_url(), Some("https://proxy.interno"));
        // `main` de outro tipo nao e vinculado.
        assert_eq!(
            bind_named(&cfg, "openai", &no_env).and_then(|b| b.entry().map(str::to_string)),
            None
        );
    }

    #[test]
    fn kind_without_entry_takes_only_the_env_credential() {
        // `llm.lmstudio` e do tipo openai e tem chave — mas `-p openai` sem
        // `llm.openai` NAO pega a chave dela: iria para api.openai.com.
        let cfg = config(&[(
            "lmstudio",
            entry("openai", Some("k-lm"), Some("http://127.0.0.1:1234/v1")),
        )]);
        let b = bind_named(&cfg, "openai", &no_env).expect("tipo construivel");
        assert_eq!(b.entry(), None);
        assert_eq!(b.base_url(), None);
        assert_eq!(b.api_key(), None);
        let env = |var: &str| (var == "OPENAI_API_KEY").then(|| "do-ambiente".to_string());
        assert_eq!(
            bind_named(&cfg, "openai", &env)
                .and_then(|b| b.api_key().map(str::to_string))
                .as_deref(),
            Some("do-ambiente")
        );
    }

    #[test]
    fn unknown_name_is_not_bound() {
        assert!(bind_named(&AppConfig::default(), "fogos", &no_env).is_none());
    }

    #[test]
    fn debug_never_prints_the_credential() {
        let b = bind_entry(
            "openai",
            &entry("openai", Some("sk-segredo-123"), None),
            &no_env,
        );
        let shown = format!("{b:?}");
        assert!(!shown.contains("sk-segredo-123"), "{shown}");
        assert!(shown.contains("api_key_set: true"), "{shown}");
    }

    #[test]
    fn build_refuses_a_kind_the_cli_cannot_build() {
        let b = bind_entry("sansa", &entry("sansa", Some("k"), None), &no_env);
        let Err(err) = build_provider(&b, "m", None, None) else {
            panic!("tipo desconhecido deve falhar");
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("llm.sansa") && msg.contains("'sansa'"),
            "{msg}"
        );
    }

    #[test]
    fn keyed_kinds_without_credential_fail_closed() {
        for kind in ["anthropic", "openrouter", "openai"] {
            let b = bind_named(&AppConfig::default(), kind, &no_env).expect("tipo");
            assert!(
                build_provider(&b, "m", None, None).is_err(),
                "{kind} sem chave"
            );
        }
    }

    /// A variavel do tipo, para cada tipo com chave: `env(var)` devolve
    /// `do-ambiente` so para ela.
    fn env_of(kind: &'static str) -> impl Fn(&str) -> Option<String> + Sync {
        move |var: &str| (Some(var) == provider_key_env(kind)).then(|| "do-ambiente".to_string())
    }

    /// Achado do verificador: `bind_entry` enchia com a variavel do tipo uma
    /// entrada sem chave MESMO com `base_url` propria, e a `OPENAI_API_KEY`
    /// (talvez de um `.env` no diretorio corrente) ia para aquele endpoint.
    /// A variavel so alcanca o host padrao do tipo.
    #[test]
    fn env_key_fills_an_entry_only_when_it_targets_the_default_host() {
        for (kind, default_forms, elsewhere) in [
            (
                "openai",
                &[
                    "https://api.openai.com",
                    "https://api.openai.com/",
                    "https://api.openai.com/v1",
                    "HTTPS://API.OPENAI.COM/v1/",
                ][..],
                &[
                    "http://127.0.0.1:1234/v1",
                    // Mesmo nome, outro esquema/porta/host: nao e o host padrao.
                    "http://api.openai.com/v1",
                    "https://api.openai.com:8443/v1",
                    "https://api.openai.com.exemplo.test/v1",
                    "https://usuario@api.openai.com/v1",
                    // O host padrao de OUTRO tipo tambem e "outro lugar".
                    "https://openrouter.ai/api/v1",
                ][..],
            ),
            (
                "openrouter",
                // O que o `garraia init` grava no `llm.openrouter`.
                &[
                    "https://openrouter.ai/api/v1",
                    "https://openrouter.ai/api/v1/",
                ][..],
                &["https://proxy.interno/api/v1", "https://openrouter.ai/v1"][..],
            ),
            (
                "anthropic",
                &["https://api.anthropic.com", "https://api.anthropic.com/"][..],
                &["https://proxy.interno", "https://api.openai.com"][..],
            ),
        ] {
            let env = env_of(kind);
            let sem_base = bind_entry(kind, &entry(kind, None, None), &env);
            assert_eq!(
                sem_base.api_key(),
                Some("do-ambiente"),
                "{kind} sem base_url"
            );
            for base in default_forms {
                let b = bind_entry(kind, &entry(kind, None, Some(base)), &env);
                assert_eq!(b.api_key(), Some("do-ambiente"), "{kind} {base}");
            }
            for base in elsewhere {
                let b = bind_entry(kind, &entry(kind, None, Some(base)), &env);
                assert_eq!(b.api_key(), None, "{kind} {base}: a env nao vai para ela");
                // A chave da propria entrada continua valendo ali.
                let com_chave =
                    bind_entry(kind, &entry(kind, Some("da-entrada"), Some(base)), &env);
                assert_eq!(com_chave.api_key(), Some("da-entrada"), "{kind} {base}");
            }
        }
        // Tipo sem entrada: host padrao, variavel do ambiente — inalterado.
        let b = bind_named(&AppConfig::default(), "openai", &env_of("openai")).expect("tipo");
        assert_eq!((b.base_url(), b.api_key()), (None, Some("do-ambiente")));
    }

    /// O mesmo achado na rede: com `OPENAI_API_KEY`/`ANTHROPIC_API_KEY`/
    /// `OPENROUTER_API_KEY` no ambiente e uma entrada sem chave apontando
    /// para um endpoint proprio, o endpoint nunca ve a variavel. O
    /// OpenAI-compativel recebe o marcador; os outros dois falham fechado
    /// antes de qualquer pedido.
    #[tokio::test]
    async fn env_key_never_reaches_the_entry_own_base_url() {
        for (kind, suffix) in [
            ("openai", "/v1"),
            ("openrouter", "/api/v1"),
            ("anthropic", ""),
        ] {
            let mock = MockEndpoint::start().await;
            let base = format!("{}{suffix}", mock.uri());
            let cfg = config(&[(kind, entry(kind, None, Some(&base)))]);
            let b = bind_named(&cfg, kind, &env_of(kind)).expect("entrada");
            match build_provider(&b, "m", None, None) {
                Ok(provider) => {
                    assert_eq!(kind, "openai", "{kind} sem chave tinha de falhar fechado");
                    assert_eq!(call(&provider).await, SENTINEL);
                    assert_eq!(
                        mock.credentials().await,
                        vec![KEYLESS_PLACEHOLDER.to_string()],
                        "{kind}"
                    );
                }
                Err(err) => {
                    assert_ne!(kind, "openai", "{kind}: {err}");
                    let msg = format!("{err}");
                    assert!(
                        msg.contains(&format!("llm.{kind}")) && msg.contains("never sent"),
                        "{msg}"
                    );
                }
            }
            assert!(
                !mock.credentials().await.iter().any(|c| c == "do-ambiente"),
                "{kind}: a variavel do ambiente chegou na base_url da entrada"
            );
        }
    }

    /// `base_url` igual ao host padrao da OpenAI, sem chave nem env: erro
    /// claro, nao o marcador `not-needed` mandado para api.openai.com.
    #[test]
    fn openai_default_host_without_key_fails_closed_even_with_explicit_base_url() {
        let cfg = config(&[(
            "openai",
            entry("openai", None, Some("https://api.openai.com/v1")),
        )]);
        let b = bind_named(&cfg, "openai", &no_env).expect("entrada");
        let Err(err) = build_provider(&b, "m", None, None) else {
            panic!("api.openai.com sem chave deve falhar");
        };
        assert!(format!("{err}").contains("OPENAI_API_KEY not set"), "{err}");
    }

    /// Achado da verificacao da #1370: o caminho explicito (`-p openrouter`,
    /// `garra_ask`/`garra_agent` com `provider=openrouter`) tinha de fazer o
    /// que a autodeteccao ja fazia com uma entrada de outro tipo sem chave.
    #[test]
    fn explicit_mismatched_entry_without_key_falls_back_to_the_env_default_host() {
        let cfg = config(&[(
            "openrouter",
            entry("openai", None, Some("https://openrouter.ai/api/v1")),
        )]);
        let b = bind_named(&cfg, "openrouter", &env_of("openrouter")).expect("vinculo");
        assert_eq!((b.entry(), b.kind()), (None, "openrouter"));
        assert_eq!(b.base_url(), None, "a env so vai para o host padrao");
        assert_eq!(b.api_key(), Some("do-ambiente"));
        // Sem a variavel, fica a entrada como ela e (sem chave).
        let b = bind_named(&cfg, "openrouter", &no_env).expect("vinculo");
        assert_eq!((b.entry(), b.kind()), (Some("openrouter"), "openai"));
        // Com chave propria, a entrada inteira vale e a env nao entra.
        let cfg = config(&[(
            "openrouter",
            entry("openai", Some("k-or"), Some("https://openrouter.ai/api/v1")),
        )]);
        let b = bind_named(&cfg, "openrouter", &env_of("openrouter")).expect("vinculo");
        assert_eq!((b.entry(), b.api_key()), (Some("openrouter"), Some("k-or")));
        // Alias que nao e nome de tipo (`lmstudio`) nunca troca de entrada.
        let cfg = config(&[(
            "lmstudio",
            entry("openai", None, Some("http://127.0.0.1:1234/v1")),
        )]);
        let b = bind_named(&cfg, "lmstudio", &env_of("openai")).expect("vinculo");
        assert_eq!(b.entry(), Some("lmstudio"));
        assert_eq!(b.base_url(), Some("http://127.0.0.1:1234/v1"));
        assert_eq!(b.api_key(), None, "a env da OpenAI nao vai para o proxy");
    }

    // ── bind_autodetect ─────────────────────────────────────────────────

    /// Achado do verificador: `llm.openrouter` com `provider: openai` e
    /// chave propria era autodetectado (a chave era achada pelo nome da
    /// entrada) e o `provider_binding` passou a descartar o candidato.
    #[test]
    fn autodetect_binds_a_mismatched_entry_whole_when_it_has_its_own_key() {
        let cfg = config(&[(
            "openrouter",
            entry("openai", Some("k-or"), Some("https://openrouter.ai/api/v1")),
        )]);
        let b = bind_autodetect(&cfg, "openrouter", &no_env).expect("candidato mantido");
        assert_eq!(b.entry(), Some("openrouter"));
        assert_eq!(b.kind(), "openai");
        assert_eq!(b.base_url(), Some("https://openrouter.ai/api/v1"));
        assert_eq!(b.api_key(), Some("k-or"));
    }

    /// Entrada de outro tipo sem chave propria: o candidato cai para o host
    /// padrao dele com a variavel dele — e a `base_url` da entrada nao
    /// recebe essa variavel.
    #[test]
    fn autodetect_mismatched_entry_without_key_falls_back_to_the_env_default_host() {
        let cfg = config(&[(
            "openrouter",
            entry("openai", None, Some("http://127.0.0.1:1234/v1")),
        )]);
        let b = bind_autodetect(&cfg, "openrouter", &env_of("openrouter")).expect("env");
        assert_eq!(b.entry(), None);
        assert_eq!(b.kind(), "openrouter");
        assert_eq!(b.base_url(), None, "a env so vai para o host padrao");
        assert_eq!(b.api_key(), Some("do-ambiente"));
        // Sem a variavel, nao ha candidato.
        assert!(bind_autodetect(&cfg, "openrouter", &no_env).is_none());
        // Tipo desconhecido declarado com chave propria: nao e construivel,
        // entao tambem cai para a env do candidato.
        let cfg = config(&[("openrouter", entry("sansa", Some("k"), None))]);
        let b = bind_autodetect(&cfg, "openrouter", &env_of("openrouter")).expect("env");
        assert_eq!((b.entry(), b.kind()), (None, "openrouter"));
    }

    /// `llm.openai` do tipo, sem chave, apontando para um proxy: a env da
    /// OpenAI nao vai para o proxy (MEDIUM) nem vira "api.openai.com com a
    /// env" — o operador disse onde a OpenAI mora.
    #[test]
    fn autodetect_same_kind_entry_pointing_elsewhere_without_key_is_not_a_candidate() {
        let cfg = config(&[(
            "openai",
            entry("openai", None, Some("https://proxy.interno/v1")),
        )]);
        assert!(bind_autodetect(&cfg, "openai", &env_of("openai")).is_none());
        // Com a chave propria, e candidato normal.
        let cfg = config(&[(
            "openai",
            entry("openai", Some("k"), Some("https://proxy.interno/v1")),
        )]);
        let b = bind_autodetect(&cfg, "openai", &env_of("openai")).expect("candidato");
        assert_eq!((b.entry(), b.api_key()), (Some("openai"), Some("k")));
    }

    // ── o endpoint de verdade: pedido chega ONDE o config manda ──────────

    /// Cada tipo com `base_url` configurada: o pedido chega no endpoint
    /// configurado, com a chave daquela entrada, e a resposta vem de la.
    #[tokio::test]
    async fn every_kind_calls_the_configured_base_url_with_its_own_key() {
        struct Row {
            kind: &'static str,
            key: Option<&'static str>,
            base_suffix: &'static str,
            expected_path: &'static str,
        }
        let rows = [
            Row {
                kind: "openai",
                key: Some("k-openai"),
                base_suffix: "/v1",
                expected_path: "/v1/chat/completions",
            },
            Row {
                kind: "openrouter",
                key: Some("k-openrouter"),
                base_suffix: "/api/v1",
                expected_path: "/api/v1/chat/completions",
            },
            Row {
                kind: "anthropic",
                key: Some("k-anthropic"),
                base_suffix: "",
                expected_path: "/v1/messages",
            },
            Row {
                kind: "ollama",
                key: None,
                base_suffix: "",
                expected_path: "/api/chat",
            },
            Row {
                kind: "llamacpp",
                key: None,
                base_suffix: "",
                expected_path: "/v1/chat/completions",
            },
        ];
        for row in rows {
            let mock = MockEndpoint::start().await;
            let base = format!("{}{}", mock.uri(), row.base_suffix);
            let cfg = config(&[(row.kind, entry(row.kind, row.key, Some(&base)))]);
            let b = bind_named(&cfg, row.kind, &no_env).expect("entrada");
            let provider =
                build_provider(&b, "m", None, None).unwrap_or_else(|e| panic!("{}: {e}", row.kind));
            let text = call(&provider).await;
            assert_eq!(
                text, SENTINEL,
                "{}: resposta nao veio do endpoint configurado",
                row.kind
            );
            assert_eq!(
                mock.paths().await,
                vec![row.expected_path.to_string()],
                "{}",
                row.kind
            );
            if let Some(key) = row.key {
                assert_eq!(
                    mock.credentials().await,
                    vec![key.to_string()],
                    "{}",
                    row.kind
                );
            }
        }
    }

    #[tokio::test]
    async fn ad_hoc_url_never_receives_the_openai_or_embedding_key() {
        let mock = MockEndpoint::start().await;
        let url = format!("{}/v1", mock.uri());
        let env = |var: &str| match var {
            "OPENAI_API_KEY" => Some("sk-da-openai".to_string()),
            "GARRAIA_EMBEDDING_API_KEY" => Some("k-embeddings".to_string()),
            _ => None,
        };
        let key = credential_for_ad_hoc_url(&AppConfig::default(), &url, &env);
        assert_eq!(key, KEYLESS_PLACEHOLDER);

        // `LLM_API_KEY` e a variavel desse caminho.
        let env_llm = |var: &str| (var == "LLM_API_KEY").then(|| "k-avulsa".to_string());
        assert_eq!(
            credential_for_ad_hoc_url(&AppConfig::default(), &url, &env_llm),
            "k-avulsa"
        );

        // Entrada do config com a MESMA base_url: a chave dela, e so dela.
        let cfg = config(&[
            (
                "outra",
                entry("openai", Some("k-outra"), Some("https://api.openai.com")),
            ),
            (
                "local",
                entry("openai", Some("k-local"), Some(&format!("{}/", mock.uri()))),
            ),
        ]);
        assert_eq!(credential_for_ad_hoc_url(&cfg, &url, &env), "k-local");
    }
}
