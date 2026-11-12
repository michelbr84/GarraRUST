//! #1708 — o que o provedor recebe no turno SEGUINTE a uma ferramenta.
//!
//! Os testes de `tool_history` provam a conversao registro -> mensagem. Aqui a
//! prova e pelas mesmas funcoes que o REPL chama: o runtime roda a ferramenta
//! de verdade, o `stream_turn` drena os `TurnEvent` e alimenta a `ToolTrail`,
//! `anexar_turno` fecha o turno no `history`, e o pedido do turno seguinte e
//! gravado pelo provedor. O laco de `run_chat` em si (leitura da linha,
//! persistencia, cartoes de erro) fica de fora.
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

/// Fica rodando ate o turno ser derrubado por fora. Ao comecar, avisa pelo
/// `Notify` compartilhado — e o "Ctrl+C" do teste de cancelamento, disparado
/// so depois que o `ToolStarted` ja saiu. O timeout do runtime por ferramenta
/// e bem maior que a espera do teste, entao quem encerra e sempre o REPL.
struct Espera(Arc<tokio::sync::Notify>);

#[async_trait::async_trait]
impl Tool for Espera {
    fn name(&self) -> &str {
        "espera"
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
        _i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        self.0.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        Ok(ToolOutput::success("nunca chega aqui"))
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
            ("espere", 0) => tool_use(self.proximo_id(), "espera"),
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
    /// Disparado pela ferramenta `espera` quando ela comeca a rodar.
    comecou: Arc<tokio::sync::Notify>,
}

fn sessao() -> Sessao {
    let dir = tempfile::tempdir().expect("tempdir");
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(Escreve(dir.path().to_path_buf())));
    rt.register_tool(Box::new(Le(dir.path().to_path_buf())));
    let comecou = Arc::new(tokio::sync::Notify::new());
    rt.register_tool(Box::new(Espera(Arc::clone(&comecou))));
    let provedor = Arc::new(Gravador::default());
    rt.register_provider(Arc::clone(&provedor) as Arc<dyn LlmProvider>);
    Sessao {
        rt,
        provedor,
        trilha: ToolTrail::default(),
        historico: Vec::new(),
        dir,
        comecou,
    }
}

/// Como o turno termina, do lado de quem esta no terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fim {
    /// Ninguem interfere: o turno precisa concluir.
    Normal,
    /// Ctrl+C assim que a ferramenta `espera` comeca.
    CtrlC,
    /// Teto de parede do turno (o `--timeout` do REPL) bem curto.
    Timeout,
}

/// O que o turno deixou, como o REPL classifica.
#[derive(PartialEq, Eq, Debug)]
enum Desfecho {
    Concluido,
    Cancelado,
    EstourouOTempo,
}

/// Um turno pelo mesmo caminho do REPL: o `stream_turn` drena os eventos e
/// e ELE quem alimenta a trilha; depois vem `fim_do_turno` e `anexar_turno`.
/// Se o `stream_turn` parar de chamar `trail.observar`, estes testes falham.
async fn turno(s: &mut Sessao, texto: &str, modo: Modo) {
    let desfecho = turno_com(s, texto, modo, None, Fim::Normal).await;
    assert_eq!(desfecho, Desfecho::Concluido, "o turno deveria concluir");
}

/// O turno do REPL por inteiro, inclusive os fins inacabados: depois do
/// `stream_turn`, o par entra no `history` pelas mesmas decisoes de
/// `run_chat` (`turno_deixa_par_no_history`, `texto_preservado`). `modo_do_agente`
/// e o modo escolhido pela pessoa (`/mode`), que liga a politica de ferramentas.
async fn turno_com(
    s: &mut Sessao,
    texto: &str,
    modo: Modo,
    modo_do_agente: Option<&str>,
    fim: Fim,
) -> Desfecho {
    s.trilha.iniciar_turno();
    let (tx, rx) = mpsc::channel::<TurnEvent>(100);
    let dir = s.dir.path().to_string_lossy().to_string();
    let mut exec = exec_do_turno(&dir, "cli-1708-e2e");
    exec.agent_mode = modo_do_agente.map(str::to_string);
    let historico = s.historico.clone();
    let call = s.rt.process_message_streaming_with_events(
        "cli-1708-e2e",
        texto,
        &historico,
        tx,
        None,
        None,
        None,
        None,
        None,
        None,
        &exec,
    );
    let caps = crate::ui::Capabilities {
        interactive: true,
        unicode: false,
        animation: false,
        width: 80,
    };
    let cancel = tokio::sync::Notify::new();
    let timeout = (fim == Fim::Timeout).then(|| std::time::Duration::from_millis(300));
    let mut tool_log = ToolLog::new();
    let mut tela = Vec::<u8>::new();
    let mut renderer = TerminalRenderer::with_prefix(caps, None, "");
    let rodando = stream_turn(
        call,
        rx,
        timeout,
        &mut tela,
        &mut renderer,
        &mut tool_log,
        &mut s.trilha,
        &cancel,
    );
    let outcome = if fim == Fim::CtrlC {
        let comecou = Arc::clone(&s.comecou);
        let ctrl_c = async {
            comecou.notified().await;
            cancel.notify_one();
            std::future::pending::<()>().await;
        };
        tokio::select! {
            o = rodando => o,
            _ = ctrl_c => unreachable!("o Ctrl+C nunca termina sozinho"),
        }
    } else {
        rodando.await
    };
    let registros = s.trilha.fim_do_turno();
    let evidencia = tool_log.resumo_desde(0);
    let deixa_par = turno_deixa_par_no_history(
        &outcome,
        turno_tem_evidencia(evidencia.as_deref(), &registros),
    );
    let desfecho = match &outcome {
        TurnOutcome::Done(Ok(_)) => Desfecho::Concluido,
        TurnOutcome::Cancelled { .. } => Desfecho::Cancelado,
        TurnOutcome::TimedOut { .. } => Desfecho::EstourouOTempo,
        TurnOutcome::Done(Err(e)) => panic!("o turno nao deveria falhar: {e}"),
    };
    let resposta = match marcador_de_interrupcao(&outcome) {
        Some(marcador) => texto_preservado(parcial_do_outcome(&outcome), marcador),
        None => match outcome {
            TurnOutcome::Done(Ok(r)) => r,
            _ => unreachable!("so o turno concluido fica sem marcador"),
        },
    };
    if deixa_par {
        anexar_turno(&mut s.historico, texto, &resposta, &registros, modo);
    }
    desfecho
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
/// bloco estruturado vai ao provedor no turno seguinte. Cobre so o ramo de
/// `anexar_turno`; a evidencia em texto que o REPL anexa no modo `text` fica
/// fora daqui.
#[tokio::test]
async fn modos_text_e_off_nao_mandam_blocos_estruturados() {
    for modo in [Modo::Text, Modo::Off] {
        let mut s = sessao();
        turno(&mut s, "crie", modo).await;
        // Sem isto o teste passaria mesmo se nenhuma ferramenta rodasse.
        assert!(s.dir.path().join("pagina.html").exists(), "a tool escreveu");
        turno(&mut s, "voce criou o arquivo?", modo).await;
        assert_eq!(
            blocos(&s.provedor.ultimo_pedido()),
            Vec::new(),
            "modo {modo:?} nao leva ToolUse/ToolResult"
        );
    }
}

/// Criterio 5, recusa de permissao: com o modo `ask` no `ExecContext` o portao
/// de politica do runtime nega `file_write`. O REPL do `garra chat` nao tem
/// `/mode` (roda sem politica); o modo aqui e posto a mao para exercitar o
/// mesmo portao que os outros canais usam. A chamada nao roda, e o turno seguinte recebe o pedido e a
/// recusa pareados pelo id — o modelo sabe que pediu e que foi barrado, em
/// vez de achar que escreveu ou que nunca pediu.
#[tokio::test]
async fn recusa_de_permissao_chega_pareada_como_erro() {
    let mut s = sessao();
    let desfecho = turno_com(&mut s, "crie", Modo::Structured, Some("ask"), Fim::Normal).await;
    assert_eq!(desfecho, Desfecho::Concluido);
    assert!(
        !s.dir.path().join("pagina.html").exists(),
        "a recusa nao pode deixar a ferramenta rodar"
    );

    turno(&mut s, "voce criou o arquivo?", Modo::Structured).await;
    let b = blocos(&s.provedor.ultimo_pedido());
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!(b[0], Bloco::Uso("call-1".into(), "file_write".into()));
    let Bloco::Resultado(id, conteudo) = &b[1] else {
        panic!("esperava o resultado da recusa: {b:?}");
    };
    assert_eq!(id, "call-1");
    assert!(
        conteudo.starts_with("erro:"),
        "a recusa chega como falha, nunca como sucesso: {conteudo:?}"
    );
    assert!(!conteudo.contains("escreveu"), "{conteudo:?}");
}

/// Criterio 5, cancelamento e timeout: o turno cai com a ferramenta ainda
/// rodando. A chamada fica no historico como interrompida — nunca como
/// sucesso, nunca apagada —, e a resposta do turno e o marcador de
/// interrupcao, para o modelo do turno seguinte nao inventar um resultado.
#[tokio::test]
async fn cancelamento_e_timeout_deixam_a_chamada_como_interrompida() {
    for (fim, esperado, marcador) in [
        (
            Fim::CtrlC,
            Desfecho::Cancelado,
            "[turno interrompido: cancelado]",
        ),
        (
            Fim::Timeout,
            Desfecho::EstourouOTempo,
            "[turno interrompido: timeout]",
        ),
    ] {
        let mut s = sessao();
        let desfecho = turno_com(&mut s, "espere", Modo::Structured, None, fim).await;
        assert_eq!(desfecho, esperado, "{fim:?}");

        turno(&mut s, "a espera terminou?", Modo::Structured).await;
        let pedido = s.provedor.ultimo_pedido();
        assert_eq!(
            blocos(&pedido),
            vec![
                Bloco::Uso("call-1".into(), "espera".into()),
                Bloco::Resultado("call-1".into(), "[interrompido: sem resultado]".into()),
            ],
            "{fim:?}: a chamada interrompida segue pareada no turno seguinte"
        );
        assert!(
            pedido.iter().any(|m| matches!(
                m,
                ChatMessage { role: ChatRole::Assistant, content: MessagePart::Text(t) }
                    if t == marcador
            )),
            "{fim:?}: a resposta do turno interrompido e o marcador: {pedido:?}"
        );
    }
}
