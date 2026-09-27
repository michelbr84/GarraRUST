//! #1438: o despacho de PRODUCAO alimenta o registro de confiabilidade.
//!
//! Sucesso, erro, timeout, negada pelo portao, indisponivel e recusa do
//! breaker chegam cada um ao seu contador — e nada do que o modelo ou o
//! usuario escreveu (input, saida, nome inventado) nem o id da sessao chega
//! ao snapshot.

use super::breaker::InsisteNaFerramenta;
use super::*;
use crate::observabilidade::{DESCONHECIDA, Snapshot, ToolSnapshot};
use crate::tools::Disponibilidade;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

/// Texto "do usuario" que viaja no input, na saida de erro e no nome
/// inventado. Nunca pode aparecer no snapshot.
const PROMPT: &str = "PROMPT-SECRETO-do-usuario-5511988887777";

/// Pede as ferramentas da lista, uma por volta, e encerra com texto. Turno
/// novo (a ultima mensagem e o texto do usuario) recomeca a lista.
struct PedeEmOrdem {
    nomes: Vec<String>,
    proxima: AtomicUsize,
}

impl PedeEmOrdem {
    fn novo(nomes: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            nomes: nomes.iter().map(|n| n.to_string()).collect(),
            proxima: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl LlmProvider for PedeEmOrdem {
    fn provider_id(&self) -> &str {
        "pede_em_ordem"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        if let Some(MessagePart::Text(_)) = request.messages.last().map(|m| &m.content) {
            self.proxima.store(0, SeqCst);
        }
        let i = self.proxima.fetch_add(1, SeqCst);
        let content = match self.nomes.get(i) {
            Some(nome) => vec![ContentBlock::ToolUse {
                id: format!("chamada-{i}"),
                name: nome.clone(),
                input: serde_json::json!({ "texto": PROMPT }),
            }],
            None => vec![ContentBlock::Text {
                text: "pronto".to_string(),
            }],
        };
        Ok(LlmResponse {
            content,
            model: "modelo-de-teste".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Falha com o texto do usuario na mensagem de erro — o registro recebe o
/// desfecho, nunca a mensagem.
struct Quebra;

#[async_trait]
impl Tool for Quebra {
    fn name(&self) -> &str {
        "quebra"
    }
    fn description(&self) -> &str {
        "sempre falha"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        Ok(ToolOutput::error(format!(
            "nao consegui processar {PROMPT}"
        )))
    }
}

/// Conta execucoes; serve de `file_write` (negada no modo `search`) e de
/// ferramenta lenta (dorme alem do timeout do orcamento).
struct Contada {
    nome: &'static str,
    dorme_s: u64,
    indisponivel: bool,
    executou: Arc<AtomicUsize>,
}

impl Contada {
    fn nova(nome: &'static str) -> (Box<Self>, Arc<AtomicUsize>) {
        let executou = Arc::new(AtomicUsize::new(0));
        let tool = Box::new(Self {
            nome,
            dorme_s: 0,
            indisponivel: false,
            executou: Arc::clone(&executou),
        });
        (tool, executou)
    }
}

#[async_trait]
impl Tool for Contada {
    fn name(&self) -> &str {
        self.nome
    }
    fn description(&self) -> &str {
        "conta"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.fetch_add(1, SeqCst);
        if self.dorme_s > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(self.dorme_s)).await;
        }
        Ok(ToolOutput::success("feito"))
    }
    fn disponibilidade(&self) -> Disponibilidade {
        if self.indisponivel {
            Disponibilidade::indisponivel("channel_offline", "o canal nao esta conectado.", None)
        } else {
            Disponibilidade::Disponivel
        }
    }
}

async fn turno(rt: &AgentRuntime, sessao: &str, exec: &ExecContext) -> String {
    rt.process_message_with_agent_config(
        sessao,
        "faz a coisa",
        &[],
        None,
        None,
        None,
        None,
        None,
        None,
        exec,
    )
    .await
    .expect("o turno termina")
}

fn snapshot(rt: &AgentRuntime) -> Snapshot {
    rt.observabilidade().snapshot(std::time::Instant::now())
}

fn linha<'a>(s: &'a Snapshot, tool: &str) -> &'a ToolSnapshot {
    s.tools
        .iter()
        .find(|t| t.tool == tool)
        .unwrap_or_else(|| panic!("sem linha para {tool}: {:?}", s.tools))
}

#[tokio::test]
async fn o_despacho_conta_sucesso_erro_e_nome_inventado_sem_vazar_texto() {
    let rt = AgentRuntime::new();
    let inventada = format!("inventada_{PROMPT}");
    let provider = PedeEmOrdem::novo(&["eco_ok", "quebra", &inventada]);
    rt.register_provider(provider);
    rt.register_tool(stub("eco_ok"));
    rt.register_tool(Box::new(Quebra));

    let resposta = turno(&rt, "sessao-1438-conta", &ExecContext::default()).await;
    assert_eq!(resposta, "pronto");

    let s = snapshot(&rt);
    let ok = linha(&s, "eco_ok");
    assert_eq!(ok.stats.calls, 1);
    assert_eq!(ok.stats.outcomes["success"], 1);
    assert_eq!(ok.stats.latency_ms.count, 1, "executou: a latencia conta");
    let quebra = linha(&s, "quebra");
    assert_eq!(quebra.stats.outcomes["error"], 1);
    assert!(quebra.stats.last_failure_ago_s.is_some());
    let desconhecida = linha(&s, DESCONHECIDA);
    assert_eq!(
        desconhecida.stats.outcomes["error"], 1,
        "nome que nao e de ferramenta registrada cai no balde constante"
    );
    assert_eq!(s.tools.len(), 3, "{:?}", s.tools);
    assert_eq!(s.tools_total.calls, 3);

    let json = serde_json::to_string(&s).expect("serializa");
    assert!(!json.contains("PROMPT-SECRETO"), "vazou texto: {json}");
    assert!(!json.contains("5511988887777"), "vazou texto: {json}");
    assert!(
        !json.contains("sessao-1438"),
        "sessao nao e dimensao: {json}"
    );
}

#[tokio::test]
async fn negada_pelo_portao_conta_como_politica_e_nao_executa() {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(PedeFerramentaMcp::novo("file_write")));
    let (tool, executou) = Contada::nova("file_write");
    rt.register_tool(tool);

    let search = ExecContext::with_mode(Some("search".to_string()));
    turno(&rt, "sessao-1438-portao", &search).await;

    assert_eq!(executou.load(SeqCst), 0, "premissa: o portao barrou");
    let s = snapshot(&rt);
    let fw = linha(&s, "file_write");
    assert_eq!(fw.stats.calls, 1);
    assert_eq!(fw.stats.outcomes["denied_by_policy"], 1);
    assert_eq!(fw.stats.outcomes["success"], 0);
    assert_eq!(fw.stats.latency_ms.count, 0, "recusa nao mede latencia");
    assert_eq!(fw.stats.success_rate, None);
}

#[tokio::test]
async fn indisponivel_conta_a_parte_e_nao_executa() {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(PedeFerramentaMcp::novo("canal_send")));
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(Contada {
        nome: "canal_send",
        dorme_s: 0,
        indisponivel: true,
        executou: Arc::clone(&executou),
    }));

    turno(&rt, "sessao-1438-indisponivel", &ExecContext::default()).await;

    assert_eq!(executou.load(SeqCst), 0);
    let s = snapshot(&rt);
    assert_eq!(linha(&s, "canal_send").stats.outcomes["unavailable"], 1);
}

/// Timeout nao e erro, abre o breaker (uma ativacao), e a repeticao no mesmo
/// turno e recusada pelo breaker sem executar.
#[tokio::test(start_paused = true)]
async fn timeout_conta_como_timeout_abre_o_breaker_e_a_repeticao_e_recusada() {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(InsisteNaFerramenta::novo(
        "lenta",
        serde_json::json!({ "texto": PROMPT }),
        &[2],
    )));
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(Contada {
        nome: "lenta",
        dorme_s: 100,
        indisponivel: false,
        executou: Arc::clone(&executou),
    }));

    turno(&rt, "sessao-1438-timeout", &ExecContext::default()).await;

    assert_eq!(executou.load(SeqCst), 1, "premissa: a segunda nao rodou");
    let s = snapshot(&rt);
    let lenta = linha(&s, "lenta");
    assert_eq!(lenta.stats.calls, 2);
    assert_eq!(lenta.stats.outcomes["timeout"], 1);
    assert_eq!(lenta.stats.outcomes["error"], 0, "timeout nao e erro");
    assert_eq!(lenta.stats.outcomes["breaker_refused"], 1);
    assert_eq!(lenta.stats.breaker_opened, 1, "o timeout abriu o breaker");
    assert_eq!(lenta.stats.timeout_rate, Some(1.0));
    assert_eq!(s.tools_total.breaker_opened, 1);
}

/// Duas sessoes chamando a mesma ferramenta: UMA linha, e nenhum id de
/// sessao no snapshot.
#[tokio::test]
async fn sessao_nao_e_dimensao() {
    let rt = AgentRuntime::new();
    rt.register_provider(PedeEmOrdem::novo(&["eco_ok"]));
    rt.register_tool(stub("eco_ok"));

    turno(&rt, "sessao-1438-a", &ExecContext::default()).await;
    turno(&rt, "sessao-1438-b", &ExecContext::default()).await;

    let s = snapshot(&rt);
    assert_eq!(s.tools.len(), 1, "{:?}", s.tools);
    assert_eq!(linha(&s, "eco_ok").stats.calls, 2);
    let json = serde_json::to_string(&s).expect("serializa");
    assert!(!json.contains("sessao-1438"), "{json}");
}

/// Guarda de fonte: a saida de toda ferramenta executada e observada num
/// ponto so, depois de alimentar o breaker (a abertura vem dele), e cada uma
/// das tres recusas antes da execucao (portao, indisponivel, breaker) tem a
/// sua observacao. Uma copia nova de laco que esqueca o registro reprova.
#[test]
fn o_despacho_observa_cada_saida_e_cada_recusa() {
    let fonte = include_str!("../../runtime.rs");
    let saidas: Vec<usize> = fonte
        .match_indices("self.observar_saida(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(saidas.len(), 1, "a saida e observada num ponto so");
    let breaker = fonte
        .find("self.breakers.registrar(")
        .expect("o despacho alimenta o breaker");
    assert!(
        saidas[0] > breaker,
        "a observacao vem depois do breaker, que diz se abriu"
    );
    assert_eq!(
        fonte.matches("self.observar_recusa(").count(),
        3,
        "portao, indisponivel e breaker aberto"
    );
}
