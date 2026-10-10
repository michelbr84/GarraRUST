//! GAR-579 / GAR-583 / #1612 — o núcleo one-shot de `garra_ask`.
//!
//! LLM-only: nenhuma ferramenta é registrada no `AgentRuntime`, e nada aqui
//! imprime, lê stdin ou toca o sistema de arquivos. Quem chama decide como
//! emitir o [`AskOutcome`] (linha JSON na CLI, `CallToolResult` no stdio,
//! envelope HTTP na ponte).

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use garraia_agents::{AgentRuntime, ChatMessage, LlmProvider};
use garraia_config::AppConfig;
use regex::Regex;
use serde_json::json;

use crate::provider::select_explicit_provider;

/// Teto do `message`, em bytes. Mesmo valor do cap de stdin da CLI, para o
/// mesmo pedido ter o mesmo tamanho máximo em qualquer superfície.
pub const ARG_MESSAGE_MAX_BYTES: usize = 64 * 1024;

/// Teto do `system_prompt` de quem chama, em bytes.
pub const ARG_SYSTEM_PROMPT_MAX_BYTES: usize = 8 * 1024;

/// Faixa de `timeout_secs` aceita por `garra_ask`.
pub const ARG_TIMEOUT_SECS_MIN: u64 = 1;
pub const ARG_TIMEOUT_SECS_MAX: u64 = 600;
pub const ARG_TIMEOUT_SECS_DEFAULT: u64 = 60;

/// Providers aceitos pelo `garra_ask`, como o `enum` anunciado no schema. As
/// duas superfícies (stdio e HTTP) leem daqui, para não divergirem.
pub const PROVEDORES_ASK: [&str; 4] = ["ollama", "anthropic", "openai", "openrouter"];

/// Truncamento de mensagens de erro do provider no envelope.
const ERROR_MSG_TRUNCATE: usize = 512;

/// GAR-579 — erros tipados com `kind` estável e códigos de saída no estilo
/// sysexits. `message()` nunca leva fingerprint de chave de API: ela passa por
/// [`sanitize_provider_error`] na fronteira.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskError {
    /// Uso incorreto da CLI ou entrada vazia. Saída 2.
    UsageError(String),
    /// Nenhum provider pôde ser resolvido (config + flags + env). Saída 69.
    NoProvider(String),
    /// O provider devolveu erro (auth, rede, rate limit...). A mensagem já
    /// passou por `sanitize_provider_error`. Saída 69.
    ProviderError(String),
    /// A chamada ao LLM estourou o timeout. Saída 124.
    Timeout(u64),
    /// Falha de I/O (leitura de stdin, etc.). Saída 74.
    IoError(String),
}

impl AskError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UsageError(_) => 2,
            Self::NoProvider(_) | Self::ProviderError(_) => 69,
            Self::Timeout(_) => 124,
            Self::IoError(_) => 74,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::UsageError(_) => "usage",
            Self::NoProvider(_) => "no_provider",
            Self::ProviderError(_) => "provider_error",
            Self::Timeout(_) => "timeout",
            Self::IoError(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::UsageError(m)
            | Self::NoProvider(m)
            | Self::ProviderError(m)
            | Self::IoError(m) => m.clone(),
            Self::Timeout(s) => format!("LLM call exceeded {s}s timeout"),
        }
    }
}

static RE_OPENROUTER_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-or-v1-[A-Za-z0-9_\-]+").expect("RE_OPENROUTER_KEY"));
static RE_GENERIC_SK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9_\-]{8,}").expect("RE_GENERIC_SK"));

/// GAR-579 — apaga fingerprints de chave de API de uma mensagem de erro do
/// provider antes de ela entrar no envelope ou no stderr.
///
/// Trunca em [`ERROR_MSG_TRUNCATE`] bytes. A ordem importa: o padrão
/// `sk-or-v1-…` é casado PRIMEIRO, senão o `sk-…` genérico o consumiria.
///
/// Defesa em profundidade — a correção de fundo da redação mora em
/// `garraia-security::RedactingWriter`.
pub fn sanitize_provider_error(msg: &str) -> String {
    let truncated: String = if msg.chars().count() > ERROR_MSG_TRUNCATE {
        let mut s: String = msg.chars().take(ERROR_MSG_TRUNCATE).collect();
        s.push('…');
        s
    } else {
        msg.to_string()
    };
    let s = RE_OPENROUTER_KEY.replace_all(&truncated, "sk-or-v1-[REDACTED]");
    RE_GENERIC_SK.replace_all(&s, "sk-[REDACTED]").into_owned()
}

/// GAR-579 — envelope JSON `garra.ask.v1`, ramo de sucesso.
pub fn success_envelope(
    answer: &str,
    provider: &str,
    model: &str,
    latency_ms: u128,
) -> serde_json::Value {
    json!({
        "schema": "garra.ask.v1",
        "ok": true,
        "answer": answer,
        "provider": provider,
        "model": model,
        "latency_ms": latency_ms,
    })
}

/// GAR-579 — envelope JSON `garra.ask.v1`, ramo de erro.
pub fn error_envelope(kind: &str, message: &str) -> serde_json::Value {
    json!({
        "schema": "garra.ask.v1",
        "ok": false,
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}

/// Valida os argumentos de um pedido `garra_ask`: `message` não vazio e dentro
/// do teto, `timeout_secs` na faixa, `system_prompt` dentro do teto. O `Err` é
/// a mensagem que o chamador recebe.
pub fn validar_argumentos(
    message: &str,
    timeout_secs: Option<u64>,
    system_prompt: Option<&str>,
) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("message must be non-empty".to_string());
    }
    if message.len() > ARG_MESSAGE_MAX_BYTES {
        return Err(format!(
            "message exceeds 64 KiB cap ({ARG_MESSAGE_MAX_BYTES} bytes)"
        ));
    }
    if let Some(ts) = timeout_secs
        && !(ARG_TIMEOUT_SECS_MIN..=ARG_TIMEOUT_SECS_MAX).contains(&ts)
    {
        return Err(format!(
            "timeout_secs out of range [{ARG_TIMEOUT_SECS_MIN}, {ARG_TIMEOUT_SECS_MAX}]"
        ));
    }
    if let Some(sp) = system_prompt
        && sp.len() > ARG_SYSTEM_PROMPT_MAX_BYTES
    {
        return Err(format!(
            "system_prompt exceeds {ARG_SYSTEM_PROMPT_MAX_BYTES}-byte cap"
        ));
    }
    Ok(())
}

/// GAR-583 — opções puras de [`ask_oneshot`] e [`executar`].
///
/// `message` é o prompt já resolvido. Os demais campos espelham as flags da
/// CLI. `provider_override` é o nome explícito do provider: o núcleo exige um
/// (ver [`ask_oneshot`]); a autodetecção é decisão da CLI.
#[derive(Debug, Clone)]
pub struct AskOptions {
    pub message: String,
    pub provider_override: Option<String>,
    pub model_override: Option<String>,
    pub url_override: Option<String>,
    pub timeout_secs: u64,
    pub system_prompt_override: Option<String>,
    /// Baixa um modelo Ollama ausente sem perguntar. `ask` não é interativo
    /// por contrato: sem isto, modelo ausente é erro limpo com dica de
    /// `ollama pull` — nunca um prompt bloqueante.
    pub assume_yes: bool,
}

/// Um provider já resolvido: `(nome, modelo, provider)`.
pub type Provedor = (String, String, Arc<dyn LlmProvider>);

/// GAR-583 — resultado puro de [`ask_oneshot`]: o sucesso carrega resposta,
/// provider, modelo e latência; a falha carrega o [`AskError`] tipado.
#[derive(Debug, Clone)]
pub enum AskOutcome {
    Success {
        answer: String,
        provider: String,
        model: String,
        latency_ms: u128,
    },
    Failure(AskError),
}

impl AskOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Monta o envelope `garra.ask.v1` deste resultado.
    pub fn to_envelope(&self) -> serde_json::Value {
        match self {
            Self::Success {
                answer,
                provider,
                model,
                latency_ms,
            } => success_envelope(answer, provider, model, *latency_ms),
            Self::Failure(err) => error_envelope(err.kind_str(), &err.message()),
        }
    }

    /// Código de saída: 0 no sucesso, o do [`AskError`] na falha.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Success { .. } => 0,
            Self::Failure(err) => err.exit_code(),
        }
    }
}

/// GAR-583 — o núcleo one-shot com provider explícito: resolve o provider,
/// chama o LLM e devolve o resultado.
///
/// Exige `opts.provider_override`. A ponte HTTP sempre passa um (o default do
/// projeto quando o chamador omite); a CLI sem `-p` usa a autodetecção e chama
/// [`executar`] direto, com o provider que ela escolheu.
///
/// **Zero I/O além da chamada HTTP ao provider.** Sem `println!`, stdin ou
/// acesso a arquivo.
pub async fn ask_oneshot(config: &AppConfig, opts: AskOptions) -> AskOutcome {
    let inicio = Instant::now();
    let Some(nome) = opts.provider_override.clone() else {
        return AskOutcome::Failure(AskError::UsageError(
            "provider nao informado para garra_ask".to_string(),
        ));
    };
    match select_explicit_provider(
        config,
        &nome,
        opts.model_override.as_deref(),
        opts.url_override.as_deref(),
    ) {
        Ok(provedor) => executar(provedor, inicio, opts).await,
        Err(e) => AskOutcome::Failure(AskError::NoProvider(sanitize_provider_error(&format!(
            "{e:#}"
        )))),
    }
}

/// GAR-583 — chama o LLM com um provider já resolvido, sob o timeout de
/// `opts.timeout_secs`. `inicio` é o instante em que a resolução começou, para
/// a latência cobrir a escolha do provider como antes.
///
/// Invariantes:
///   - **NUNCA** registra tool no `AgentRuntime` (LLM-only).
///   - Erros do provider passam por [`sanitize_provider_error`].
///   - O canal de streaming é drenado em tarefa de fundo, para o produtor
///     nunca bloquear num consumidor lento.
pub async fn executar(provedor: Provedor, inicio: Instant, opts: AskOptions) -> AskOutcome {
    let (provider_name, model_name, provider) = provedor;

    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);

    let system_prompt = opts.system_prompt_override.unwrap_or_else(|| {
        "Voce e o GarraIA. Responda de forma concisa, direta e no idioma do usuario.".to_string()
    });
    runtime.set_system_prompt(system_prompt);
    runtime.set_max_tokens(4096);

    // Só existe a API de streaming; drenamos os deltas em tarefa de fundo.
    let session_id = format!("ask-{}", uuid::Uuid::new_v4());
    let history: Vec<ChatMessage> = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);
    let drain_handle = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let call = runtime.process_message_streaming(
        &session_id,
        &opts.message,
        &history,
        tx,
        Some(&model_name),
    );
    let result = tokio::time::timeout(Duration::from_secs(opts.timeout_secs), call).await;

    drain_handle.abort();

    let latency_ms = inicio.elapsed().as_millis();

    match result {
        Ok(Ok(full)) => AskOutcome::Success {
            answer: full,
            provider: provider_name,
            model: model_name,
            latency_ms,
        },
        Ok(Err(e)) => AskOutcome::Failure(AskError::ProviderError(sanitize_provider_error(
            &format!("{e:#}"),
        ))),
        Err(_elapsed) => AskOutcome::Failure(AskError::Timeout(opts.timeout_secs)),
    }
}

#[cfg(test)]
mod tests {
    //! GAR-579 — testes puros. Zero rede, zero mutação de env, zero disco
    //! (exceto o endpoint de loopback dos testes de roteamento, no fim).

    use super::*;

    // ─── AskError exit codes + kind labels ─────────────────────────────

    #[test]
    fn ask_error_exit_codes_match_doc() {
        let cases: &[(AskError, i32)] = &[
            (AskError::UsageError("x".into()), 2),
            (AskError::NoProvider("x".into()), 69),
            (AskError::ProviderError("x".into()), 69),
            (AskError::Timeout(60), 124),
            (AskError::IoError("x".into()), 74),
        ];
        for (err, expected) in cases {
            assert_eq!(err.exit_code(), *expected, "exit_code mismatch for {err:?}");
        }
    }

    #[test]
    fn ask_error_kind_str_stable() {
        assert_eq!(AskError::UsageError("".into()).kind_str(), "usage");
        assert_eq!(AskError::NoProvider("".into()).kind_str(), "no_provider");
        assert_eq!(
            AskError::ProviderError("".into()).kind_str(),
            "provider_error"
        );
        assert_eq!(AskError::Timeout(30).kind_str(), "timeout");
        assert_eq!(AskError::IoError("".into()).kind_str(), "io");
    }

    // ─── JSON envelopes ────────────────────────────────────────────────

    #[test]
    fn json_envelope_success_shape() {
        let env = success_envelope("hello", "openrouter", "openrouter/free", 123);
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "hello");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["latency_ms"], 123);
        assert!(env.get("error").is_none());
    }

    #[test]
    fn json_envelope_error_shape() {
        let env = error_envelope("timeout", "exceeded 30s");
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "timeout");
        assert_eq!(env["error"]["message"], "exceeded 30s");
        assert!(env.get("answer").is_none());
    }

    // ─── sanitize_provider_error ───────────────────────────────────────

    #[test]
    fn sanitize_redacts_openrouter_key_fingerprint() {
        let leaked = "401 Unauthorized — key sk-or-v1-abcdefGHIJ12345 invalid";
        let out = sanitize_provider_error(leaked);
        assert!(!out.contains("sk-or-v1-abcdefGHIJ12345"));
        assert!(out.contains("sk-or-v1-[REDACTED]"));
    }

    #[test]
    fn sanitize_redacts_openai_style_key_fingerprint() {
        let leaked = "Incorrect API key provided: sk-projAbCd123xyz_456_more";
        let out = sanitize_provider_error(leaked);
        assert!(!out.contains("AbCd123xyz_456_more"));
        assert!(out.contains("sk-[REDACTED]"));
    }

    #[test]
    fn sanitize_truncates_long_messages() {
        let huge = "x".repeat(2_000);
        let out = sanitize_provider_error(&huge);
        assert!(out.chars().count() <= ERROR_MSG_TRUNCATE + 1);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn sanitize_passes_clean_messages_through() {
        let benign = "Connection refused at localhost:11434";
        assert_eq!(sanitize_provider_error(benign), benign);
    }

    // ─── validar_argumentos ────────────────────────────────────────────

    #[test]
    fn validar_argumentos_aceita_o_que_o_schema_anuncia() {
        assert!(validar_argumentos("oi", None, None).is_ok());
        assert!(validar_argumentos("oi", Some(ARG_TIMEOUT_SECS_MIN), None).is_ok());
        assert!(validar_argumentos("oi", Some(ARG_TIMEOUT_SECS_MAX), None).is_ok());
    }

    #[test]
    fn validar_argumentos_recusa_fora_dos_limites() {
        assert!(validar_argumentos("   ", None, None).is_err());
        assert!(validar_argumentos(&"a".repeat(ARG_MESSAGE_MAX_BYTES + 1), None, None).is_err());
        assert!(validar_argumentos("oi", Some(0), None).is_err());
        assert!(validar_argumentos("oi", Some(ARG_TIMEOUT_SECS_MAX + 1), None).is_err());
        let longo = "a".repeat(ARG_SYSTEM_PROMPT_MAX_BYTES + 1);
        assert!(validar_argumentos("oi", None, Some(&longo)).is_err());
    }

    // ─── AskOutcome ────────────────────────────────────────────────────

    fn make_success() -> AskOutcome {
        AskOutcome::Success {
            answer: "hello".to_string(),
            provider: "openrouter".to_string(),
            model: "openrouter/free".to_string(),
            latency_ms: 1234,
        }
    }

    fn make_failure(err: AskError) -> AskOutcome {
        AskOutcome::Failure(err)
    }

    #[test]
    fn ask_outcome_is_ok_distinguishes_variants() {
        assert!(make_success().is_ok());
        assert!(!make_failure(AskError::Timeout(30)).is_ok());
    }

    #[test]
    fn ask_outcome_to_envelope_success_shape() {
        let env = make_success().to_envelope();
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "hello");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["latency_ms"], 1234);
        assert!(env.get("error").is_none());
    }

    #[test]
    fn ask_outcome_to_envelope_error_shape() {
        let env = make_failure(AskError::ProviderError("bad".to_string())).to_envelope();
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "provider_error");
        assert_eq!(env["error"]["message"], "bad");
        assert!(env.get("answer").is_none());
    }

    #[test]
    fn ask_outcome_exit_code_mapping_table_driven() {
        let cases: &[(AskOutcome, i32)] = &[
            (make_success(), 0),
            (make_failure(AskError::UsageError("x".into())), 2),
            (make_failure(AskError::NoProvider("x".into())), 69),
            (make_failure(AskError::ProviderError("x".into())), 69),
            (make_failure(AskError::Timeout(60)), 124),
            (make_failure(AskError::IoError("x".into())), 74),
        ];
        for (outcome, expected) in cases {
            assert_eq!(
                outcome.exit_code(),
                *expected,
                "exit_code mismatch for {outcome:?}"
            );
        }
    }

    // ─── Auditoria: este módulo nunca registra tool ────────────────────

    /// Garantia de leitura. Varre só a parte de produção do arquivo (tudo antes
    /// de `#[cfg(test)]`) em busca de padrões de registro de tool. Uma PR que
    /// tente encaixar uma tool aqui faz este teste falhar.
    #[test]
    fn ask_module_never_registers_a_tool() {
        let source = include_str!("ask.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            "register_tool",
            "BashTool",
            "FileReadTool",
            "FileWriteTool",
            "GitDiffTool",
        ];
        for needle in forbidden {
            assert!(
                !production.contains(needle),
                "ask.rs production code must not contain `{needle}` (GAR-579 invariant)"
            );
        }
    }
}

#[cfg(test)]
mod provider_routing_tests {
    //! Rede só de loopback: um endpoint falso em 127.0.0.1 no lugar do
    //! provider. É o núcleo de `garraia ask -p <provider>` E do `garra_ask`
    //! do MCP HTTP (os dois chamam [`ask_oneshot`] com `provider_override`).

    use super::*;
    use crate::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
    use garraia_config::LlmProviderConfig;

    /// O `ask_explicit` do smoke de instalação limpa: `-p openai` com
    /// `llm.openai.base_url` apontando para outro endpoint tem de chegar LÁ,
    /// com a chave daquela entrada — e o mesmo para os outros provedores e
    /// para um alias em `llm:`.
    #[tokio::test]
    async fn ask_with_explicit_provider_reaches_the_configured_base_url() {
        for (name, kind, suffix) in [
            ("openai", "openai", "/v1"),
            ("openrouter", "openrouter", "/api/v1"),
            ("anthropic", "anthropic", ""),
            ("lmstudio", "openai", "/v1"),
        ] {
            let mock = MockEndpoint::start().await;
            let key = format!("k-{name}");
            let mut config = AppConfig::default();
            config.llm.insert(
                name.to_string(),
                LlmProviderConfig {
                    provider: kind.to_string(),
                    model: Some("m".to_string()),
                    api_key: Some(key.clone()),
                    base_url: Some(format!("{}{suffix}", mock.uri())),
                    extra: Default::default(),
                },
            );
            let outcome = ask_oneshot(
                &config,
                AskOptions {
                    message: "oi".to_string(),
                    provider_override: Some(name.to_string()),
                    model_override: None,
                    url_override: None,
                    timeout_secs: 30,
                    system_prompt_override: None,
                    assume_yes: false,
                },
            )
            .await;
            match outcome {
                AskOutcome::Success {
                    answer,
                    provider,
                    model,
                    ..
                } => {
                    assert_eq!(answer, SENTINEL, "{name}");
                    assert_eq!((provider.as_str(), model.as_str()), (name, "m"));
                }
                AskOutcome::Failure(e) => panic!("{name}: {e:?}"),
            }
            let creds = mock.credentials().await;
            assert!(
                !creds.is_empty() && creds.iter().all(|c| *c == key),
                "{name}: credenciais recebidas {creds:?}"
            );
        }
    }
}
