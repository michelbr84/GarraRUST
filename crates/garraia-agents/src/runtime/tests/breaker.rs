//! #1417: o circuit breaker de ferramentas no caminho de PRODUCAO do runtime.
//!
//! O relato do dogfood da v0.4.5: o modelo repetiu `repo_search` varias vezes
//! no mesmo turno depois de uma falha deterministica (sem repositorio) e de
//! timeouts. Aqui o provider roteirizado insiste na mesma ferramenta e o que
//! se mede e (a) quantas vezes ela RODOU e (b) o que voltou ao modelo na
//! segunda vez — o motivo estruturado, e nao mais uma execucao.

use super::*;
use crate::tools::file_jail::{DENIAL_MESSAGE, NO_ROOTS_MESSAGE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

/// Provider roteirizado: em cada turno pede `alvo` N vezes (uma por volta,
/// com um input diferente a cada vez — o detector de loop por assinatura e
/// OUTRO mecanismo, e nao pode ser ele a barrar aqui) e depois encerra com
/// texto. Anota todo `ToolResult` que recebe de volta.
///
/// Turno novo = a ultima mensagem do pedido e o texto do usuario; volta
/// seguinte = a ultima mensagem sao os `ToolResult` da volta anterior.
pub(super) struct InsisteNaFerramenta {
    alvo: &'static str,
    input: serde_json::Value,
    /// Quantas chamadas fazer em cada turno, na ordem dos turnos.
    roteiro: std::sync::Mutex<VecDeque<usize>>,
    pedidas_neste_turno: AtomicUsize,
    a_pedir_neste_turno: AtomicUsize,
    resultados: std::sync::Mutex<Vec<String>>,
}

impl InsisteNaFerramenta {
    pub(super) fn novo(alvo: &'static str, input: serde_json::Value, roteiro: &[usize]) -> Self {
        Self {
            alvo,
            input,
            roteiro: std::sync::Mutex::new(roteiro.iter().copied().collect()),
            pedidas_neste_turno: AtomicUsize::new(0),
            a_pedir_neste_turno: AtomicUsize::new(0),
            resultados: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(super) fn resultados(&self) -> Vec<String> {
        self.resultados.lock().expect("lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for InsisteNaFerramenta {
    fn provider_id(&self) -> &str {
        "insiste_na_ferramenta"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        match request.messages.last().map(|m| &m.content) {
            Some(MessagePart::Text(_)) => {
                self.pedidas_neste_turno.store(0, SeqCst);
                let n = self.roteiro.lock().expect("lock").pop_front().unwrap_or(0);
                self.a_pedir_neste_turno.store(n, SeqCst);
            }
            Some(MessagePart::Parts(blocos)) => {
                for b in blocos {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        self.resultados.lock().expect("lock").push(content.clone());
                    }
                }
            }
            None => {}
        }
        let feitas = self.pedidas_neste_turno.load(SeqCst);
        let content = if feitas < self.a_pedir_neste_turno.load(SeqCst) {
            self.pedidas_neste_turno.fetch_add(1, SeqCst);
            let mut input = self.input.clone();
            if let Some(obj) = input.as_object_mut() {
                obj.insert("tentativa".to_string(), serde_json::json!(feitas));
            }
            vec![ContentBlock::ToolUse {
                id: format!("chamada-{feitas}"),
                name: self.alvo.to_string(),
                input,
            }]
        } else {
            vec![ContentBlock::Text {
                text: "desisti".to_string(),
            }]
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

/// `file_read` numa sessao sem raiz: falha deterministica, com a frase unica
/// do jail. Conta quantas vezes rodou.
struct SemRaiz {
    executou: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for SemRaiz {
    fn name(&self) -> &str {
        "file_read"
    }
    fn description(&self) -> &str {
        "le arquivo"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.fetch_add(1, SeqCst);
        Ok(ToolOutput::error(NO_ROOTS_MESSAGE))
    }
}

/// Provider que pede `file_read` com UM caminho por volta, na ordem dada, e
/// depois encerra com texto. Anota os `ToolResult` que recebe.
struct SegueOsCaminhos {
    caminhos: Vec<&'static str>,
    volta: AtomicUsize,
    resultados: std::sync::Mutex<Vec<String>>,
}

impl SegueOsCaminhos {
    fn novo(caminhos: &[&'static str]) -> Self {
        Self {
            caminhos: caminhos.to_vec(),
            volta: AtomicUsize::new(0),
            resultados: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn resultados(&self) -> Vec<String> {
        self.resultados.lock().expect("lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for SegueOsCaminhos {
    fn provider_id(&self) -> &str {
        "segue_os_caminhos"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        if let Some(MessagePart::Parts(blocos)) = request.messages.last().map(|m| &m.content) {
            for b in blocos {
                if let ContentBlock::ToolResult { content, .. } = b {
                    self.resultados.lock().expect("lock").push(content.clone());
                }
            }
        }
        let i = self.volta.fetch_add(1, SeqCst);
        let content = match self.caminhos.get(i) {
            Some(caminho) => vec![ContentBlock::ToolUse {
                id: format!("chamada-{i}"),
                name: "file_read".to_string(),
                input: serde_json::json!({ "path": caminho }),
            }],
            None => vec![ContentBlock::Text {
                text: "li".to_string(),
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

/// Um `file_read` com jail de mentira: caminho sob `/etc` e negado com a
/// frase unica do jail; qualquer outro le. Conta as execucoes.
struct JailDeMentira {
    executou: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for JailDeMentira {
    fn name(&self) -> &str {
        "file_read"
    }
    fn description(&self) -> &str {
        "le arquivo dentro das raizes"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.fetch_add(1, SeqCst);
        let caminho = i.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        if caminho.starts_with("/etc") {
            Ok(ToolOutput::error(format!("file_read: {DENIAL_MESSAGE}")))
        } else {
            Ok(ToolOutput::success("conteudo do arquivo"))
        }
    }
}

/// Revisao do integrador em #1417: a recusa de caminho fora das raizes vale
/// para AQUELE caminho, nao para a ferramenta. O padrao legitimo — pedir
/// `/etc/x`, ler a recusa, corrigir para `./src/x` — tem de rodar a segunda
/// chamada; uma recusa so nao pode pausar `file_read`.
#[tokio::test]
async fn recusa_fora_das_raizes_nao_abre_e_o_caminho_corrigido_executa() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(SegueOsCaminhos::novo(&["/etc/passwd", "./src/main.rs"]));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(JailDeMentira {
        executou: Arc::clone(&executou),
    }));

    let resposta = turno(&rt, "sessao-1417-jail", &ExecContext::default()).await;

    assert_eq!(
        executou.load(SeqCst),
        2,
        "a segunda chamada, com caminho dentro da raiz, tem de rodar"
    );
    let resultados = provider.resultados();
    assert_eq!(resultados.len(), 2, "{resultados:?}");
    assert!(resultados[0].contains(DENIAL_MESSAGE), "{}", resultados[0]);
    assert_eq!(
        resultados[1], "conteudo do arquivo",
        "a correcao de caminho nao pode receber a recusa do breaker"
    );
    assert_eq!(resposta, "li");
    assert!(
        rt.estado_do_breaker("sessao-1417-jail").is_empty(),
        "o sucesso fechou o que a recusa contou"
    );
}

/// Dorme `segundos` (bem alem do timeout do orcamento) e conta quantas vezes
/// foi CHAMADA — o que importa e a chamada, ja que ela nunca termina.
struct LentaQueConta {
    executou: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for LentaQueConta {
    fn name(&self) -> &str {
        "lenta"
    }
    fn description(&self) -> &str {
        "demora"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.fetch_add(1, SeqCst);
        let segundos = i.get("segundos").and_then(|v| v.as_u64()).unwrap_or(100);
        tokio::time::sleep(std::time::Duration::from_secs(segundos)).await;
        Ok(ToolOutput::success("feito"))
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

/// O cenario da issue: a primeira chamada falha por um motivo que nao muda
/// (sem raiz), o modelo pede de novo no mesmo turno, e a segunda NAO roda —
/// volta o motivo estruturado. E o `garra_status` (via `estado_do_breaker`)
/// ve a ferramenta em pausa nesta sessao.
#[tokio::test]
async fn falha_deterministica_suprime_a_segunda_chamada_no_mesmo_turno() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(InsisteNaFerramenta::novo(
        "file_read",
        serde_json::json!({"path": "README.md"}),
        &[2],
    ));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(SemRaiz {
        executou: Arc::clone(&executou),
    }));

    let resposta = turno(&rt, "sessao-1417", &ExecContext::default()).await;

    assert_eq!(
        executou.load(SeqCst),
        1,
        "a segunda chamada tinha de ser suprimida pelo breaker"
    );
    let resultados = provider.resultados();
    assert_eq!(resultados.len(), 2, "{resultados:?}");
    assert!(
        resultados[0].contains(NO_ROOTS_MESSAGE),
        "{}",
        resultados[0]
    );
    assert!(
        resultados[1].contains("temporariamente indisponivel nesta conversa (no_roots)"),
        "a segunda volta recebe o motivo estruturado: {}",
        resultados[1]
    );
    assert!(
        resultados[1].contains("Nao repita a chamada neste turno"),
        "{}",
        resultados[1]
    );
    assert!(
        !resultados[1].contains("nao e permitida"),
        "breaker nao e o portao: {}",
        resultados[1]
    );
    assert_eq!(resposta, "desisti");

    let abertas = rt.estado_do_breaker("sessao-1417");
    assert_eq!(abertas.len(), 1, "{abertas:?}");
    assert_eq!(abertas[0].tool, "file_read");
    assert_eq!(abertas[0].codigo, "no_roots");
    assert!(rt.estado_do_breaker("outra-sessao").is_empty());
}

/// A parte deterministica vale ATE O FIM DO TURNO: o turno seguinte sonda de
/// novo (o usuario pode ter selecionado um projeto) e, falhando igual, volta
/// a suprimir a repeticao.
#[tokio::test]
async fn o_turno_seguinte_sonda_de_novo_e_volta_a_suprimir() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(InsisteNaFerramenta::novo(
        "file_read",
        serde_json::json!({"path": "README.md"}),
        &[2, 2],
    ));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(SemRaiz {
        executou: Arc::clone(&executou),
    }));

    turno(&rt, "sessao-1417-b", &ExecContext::default()).await;
    assert_eq!(executou.load(SeqCst), 1);
    turno(&rt, "sessao-1417-b", &ExecContext::default()).await;
    assert_eq!(executou.load(SeqCst), 2, "o turno novo sonda uma vez");

    let resultados = provider.resultados();
    assert_eq!(resultados.len(), 4, "{resultados:?}");
    assert!(
        resultados[2].contains(NO_ROOTS_MESSAGE),
        "{}",
        resultados[2]
    );
    assert!(
        resultados[3].contains("(no_roots)"),
        "a repeticao do segundo turno tambem e suprimida: {}",
        resultados[3]
    );
}

/// Timeout e transitorio: abre por um cooldown, e a segunda chamada do mesmo
/// turno nao roda. Tempo do tokio pausado: o `sleep` de 100s da ferramenta
/// perde para o `timeout` de 30s do orcamento sem esperar nada de verdade.
#[tokio::test(start_paused = true)]
async fn timeout_abre_o_breaker_e_a_segunda_chamada_do_turno_nao_executa() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(InsisteNaFerramenta::novo(
        "lenta",
        serde_json::json!({"segundos": 100}),
        &[2],
    ));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(LentaQueConta {
        executou: Arc::clone(&executou),
    }));

    turno(&rt, "sessao-1417-t", &ExecContext::default()).await;

    assert_eq!(executou.load(SeqCst), 1, "a segunda chamada nao pode rodar");
    let resultados = provider.resultados();
    assert_eq!(resultados.len(), 2, "{resultados:?}");
    assert!(
        resultados[0].starts_with("tool timeout: lenta"),
        "{}",
        resultados[0]
    );
    assert!(
        resultados[1].contains("temporariamente indisponivel nesta conversa (timeout)"),
        "{}",
        resultados[1]
    );
    let abertas = rt.estado_do_breaker("sessao-1417-t");
    assert_eq!(abertas.len(), 1, "{abertas:?}");
    assert_eq!(abertas[0].codigo, "timeout");
}

/// O cooldown do timeout atravessa turnos (um timeout de 30s nao vira
/// "tenta de novo" porque o usuario mandou outra mensagem um segundo depois),
/// mas um `working_dir` diferente e outro contexto: limpa tudo, e a
/// ferramenta roda de novo.
#[tokio::test(start_paused = true)]
async fn cooldown_do_timeout_atravessa_turnos_e_working_dir_novo_limpa() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(InsisteNaFerramenta::novo(
        "lenta",
        serde_json::json!({"segundos": 100}),
        &[1, 1, 1],
    ));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(LentaQueConta {
        executou: Arc::clone(&executou),
    }));

    let mesmo = ExecContext::with_working_dir(Some("/projeto/a".to_string()));
    turno(&rt, "sessao-1417-c", &mesmo).await;
    assert_eq!(executou.load(SeqCst), 1);

    turno(&rt, "sessao-1417-c", &mesmo).await;
    assert_eq!(
        executou.load(SeqCst),
        1,
        "mesmo contexto, cooldown vigente: nao roda"
    );
    let resultados = provider.resultados();
    assert!(
        resultados[1].contains("(timeout)"),
        "o segundo turno recebe o motivo: {}",
        resultados[1]
    );

    let outro = ExecContext::with_working_dir(Some("/projeto/b".to_string()));
    turno(&rt, "sessao-1417-c", &outro).await;
    assert_eq!(
        executou.load(SeqCst),
        2,
        "working_dir novo e contexto novo: o breaker foi limpo"
    );
}

/// Guarda de fonte: os tres turnos do runtime abrem o turno do breaker, e o
/// despacho o consulta DEPOIS da disponibilidade (#1425) e antes de executar.
/// Uma copia nova de laco que esqueca o breaker reprova aqui.
#[test]
fn todo_turno_abre_o_breaker_e_o_despacho_o_consulta_depois_da_disponibilidade() {
    let fonte = include_str!("../../runtime.rs");
    assert!(
        fonte
            .matches("self.abrir_turno_do_breaker(session_id, exec);")
            .count()
            >= 3,
        "os tres caminhos de turno abrem o turno do breaker"
    );
    let disponibilidade = fonte
        .find("let disponibilidade = self.disponibilidade_de(name);")
        .expect("o despacho consulta a disponibilidade");
    let breaker = fonte
        .find("let breaker = self.breakers.estado(")
        .expect("o despacho consulta o breaker");
    assert!(
        disponibilidade < breaker,
        "o breaker vem depois da disponibilidade: indisponivel nao e falha repetida"
    );
    // Um unico ponto de alimentacao, dentro do despacho e depois da consulta
    // (o `rustfmt` quebra a chamada em linhas, por isso so o inicio dela).
    let alimenta: Vec<usize> = fonte
        .match_indices("self.breakers.registrar(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        alimenta.len(),
        1,
        "toda saida de ferramenta alimenta o breaker num ponto so"
    );
    assert!(
        alimenta[0] > breaker,
        "a saida alimenta o breaker depois de ele ter sido consultado"
    );
}
