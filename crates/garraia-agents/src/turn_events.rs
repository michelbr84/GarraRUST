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

/// Teto por saida capturada para inspecao posterior (#938).
///
/// Nao e limite de exibicao — e limite de **memoria**: e o que impede um
/// `find /` de morar no processo do chat. Generoso o bastante para um
/// `cargo test` inteiro (que e o caso de uso citado na issue) e pequeno o
/// bastante para que vinte deles nao pesem.
const CAPTURE_MAX_BYTES: usize = 64 * 1024;

/// Do teto, quanto fica do **comeco**. O resto fica do fim.
///
/// O fim leva a maior parte de proposito: em falha de compilacao ou de teste,
/// o que importa esta no fim. Mas o comeco nao pode ir a zero — e la que mora
/// o comando que gerou a saida e as primeiras linhas de erro de um build.
const CAPTURE_HEAD_BYTES: usize = 16 * 1024;

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
        /// A saida inteira, ja redigida, saneada e capada por
        /// [`capture_tool_output`] — o que o `/tool` do CLI mostra (#938).
        ///
        /// Vive no evento, e nao num canal a parte, para nao repetir o erro
        /// que o docblock deste modulo descreve: dois canais nao garantem
        /// ordem entre si, e a saida tem de chegar junto com o resultado a
        /// que ela pertence.
        output: String,
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
        output: String,
    ) {
        if let Self::Events(tx) = self {
            let _ = tx
                .send(TurnEvent::ToolFinished {
                    name: name.to_string(),
                    duration,
                    success,
                    summary,
                    output,
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
    // Sanear **antes** de classificar, e nao depois. Saida de terminal costuma
    // vir colorida: `cargo test` emite `\x1b[32mtest result: ok...`, e um
    // reconhecedor rodando no texto cru procuraria "test result:" numa linha
    // que comeca com escape e nao acharia. O caso comum quebraria em silencio,
    // caindo no resumo generico sem nada indicando por que.
    let limpo = sanear_controles(&garraia_security::redact_secrets(content), true);

    let linhas: Vec<&str> = limpo.lines().filter(|l| !l.trim().is_empty()).collect();

    let Some(primeira) = linhas.first() else {
        return String::new();
    };

    // Uma classe reconhecida ganha resumo proprio; o resto segue no
    // comportamento generico. Ferramenta ou formato sem caso escrito nao e
    // adivinhado — mesma regra do `summarize_tool_input`, e pela mesma razao:
    // adivinhar e como o resumo do input chegou a exibir connection string.
    if let Some(resumo) = resumo_por_classe(&linhas, success) {
        return truncar(&resumo, SUMMARY_MAX_CHARS);
    }

    // Em falha o que importa e a causa. A primeira linha nao-vazia costuma ser
    // ruido de progresso ("Compiling ..."), entao procura-se antes uma linha
    // que se parece com erro.
    if !success {
        if let Some(erro) = linhas.iter().find(|l| parece_erro(l)) {
            return sanear(erro);
        }
        return sanear(primeira);
    }

    if linhas.len() > 1 {
        let resumo = sanear(primeira);
        let restantes = linhas.len() - 1;
        return format!("{resumo} (+{restantes} linha(s))");
    }

    sanear(primeira)
}

/// Uma linha que anuncia falha, em vez de progresso.
///
/// Deliberadamente simples e ancorado no **inicio** da linha: procurar a
/// palavra em qualquer posicao faria `Compiling error-chain v0.12` — um nome
/// de crate — passar por erro.
fn parece_erro(linha: &str) -> bool {
    let l = linha.trim_start();
    l.starts_with("error")
        || l.starts_with("Error")
        || l.starts_with("ERROR")
        || l.starts_with("fatal:")
        || l.starts_with("panicked at")
        || l.starts_with("thread '")
        || l.starts_with("FAILED")
}

/// Resumo especifico quando a **forma da saida** e reconhecida (#938).
///
/// A classificacao olha o formato do texto, e nao o comando que o gerou. E de
/// proposito: a mesma contagem de teste sai de `cargo test`, de `cargo
/// nextest` e de um `make test` que embrulhe qualquer um dos dois, e o
/// comando nem chega ate aqui. A forma e o sinal honesto.
///
/// Devolve `None` quando nada e reconhecido — e o chamador segue no generico.
fn resumo_por_classe(linhas: &[&str], success: bool) -> Option<String> {
    if let Some(r) = resumo_de_teste(linhas) {
        return Some(r);
    }
    if let Some(r) = resumo_de_compilacao(linhas, success) {
        return Some(r);
    }
    resumo_de_diffstat(linhas)
}

/// `test result: ok. 148 passed; 0 failed; ...`
///
/// Soma os binarios: um `cargo test --workspace` emite uma linha dessas por
/// alvo, e "148 passed" espalhado em 54 linhas nao e resposta.
fn resumo_de_teste(linhas: &[&str]) -> Option<String> {
    let mut passou = 0usize;
    let mut falhou = 0usize;
    let mut ignorou = 0usize;
    // Achar a **linha** nao basta: e preciso ter lido ao menos um numero dela.
    // Sem isso, um texto que so contenha "test result: ok" viraria "0 passou",
    // que afirma que nenhum teste passou quando o que houve foi nao conseguir
    // parsear. Uma corrida legitima de zero testes ("0 passed; 0 failed") le
    // os numeros normalmente e continua funcionando.
    let mut leu_numero = false;

    for l in linhas {
        let Some(resto) = l.trim_start().strip_prefix("test result:") else {
            continue;
        };
        for (palavra, alvo) in [
            ("passed", &mut passou),
            ("failed", &mut falhou),
            ("ignored", &mut ignorou),
        ] {
            if let Some(n) = contagem_antes_de(resto, palavra) {
                *alvo += n;
                leu_numero = true;
            }
        }
    }

    if !leu_numero {
        return None;
    }

    // A falha vem primeiro quando existe: e a informacao que muda o que a
    // pessoa faz em seguida.
    let mut partes = Vec::new();
    if falhou > 0 {
        partes.push(format!("{falhou} falhou"));
    }
    partes.push(format!("{passou} passou"));
    if ignorou > 0 {
        partes.push(format!("{ignorou} ignorado"));
    }
    Some(partes.join(", "))
}

/// O numero imediatamente antes de uma palavra, em `"... 148 passed; ..."`.
fn contagem_antes_de(texto: &str, palavra: &str) -> Option<usize> {
    let tokens: Vec<&str> = texto.split_whitespace().collect();
    let i = tokens
        .iter()
        .position(|t| t.trim_end_matches(';') == palavra)?;
    tokens.get(i.checked_sub(1)?)?.parse().ok()
}

/// Saida de compilador: conta erro e aviso, e em falha mostra o primeiro erro.
fn resumo_de_compilacao(linhas: &[&str], success: bool) -> Option<String> {
    let erros = linhas.iter().filter(|l| eh_linha_de_erro(l)).count();
    let avisos = linhas
        .iter()
        .filter(|l| l.trim_start().starts_with("warning:"))
        .count();

    if erros == 0 && avisos == 0 {
        return None;
    }

    if !success && erros > 0 {
        // O primeiro erro **e** o resumo: e a "concise relevant excerpt" que a
        // #938 pede, e vale muito mais que "1 erro".
        let primeiro = linhas.iter().find(|l| eh_linha_de_erro(l))?;
        let texto = primeiro.trim();
        return Some(if erros > 1 {
            format!("{texto} (+{} erro(s))", erros - 1)
        } else {
            texto.to_string()
        });
    }

    let mut partes = Vec::new();
    if erros > 0 {
        partes.push(format!("{erros} erro(s)"));
    }
    if avisos > 0 {
        partes.push(format!("{avisos} aviso(s)"));
    }
    Some(partes.join(", "))
}

/// `error: msg` ou `error[E0382]: msg`, ancorado no inicio da linha.
///
/// A linha-resumo do cargo (`error: could not compile ... due to N previous
/// errors`) fica de fora: conta-la dobraria o numero de erros de todo build
/// que falha.
fn eh_linha_de_erro(linha: &str) -> bool {
    let l = linha.trim_start();
    if !l.starts_with("error") {
        return false;
    }
    // As duas frases de fecho, porque cargo e rustc escrevem diferente. Contar
    // qualquer uma somaria um erro inexistente a todo build que falha.
    //
    // Eu so tinha excluido a do cargo. Foi rodar o resumo contra a saida real
    // de um `rustc` que o erro apareceu: um unico `E0382` virava "(+1
    // erro(s))". Saida sintetica nao teria pego — eu a escrevia com a frase
    // que ja conhecia.
    if l.starts_with("error: could not compile") || l.starts_with("error: aborting due to") {
        return false;
    }
    l.starts_with("error:") || l.starts_with("error[")
}

/// `3 files changed, 42 insertions(+), 7 deletions(-)` — git diff/commit.
fn resumo_de_diffstat(linhas: &[&str]) -> Option<String> {
    let l = linhas
        .iter()
        .rev()
        .find(|l| l.contains("file changed") || l.contains("files changed"))?;
    Some(l.trim().to_string())
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
    let sem_controle = sanear_controles(&redigido, false);
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
fn sanear_controles(texto: &str, preservar_quebras: bool) -> String {
    let mut out = String::with_capacity(texto.len());
    let mut chars = texto.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Na saida completa a quebra e a tabulacao sao a estrutura do
            // texto e ficam. No resumo de uma linha viram espaco, em vez de
            // sumir: elas separam palavras, e apaga-las grudaria tokens que o
            // usuario precisa ler separados.
            '\n' | '\t' if preservar_quebras => out.push(c),
            '\t' | '\n' => out.push(' '),

            // O `\r` nunca sobrevive: sozinho ele devolve o cursor ao inicio da
            // linha, que e a primitiva de sobrescrever texto ja impresso que o
            // #995 fechou.
            //
            // Mas na saida completa ele vira **quebra de linha**, e nao sumico.
            // A auditoria do #938 deu o caso que decide isso: uma barra de
            // progresso emite `10%\r20%\r100%`, e apaga-lo produzia
            // `10%20%100%` — um amontoado que o leitor nao distingue de uma
            // saida que era assim mesmo. Como quebra, cada estado vira sua
            // linha, que e o que o programa quis desenhar.
            //
            // O `\r\n` legitimo continua virando um `\n` so: o `\r` e engolido
            // porque o `\n` seguinte ja faz o trabalho.
            '\r' if preservar_quebras => {
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                out.push('\n');
            }
            '\r' => out.push(' '),

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

/// A saida da ferramenta inteira, pronta para ser guardada e relida (#938).
///
/// Passa pelas **mesmas** garantias do resumo — redige segredo, tira controle
/// de terminal — porque a inspecao mostra mais texto, nao texto menos seguro.
/// Era um criterio de aceite explicito da #938 ("secret redaction applies to
/// both summaries and full output") e teria sido facil de errar: o resumo ja
/// era seguro, e daria para achar que a saida crua tambem era.
///
/// A diferenca para o resumo e a forma, nao a protecao: aqui a quebra de linha
/// e a tabulacao sobrevivem, porque sao a estrutura do texto que o usuario
/// pediu para ler. O `\r` continua saindo — ver [`sanear_controles`].
///
/// # Por que capar aqui, e nao no CLI
///
/// O cap acontece **antes** de virar evento, na origem, pelo mesmo motivo que
/// a redacao acontece aqui: quem guarda nao precisa lembrar do limite. Se o
/// cap morasse no CLI, a saida inteira de um `find /` ja teria atravessado o
/// canal e existido em memoria antes de alguem a cortar.
///
/// O corte preserva **comeco e fim**, com um marcador dizendo quanto sumiu.
/// Cortar so o fim perderia a causa de uma falha de teste; cortar so o comeco
/// perderia o que estava sendo compilado.
pub fn capture_tool_output(content: &str) -> String {
    let redigido = garraia_security::redact_secrets(content);
    let limpo = sanear_controles(&redigido, true);

    if limpo.len() <= CAPTURE_MAX_BYTES {
        return limpo;
    }

    // Fatiar `String` por byte quebra UTF-8 no meio de um caractere. Os dois
    // limites andam para tras ate uma fronteira de caractere — no pior caso
    // tres bytes, o que nao muda nada de util e evita um panic.
    let fim_do_comeco = fronteira_para_tras(&limpo, CAPTURE_HEAD_BYTES);
    let tamanho_do_fim = CAPTURE_MAX_BYTES - CAPTURE_HEAD_BYTES;
    let inicio_do_fim = fronteira_para_frente(&limpo, limpo.len() - tamanho_do_fim);

    let omitidos = inicio_do_fim - fim_do_comeco;
    format!(
        "{}\n\n[... {omitidos} bytes omitidos — a saida passou de {} KiB ...]\n\n{}",
        &limpo[..fim_do_comeco],
        CAPTURE_MAX_BYTES / 1024,
        &limpo[inicio_do_fim..],
    )
}

/// Recua ate uma fronteira de caractere, para poder fatiar sem panic.
fn fronteira_para_tras(texto: &str, mut i: usize) -> usize {
    while i > 0 && !texto.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Avanca ate uma fronteira de caractere.
fn fronteira_para_frente(texto: &str, mut i: usize) -> usize {
    while i < texto.len() && !texto.is_char_boundary(i) {
        i += 1;
    }
    i
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
        let saida = summarize_tool_output("\u{1b}[32mtudo certo por aqui\u{1b}[0m", true);
        assert_eq!(saida, "tudo certo por aqui");
    }

    /// Uma linha de resultado sem numero legivel nao pode virar "0 passou" —
    /// isso afirmaria que nenhum teste passou, quando o que houve foi nao
    /// conseguir parsear. Cai no generico.
    #[test]
    fn resultado_de_teste_sem_numero_nao_inventa_zero() {
        let saida = summarize_tool_output("test result: ok", true);
        assert_eq!(saida, "test result: ok");
    }

    /// Mas uma corrida legitima de zero testes le os numeros e reporta certo.
    #[test]
    fn zero_testes_de_verdade_e_reportado_como_zero() {
        let saida = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s";
        assert_eq!(summarize_tool_output(saida, true), "0 passou");
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

    /// A saida completa passa pelas MESMAS garantias do resumo. Era criterio
    /// de aceite explicito da #938 e daria para errar: o resumo ja era seguro,
    /// e dava para achar que a saida crua tambem era.
    #[test]
    fn saida_completa_redige_segredo_e_saneia_controle() {
        let bruto = "linha 1\nsk-ant-api03-aaaaaaaaaaaaaaaaaaaa\n\u{1b}[2Jlinha 3";
        let capturado = capture_tool_output(bruto);
        assert!(
            !capturado.contains('\u{1b}'),
            "ESC sobreviveu: {capturado:?}"
        );
        assert!(
            !capturado.contains("sk-ant-api03-aaaaaaaaaaaaaaaaaaaa"),
            "segredo sobreviveu: {capturado:?}"
        );
    }

    /// Diferente do resumo, a saida completa preserva a estrutura do texto —
    /// e para le-la que o usuario pediu.
    #[test]
    fn saida_completa_preserva_quebra_de_linha() {
        let capturado = capture_tool_output("um\ndois\ntres");
        assert_eq!(capturado, "um\ndois\ntres");
    }

    /// O `\r` sai mesmo na saida completa: sozinho ele devolve o cursor ao
    /// inicio da linha, que e a primitiva de sobrescrever texto ja impresso.
    /// O `\r` sai, mas vira quebra em vez de sumir.
    ///
    /// A auditoria do #938 apontou o caso que decide: apagando, uma barra de
    /// progresso vira `10%20%100%`, um amontoado indistinguivel de uma saida
    /// que era assim mesmo. Como quebra, cada estado vira sua linha.
    #[test]
    fn retorno_de_carro_vira_quebra_e_nao_sumico() {
        let capturado = capture_tool_output("10%\r20%\r100%");
        assert!(!capturado.contains('\r'), "CR sobreviveu: {capturado:?}");
        assert_eq!(capturado, "10%\n20%\n100%");

        // O `\r\n` legitimo vira UM `\n`, nao dois — senao toda saida de
        // Windows viria com o dobro das linhas.
        assert_eq!(capture_tool_output("um\r\ndois"), "um\ndois");
        assert_eq!(capture_tool_output("a\r\nb\r\nc"), "a\nb\nc");
    }

    /// O corte preserva comeco E fim: so o fim perderia o que estava sendo
    /// compilado, so o comeco perderia a causa da falha.
    #[test]
    fn saida_gigante_e_cortada_no_meio_preservando_as_pontas() {
        let gigante = format!(
            "COMECO\n{}\nFIM-DO-ERRO",
            "linha de ruido\n".repeat(CAPTURE_MAX_BYTES / 10)
        );
        let capturado = capture_tool_output(&gigante);

        assert!(capturado.starts_with("COMECO"), "perdeu o comeco");
        assert!(capturado.ends_with("FIM-DO-ERRO"), "perdeu o fim");
        assert!(capturado.contains("bytes omitidos"), "nao marcou o corte");
        // Cabe no teto, com folga para o marcador.
        assert!(
            capturado.len() < CAPTURE_MAX_BYTES + 200,
            "passou do teto: {}",
            capturado.len()
        );
    }

    /// Cortar `String` por byte no meio de um caractere multibyte causaria
    /// panic. O corte anda ate a fronteira.
    #[test]
    fn corte_nao_quebra_caractere_multibyte() {
        // Enche com um caractere de 2 bytes para que o limite caia no meio.
        let gigante = "ç".repeat(CAPTURE_MAX_BYTES);
        let capturado = capture_tool_output(&gigante);
        assert!(capturado.contains("bytes omitidos"));
        // Se tivesse quebrado, o `String` nem existiria — chegar aqui ja prova.
        assert!(capturado.starts_with('ç'));
    }

    /// O backspace apaga o caractere anterior no terminal — e outra forma de
    /// reescrever o que ja foi impresso, como o `\r`. Junto com tabulacao
    /// vertical e form feed, que tambem movem o cursor.
    #[test]
    fn saida_completa_neutraliza_backspace_e_movimento_vertical() {
        for (bruto, o_que_e) in [
            (
                "senha: xxx\u{8}\u{8}\u{8}ok",
                "backspace apaga o que foi escrito",
            ),
            ("um\u{b}dois", "tabulacao vertical"),
            ("um\u{c}dois", "form feed"),
        ] {
            let capturado = capture_tool_output(bruto);
            assert!(
                !capturado
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'),
                "controle sobreviveu ({o_que_e}): {capturado:?}"
            );
        }
    }

    /// As bordas exatas do corte. Um byte a menos que o teto passa inteiro;
    /// um a mais entra no caminho do corte — e e la que mora a aritmetica que
    /// poderia dar underflow.
    #[test]
    fn as_bordas_do_corte_nao_estouram() {
        for n in [
            CAPTURE_MAX_BYTES - 1,
            CAPTURE_MAX_BYTES,
            CAPTURE_MAX_BYTES + 1,
            CAPTURE_MAX_BYTES + CAPTURE_HEAD_BYTES,
        ] {
            let capturado = capture_tool_output(&"a".repeat(n));
            if n <= CAPTURE_MAX_BYTES {
                assert_eq!(capturado.len(), n, "cabia no teto e foi cortado (n={n})");
            } else {
                assert!(
                    capturado.contains("bytes omitidos"),
                    "passou do teto e nao foi cortado (n={n})"
                );
            }
        }
    }

    /// Saida que cabe no teto atravessa inteira, sem marcador.
    #[test]
    fn saida_pequena_nao_ganha_marcador() {
        let capturado = capture_tool_output("tudo certo");
        assert_eq!(capturado, "tudo certo");
    }

    /// O caso que a #938 usa de exemplo: `cargo test` cuja primeira linha
    /// e "Compiling", que nao diz nada sobre o resultado.
    #[test]
    fn conta_os_testes_em_vez_de_mostrar_o_compiling() {
        let saida = "   Compiling garraia-agents v0.3.9\n    Finished test profile\n     Running unittests\ntest result: ok. 148 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.30s";
        assert_eq!(summarize_tool_output(saida, true), "148 passou");
    }

    /// `cargo test --workspace` emite uma linha por binario. "148 passou"
    /// espalhado em 54 linhas nao e resposta — soma.
    #[test]
    fn soma_os_binarios_de_teste() {
        let saida = "test result: ok. 100 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1s\n                     test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2s";
        assert_eq!(summarize_tool_output(saida, true), "148 passou, 2 ignorado");
    }

    /// Falha vem primeiro: e o que muda o que a pessoa faz em seguida.
    #[test]
    fn falha_de_teste_aparece_antes_do_que_passou() {
        let saida = "test result: FAILED. 145 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6s";
        assert_eq!(summarize_tool_output(saida, false), "3 falhou, 145 passou");
    }

    /// Saida colorida e o caso comum num terminal. Se a classificacao rodasse
    /// no texto cru, o escape antes de "test result:" faria o reconhecedor
    /// falhar em silencio e cair no resumo generico.
    #[test]
    fn reconhece_mesmo_com_saida_colorida() {
        let saida = "\u{1b}[32mtest result\u{1b}[0m: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1s";
        assert_eq!(summarize_tool_output(saida, true), "12 passou");
    }

    /// O exemplo literal da issue: em falha de compilacao, o primeiro erro E o
    /// resumo — vale muito mais que "1 erro".
    #[test]
    fn falha_de_compilacao_mostra_o_primeiro_erro() {
        let saida = "   Compiling garraia v0.3.9\n                     error[E0382]: borrow of moved value `config`\n                     error: could not compile `garraia` (lib) due to 1 previous error";
        assert_eq!(
            summarize_tool_output(saida, false),
            "error[E0382]: borrow of moved value `config`"
        );
    }

    /// Mais de um erro: mostra o primeiro e diz quantos faltam.
    #[test]
    fn varios_erros_mostram_o_primeiro_e_a_contagem() {
        let saida = "error[E0382]: primeiro\nerror[E0433]: segundo\nerror[E0599]: terceiro";
        assert_eq!(
            summarize_tool_output(saida, false),
            "error[E0382]: primeiro (+2 erro(s))"
        );
    }

    /// As linhas de fecho nao contam como erro — senao todo build que falha
    /// teria um erro a mais do que tem.
    ///
    /// Sao duas frases porque cargo e rustc escrevem diferente. A do rustc so
    /// apareceu ao rodar o resumo contra saida real: com saida sintetica eu
    /// escrevia a frase que ja conhecia, e o teste passava sem cobrir o caso.
    #[test]
    fn as_linhas_de_fecho_nao_contam_como_erro() {
        for fecho in [
            "error: could not compile `x` (lib) due to 1 previous error",
            "error: aborting due to 1 previous error",
        ] {
            let saida = format!("error[E0382]: unico\n{fecho}");
            let resumo = summarize_tool_output(&saida, false);
            assert_eq!(
                resumo, "error[E0382]: unico",
                "contou a linha de fecho {fecho:?}"
            );
        }
    }

    /// Nome de crate com "error" dentro nao vira erro.
    #[test]
    fn nome_de_crate_com_error_nao_vira_erro() {
        let saida = "   Compiling error-chain v0.12.4\n    Finished dev profile";
        let resumo = summarize_tool_output(saida, true);
        assert!(
            !resumo.contains("erro(s)"),
            "confundiu nome de crate com erro: {resumo}"
        );
    }

    #[test]
    fn conta_avisos_quando_compila() {
        let saida =
            "warning: unused variable `x`\nwarning: unused import\n    Finished dev profile";
        assert_eq!(summarize_tool_output(saida, true), "2 aviso(s)");
    }

    #[test]
    fn reconhece_o_diffstat_do_git() {
        let saida = " src/main.rs | 12 ++++--\n 3 files changed, 42 insertions(+), 7 deletions(-)";
        assert_eq!(
            summarize_tool_output(saida, true),
            "3 files changed, 42 insertions(+), 7 deletions(-)"
        );
    }

    /// Saida sem classe reconhecida segue no comportamento de antes — nao se
    /// adivinha formato, pela mesma razao que nao se adivinha campo de input.
    #[test]
    fn formato_desconhecido_cai_no_generico() {
        let saida = "alguma coisa\noutra coisa\nmais uma";
        assert_eq!(
            summarize_tool_output(saida, true),
            "alguma coisa (+2 linha(s))"
        );
    }

    /// Em falha, uma linha que parece erro ganha da primeira linha qualquer.
    #[test]
    fn falha_generica_procura_a_linha_de_erro() {
        let saida = "iniciando processo\nlendo config\nfatal: not a git repository";
        assert_eq!(
            summarize_tool_output(saida, false),
            "fatal: not a git repository"
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
        sink.tool_finished(
            "bash",
            Duration::from_secs(1),
            true,
            "ok".into(),
            "saida completa".into(),
        )
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
            "test result: ok. 148 passed".into(),
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
                    summary: "148 passed".into(),
                    output: "test result: ok. 148 passed".into()
                },
                TurnEvent::TextDelta("depois".into()),
            ]
        );
    }
}
