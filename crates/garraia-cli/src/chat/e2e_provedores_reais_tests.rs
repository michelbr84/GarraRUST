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
    rt: Arc<AgentRuntime>,
    trilha: ToolTrail,
    historico: Vec<ChatMessage>,
    dir: tempfile::TempDir,
    /// #1712 — o mesmo `Arc` que o REPL compartilha com o `/model`: trocar o
    /// modelo selecionado muda o que o `cli_inspect` projeta em `model_requested`.
    modelo_pedido: Option<Arc<std::sync::RwLock<String>>>,
}

impl Sessao {
    fn nova(provedor: Arc<dyn LlmProvider>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let rt = Arc::new(AgentRuntime::new());
        rt.register_tool(Box::new(Escreve(dir.path().to_path_buf())));
        rt.register_provider(provedor);
        Sessao {
            rt,
            trilha: ToolTrail::default(),
            historico: Vec::new(),
            dir,
            modelo_pedido: None,
        }
    }

    /// #1712 fatia 7 — sessão com a tool REAL de inspeção registrada do mesmo
    /// jeito que o REPL faz: `Arc::downgrade` do runtime, nome do provider
    /// selecionado, modelo pedido, estado DECLARADO (gateway/MCP) coletado da
    /// configuração e o snapshot de efeitos do executor.
    fn com_inspect(
        provedor: Arc<dyn LlmProvider>,
        provedor_nome: &str,
        modelo: &str,
        config: &garraia_config::AppConfig,
        dir_config: &std::path::Path,
    ) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let rt = Arc::new(AgentRuntime::new());
        let modelo_pedido = Arc::new(std::sync::RwLock::new(modelo.to_string()));
        rt.register_provider(provedor);
        rt.register_tool(Box::new(crate::cli_inspect_tool::CliInspectTool::new(
            Arc::downgrade(&rt),
            provedor_nome.to_string(),
            Arc::clone(&modelo_pedido),
            crate::estado_declarado::coletar(
                config,
                &garraia_config::ConfigLoader::with_dir(dir_config),
            ),
            Arc::new(std::sync::Mutex::new(
                crate::efeitos_de_ferramenta::SnapshotDeEfeitos::default(),
            )),
        )));
        Sessao {
            rt,
            trilha: ToolTrail::default(),
            historico: Vec::new(),
            dir,
            modelo_pedido: Some(modelo_pedido),
        }
    }

    /// O mesmo gesto do `/model`: a seleção corrente da CLI muda. O runtime e o
    /// provider não são tocados — é o que torna o "pedido × confirmado"
    /// observável no relatório do turno seguinte.
    fn trocar_modelo(&self, novo: &str) {
        let handle = self.modelo_pedido.as_ref().expect("sessao com com_inspect");
        *handle.write().expect("lock do modelo pedido") = novo.to_string();
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
            // `stream_turn` do REPL, com a mesma drenagem pos-`drop` (ver
            // `turno_com_trilha`): o bloco solta o `&mut self.trilha` antes do
            // `fim_do_turno` abaixo.
            let trilha = &mut self.trilha;
            turno_com_trilha(call, rx, trilha)
                .await
                .expect("turno nao pode falhar")
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

// ── Drenagem do turno: o mesmo invariante do `stream_turn` do REPL ────────
//
// Um `select!` que escolhe apenas o `call` e **dropa o dreno** perde os eventos
// que ainda estavam no buffer do canal quando o turno terminou. E exatamente o
// que acontece quando a tool que falha encerra o turno sem nova chamada ao
// modelo: `ToolStarted`/`ToolFinished` sao enviados e o `call` fica pronto no
// mesmo poll, sem nenhum `await` Pendente no meio. Foi o flake que deixou
// `Test (ubuntu-latest)` vermelho na main (`registros` vazio → "index out of
// bounds: the len is 0"), reprovado 1 em 30 sob carga local.
//
// A producao nao tem essa janela: `stream_turn` (chat.rs) dropa o future
// **depois** do `select!` — o sender dono dele morre junto, o canal fecha — e
// so entao drena o restante ate `None`. Este helper e a copia desse gesto para
// os testes que afirmam sobre a trilha.
async fn turno_com_trilha<F, T, E>(
    call: F,
    mut rx: mpsc::Receiver<TurnEvent>,
    trilha: &mut ToolTrail,
) -> std::result::Result<T, E>
where
    F: std::future::Future<Output = std::result::Result<T, E>>,
{
    // `Box::pin`: o future dono do sender precisa poder ser dropado ANTES da
    // drenagem final (mesmo comentario do `stream_turn`).
    let mut call = Box::pin(call);
    let mut rx_aberto = true;
    let resultado = loop {
        tokio::select! {
            r = &mut call => break r,
            talvez = rx.recv(), if rx_aberto => match talvez {
                Some(evento) => trilha.observar(&evento),
                None => rx_aberto = false,
            },
        }
    };
    // Aqui o sender morre com o future: o canal fecha e o laco abaixo consome
    // o que ficou no buffer sem esperar produtor nenhum.
    drop(call);
    if rx_aberto {
        while let Some(evento) = rx.recv().await {
            trilha.observar(&evento);
        }
    }
    resultado
}

/// Regressao do flake que deixou `Test (ubuntu-latest)` vermelho na main
/// (2026-11-15, `0b6fbb39`): um turno que termina no MESMO poll em que emite o
/// par `ToolStarted`/`ToolFinished` — tool que falha e encerra o turno, sem
/// nova chamada ao modelo — precisa ter os dois eventos na trilha.
///
/// O `select!` antigo dropava o dreno nesse instante: `fim_do_turno` voltava
/// vazio e o teste de divergencia estourava em `registros[0]`. Reprovava ~1 em
/// 30 sob carga; com a drenagem pos-`drop` (o gesto do `stream_turn`), o
/// resultado passa a ser deterministico.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_drenagem_guarda_os_eventos_que_terminam_no_mesmo_poll_do_turno() {
    let (tx, rx) = mpsc::channel::<TurnEvent>(8);
    let produtor = async move {
        // `try_send`: preenche o buffer SEM ceder o controle, entao o turno
        // fica pronto no mesmo poll em que os eventos foram emitidos.
        tx.try_send(TurnEvent::ToolStarted {
            id: "call-flake".to_string(),
            name: "file_write".to_string(),
            detail: String::new(),
            input: json!({}),
        })
        .expect("buffer do canal");
        tx.try_send(TurnEvent::ToolFinished {
            id: "call-flake".to_string(),
            name: "file_write".to_string(),
            duration: std::time::Duration::from_millis(1),
            success: false,
            summary: "permissao negada".to_string(),
            output: String::new(),
        })
        .expect("buffer do canal");
        Ok::<_, String>(())
    };

    let mut trilha = ToolTrail::default();
    trilha.iniciar_turno();
    turno_com_trilha(produtor, rx, &mut trilha)
        .await
        .expect("produtor");

    let registros = trilha.fim_do_turno();
    assert_eq!(
        registros.len(),
        1,
        "o par de eventos ficou no buffer do canal: {registros:?}"
    );
    assert_eq!(
        registros[0].status,
        tool_history::Status::Error,
        "o `ToolFinished` de falha tem de fechar o registro: {registros:?}"
    );
}

// ── #1709: E2E de divergência entre alegação e o ledger do runtime ────────

/// Uma ferramenta que SEMPRE falha: o executor registra `success=false`, que e
/// o oposto do que o modelo costuma alegar. Usada para provar que a divergencia
/// entre "eu escrevi" e o registro real e detectada pelo `detect_divergencia`.
struct FalhaSempre;

#[async_trait::async_trait]
impl Tool for FalhaSempre {
    fn name(&self) -> &str {
        "file_write"
    }
    fn description(&self) -> &str {
        "tenta escrever e falha"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: Value) -> garraia_common::Result<ToolOutput> {
        Ok(ToolOutput::error("permissao negada".to_string()))
    }
}

/// Criterio "E2E de divergencia": o executor roda uma ferramenta que FALHA e o
/// registro (`TurnEvent` -> `ToolTrail` -> `EvidenceRef`) guarda `success=false`.
/// Se o modelo, no turno seguinte, afirma que a operacao foi um sucesso, o
/// `detect_divergencia` tem de sinalizar `AfirmaSucessoMasLedgerFalhou` — a
/// correcao e reformular com o dado observavel, nunca aceitar a alegacao.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_divergencia_modelo_afirma_sucesso_mas_ledger_registrou_falha() {
    use garraia_agents::{Divergencia, detect_divergencia};

    // Sessao com uma ferramenta que falha; o provedor pede a ferramenta e
    // depois responde texto (mesmo Roteiro do restante do arquivo).
    let s = servidor().await;
    let anthropic = Arc::new(
        AnthropicProvider::new("chave-anth", Some("modelo-anth".to_string()), Some(s.uri()))
            .with_name("anthropic"),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(FalhaSempre));
    rt.register_provider(anthropic as Arc<dyn LlmProvider>);

    let mut trilha = ToolTrail::default();
    trilha.iniciar_turno();
    let (tx, rx) = mpsc::channel::<TurnEvent>(64);
    let exec = exec_do_turno(
        &dir.path().to_string_lossy(),
        "sessao-1709-divergencia",
        None,
    );
    let call = rt.process_message_streaming_with_events(
        "sessao-1709-divergencia",
        "CRIE_AGORA",
        &[],
        tx,
        None,
        None,
        Some("anthropic"),
        None,
        None,
        None,
        &exec,
    );
    // A trilha so vale se a drenagem cobrir os eventos do fim do turno — o
    // mesmo invariante do `stream_turn` do REPL (ver `turno_com_trilha`).
    turno_com_trilha(call, rx, &mut trilha)
        .await
        .expect("turno nao pode falhar");
    let registros = trilha.fim_do_turno();
    assert_eq!(registros.len(), 1, "uma chamada registrada: {registros:?}");
    assert_eq!(
        registros[0].status,
        tool_history::Status::Error,
        "o executor registrou a falha: {registros:?}"
    );

    // O LEDGER do runtime e o que a `ToolTrail` guardou. O `resumo_de_claims`
    // projeta cada registro em `Claim`; a `EvidenceRef` de uma falha carrega
    // `success=false`. Aqui montamos o ledger como a CLI o ve.
    let ledger: Vec<garraia_agents::EvidenceRef> = registros
        .iter()
        .map(|r| garraia_agents::EvidenceRef {
            tool_call_id: r.call_id.clone(),
            tool_name: r.name.clone(),
            success: r.status == tool_history::Status::Ok,
            turno: r.turno,
            ordem: r.ordem,
        })
        .collect();

    // Estruturamos a polaridade da resposta separadamente da evidência real.
    let alegacao = garraia_agents::AlegacaoOperacional::sucesso(
        registros[0].call_id.clone(),
        registros[0].name.clone(),
    );

    match detect_divergencia(&alegacao, &ledger) {
        Divergencia::AfirmaSucessoMasLedgerFalhou {
            tool_call_id,
            tool_name,
        } => {
            assert_eq!(tool_call_id, registros[0].call_id);
            assert_eq!(tool_name, registros[0].name);
        }
        outro => panic!("a divergencia nao foi detectada: {outro:?}"),
    }
}

/// O inverso do caso acima: a alegacao bate com o ledger (falha com falha) e o
/// `detect_divergencia` reconhece como `Sustentada` — o detector nao gera falso
/// positivo so porque ha registro.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_divergencia_sem_contradicao_e_sustentada() {
    use garraia_agents::{Divergencia, detect_divergencia};

    let s = servidor().await;
    let anthropic = Arc::new(
        AnthropicProvider::new("chave-anth", Some("modelo-anth".to_string()), Some(s.uri()))
            .with_name("anthropic"),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(FalhaSempre));
    rt.register_provider(anthropic as Arc<dyn LlmProvider>);

    let mut trilha = ToolTrail::default();
    trilha.iniciar_turno();
    let (tx, rx) = mpsc::channel::<TurnEvent>(64);
    let exec = exec_do_turno(
        &dir.path().to_string_lossy(),
        "sessao-1709-divergencia-2",
        None,
    );
    let call = rt.process_message_streaming_with_events(
        "sessao-1709-divergencia-2",
        "CRIE_AGORA",
        &[],
        tx,
        None,
        None,
        Some("anthropic"),
        None,
        None,
        None,
        &exec,
    );
    // Mesmo invariante do teste anterior: sem a drenagem pos-`drop` os eventos
    // do fim do turno podem ficar no buffer e a trilha volta vazia.
    turno_com_trilha(call, rx, &mut trilha)
        .await
        .expect("turno nao pode falhar");
    let registros = trilha.fim_do_turno();
    assert!(
        !registros.is_empty(),
        "o turno nao executou `file_write`: a trilha ficou vazia"
    );

    let ledger: Vec<garraia_agents::EvidenceRef> = registros
        .iter()
        .map(|r| garraia_agents::EvidenceRef {
            tool_call_id: r.call_id.clone(),
            tool_name: r.name.clone(),
            success: r.status == tool_history::Status::Ok,
            turno: r.turno,
            ordem: r.ordem,
        })
        .collect();

    // Alegacao coerente: "a escrita falhou" para uma execucao que falhou.
    let alegacao = garraia_agents::AlegacaoOperacional::falha(
        registros[0].call_id.clone(),
        registros[0].name.clone(),
    );
    assert_eq!(
        detect_divergencia(&alegacao, &ledger),
        Divergencia::Sustentada
    );
}

// ── #1712 fatia 7: E2E multi-provedor do `cli_inspect` ─────────────────────
//
// O critério que faltava na #1712 era o E2E do relatório de inspeção **pelo
// fio**, em mais de uma família de provedor: a tool REAL executada pelo
// dispatch do runtime, com `AnthropicProvider`/`OpenAiProvider` de verdade
// serializando o histórico no protocolo de cada um e mandando por HTTP a um
// servidor local. O que se afirma é o `tool_result` que o modelo recebe no
// turno seguinte — o relatório declarado de gateway/MCP, as permissões do
// turno observadas no dispatch e a ausência de vazamento de credencial.
//
// O gateway está DESLIGADO (nada escuta na porta configurada além do próprio
// mock, que registra toda requisição): se a CLI sondasse o gateway, a sonda
// apareceria na lista de caminhos recebidos e o teste falharia.

/// Marcador que dispara o pedido de inspeção no roteiro.
const INSPECIONE_AGORA: &str = "INSPECIONE_AGORA";
const ID_INSPECAO: &str = "call-e2e-1712";
const APOS_INSPECAO: &str = "o que voce viu?";

/// Sentinela de credencial montada em TEMPO DE EXECUÇÃO: nenhum literal com
/// forma de segredo no fonte (o gate de secret scan não deve achar aqui).
fn sentinela_de_credencial(papel: &str) -> String {
    format!("{papel}-{}", ["creden", "cial-e2e", "-1712"].concat())
}

/// Roteiro da inspeção: quando a última mensagem é a pergunta marcada, pede
/// `cli_inspect` com `scope=all`; em qualquer outro caso, devolve texto. Nos
/// dois protocolos, sempre em SSE (o runtime usa `stream_complete`).
struct RoteiroInspecao;

impl Respond for RoteiroInspecao {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let corpo: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let caminho = request.url.path();
        // Pergunta nova = última mensagem `user` com conteúdo textual. Resultado
        // de ferramenta chega como `role=tool` (OpenAI) ou como bloco
        // `tool_result` (Anthropic) — nesses casos, e só nesses, a resposta é
        // texto. Assim o roteiro pede a inspeção em CADA turno, o que é o que
        // permite ao segundo relatório ver as estatísticas do primeiro turno.
        let pede = corpo["messages"]
            .as_array()
            .and_then(|m| m.last())
            .is_some_and(|m| m["role"] == "user" && m["content"].is_string());

        let sse = if caminho.ends_with("/messages") {
            let eventos: Vec<Value> = if pede {
                vec![
                    json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude-matrix","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":ID_INSPECAO,"name":"cli_inspect","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":json!({"scope":"all"}).to_string()}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":1,"output_tokens":1}}),
                    json!({"type":"message_stop"}),
                ]
            } else {
                vec![
                    json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude-matrix","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"inspecionado"}}),
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
            let chunks: Vec<Value> = if pede {
                vec![
                    json!({"id":"c","object":"chat.completion.chunk","model":"gpt-matrix","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":ID_INSPECAO,"type":"function","function":{"name":"cli_inspect","arguments":json!({"scope":"all"}).to_string()}}]},"finish_reason":null}]}),
                    json!({"id":"c","object":"chat.completion.chunk","model":"gpt-matrix","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                ]
            } else {
                vec![
                    json!({"id":"c","object":"chat.completion.chunk","model":"gpt-matrix","choices":[{"index":0,"delta":{"role":"assistant","content":"inspecionado"},"finish_reason":null}]}),
                    json!({"id":"c","object":"chat.completion.chunk","model":"gpt-matrix","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
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

async fn servidor_inspecao() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(RoteiroInspecao)
        .mount(&s)
        .await;
    s
}

/// Configuração da matriz: provider apontado para o mock, gateway com a porta
/// do PRÓPRIO mock (para que qualquer sonda apareça registrada) e um servidor
/// MCP declarado, desabilitado, com token na URL.
struct AmbienteInspecao {
    config: garraia_config::AppConfig,
    dir: tempfile::TempDir,
    sentinela_gateway: String,
    sentinela_mcp: String,
}

fn ambiente_inspecao(
    uri: &str,
    porta_gateway: u16,
    provedor: &str,
    modelo: &str,
) -> AmbienteInspecao {
    let dir = tempfile::tempdir().expect("tempdir config");
    let sentinela_gateway = sentinela_de_credencial("gateway");
    let sentinela_mcp = sentinela_de_credencial("mcp");
    let base = if provedor == "openai" {
        format!("{uri}/v1")
    } else {
        uri.to_string()
    };
    let yaml = format!(
        "llm:\n  {provedor}:\n    provider: {provedor}\n    model: {modelo}\n    api_key: chave-de-teste\n    base_url: {base}\n\
         gateway:\n  host: 127.0.0.1\n  port: {porta_gateway}\n  api_key: {sentinela_gateway}\n\
         mcp:\n  remoto:\n    url: \"https://mcp.example/sse?token={sentinela_mcp}\"\n    enabled: false\n"
    );
    std::fs::write(dir.path().join("config.yml"), yaml).expect("config.yml");
    let config = garraia_config::ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("config carrega");
    AmbienteInspecao {
        config,
        dir,
        sentinela_gateway,
        sentinela_mcp,
    }
}

fn relatorio_openai(corpo: &Value) -> Value {
    let msgs = corpo["messages"].as_array().expect("messages");
    let mensagem = msgs
        .iter()
        .rev()
        .find(|m| m["role"] == "tool" && m["tool_call_id"] == ID_INSPECAO)
        .unwrap_or_else(|| panic!("sem role=tool do cli_inspect: {msgs:#?}"));
    serde_json::from_str(mensagem["content"].as_str().expect("conteudo textual"))
        .expect("o relatorio e JSON")
}

fn relatorio_anthropic(corpo: &Value) -> Value {
    for mensagem in corpo["messages"].as_array().expect("messages").iter().rev() {
        let Some(blocos) = mensagem["content"].as_array() else {
            continue;
        };
        for bloco in blocos.iter().rev() {
            if bloco["type"] == "tool_result" && bloco["tool_use_id"] == ID_INSPECAO {
                return serde_json::from_str(bloco["content"].as_str().expect("conteudo textual"))
                    .expect("o relatorio e JSON");
            }
        }
    }
    panic!("sem tool_result do cli_inspect: {corpo:#?}")
}

/// O relatório é o mesmo nos dois protocolos, e cada campo tem de ser o
/// DECLARADO — nunca uma afirmação de saúde que a CLI não pode fazer.
fn assert_relatorio_declarado(relatorio: &Value, provedor: &str, modelo: &str, listen: &str) {
    assert_eq!(relatorio["origin"], "cli_runtime");
    assert_eq!(relatorio["read_only"], true);
    assert_eq!(relatorio["channel"], "cli");
    assert_eq!(relatorio["scope"], "all");

    assert_eq!(
        relatorio["identity"]["provider_selected"]["value"],
        provedor
    );
    assert_eq!(
        relatorio["identity"]["provider_selected"]["source"],
        "cli_selection"
    );
    assert_eq!(relatorio["identity"]["model_requested"]["value"], modelo);
    assert_eq!(relatorio["identity"]["last_turn"]["state"], "observed");
    // #1712 — "diferencia modelo pedido, provider ativo, confirmação pelo
    // provider e fallback": este caminho é o STREAMING (o REPL usa
    // `stream_complete`), que só recebe deltas de texto — não há modelo na
    // resposta para confirmar. O relatório tem de DIZER isso, em vez de
    // apresentar o modelo pedido como confirmado.
    assert_eq!(relatorio["identity"]["last_turn"]["provider"], provedor);
    assert_eq!(relatorio["identity"]["last_turn"]["model"], modelo);
    assert_eq!(
        relatorio["identity"]["last_turn"]["model_confirmed_by_provider"], false,
        "streaming nao confirma modelo; afirmar o contrario seria mentir: {}",
        relatorio["identity"]["last_turn"]
    );
    assert_eq!(
        relatorio["identity"]["last_turn"]["source"],
        "runtime_requested_model"
    );
    assert_eq!(relatorio["identity"]["last_turn"]["fallback_used"], false);
    assert!(
        relatorio["identity"]["last_turn"]["tool_calls"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "o turno anterior executou o cli_inspect: {}",
        relatorio["identity"]["last_turn"]
    );

    assert_eq!(relatorio["gateway"]["state"], "declared");
    assert_eq!(
        relatorio["gateway"]["live_verification"],
        "unavailable_from_cli"
    );
    assert_eq!(relatorio["gateway"]["listen"], listen);

    assert_eq!(relatorio["mcp_servers"]["state"], "declared");
    assert_eq!(
        relatorio["mcp_servers"]["live_verification"],
        "unavailable_from_cli"
    );
    assert_eq!(relatorio["mcp_servers"]["count"], 1);
    assert_eq!(relatorio["mcp_servers"]["enabled_count"], 0);
    let servidor = &relatorio["mcp_servers"]["servers"][0];
    assert_eq!(servidor["name"], "remoto");
    assert_eq!(servidor["transport"], "http");
    assert_eq!(servidor["enabled"], false);
    for proibido in ["url", "env", "args", "command"] {
        assert!(
            servidor.get(proibido).is_none(),
            "campo sensivel projetado no MCP: {proibido}"
        );
    }

    // Permissão vem do snapshot do dispatch do turno REAL, não do registro.
    assert_eq!(relatorio["permissions"]["state"], "verified");
    assert_eq!(
        relatorio["permissions"]["source"],
        "runtime_dispatch_turn_tools_snapshot"
    );

    assert!(relatorio["tools"]["count"].as_u64().unwrap_or(0) >= 1);
    let nomes: Vec<&str> = relatorio["tools"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert!(
        nomes.contains(&"cli_inspect"),
        "a propria cli_inspect tem de aparecer liberada no turno: {nomes:?}"
    );

    // Nenhum turno alimentou o snapshot de efeitos desta sessão de teste: o
    // relatório diz `unverified` em vez de inventar "nada rodou".
    assert_eq!(relatorio["tool_effects"]["state"], "unverified");
}

/// Nenhuma credencial rotulada da configuração atravessa o fio — nem no
/// relatório, nem em qualquer outra parte do payload.
fn assert_sem_vazamento(corpo: &Value, ambiente: &AmbienteInspecao) {
    let texto = corpo.to_string();
    for (rotulo, valor) in [
        ("gateway", &ambiente.sentinela_gateway),
        ("mcp", &ambiente.sentinela_mcp),
    ] {
        assert!(
            !texto.contains(valor.as_str()),
            "credencial de {rotulo} vazou no payload do turno seguinte"
        );
    }
}

/// Gateway desligado de verdade: toda requisição recebida pelo servidor foi do
/// provedor. Sonda ao gateway (que aponta para esta mesma porta) apareceria
/// aqui como um caminho estranho.
async fn assert_nenhuma_sonda(server: &MockServer, caminho_do_provedor: &str) {
    let caminhos: Vec<String> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(!caminhos.is_empty(), "o provedor nao recebeu nada");
    let sondas: Vec<&String> = caminhos
        .iter()
        .filter(|c| c.as_str() != caminho_do_provedor)
        .collect();
    assert!(
        sondas.is_empty(),
        "a CLI falou com algo alem do provedor (gateway desligado): {sondas:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_cli_inspect_anthropic_declara_estado_e_nao_vaza_credencial() {
    let server = servidor_inspecao().await;
    let porta = server.address().port();
    let ambiente = ambiente_inspecao(&server.uri(), porta, "anthropic", "claude-matrix");
    let provedor = Arc::new(
        AnthropicProvider::new(
            "chave-anth",
            Some("claude-matrix".to_string()),
            Some(server.uri()),
        )
        .with_name("anthropic"),
    );
    let mut sessao = Sessao::com_inspect(
        provedor as Arc<dyn LlmProvider>,
        "anthropic",
        "claude-matrix",
        &ambiente.config,
        ambiente.dir.path(),
    );

    sessao.turno(INSPECIONE_AGORA, "anthropic").await;
    sessao.turno(APOS_INSPECAO, "anthropic").await;

    let corpos = corpos(&server).await;
    // O ÚLTIMO pedido é o que fecha o turno 2: é nele que o relatório do
    // segundo `cli_inspect`, já com as estatísticas do turno 1, chega ao modelo.
    let ultimo = corpos.last().cloned().expect("pedidos ao provedor");
    let relatorio = relatorio_anthropic(&ultimo);
    assert_relatorio_declarado(
        &relatorio,
        "anthropic",
        "claude-matrix",
        &format!("127.0.0.1:{porta}"),
    );
    assert_sem_vazamento(&ultimo, &ambiente);
    assert_nenhuma_sonda(&server, "/v1/messages").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_cli_inspect_openai_declara_estado_e_nao_vaza_credencial() {
    let server = servidor_inspecao().await;
    let porta = server.address().port();
    let ambiente = ambiente_inspecao(&server.uri(), porta, "openai", "gpt-matrix");
    let provedor = Arc::new(
        OpenAiProvider::new(
            "chave-oai",
            Some("gpt-matrix".to_string()),
            Some(format!("{}/v1", server.uri())),
        )
        .with_name("openai"),
    );
    let mut sessao = Sessao::com_inspect(
        provedor as Arc<dyn LlmProvider>,
        "openai",
        "gpt-matrix",
        &ambiente.config,
        ambiente.dir.path(),
    );

    sessao.turno(INSPECIONE_AGORA, "openai").await;
    sessao.turno(APOS_INSPECAO, "openai").await;

    let corpos = corpos(&server).await;
    let ultimo = corpos.last().cloned().expect("pedidos ao provedor");
    let relatorio = relatorio_openai(&ultimo);
    assert_relatorio_declarado(
        &relatorio,
        "openai",
        "gpt-matrix",
        &format!("127.0.0.1:{porta}"),
    );
    assert_sem_vazamento(&ultimo, &ambiente);
    assert_nenhuma_sonda(&server, "/v1/chat/completions").await;
}

/// #1712 — critério "acompanha alteração via `/model`" junto do "diferencia
/// modelo pedido de modelo confirmado": depois da troca da seleção, o relatório
/// mostra o modelo NOVO em `model_requested` e mantém em `last_turn.model` o
/// modelo do turno que de fato rodou — os dois campos não podem ser o mesmo
/// número com etiqueta diferente.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_cli_inspect_acompanha_troca_de_modelo_sem_fundir_pedido_e_confirmado() {
    let server = servidor_inspecao().await;
    let porta = server.address().port();
    let ambiente = ambiente_inspecao(&server.uri(), porta, "anthropic", "claude-matrix");
    let provedor = Arc::new(
        AnthropicProvider::new(
            "chave-anth",
            Some("claude-matrix".to_string()),
            Some(server.uri()),
        )
        .with_name("anthropic"),
    );
    let mut sessao = Sessao::com_inspect(
        provedor as Arc<dyn LlmProvider>,
        "anthropic",
        "claude-matrix",
        &ambiente.config,
        ambiente.dir.path(),
    );

    sessao.turno(INSPECIONE_AGORA, "anthropic").await;
    sessao.turno(APOS_INSPECAO, "anthropic").await;
    // O gesto do `/model` entre turnos: só a seleção corrente muda.
    sessao.trocar_modelo("claude-matrix-sombra");
    sessao.turno(INSPECIONE_AGORA, "anthropic").await;

    let ultimo = corpos(&server).await.last().cloned().expect("pedidos");
    let relatorio = relatorio_anthropic(&ultimo);

    assert_eq!(
        relatorio["identity"]["model_requested"]["value"], "claude-matrix-sombra",
        "a selecao corrente da CLI tem de aparecer no relatorio"
    );
    assert_eq!(
        relatorio["identity"]["model_requested"]["source"],
        "current_cli_selection"
    );
    assert_eq!(
        relatorio["identity"]["last_turn"]["model"], "claude-matrix",
        "o ultimo turno rodou com o modelo anterior; nao pode ser reescrito pela troca"
    );
    assert_eq!(
        relatorio["identity"]["last_turn"]["model_confirmed_by_provider"],
        false
    );
    assert_eq!(
        relatorio["identity"]["last_turn"]["source"],
        "runtime_requested_model"
    );
    assert_eq!(
        relatorio["identity"]["provider_selected"]["value"],
        "anthropic"
    );
    assert_sem_vazamento(&ultimo, &ambiente);
}
