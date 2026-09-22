//! `garra ask` contra o binario de verdade: com `llm.<nome>.base_url`
//! apontando para um endpoint local, o pedido chega LA, com a chave daquela
//! entrada, e NADA sai para o host padrao do provider.
//!
//! E o `ask_explicit` do smoke de instalacao limpa da v0.4.4 levado para a
//! suite: `garraia ask -p openai` mandava `llm.openai.api_key` para
//! `https://api.openai.com` e ignorava `llm.openai.base_url`.
//!
//! "Nada sai para o host padrao" e afirmado por uma armadilha, nao pela
//! ausencia de rede: `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` do subprocesso
//! apontam para um listener que registra a primeira linha de cada conexao
//! (`CONNECT api.openai.com:443 HTTP/1.1` quando o destino e HTTPS) e fecha.
//! `NO_PROXY` cobre `127.0.0.1`, entao o endpoint configurado e alcancado
//! direto. Um pedido que fosse ao host padrao apareceria na armadilha.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const SENTINEL: &str = "resposta-do-endpoint-configurado";

/// Responde como OpenAI (SSE) e Anthropic (SSE) e Ollama (NDJSON) — o
/// `garra ask` sempre pede streaming.
struct Responder;

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let path = request.url.path();
        if path.ends_with("/chat/completions") {
            let chunk = serde_json::json!({
                "id": "c", "object": "chat.completion.chunk", "model": "m",
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": SENTINEL}, "finish_reason": null}]
            });
            let end = serde_json::json!({
                "id": "c", "object": "chat.completion.chunk", "model": "m",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            });
            return ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"));
        }
        if path.ends_with("/v1/messages") {
            let events = [
                serde_json::json!({"type": "message_start", "message": {"id": "m", "type": "message", "role": "assistant", "model": "m", "content": [], "usage": {"input_tokens": 1, "output_tokens": 0}}}),
                serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
                serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": SENTINEL}}),
                serde_json::json!({"type": "content_block_stop", "index": 0}),
                serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"input_tokens": 1, "output_tokens": 1}}),
                serde_json::json!({"type": "message_stop"}),
            ];
            let sse: String = events
                .iter()
                .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("")))
                .collect();
            return ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse);
        }
        if path.ends_with("/api/chat") {
            let done = serde_json::json!({
                "model": "m", "message": {"role": "assistant", "content": SENTINEL}, "done": true
            });
            return ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-ndjson")
                .set_body_string(format!("{done}\n"));
        }
        ResponseTemplate::new(404)
    }
}

async fn endpoint() -> MockServer {
    let server = MockServer::start().await;
    for verb in ["POST", "GET"] {
        Mock::given(method(verb))
            .and(path_regex(".*"))
            .respond_with(Responder)
            .mount(&server)
            .await;
    }
    server
}

/// Credencial de cada pedido recebido (Bearer ou `x-api-key`).
async fn credentials(server: &MockServer) -> Vec<String> {
    server
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

/// A armadilha: um "proxy" que so anota a primeira linha de cada conexao.
struct Trap {
    url: String,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Trap {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind da armadilha");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut first = String::new();
                if let Ok(clone) = stream.try_clone() {
                    let _ = BufReader::new(clone).read_line(&mut first);
                }
                if let Ok(mut guard) = sink.lock() {
                    guard.push(first.trim().to_string());
                }
                let mut stream = stream;
                let _ = stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n");
            }
        });
        Self { url, lines }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Diretorio de config isolado com o `config.yml` dado.
fn config_dir(yaml: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("config.yml"), yaml).expect("config.yml");
    dir
}

/// Roda `garra ask --json <args>` isolado: config no tempdir, nenhuma
/// credencial herdada do ambiente de quem roda a suite, e todo trafego que
/// nao for loopback forcado pela armadilha.
async fn garra_ask(dir: &TempDir, trap: &Trap, args: &[&str]) -> std::process::Output {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_garra"));
    cmd.arg("ask")
        .arg("--json")
        .args(args)
        .current_dir(dir.path())
        .env("GARRAIA_CONFIG_DIR", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("HOME", dir.path())
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(std::process::Stdio::null());
    for var in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env(var, &trap.url);
    }
    for var in [
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "LLM_API_KEY",
        "GARRAIA_EMBEDDING_API_KEY",
        "GARRAIA_VAULT_PASSPHRASE",
        "GarraIA_VAULT_PASSPHRASE",
        "OLLAMA_BASE_URL",
    ] {
        cmd.env_remove(var);
    }
    tokio::time::timeout(Duration::from_secs(90), cmd.output())
        .await
        .expect("garra ask nao terminou em 90s")
        .expect("spawn garra")
}

fn describe(out: &std::process::Output) -> String {
    format!(
        "exit={:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Tabela: cada provider explicito com `base_url` configurada. Ao lado de
/// cada entrada fica uma isca do mesmo tipo, com outra chave e outro
/// endpoint: nem a chave nem um pedido podem chegar nela.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_provider_reaches_the_configured_base_url_and_nothing_else() {
    // (nome no -p, tipo, chave, sufixo da base_url)
    let rows: [(&str, &str, Option<&str>, &str); 5] = [
        ("openai", "openai", Some("k-openai"), "/v1"),
        ("openrouter", "openrouter", Some("k-openrouter"), "/api/v1"),
        ("anthropic", "anthropic", Some("k-anthropic"), ""),
        ("lmstudio", "openai", Some("k-lmstudio"), "/v1"),
        ("ollama", "ollama", None, ""),
    ];
    for (name, kind, key, suffix) in rows {
        let target = endpoint().await;
        let decoy = endpoint().await;
        let trap = Trap::start();
        let key_line = key
            .map(|k| format!("    api_key: {k}\n"))
            .unwrap_or_default();
        let yaml = format!(
            "llm:\n  {name}:\n    provider: {kind}\n    model: m\n{key_line}    base_url: {target}{suffix}\n  isca:\n    provider: {kind}\n    model: m\n    api_key: chave-isca\n    base_url: {decoy}{suffix}\n",
            target = target.uri(),
            decoy = decoy.uri(),
        );
        let dir = config_dir(&yaml);
        let out = garra_ask(&dir, &trap, &["-p", name, "oi"]).await;

        let seen = credentials(&target).await;
        assert!(
            !seen.is_empty(),
            "-p {name}: o endpoint configurado nao recebeu nada\n{}",
            describe(&out)
        );
        if let Some(k) = key {
            assert!(
                seen.iter().all(|c| c == k),
                "-p {name}: credenciais {seen:?}"
            );
        }
        assert!(
            decoy
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty(),
            "-p {name}: a isca recebeu pedido"
        );
        assert_eq!(
            trap.lines(),
            Vec::<String>::new(),
            "-p {name}: trafego saiu para fora do loopback\n{}",
            describe(&out)
        );
        assert_eq!(out.status.code(), Some(0), "-p {name}\n{}", describe(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(SENTINEL),
            "-p {name}: resposta nao veio do endpoint\n{}",
            describe(&out)
        );
        assert!(
            !stdout.contains("chave-isca")
                && !String::from_utf8_lossy(&out.stderr).contains("chave-isca")
        );
    }
}

/// Achado do verificador (MEDIUM), no binario: a `OPENAI_API_KEY` que o
/// `dotenvy` carrega do `.env` do diretorio corrente nunca vai para a
/// `base_url` propria de uma entrada sem `api_key` — nem por `-p openai`,
/// nem pelo `agent.default_provider`. O endpoint ve o marcador de "sem
/// chave". E sem `base_url` a mesma variavel continua indo para o host
/// padrao (a armadilha ve o `CONNECT api.openai.com:443`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dotenv_openai_key_never_reaches_an_entry_own_base_url() {
    const DO_ENV: &str = "sk-do-dotenv-nao-pode-sair";
    for args in [&["-p", "openai", "oi"][..], &["oi"][..]] {
        let target = endpoint().await;
        let trap = Trap::start();
        let yaml = format!(
            "agent:\n  default_provider: openai\nllm:\n  openai:\n    provider: openai\n    model: m\n    base_url: {}/v1\n",
            target.uri()
        );
        let dir = config_dir(&yaml);
        std::fs::write(
            dir.path().join(".env"),
            format!("OPENAI_API_KEY={DO_ENV}\n"),
        )
        .expect(".env");
        let out = garra_ask(&dir, &trap, args).await;
        let seen = credentials(&target).await;
        assert!(
            !seen.is_empty() && seen.iter().all(|c| c == "not-needed"),
            "{args:?}: o endpoint viu {seen:?}\n{}",
            describe(&out)
        );
        assert_eq!(
            trap.lines(),
            Vec::<String>::new(),
            "{args:?}\n{}",
            describe(&out)
        );
        assert_eq!(out.status.code(), Some(0), "{args:?}\n{}", describe(&out));
    }

    // Sem `base_url`: host padrao, com a variavel — comportamento mantido.
    let trap = Trap::start();
    let dir = config_dir("llm:\n  openai:\n    provider: openai\n    model: m\n");
    std::fs::write(
        dir.path().join(".env"),
        format!("OPENAI_API_KEY={DO_ENV}\n"),
    )
    .expect(".env");
    let out = garra_ask(&dir, &trap, &["-p", "openai", "oi"]).await;
    assert!(
        trap.lines()
            .iter()
            .any(|l| l.starts_with("CONNECT api.openai.com:443")),
        "sem base_url o destino e o host padrao: {:?}\n{}",
        trap.lines(),
        describe(&out)
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains(DO_ENV)
            && !String::from_utf8_lossy(&out.stderr).contains(DO_ENV),
        "a chave nunca aparece na saida\n{}",
        describe(&out)
    );
}

/// Negativo pelo caminho do `agent.default_provider` (sem `-p`): com
/// `default_provider: lmstudio` e um `llm.openai` ao lado, a chave do
/// `llm.openai` nunca vai para o endpoint do LM Studio — e nada vai ao
/// endpoint do `llm.openai`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_provider_never_borrows_the_key_of_another_entry() {
    let a = endpoint().await;
    let b = endpoint().await;
    let trap = Trap::start();
    let yaml = format!(
        "agent:\n  default_provider: lmstudio\nllm:\n  openai:\n    provider: openai\n    model: m\n    api_key: chave-a\n    base_url: {a}/v1\n  lmstudio:\n    provider: openai\n    model: m\n    api_key: chave-b\n    base_url: {b}/v1\n",
        a = a.uri(),
        b = b.uri(),
    );
    let dir = config_dir(&yaml);
    let out = garra_ask(&dir, &trap, &["oi"]).await;
    assert!(
        a.received_requests().await.unwrap_or_default().is_empty(),
        "A recebeu pedido\n{}",
        describe(&out)
    );
    let seen = credentials(&b).await;
    assert!(
        !seen.is_empty() && seen.iter().all(|c| c == "chave-b"),
        "B viu {seen:?}\n{}",
        describe(&out)
    );
    assert_eq!(trap.lines(), Vec::<String>::new(), "{}", describe(&out));
    assert_eq!(out.status.code(), Some(0), "{}", describe(&out));
}
