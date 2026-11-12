//! #1708 — o que o provedor recebe no turno SEGUINTE a uma ferramenta.
//!
//! Os testes de `tool_history` provam a conversao registro -> mensagem. Aqui a
//! prova e de ponta a ponta pelo caminho do REPL: o runtime roda a ferramenta
//! de verdade, os `TurnEvent` alimentam a `ToolTrail`, `anexar_turno` fecha o
//! turno no `history`, e o pedido do turno seguinte e gravado pelo provedor.
//! Se o par `ToolUse`/`ToolResult` nao chega la, o modelo nao tem como saber
//! que escreveu o arquivo — o defeito relatado na issue.
//!
//! O provedor e roteirizado, entao o que se afirma e o CONTEXTO entregue ao
//! modelo, nao a frase que um modelo real diria com ele.

use super::*;
use garraia_agents::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, Tool,
    ToolContext, ToolOutput,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Escreve de verdade num diretorio temporario; o nome e o da tool real para
/// o historico ficar igual ao de producao.
struct Escreve(PathBuf);

#[async_trait::async_trait]
impl Tool for Escreve {
    fn name(&self) -> &str {
        "file_write"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _c: &ToolContext,
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        let nome = i["path"].as_str().unwrap_or_default();
        let conteudo = i["content"].as_str().unwrap_or_default();
        match std::fs::write(self.0.join(nome), conteudo) {
            Ok(()) => Ok(ToolOutput::success(format!(
                "escreveu {} bytes em {nome}",
                conteudo.len()
            ))),
            Err(e) => Ok(ToolOutput::error(format!("falha ao escrever {nome}: {e}"))),
        }
    }
}

struct Le(PathBuf);

#[async_trait::async_trait]
impl Tool for Le {
    fn name(&self) -> &str {
        "file_read"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _c: &ToolContext,
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        let nome = i["path"].as_str().unwrap_or_default();
        match std::fs::read_to_string(self.0.join(nome)) {
            Ok(texto) => Ok(ToolOutput::success(texto)),
            Err(_) => Ok(ToolOutput::error(format!("{nome} nao existe"))),
        }
    }
}

const CONTEUDO: &str = "<h1>ola</h1>";

/// Grava cada pedido e segue um roteiro pelo texto da ultima mensagem humana:
/// "crie" pede `file_write`, "leia" pede `file_read`, "crie e leia" pede um
/// depois do outro. Qualquer outra coisa e so texto.
#[derive(Default)]
struct Gravador {
    pedidos: Mutex<Vec<Vec<ChatMessage>>>,
    chamadas: Mutex<u32>,
}

impl Gravador {
    fn proximo_id(&self) -> String {
        let mut n = self.chamadas.lock().expect("contador");
        *n += 1;
        format!("call-{n}")
    }

    fn ultimo_pedido(&self) -> Vec<ChatMessage> {
        self.pedidos
            .lock()
            .expect("pedidos")
            .last()
            .cloned()
            .expect("o provedor recebeu ao menos um pedido")
    }
}

fn tool_use(id: String, nome: &str) -> Vec<ContentBlock> {
    let input = if nome == "file_write" {
        serde_json::json!({ "path": "pagina.html", "content": CONTEUDO })
    } else {
        serde_json::json!({ "path": "pagina.html" })
    };
    vec![ContentBlock::ToolUse {
        id,
        name: nome.into(),
        input,
    }]
}

#[async_trait::async_trait]
impl LlmProvider for Gravador {
    fn provider_id(&self) -> &str {
        "gravador"
    }
    async fn complete(&self, r: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        self.pedidos
            .lock()
            .expect("pedidos")
            .push(r.messages.clone());
        // A pergunta humana deste turno e a ultima mensagem de texto do user.
        let pergunta = r
            .messages
            .iter()
            .rev()
            .find_map(|m| match (&m.role, &m.content) {
                (ChatRole::User, MessagePart::Text(t)) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        // Quantos resultados de ferramenta ja vieram DEPOIS dessa pergunta.
        let resultados = r
            .messages
            .iter()
            .rev()
            .take_while(|m| !matches!(&m.content, MessagePart::Text(t) if *t == pergunta))
            .filter(|m| {
                matches!(&m.content, MessagePart::Parts(p)
                    if p.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })))
            })
            .count();
        let content = match (pergunta.as_str(), resultados) {
            ("crie", 0) | ("crie e leia", 0) => tool_use(self.proximo_id(), "file_write"),
            ("crie e leia", 1) | ("leia", 0) => tool_use(self.proximo_id(), "file_read"),
            _ => vec![ContentBlock::Text {
                text: "pronto".into(),
            }],
        };
        Ok(LlmResponse {
            content,
            model: "m".into(),
            stop_reason: None,
            usage: None,
        })
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

struct Sessao {
    rt: AgentRuntime,
    provedor: Arc<Gravador>,
    trilha: ToolTrail,
    historico: Vec<ChatMessage>,
    dir: tempfile::TempDir,
}

fn sessao() -> Sessao {
    let dir = tempfile::tempdir().expect("tempdir");
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(Escreve(dir.path().to_path_buf())));
    rt.register_tool(Box::new(Le(dir.path().to_path_buf())));
    let provedor = Arc::new(Gravador::default());
    rt.register_provider(Arc::clone(&provedor) as Arc<dyn LlmProvider>);
    Sessao {
        rt,
        provedor,
        trilha: ToolTrail::default(),
        historico: Vec::new(),
        dir,
    }
}

/// Um turno pelo mesmo caminho do REPL: eventos -> trilha -> `anexar_turno`.
async fn turno(s: &mut Sessao, texto: &str, modo: Modo) {
    s.trilha.iniciar_turno();
    let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
    let dreno = tokio::spawn(async move {
        let mut eventos = Vec::new();
        while let Some(e) = rx.recv().await {
            eventos.push(e);
        }
        eventos
    });
    let dir = s.dir.path().to_string_lossy().to_string();
    let exec = exec_do_turno(&dir, "cli-1708-e2e");
    let resposta =
        s.rt.process_message_streaming_with_events(
            "cli-1708-e2e",
            texto,
            &s.historico,
            tx,
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect("turno");
    for e in dreno.await.expect("dreno") {
        s.trilha.observar(&e);
    }
    let registros = s.trilha.fim_do_turno();
    anexar_turno(&mut s.historico, texto, &resposta, &registros, modo);
}

/// `(id, nome)` de cada `ToolUse` e `(tool_use_id, conteudo)` de cada
/// `ToolResult`, na ordem em que aparecem no pedido.
#[derive(Debug, PartialEq, Eq)]
enum Bloco {
    Uso(String, String),
    Resultado(String, String),
}

fn blocos(pedido: &[ChatMessage]) -> Vec<Bloco> {
    let mut v = Vec::new();
    for m in pedido {
        let MessagePart::Parts(partes) = &m.content else {
            continue;
        };
        for b in partes {
            match b {
                ContentBlock::ToolUse { id, name, .. } => {
                    assert_eq!(m.role, ChatRole::Assistant, "ToolUse e do assistant");
                    v.push(Bloco::Uso(id.clone(), name.clone()));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    assert_eq!(m.role, ChatRole::User, "ToolResult volta como user");
                    v.push(Bloco::Resultado(tool_use_id.clone(), content.clone()));
                }
                _ => {}
            }
        }
    }
    v
}

/// Criterio 1 da issue: `Write pagina.html` -> turno seguinte. O pedido do
/// segundo turno leva a chamada e o resultado, pareados pelo id do executor.
#[tokio::test]
async fn turno_seguinte_recebe_a_escrita_com_use_e_result_pareados() {
    let mut s = sessao();
    turno(&mut s, "crie", Modo::Structured).await;
    assert!(s.dir.path().join("pagina.html").exists(), "a tool escreveu");

    turno(&mut s, "voce criou o arquivo?", Modo::Structured).await;
    let pedido = s.provedor.ultimo_pedido();
    let b = blocos(&pedido);
    assert_eq!(
        b,
        vec![
            Bloco::Uso("call-1".into(), "file_write".into()),
            Bloco::Resultado(
                "call-1".into(),
                format!("escreveu {} bytes em pagina.html", CONTEUDO.len())
            ),
        ],
        "o turno seguinte precisa ver a escrita do turno anterior"
    );
    // A pergunta nova vem depois da evidencia, e e a ultima mensagem.
    assert!(
        matches!(
            pedido.last(),
            Some(ChatMessage { role: ChatRole::User, content: MessagePart::Text(t) })
                if t == "voce criou o arquivo?"
        ),
        "{pedido:?}"
    );
}

/// Criterio 2: `Write` -> `Read` no mesmo turno. O turno seguinte distingue
/// as duas chamadas: ids diferentes, cada resultado no seu id, em ordem.
#[tokio::test]
async fn escrita_e_leitura_chegam_como_dois_ciclos_distintos() {
    let mut s = sessao();
    turno(&mut s, "crie e leia", Modo::Structured).await;
    turno(&mut s, "o que voce fez?", Modo::Structured).await;

    assert_eq!(
        blocos(&s.provedor.ultimo_pedido()),
        vec![
            Bloco::Uso("call-1".into(), "file_write".into()),
            Bloco::Resultado(
                "call-1".into(),
                format!("escreveu {} bytes em pagina.html", CONTEUDO.len())
            ),
            Bloco::Uso("call-2".into(), "file_read".into()),
            Bloco::Resultado("call-2".into(), CONTEUDO.into()),
        ],
        "escrita e leitura nao podem se confundir no historico"
    );
}

/// Criterio 3: o arquivo e apagado por fora depois da escrita. A leitura
/// seguinte falha, e o historico guarda os dois fatos: a escrita que deu
/// certo continua la, e a ausencia atual aparece como resultado proprio.
#[tokio::test]
async fn delecao_externa_nao_apaga_a_escrita_do_historico() {
    let mut s = sessao();
    turno(&mut s, "crie", Modo::Structured).await;
    std::fs::remove_file(s.dir.path().join("pagina.html")).expect("apaga por fora");

    turno(&mut s, "leia", Modo::Structured).await;
    turno(&mut s, "o arquivo existe?", Modo::Structured).await;

    let b = blocos(&s.provedor.ultimo_pedido());
    assert_eq!(b.len(), 4, "{b:?}");
    assert_eq!(
        b[..2],
        [
            Bloco::Uso("call-1".into(), "file_write".into()),
            Bloco::Resultado(
                "call-1".into(),
                format!("escreveu {} bytes em pagina.html", CONTEUDO.len())
            ),
        ],
        "a escrita anterior segue registrada como sucesso"
    );
    assert_eq!(b[2], Bloco::Uso("call-2".into(), "file_read".into()));
    let Bloco::Resultado(id, conteudo) = &b[3] else {
        panic!("esperava o resultado da leitura: {b:?}");
    };
    assert_eq!(id, "call-2");
    assert!(
        conteudo.contains("pagina.html nao existe"),
        "a ausencia atual chega como falha da leitura: {conteudo:?}"
    );
}

/// `GARRAIA_TOOL_HISTORY=text|off` continua valendo: nesses modos nenhum
/// bloco estruturado vai ao provedor no turno seguinte.
#[tokio::test]
async fn modos_text_e_off_nao_mandam_blocos_estruturados() {
    for modo in [Modo::Text, Modo::Off] {
        let mut s = sessao();
        turno(&mut s, "crie", modo).await;
        turno(&mut s, "voce criou o arquivo?", modo).await;
        assert_eq!(
            blocos(&s.provedor.ultimo_pedido()),
            Vec::new(),
            "modo {modo:?} nao leva ToolUse/ToolResult"
        );
    }
}
