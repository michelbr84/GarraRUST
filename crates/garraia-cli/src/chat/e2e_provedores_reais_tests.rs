//! #1708 — E2E REAL pela fio: o payload que o provedor recebe no turno
//! SEGUINTE carrega o `tool_use`/`tool_result` do turno anterior, em DOIS
//! provedores de verdade (Anthropic e um segundo, o OpenAI compatível).
//!
//! Os testes de `historico_de_ferramentas_tests` provam o caminho do REPL com
//! um provedor de mentira (memoria). Aqui a diferenca e o **fio**: os providers
//! REAIS `AnthropicProvider` e `OpenAiProvider` serializam o historico para o
//! formato de cada protocolo e mandam por HTTP a um servidor local que grava o
//! corpo JSON bruto de cada pedido. O que se afirma e o que sai no wire, nao o
//! que um modelo diria.
//!
//! Cobre o criterio de aceite da issue que faltava na prova de memoria:
//! **"Testes com pelo menos Anthropic e um segundo provedor, comprovando o
//! pareamento correto das mensagens de ferramentas"**, mais o **caso de
//! denegação**: uma operacao registrada no turno 1 (a ferramenta rodou de
//! verdade) aparece no payload do turno 2 de forma que o modelo nao tem como
//! nega-la.
//!
//! O turno e montado com as mesmas funcoes do REPL (`stream_turn` drena os
//! `TurnEvent` e alimenta a `ToolTrail`; `anexar_turno` fecha o turno no
//! `history`). Tudo em diretorio temporario, sem rede externa e sem credencial
//! real.

use super::*;
use garraia_agents::{AnthropicProvider, Tool, ToolContext, ToolOutput};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const CONTEUDO: &str = "<h1>ola</h1>";
const ID_CHAMADA: &str = "call-e2e-1708";

/// Escreve de verdade num diretorio temporario; o nome e o da tool real para o
/// historico ficar igual ao de producao.
struct Escreve(std::path::PathBuf);

#[async_trait::async_trait]
impl Tool for Escreve {
    fn name(&self) -> &str {
        "file_write"
    }
    fn description(&self) -> &str {
        "escreve um arquivo"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, i: Value) -> garraia_common::Result<ToolOutput> {
        let nome = i["path"].as_str().unwrap_or_default();
        let conteudo = i["content"].as_str().unwrap_or_default();
        match std::fs::write(self.0.join(nome), conteudo) {
            Ok(()) => Ok(ToolOutput::success(format!("escreveu {nome}: {conteudo}"))),
            Err(e) => Ok(ToolOutput::error(format!("falha ao escrever {nome}: {e}"))),
        }
    }
}

// ── O servidor de fio: grava cada corpo JSON e responde por protocolo ─────

/// Decide, pelo corpo que chegou, se o turno pede a ferramenta (`file_write`)
/// ou se e a pergunta do segundo turno. Roteiro identico nos dois protocolos.
fn pede_ferramenta(corpo: &Value) -> bool {
    let msgs = corpo["messages"].as_array().cloned().unwrap_or_default();
    match msgs.last() {
        Some(m) => {
            let papel = m["role"].as_str().unwrap_or_default();
            let texto = m["content"].to_string();
            papel == "user" && texto.contains("CRIE_AGORA")
        }
        None => false,
    }
}

/// Responde no formato do provedor que o corpo indica, sempre em **SSE** —
/// o runtime chama `stream_complete`. Anthropic usa `/v1/messages` (blocos
/// `tool_use`); OpenAI usa `/chat/completions` (`tool_calls`). Nos dois, o
/// segundo turno devolve texto.
struct Roteiro;

impl Respond for Roteiro {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let corpo: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let caminho = request.url.path();
        let pede = pede_ferramenta(&corpo);
        let args = json!({"path": "pagina.html", "content": CONTEUDO});

        let sse = if caminho.ends_with("/messages") {
            // Anthropic: SSE de blocos. tool_use quando pede ferramenta.
            let eventos: Vec<Value> = if pede {
                vec![
                    json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"m","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":ID_CHAMADA,"name":"file_write","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args.to_string()}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":1,"output_tokens":1}}),
                    json!({"type":"message_stop"}),
                ]
            } else {
                vec![
                    json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"m","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"pronto"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":1,"output_tokens":1}}),
                    json!({"type":"message_stop"}),
                ]
            };
            eventos
                .iter()
                .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("")))
                .collect::<String>()
        } else if caminho.ends_with("/chat/completions") {
            // OpenAI: chunks SSE. tool_calls quando pede ferramenta.
            let chunks: Vec<Value> = if pede {
                vec![
                    json!({"id":"c","object":"chat.completion.chunk","model":"m","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":ID_CHAMADA,"type":"function","function":{"name":"file_write","arguments":args.to_string()}}]},"finish_reason":null}]}),
                    json!({"id":"c","object":"chat.completion.chunk","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                ]
            } else {
                vec![
                    json!({"id":"c","object":"chat.completion.chunk","model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"pronto"},"finish_reason":null}]}),
                    json!({"id":"c","object":"chat.completion.chunk","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                ]
            };
            chunks
                .iter()
                .map(|c| format!("data: {c}\n\n"))
                .chain(std::iter::once("data: [DONE]\n\n".to_string()))
                .collect()
        } else {
            return ResponseTemplate::new(404);
        };

        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse)
    }
}

async fn servidor() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Roteiro)
        .mount(&s)
        .await;
    s
}

/// Corpos JSON que o servidor recebeu, em ordem.
async fn corpos(s: &MockServer) -> Vec<Value> {
    s.received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|r| serde_json::from_slice::<Value>(&r.body).ok())
        .collect()
}

/// O corpo do pedido do segundo turno (o que termina com `SEGUNDA`).
fn segundo_turno(corpos: &[Value]) -> Value {
    corpos
        .iter()
        .find(|c| {
            c["messages"]
                .as_array()
                .and_then(|m| m.last())
                .is_some_and(|m| m["content"].to_string().contains(SEGUNDA))
        })
        .cloned()
        .unwrap_or_else(|| panic!("o segundo turno nao chegou ao provedor: {corpos:#?}"))
}

const SEGUNDA: &str = "voce criou o arquivo?";

// ── Sessao: runtime real + trilha + historico, pelo caminho do REPL ───────

struct Sessao {
    rt: AgentRuntime,
    trilha: ToolTrail,
    historico: Vec<ChatMessage>,
    dir: tempfile::TempDir,
}

impl Sessao {
    fn nova(provedor: Arc<dyn LlmProvider>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let rt = AgentRuntime::new();
        rt.register_tool(Box::new(Escreve(dir.path().to_path_buf())));
        rt.register_provider(provedor);
        Sessao {
            rt,
            trilha: ToolTrail::default(),
            historico: Vec::new(),
            dir,
        }
    }

    /// Um turno pelo mesmo caminho do REPL: o `process_message_streaming_with_events`
    /// emite os `TurnEvent`, o `stream_turn` drena e alimenta a `ToolTrail`, e o
    /// `anexar_turno` fecha o turno no `history` (modo estruturado).
    async fn turno(&mut self, texto: &str, provedor_id: &str) {
        self.trilha.iniciar_turno();
        let (tx, rx) = mpsc::channel::<TurnEvent>(64);
        let exec = exec_do_turno(&self.dir.path().to_string_lossy(), "sessao-1708-e2e", None);
        let historico = self.historico.clone();
        let call = self.rt.process_message_streaming_with_events(
            "sessao-1708-e2e",
            texto,
            &historico,
            tx,
            None,
            None,
            Some(provedor_id),
            None,
            None,
            None,
            &exec,
        );
        let resposta = {
            // Drena os eventos enquanto o turno roda — o mesmo papel do
            // `stream_turn` do REPL; quem alimenta a trilha e o dreno. O bloco
            // encerra o `dreno` (e solta o `&mut self.trilha`) antes do
            // `fim_do_turno` abaixo.
            let dreno = {
                let trilha = &mut self.trilha;
                let mut rx = rx;
                async move {
                    while let Some(ev) = rx.recv().await {
                        trilha.observar(&ev);
                    }
                }
            };
            tokio::pin!(dreno);
            tokio::select! {
                r = call => r.expect("turno nao pode falhar"),
                _ = &mut dreno => unreachable!("o dreno nunca termina antes do turno"),
            }
        };
        let registros = self.trilha.fim_do_turno();
        anexar_turno(
            &mut self.historico,
            texto,
            &resposta,
            &registros,
            Modo::Structured,
        );
    }
}

// ── Assercoes de fio ─────────────────────────────────────────────────────

/// No formato ANTHROPIC: o `tool_use` do turno 1 e o `tool_result` pareado
/// pelo mesmo id, nesta ordem, antes da pergunta nova.
fn assert_wire_anthropic(corpo: &Value) {
    let msgs = corpo["messages"].as_array().expect("messages");
    let mut idx_uso = None;
    let mut idx_res = None;
    for (i, m) in msgs.iter().enumerate() {
        if m["role"] == "assistant"
            && let Some(blocos) = m["content"].as_array()
            && blocos
                .iter()
                .any(|b| b["type"] == "tool_use" && b["id"] == ID_CHAMADA)
        {
            idx_uso = Some(i);
        }
        if m["role"] == "user"
            && let Some(blocos) = m["content"].as_array()
            && blocos
                .iter()
                .any(|b| b["type"] == "tool_result" && b["tool_use_id"] == ID_CHAMADA)
        {
            idx_res = Some(i);
        }
    }
    let iu = idx_uso.expect("sem tool_use do turno 1 no wire anthropic");
    let ir = idx_res.expect("sem tool_result pareado no wire anthropic");
    assert!(iu < ir, "tool_use antes do tool_result: {msgs:#?}");
    let pergunta = msgs.len() - 1;
    assert!(
        ir < pergunta,
        "a evidencia vem antes da pergunta nova: {msgs:#?}"
    );
}

/// No formato OPENAI: `tool_calls[].id` no assistant e `role=tool` com
/// `tool_call_id` igual, nesta ordem, antes da pergunta nova.
fn assert_wire_openai(corpo: &Value) {
    let msgs = corpo["messages"].as_array().expect("messages");
    let mut idx_chamada = None;
    let mut idx_resultado = None;
    for (i, m) in msgs.iter().enumerate() {
        if m["role"] == "assistant"
            && let Some(calls) = m["tool_calls"].as_array()
            && calls.iter().any(|c| c["id"] == ID_CHAMADA)
        {
            idx_chamada = Some(i);
        }
        if m["role"] == "tool" && m["tool_call_id"] == ID_CHAMADA {
            idx_resultado = Some(i);
        }
    }
    let ic = idx_chamada.expect("sem tool_calls do turno 1 no wire openai");
    let ir = idx_resultado.expect("sem role=tool pareado no wire openai");
    assert!(ic < ir, "tool_calls antes do role=tool: {msgs:#?}");
    let pergunta = msgs.len() - 1;
    assert!(
        ir < pergunta,
        "a evidencia vem antes da pergunta nova: {msgs:#?}"
    );
}

// ── Os testes E2E ─────────────────────────────────────────────────────────

/// Criterio de aceite: **Anthropic e um segundo provedor**. Turno 1 escreve o
/// arquivo; o payload do turno 2, no formato de cada protocolo, carrega o par
/// `tool_use`/`tool_result` pareado pelo id do executor.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_dois_provedores_mandam_o_par_no_turno_seguinte() {
    // Anthropic real.
    let s = servidor().await;
    let anthropic = Arc::new(
        AnthropicProvider::new("chave-anth", Some("modelo-anth".to_string()), Some(s.uri()))
            .with_name("anthropic"),
    );
    let mut sa = Sessao::nova(anthropic as Arc<dyn LlmProvider>);
    sa.turno("CRIE_AGORA", "anthropic").await;
    assert!(
        sa.dir.path().join("pagina.html").exists(),
        "a ferramenta rodou de verdade"
    );
    sa.turno(SEGUNDA, "anthropic").await;
    assert_wire_anthropic(&segundo_turno(&corpos(&s).await));

    // OpenAI compativel real (o segundo provedor).
    let s2 = servidor().await;
    let openai = Arc::new(
        OpenAiProvider::new(
            "chave-oai",
            Some("modelo-oai".to_string()),
            Some(format!("{}/v1", s2.uri())),
        )
        .with_name("openai"),
    );
    let mut so = Sessao::nova(openai as Arc<dyn LlmProvider>);
    so.turno("CRIE_AGORA", "openai").await;
    assert!(
        so.dir.path().join("pagina.html").exists(),
        "a ferramenta rodou de verdade"
    );
    so.turno(SEGUNDA, "openai").await;
    assert_wire_openai(&segundo_turno(&corpos(&s2).await));
}

/// Caso de denegação: uma operacao REGISTRADA (a ferramenta rodou) nao pode
/// sumir do payload do turno 2 — o modelo so pode negar se a evidencia some.
/// Aqui o payload do turno 2 traz o `tool_result` com o conteudo real escrito,
/// entao a afirmacao "eu escrevi pagina.html" e sustentada pelo fio.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_denegacao_operacao_registrada_aparece_no_payload() {
    let s = servidor().await;
    let anthropic = Arc::new(
        AnthropicProvider::new("chave-anth", Some("modelo-anth".to_string()), Some(s.uri()))
            .with_name("anthropic"),
    );
    let mut sa = Sessao::nova(anthropic as Arc<dyn LlmProvider>);
    sa.turno("CRIE_AGORA", "anthropic").await;
    sa.turno(SEGUNDA, "anthropic").await;

    let corpo = segundo_turno(&corpos(&s).await);
    let msgs = corpo["messages"].as_array().expect("messages");

    // O resultado REAL da escrita esta no payload: o modelo nao tem como negar
    // uma operacao cujo resultado esta na propria mensagem.
    let tem_resultado_real = msgs.iter().any(|m| {
        m["role"] == "user"
            && m["content"].as_array().is_some_and(|blocos| {
                blocos.iter().any(|b| {
                    b["type"] == "tool_result"
                        && b["tool_use_id"] == ID_CHAMADA
                        && b["content"].to_string().contains(CONTEUDO)
                })
            })
    });
    assert!(
        tem_resultado_real,
        "a operacao registrada tem de aparecer com o resultado real no payload: {msgs:#?}"
    );
}
