//! O que o runtime **conta** sobre um turno (#937).
//!
//! Nasce da ADR 0017: o `UiEvent` e o `TerminalRenderer` moram no CLI, e o
//! runtime nao pode depender deles — senao o crate de agentes carregaria
//! apresentacao junto para todo mundo que o usa (gateway, canais, MCP).
//! Entao o runtime fala numa linguagem propria, [`TurnEvent`], e quem desenha
//! traduz.
//!
//! # Por que um enum, e nao um segundo canal
//!
//! Ate aqui o turno empurrava `mpsc::Sender<String>` — so texto. A alternativa
//! obvia para carregar ciclo de vida de ferramenta seria um segundo canal, so
//! para eventos. **Dois canais nao garantem ordem entre si**, e o agente
//! intercala texto e chamada de ferramenta no mesmo turno: o evento da tool
//! poderia ser desenhado depois do texto que veio depois dele. Um canal so,
//! com um enum, preserva a ordem de emissao.
//!
//! # E os sete chamadores que so querem texto
//!
//! Ficam intactos. O [`TurnSink`] aceita **ou** o `Sender<String>` de hoje
//! **ou** um `Sender<TurnEvent>`; num sink de texto, os eventos de ferramenta
//! sao descartados na origem — o Telegram nao tem o que fazer com "Bash
//! comecou". Quem quer o fluxo inteiro pede pelo
//! `process_message_streaming_with_events`.
//!
//! # Segredo nao sai daqui
//!
//! O resumo do input e o do output passam por [`garraia_security::redact_secrets`]
//! **antes** de virar evento. E deliberado que a redacao esteja na origem, e
//! nao no renderer: qualquer consumidor futuro (console web, outro canal)
//! herda a garantia sem precisar lembrar dela. Foi a licao do #948/#974 —
//! corrigir no ponto onde o dado nasce, nao onde ele e impresso.

use std::time::Duration;

use tokio::sync::mpsc;

/// Quanto de um resumo de ferramenta cabe numa linha de terminal.
const SUMMARY_MAX_CHARS: usize = 72;

/// Um acontecimento do turno, na linguagem de quem **observa** o agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// Um pedaco do texto da resposta, como saiu do modelo.
    TextDelta(String),

    /// Uma ferramenta comecou a rodar.
    ToolStarted {
        /// Nome registrado da ferramenta (`bash`, `file_read`, …).
        name: String,
        /// O que ela vai fazer, ja redigido e truncado — o `cargo test` de
        /// `Bash cargo test`. Vazio quando nao ha nada util a dizer.
        detail: String,
    },

    /// Uma ferramenta terminou.
    ToolFinished {
        name: String,
        /// Tempo de parede da execucao.
        duration: Duration,
        success: bool,
        /// Uma linha sobre o resultado, ja redigida e truncada. Em falha, o
        /// trecho mais relevante do erro.
        summary: String,
    },
}

/// Para onde o turno escreve.
///
/// `Text` e o que os sete chamadores de hoje usam e mantem o comportamento
/// exato de antes do #937. `Events` e o fluxo completo.
#[derive(Debug, Clone)]
pub enum TurnSink {
    Text(mpsc::Sender<String>),
    Events(mpsc::Sender<TurnEvent>),
}

impl TurnSink {
    /// Um pedaco de texto da resposta. Chega nos dois tipos de sink.
    pub async fn text(&self, delta: String) {
        match self {
            Self::Text(tx) => {
                let _ = tx.send(delta).await;
            }
            Self::Events(tx) => {
                let _ = tx.send(TurnEvent::TextDelta(delta)).await;
            }
        }
    }

    /// Ciclo de vida de ferramenta. **Descartado** num sink de texto — e o
    /// ponto: o canal do Telegram nao ganha ruido por causa de uma feature de
    /// terminal.
    pub async fn tool_started(&self, name: &str, detail: String) {
        if let Self::Events(tx) = self {
            let _ = tx
                .send(TurnEvent::ToolStarted {
                    name: name.to_string(),
                    detail,
                })
                .await;
        }
    }

    pub async fn tool_finished(
        &self,
        name: &str,
        duration: Duration,
        success: bool,
        summary: String,
    ) {
        if let Self::Events(tx) = self {
            let _ = tx
                .send(TurnEvent::ToolFinished {
                    name: name.to_string(),
                    duration,
                    success,
                    summary,
                })
                .await;
        }
    }

    /// Este sink se importa com eventos de ferramenta? O runtime usa para nao
    /// pagar o custo de montar resumo que ninguem vai ler.
    pub fn wants_tool_events(&self) -> bool {
        matches!(self, Self::Events(_))
    }
}

/// O que a ferramenta vai fazer, em uma linha.
///
/// Escolhe o campo que interessa por ferramenta — o `command` do bash, o
/// `path` de leitura e escrita, o `pattern` de busca — porque o schema de cada
/// uma e conhecimento **do crate de agentes**, nao do renderer. O renderer
/// recebe texto pronto e decide so como desenhar (ADR 0017).
///
/// Redige e trunca sempre. E ferramenta sem caso proprio devolve vazio, em
/// vez de adivinhar um campo: uma ferramenta nova nao pode vazar segredo so
/// por ainda nao ter caso proprio aqui.
///
/// O caminho de arquivo aparece **inteiro**, de proposito: `Read
/// ~/.ssh/id_rsa` e exatamente a informacao que a #937 quer dar ao operador —
/// saber o que o agente leu. O caminho nao e o segredo; o conteudo e, e ele
/// nao sai daqui.
pub fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    let bruto = match name {
        "bash" => campo(input, "command"),
        "file_read" | "file_write" => campo(input, "path"),
        "repo_search" | "web_search" => campo(input, "query").or_else(|| campo(input, "pattern")),
        "web_fetch" => campo(input, "url"),
        "git_diff" => campo(input, "path"),
        // Ferramenta sem caso proprio nao mostra nada.
        //
        // A versao anterior pegava o primeiro valor textual do input, e a
        // auditoria mostrou por que isso e ruim: `serde_json::Map` preserva a
        // ordem de insercao, entao uma ferramenta nova com
        // `{"connection_string": "...", "query": "..."}` exibiria a connection
        // string. "Ferramenta nova nao pode vazar segredo so por ainda nao ter
        // caso proprio aqui" era a regra que eu mesmo escrevi — e adivinhar o
        // campo a violava. Ferramenta nova aparece so com o nome ate alguem
        // dizer qual campo dela e o interessante.
        _ => None,
    };

    match bruto {
        Some(texto) => sanear(&texto),
        None => String::new(),
    }
}

/// Uma linha sobre o que a ferramenta devolveu.
///
/// Em sucesso: a primeira linha nao-vazia, ou a contagem de linhas quando a
/// saida e longa — "nao despejar output grande por padrao" e criterio de
/// aceite da #937. Em falha: a primeira linha nao-vazia, que e onde as
/// ferramentas deste projeto poem a causa.
pub fn summarize_tool_output(content: &str, success: bool) -> String {
    let linhas: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    let Some(primeira) = linhas.first() else {
        return String::new();
    };

    // Em falha o que importa e a causa, mesmo que a saida seja longa.
    if success && linhas.len() > 1 {
        let resumo = sanear(primeira);
        let restantes = linhas.len() - 1;
        return format!("{resumo} (+{restantes} linha(s))");
    }

    sanear(primeira)
}

fn campo(input: &serde_json::Value, chave: &str) -> Option<String> {
    input
        .get(chave)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Redige segredo, tira controle de terminal e trunca — nessa ordem, sempre.
///
/// Truncar antes de redigir poderia cortar uma chave ao meio e deixar o
/// prefixo passar pelo regex sem casar. E truncar antes de tirar os controles
/// poderia deixar meia sequencia passar pela mesma razao.
fn sanear(texto: &str) -> String {
    let redigido = garraia_security::redact_secrets(texto.trim());
    let sem_controle = sanear_controles(&redigido);
    truncar(&sem_controle, SUMMARY_MAX_CHARS)
}

/// Tira do texto tudo que o terminal interpretaria como comando (#995).
///
/// Saida de ferramenta **nao e conteudo confiavel**: e o que o agente leu de
/// um arquivo, baixou de uma pagina ou o que um comando escreveu. Ate o #995
/// esse texto ia direto para o terminal, e um `README` de repositorio clonado
/// conseguia limpar a tela do usuario com `\x1b[2J`, trocar o titulo da janela
/// com OSC, ou reposicionar o cursor para sobrescrever linhas ja impressas —
/// forjando texto que parece ter vindo do proprio Garra.
///
/// O caso mais pontudo era `\x1b[?25l`: o projeto tem invariante explicita de
/// que essa sequencia nao e emitida em lugar nenhum, para que nenhum caminho
/// de saida deixe o terminal sem cursor. Saida de ferramenta a violava.
///
/// # A seguranca nao depende do reconhecimento de sequencia
///
/// Esta e a parte que importa entender antes de mexer aqui. **Tirar o `ESC` ja
/// neutraliza**: sem ele, `[2J` e texto inerte, que o terminal imprime como
/// tres caracteres em vez de executar. Todo controle C0, C1 e DEL sai, sem
/// excecao — e essa regra por caractere, nao por padrao, e o que fecha o
/// buraco. Reconhecer "sequencias ANSI" com regex e um jogo que se perde: ha
/// CSI, OSC, DCS, formas de dois caracteres, com e sem terminador.
///
/// O reconhecimento que existe abaixo serve so para **legibilidade**. Saida
/// colorida e comum e legitima — `cargo test` emite `\x1b[32mok\x1b[0m` —, e
/// trocar so o `ESC` por um marcador deixaria `\u{fffd}[32mok\u{fffd}[0m` na
/// tela, que e ruido para o caso normal. Entao quando a sequencia e
/// reconhecivel ela sai inteira, e o resultado fica limpo.
///
/// A consequencia de o reconhecimento errar e **cosmetica, nao de seguranca**:
/// uma sequencia exotica que o parser nao entenda perde o `ESC` do mesmo jeito
/// e sobra como texto literal feio. E o modo de falhar que se quer.
fn sanear_controles(texto: &str) -> String {
    let mut out = String::with_capacity(texto.len());
    let mut chars = texto.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Separadores viram espaco em vez de sumir: num resumo de uma
            // linha eles separam palavras, e apaga-los grudaria tokens que o
            // usuario precisa ler separados.
            '\t' | '\n' | '\r' => out.push(' '),

            '\u{1b}' => match chars.peek() {
                // CSI: `ESC [` ... byte final na faixa 0x40-0x7e. Cobre cor,
                // movimento de cursor, limpeza de tela, `?25l`.
                Some('[') => {
                    chars.next();
                    for f in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&f) {
                            break;
                        }
                    }
                }
                // OSC: `ESC ]` ... BEL ou ST (`ESC \`). E onde mora a troca de
                // titulo de janela e o hyperlink.
                Some(']') => {
                    chars.next();
                    while let Some(f) = chars.next() {
                        if f == '\u{7}' {
                            break;
                        }
                        if f == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Sequencia de dois caracteres (`ESC c` reseta o terminal) ou
                // um `ESC` solto no fim do texto. Em ambos o `ESC` sai.
                Some(_) => {
                    chars.next();
                }
                None => {}
            },

            // Todo o resto que o terminal trataria como comando. Marcar em vez
            // de apagar deixa visivel que algo foi removido — apagar em
            // silencio faria a tentativa de injecao parecer texto normal.
            c if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) => out.push('\u{fffd}'),

            c => out.push(c),
        }
    }

    out
}

/// Corta por **caractere**, nunca por byte — comando e caminho carregam
/// acento, e cortar no meio de um `ç` produz bytes invalidos.
fn truncar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        return texto.to_string();
    }
    let mut out: String = texto.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Saida de ferramenta nao pode escrever comando no terminal (#995).
    ///
    /// Os tres casos sao os do mundo real: limpar a tela, esconder o cursor
    /// (que viola invariante explicita do projeto) e trocar o titulo da
    /// janela por OSC.
    #[test]
    fn saida_nao_injeta_sequencia_no_terminal() {
        for (bruto, o_que_e) in [
            ("\u{1b}[2Jlimpou", "limpa a tela"),
            ("\u{1b}[?25lescondeu", "esconde o cursor"),
            (
                "\u{1b}]0;titulo novo\u{7}trocou",
                "troca o titulo (OSC/BEL)",
            ),
            ("\u{1b}]0;titulo\u{1b}\\trocou", "troca o titulo (OSC/ST)"),
            ("\u{1b}creset", "reseta o terminal"),
        ] {
            let saida = summarize_tool_output(bruto, true);
            assert!(
                !saida.contains('\u{1b}'),
                "ESC sobreviveu ao resumo ({o_que_e}): {saida:?}"
            );
            assert!(
                !saida.chars().any(|c| c.is_control()),
                "controle sobreviveu ao resumo ({o_que_e}): {saida:?}"
            );
        }
    }

    /// O mesmo vale para o input — o `command` do bash tambem vem de fora.
    #[test]
    fn input_nao_injeta_sequencia_no_terminal() {
        let input = json!({ "command": "echo \u{1b}[2J" });
        let saida = summarize_tool_input("bash", &input);
        assert!(!saida.contains('\u{1b}'), "ESC sobreviveu: {saida:?}");
    }

    /// Saida colorida e comum e legitima. O reconhecimento de sequencia existe
    /// para que o caso normal fique limpo em vez de virar `\u{fffd}[32m`.
    #[test]
    fn saida_colorida_fica_legivel() {
        let saida = summarize_tool_output("\u{1b}[32mtest result: ok\u{1b}[0m", true);
        assert_eq!(saida, "test result: ok");
    }

    /// Um controle que nao faz parte de sequencia reconhecivel vira marcador
    /// visivel, e nao sumico silencioso: a tentativa de injecao tem de
    /// aparecer.
    #[test]
    fn controle_solto_vira_marcador_visivel() {
        let saida = summarize_tool_output("antes\u{0}depois", true);
        assert_eq!(saida, "antes\u{fffd}depois");
    }

    /// Sanear nao pode quebrar texto legitimo com acento.
    #[test]
    fn acento_atravessa_intacto() {
        let saida = summarize_tool_output("compilação concluída com açúcar", true);
        assert_eq!(saida, "compilação concluída com açúcar");
    }

    /// A ordem importa: redigir, sanear, truncar. Um segredo seguido de ESC
    /// tem de sair redigido **e** sem o controle.
    #[test]
    fn segredo_e_controle_saem_os_dois() {
        let saida = summarize_tool_output("sk-ant-api03-aaaaaaaaaaaaaaaaaaaa\u{1b}[2J", true);
        assert!(!saida.contains('\u{1b}'), "ESC sobreviveu: {saida:?}");
        assert!(
            !saida.contains("sk-ant-api03-aaaaaaaaaaaaaaaaaaaa"),
            "segredo sobreviveu: {saida:?}"
        );
    }

    #[test]
    fn resume_o_comando_do_bash() {
        let input = json!({ "command": "cargo test --workspace" });
        assert_eq!(
            summarize_tool_input("bash", &input),
            "cargo test --workspace"
        );
    }

    #[test]
    fn resume_o_caminho_das_ferramentas_de_arquivo() {
        let input = json!({ "path": "crates/garraia-cli/src/chat.rs" });
        assert_eq!(
            summarize_tool_input("file_read", &input),
            "crates/garraia-cli/src/chat.rs"
        );
        assert_eq!(
            summarize_tool_input("file_write", &input),
            "crates/garraia-cli/src/chat.rs"
        );
    }

    /// Ferramenta sem caso proprio aparece so com o nome — nunca adivinhando
    /// um campo do input que pode ser o segredo.
    #[test]
    fn ferramenta_desconhecida_nao_adivinha_campo() {
        let input = json!({
            "connection_string": "postgres://admin:s3cr3tpass@host/db",
            "query": "SELECT 1"
        });
        assert_eq!(summarize_tool_input("ferramenta_nova", &input), "");
    }

    #[test]
    fn input_sem_nada_util_vira_vazio() {
        assert_eq!(summarize_tool_input("bash", &json!({})), "");
        assert_eq!(summarize_tool_input("bash", &json!(42)), "");
    }

    /// O criterio de aceite da #937 sobre nao vazar segredo de argumento.
    #[test]
    fn segredo_no_comando_e_redigido_na_origem() {
        let chave = format!("sk-{}", "a".repeat(40));
        let input = json!({ "command": format!("curl -H 'auth: {chave}' https://x") });
        let resumo = summarize_tool_input("bash", &input);
        assert!(!resumo.contains(&chave), "chave vazou: {resumo}");
        assert!(resumo.contains("[REDACTED]"), "{resumo}");
    }

    #[test]
    fn segredo_na_saida_tambem_e_redigido() {
        let chave = format!("xoxb-{}", "b".repeat(30));
        let resumo = summarize_tool_output(&format!("token={chave}"), true);
        assert!(!resumo.contains(&chave), "chave vazou: {resumo}");
    }

    /// Truncar ANTES de redigir cortaria a chave e deixaria o prefixo passar.
    #[test]
    fn redige_antes_de_truncar() {
        let chave = format!("sk-{}", "c".repeat(120));
        let resumo = summarize_tool_input("bash", &json!({ "command": chave.clone() }));
        assert!(
            !resumo.contains("sk-cccc"),
            "prefixo de chave sobreviveu: {resumo}"
        );
    }

    #[test]
    fn resumo_longo_e_truncado_sem_quebrar_caractere() {
        let longo = "ç".repeat(200);
        let resumo = summarize_tool_input("bash", &json!({ "command": longo }));
        assert!(resumo.chars().count() <= SUMMARY_MAX_CHARS);
        assert!(resumo.ends_with('…'));
    }

    #[test]
    fn saida_de_uma_linha_sai_inteira() {
        assert_eq!(summarize_tool_output("148 passed", true), "148 passed");
    }

    /// "Long tool output is not dumped by default" — criterio de aceite.
    #[test]
    fn saida_longa_vira_primeira_linha_mais_contagem() {
        let saida = "primeira\nsegunda\nterceira\n";
        assert_eq!(summarize_tool_output(saida, true), "primeira (+2 linha(s))");
    }

    /// Em falha o que importa e a causa, nao a contagem.
    #[test]
    fn falha_mostra_a_primeira_linha_do_erro() {
        let saida = "error: exit 101\nstack trace...\nmais ruido";
        assert_eq!(summarize_tool_output(saida, false), "error: exit 101");
    }

    #[test]
    fn saida_vazia_vira_resumo_vazio() {
        assert_eq!(summarize_tool_output("", true), "");
        assert_eq!(summarize_tool_output("\n\n  \n", true), "");
    }

    #[tokio::test]
    async fn sink_de_texto_descarta_evento_de_ferramenta() {
        let (tx, mut rx) = mpsc::channel::<String>(8);
        let sink = TurnSink::Text(tx);
        assert!(!sink.wants_tool_events());

        sink.tool_started("bash", "cargo test".into()).await;
        sink.text("oi".into()).await;
        sink.tool_finished("bash", Duration::from_secs(1), true, "ok".into())
            .await;
        drop(sink);

        let mut recebidos = Vec::new();
        while let Some(t) = rx.recv().await {
            recebidos.push(t);
        }
        assert_eq!(recebidos, vec!["oi".to_string()]);
    }

    #[tokio::test]
    async fn sink_de_eventos_preserva_a_ordem_de_emissao() {
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(8);
        let sink = TurnSink::Events(tx);
        assert!(sink.wants_tool_events());

        sink.text("antes ".into()).await;
        sink.tool_started("bash", "cargo test".into()).await;
        sink.tool_finished(
            "bash",
            Duration::from_millis(1500),
            true,
            "148 passed".into(),
        )
        .await;
        sink.text("depois".into()).await;
        drop(sink);

        let mut recebidos = Vec::new();
        while let Some(e) = rx.recv().await {
            recebidos.push(e);
        }
        assert_eq!(
            recebidos,
            vec![
                TurnEvent::TextDelta("antes ".into()),
                TurnEvent::ToolStarted {
                    name: "bash".into(),
                    detail: "cargo test".into()
                },
                TurnEvent::ToolFinished {
                    name: "bash".into(),
                    duration: Duration::from_millis(1500),
                    success: true,
                    summary: "148 passed".into()
                },
                TurnEvent::TextDelta("depois".into()),
            ]
        );
    }
}
