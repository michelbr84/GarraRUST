//! #1708, criterio 4, pelo binario: `garra chat` com uma ferramenta de
//! verdade no meio da conversa.
//!
//! - **Sem `--persist`**: a trilha de ferramentas vive so na memoria. Nada
//!   do que foi dito ou feito no turno chega ao disco — nenhum
//!   `sessions.db`, nenhum `history`, e nenhum arquivo do diretorio da
//!   pessoa contendo a pergunta ou os argumentos da ferramenta.
//! - **Com `--persist`**: o mesmo turno vai para o `sessions.db`, com a
//!   trilha da ferramenta junto.
//!
//! Nos dois casos o turno SEGUINTE manda ao provedor a chamada e o
//! resultado pareados pelo id (`tool_calls[].id` == `tool_call_id`), no
//! formato do protocolo OpenAI — o segundo provedor do criterio 6, aqui
//! pelo fio e nao so pela serializacao em `openai.rs`.
//!
//! O provedor e um endpoint local roteirizado (wiremock); nenhum trafego
//! sai do loopback, e nenhuma credencial do ambiente de quem roda a suite
//! entra no processo.

use std::path::Path;
use std::time::Duration;

use tempfile::TempDir;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Marcador da pergunta: aparece no disco se, e so se, o turno foi gravado.
const PERGUNTA: &str = "CRIE_AGORA pergunta-sentinela-1708";
/// Marcador do conteudo que a ferramenta escreve (vai nos argumentos).
const CONTEUDO: &str = "conteudo-sentinela-1708";
const SEGUNDA: &str = "voce gravou o arquivo?";
const ID_DA_CHAMADA: &str = "call_sentinela_1708";

/// Roteiro: pergunta com `CRIE_AGORA` e sem resultado de ferramenta depois
/// dela -> pede `file_write`; qualquer outra coisa -> texto. Responde em SSE
/// quando o pedido e de streaming, e em JSON quando nao e.
struct Roteiro;

fn pede_ferramenta(corpo: &serde_json::Value) -> bool {
    let msgs = corpo["messages"].as_array().cloned().unwrap_or_default();
    match msgs.last() {
        Some(m) => m["role"] == "user" && m["content"].to_string().contains("CRIE_AGORA"),
        None => false,
    }
}

impl Respond for Roteiro {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if !request.url.path().ends_with("/chat/completions") {
            return ResponseTemplate::new(404);
        }
        let corpo: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let stream = corpo["stream"].as_bool().unwrap_or(false);
        let args = serde_json::json!({ "path": "pagina.html", "content": CONTEUDO }).to_string();
        let ferramenta = pede_ferramenta(&corpo);

        if !stream {
            let message = if ferramenta {
                serde_json::json!({
                    "role": "assistant", "content": null,
                    "tool_calls": [{"id": ID_DA_CHAMADA, "type": "function",
                        "function": {"name": "file_write", "arguments": args}}]
                })
            } else {
                serde_json::json!({"role": "assistant", "content": "pronto"})
            };
            let body = serde_json::json!({
                "id": "c", "object": "chat.completion", "model": "m",
                "choices": [{"index": 0, "message": message,
                    "finish_reason": if ferramenta { "tool_calls" } else { "stop" }}]
            });
            return ResponseTemplate::new(200).set_body_json(body);
        }

        let chunks = if ferramenta {
            vec![
                serde_json::json!({"id": "c", "object": "chat.completion.chunk", "model": "m",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{
                        "index": 0, "id": ID_DA_CHAMADA, "type": "function",
                        "function": {"name": "file_write", "arguments": args}}]},
                        "finish_reason": null}]}),
                serde_json::json!({"id": "c", "object": "chat.completion.chunk", "model": "m",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
            ]
        } else {
            vec![
                serde_json::json!({"id": "c", "object": "chat.completion.chunk", "model": "m",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": "pronto"},
                        "finish_reason": null}]}),
                serde_json::json!({"id": "c", "object": "chat.completion.chunk", "model": "m",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
            ]
        };
        let sse: String = chunks
            .iter()
            .map(|c| format!("data: {c}\n\n"))
            .chain(std::iter::once("data: [DONE]\n\n".to_string()))
            .collect();
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse)
    }
}

async fn provedor() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Roteiro)
        .mount(&server)
        .await;
    server
}

/// `casa` e o diretorio da pessoa (config, dados, historico); `projeto` e o
/// diretorio corrente, onde a ferramenta escreve. Separados para que o
/// arquivo escrito de proposito nao conte como gravacao involuntaria.
struct Ambiente {
    casa: TempDir,
    projeto: TempDir,
}

fn ambiente(server: &MockServer) -> Ambiente {
    let casa = tempfile::tempdir().expect("tempdir casa");
    let projeto = tempfile::tempdir().expect("tempdir projeto");
    let yaml = format!(
        "llm:\n  openai:\n    provider: openai\n    model: m\n    api_key: k-teste\n    base_url: {}/v1\n",
        server.uri()
    );
    std::fs::write(casa.path().join("config.yml"), yaml).expect("config.yml");
    Ambiente { casa, projeto }
}

async fn garra_chat(amb: &Ambiente, extra: &[&str]) -> std::process::Output {
    use tokio::io::AsyncWriteExt as _;

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_garra"));
    cmd.arg("chat")
        .args(["-p", "openai"])
        .args(extra)
        .current_dir(amb.projeto.path())
        .env("GARRAIA_CONFIG_DIR", amb.casa.path())
        .env("XDG_CONFIG_HOME", amb.casa.path())
        .env("XDG_DATA_HOME", amb.casa.path())
        .env("HOME", amb.casa.path())
        .env("NO_COLOR", "1")
        .env("GARRAIA_NO_SPINNER", "1")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for var in [
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "LLM_API_KEY",
        "GARRAIA_EMBEDDING_API_KEY",
        "GARRAIA_VAULT_PASSPHRASE",
        "GarraIA_VAULT_PASSPHRASE",
        "OLLAMA_BASE_URL",
        "GARRAIA_TOOL_HISTORY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env_remove(var);
    }
    let mut filho = cmd.spawn().expect("spawn garra chat");
    let mut stdin = filho.stdin.take().expect("stdin");
    stdin
        .write_all(format!("{PERGUNTA}\n{SEGUNDA}\n").as_bytes())
        .await
        .expect("escreve no stdin");
    // EOF encerra o REPL depois do segundo turno.
    drop(stdin);
    tokio::time::timeout(Duration::from_secs(90), filho.wait_with_output())
        .await
        .expect("garra chat nao terminou em 90s")
        .expect("saida do garra chat")
}

fn descreve(out: &std::process::Output) -> String {
    format!(
        "exit={:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Todo arquivo abaixo de `raiz`, recursivo, sem seguir symlink.
fn arquivos(raiz: &Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    let mut pilha = vec![raiz.to_path_buf()];
    while let Some(dir) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entradas.flatten() {
            let Ok(tipo) = e.file_type() else { continue };
            if tipo.is_dir() {
                pilha.push(e.path());
            } else if tipo.is_file() {
                v.push(e.path());
            }
        }
    }
    v
}

/// Arquivos abaixo de `raiz` cujo conteudo bruto contem `agulha`.
fn quem_contem(raiz: &Path, agulha: &str) -> Vec<std::path::PathBuf> {
    arquivos(raiz)
        .into_iter()
        .filter(|p| {
            std::fs::read(p)
                .map(|b| b.windows(agulha.len()).any(|w| w == agulha.as_bytes()))
                .unwrap_or(false)
        })
        .collect()
}

/// Corpos JSON dos pedidos de chat que o provedor recebeu, em ordem.
async fn pedidos(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path().ends_with("/chat/completions"))
        .filter_map(|r| serde_json::from_slice(&r.body).ok())
        .collect()
}

/// O pedido do segundo turno (o que carrega a pergunta `SEGUNDA` como
/// ultima mensagem) leva a chamada e o resultado do turno anterior,
/// pareados pelo id do executor.
fn assert_turno_seguinte_pareado(pedidos: &[serde_json::Value], out: &std::process::Output) {
    let segundo = pedidos
        .iter()
        .find(|p| {
            p["messages"]
                .as_array()
                .and_then(|m| m.last())
                .is_some_and(|m| m["role"] == "user" && m["content"].to_string().contains(SEGUNDA))
        })
        .unwrap_or_else(|| panic!("o segundo turno nao chegou ao provedor\n{}", descreve(out)));
    let msgs = segundo["messages"].as_array().expect("messages");

    let chamada = msgs
        .iter()
        .position(|m| {
            m["role"] == "assistant"
                && m["tool_calls"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c["function"]["name"] == "file_write"))
        })
        .unwrap_or_else(|| panic!("sem tool_calls de file_write no turno seguinte: {msgs:#?}"));
    let id = msgs[chamada]["tool_calls"][0]["id"]
        .as_str()
        .expect("id da chamada")
        .to_string();
    let resultado = msgs
        .iter()
        .position(|m| m["role"] == "tool" && m["tool_call_id"] == id.as_str())
        .unwrap_or_else(|| panic!("sem resultado pareado ao id {id}: {msgs:#?}"));
    assert!(
        chamada < resultado,
        "a chamada vem antes do resultado: {msgs:#?}"
    );
    let pergunta_nova = msgs.len() - 1;
    assert!(
        resultado < pergunta_nova,
        "a evidencia vem antes da pergunta nova: {msgs:#?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sem_persist_a_trilha_nao_vai_a_disco_mas_chega_ao_turno_seguinte() {
    let server = provedor().await;
    let amb = ambiente(&server);
    let antes = arquivos(amb.casa.path());

    let out = garra_chat(&amb, &[]).await;
    assert_eq!(out.status.code(), Some(0), "{}", descreve(&out));

    // A ferramenta rodou de verdade, no projeto.
    let escrito = std::fs::read_to_string(amb.projeto.path().join("pagina.html"))
        .unwrap_or_else(|e| panic!("file_write nao escreveu ({e})\n{}", descreve(&out)));
    assert_eq!(escrito, CONTEUDO);

    assert_turno_seguinte_pareado(&pedidos(&server).await, &out);

    // Nada do turno chegou ao diretorio da pessoa.
    for agulha in [PERGUNTA, SEGUNDA, CONTEUDO, ID_DA_CHAMADA] {
        assert_eq!(
            quem_contem(amb.casa.path(), agulha),
            Vec::<std::path::PathBuf>::new(),
            "sem --persist, {agulha:?} nao pode estar em disco\n{}",
            descreve(&out)
        );
    }
    let novos: Vec<_> = arquivos(amb.casa.path())
        .into_iter()
        .filter(|p| !antes.contains(p))
        .collect();
    for nome in ["sessions.db", "history"] {
        assert!(
            novos.iter().all(|p| p.file_name() != Some(nome.as_ref())),
            "sem --persist nao se cria {nome}: {novos:?}"
        );
    }
}

/// O outro lado do criterio: com `--persist` o mesmo turno, trilha
/// incluida, vai para o `sessions.db`. Sem este lado, o teste acima passaria
/// mesmo que a busca por marcadores nunca achasse nada.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn com_persist_a_trilha_vai_para_o_sessions_db() {
    let server = provedor().await;
    let amb = ambiente(&server);

    let out = garra_chat(&amb, &["--persist"]).await;
    assert_eq!(out.status.code(), Some(0), "{}", descreve(&out));
    assert_turno_seguinte_pareado(&pedidos(&server).await, &out);

    let com_pergunta = quem_contem(amb.casa.path(), PERGUNTA);
    assert!(
        com_pergunta.iter().any(|p| p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("sessions.db"))),
        "com --persist a pergunta vai para o sessions.db: {com_pergunta:?}\n{}",
        descreve(&out)
    );
    let com_trilha = quem_contem(amb.casa.path(), "file_write");
    assert!(
        com_trilha.iter().any(|p| p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("sessions.db"))),
        "com --persist a trilha da ferramenta vai junto: {com_trilha:?}\n{}",
        descreve(&out)
    );
}
