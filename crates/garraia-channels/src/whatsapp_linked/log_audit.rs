//! Analise de texto para as varreduras de fonte que provam "isto nunca vai
//! para um log".
//!
//! # Por que isto existe como modulo, e nao como duas copias
//!
//! Havia duas varreduras de PII neste canal, de qualidade oposta, e a boa
//! cobria a ruim apenas na aparencia.
//!
//! A de `bootstrap/whatsapp_linked/tests.rs` (gateway) extrai a **invocacao
//! inteira** da macro, contando parenteses, e so depois separa o que pode
//! carregar valor. A de [`super::source_scan`] — que guarda o ativo mais
//! valioso do canal, o blob de sessao — casava padrao **linha a linha**. E o
//! `rustfmt` quebra toda macro `tracing!` com campos estruturados em varias
//! linhas, entao estas duas mutacoes sobreviviam a ela:
//!
//! ```text
//! tracing::info!(
//!     %blob,
//!     "sessao"
//! );
//!
//! let apelido = blob.expose().to_string();
//! info!("{apelido}");
//! ```
//!
//! A primeira porque `%blob` esta numa linha sem `info!`; a segunda porque
//! `apelido` nao e um nome proibido. As mesmas duas numa linha so eram pegas —
//! o que faz do teste uma funcao da formatacao, e nao do conteudo.
//!
//! Este modulo e a definicao unica: multi-linha por construcao, e com
//! [`bindings_contaminados`] para o segundo caso. Mora em `garraia-channels`
//! porque e a crate que possui o segredo; o gateway depende dela.
//!
//! `#[doc(hidden)]` no reexport: e superficie de teste, nao API do canal.
//!
//! # O que e ciente de literal, e por que isso nao e detalhe
//!
//! Tres passos deste modulo tem de saber onde comeca e termina um literal de
//! string, e e sempre [`fim_de_literal`] quem responde — **um** parser, nao
//! tres:
//!
//! 1. a contagem de delimitadores em [`chamadas_de_log`], para que o `)` de
//!    `"foo (bar)"` nao feche a macro no lugar errado;
//! 2. a separacao de [`parte_arriscada`], que so conta como prosa o que de
//!    fato esta dentro das aspas;
//! 3. o corte de comentario em [`codigo_sem_comentario_nem_teste`], que nao
//!    pode transformar `"https://exemplo"` no comeco de um comentario.
//!
//! String crua (`r"…"`, `r#"…"#`) entra aqui de proposito: dentro dela `\"`
//! **nao** escapa nada, e um parser que achasse que escapa continuaria lendo
//! como literal um trecho que ja e codigo — engolindo, junto, o log seguinte.

/// Fim de um literal de string que comeca em `at` (o proprio `"`), incluindo o
/// `"` que fecha. Trata escape (`\"`) e string crua (`r"…"`, `r#"…"#`).
///
/// O prefixo e descoberto por lookbehind: conta os `#` imediatamente antes da
/// aspa e exige um `r` antes deles. `bytes` tem de ser o fonte inteiro, e nao
/// uma fatia que ja comece na aspa, senao o prefixo nao esta la para ser visto.
fn fim_de_literal(bytes: &[u8], at: usize) -> usize {
    let mut cerquilhas = 0usize;
    while at > cerquilhas && bytes[at - 1 - cerquilhas] == b'#' {
        cerquilhas += 1;
    }
    let crua = at > cerquilhas && bytes[at - 1 - cerquilhas] == b'r';

    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if !crua => i += 2,
            b'"' => {
                if !crua {
                    return i + 1;
                }
                // Crua: fecha na aspa seguida do mesmo numero de `#`.
                let fechando = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if fechando >= cerquilhas {
                    return i + 1 + cerquilhas;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

/// O proximo limite de caractere depois de `i`, para copiar um caractere
/// multi-byte sem cortar no meio.
fn fim_do_caractere(texto: &str, i: usize) -> usize {
    (i + 1..=texto.len())
        .find(|e| texto.is_char_boundary(*e))
        .unwrap_or(texto.len())
}

const MACROS: &[&str] = &["info!", "warn!", "error!", "debug!", "trace!"];

/// Cada invocacao de macro de log do fonte, inteira, atravessando linhas, com
/// o numero da linha em que ela **comeca**.
///
/// Ignora comentario (de linha inteira e de fim de linha) e o corpo de
/// `#[cfg(test)]` — nos dois os termos aparecem como prosa ou como fixture, e
/// nao como valor logado.
///
/// A contagem de delimitadores e **ciente de literal**: um `)` dentro de
/// `"foo (bar)"` nao fecha a macro.
///
/// # Macro sem delimitador
///
/// `info!` seguido de qualquer coisa que nao seja `(`, `[` ou `{` nao e a
/// forma que este parser conhece. Em vez de seguir adiante — que e dizer
/// "verde" sobre codigo que nao foi lido — ele devolve a linha crua, e quem
/// chama decide. Fail-closed: uma forma nova de escrever log aparece como
/// achado, nao como silencio.
pub fn chamadas_de_log(fonte: &str) -> Vec<(usize, String)> {
    let codigo = codigo_sem_comentario_nem_teste(fonte);
    let bytes = codigo.as_bytes();
    let mut saida = Vec::new();
    let mut linha = 1usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                linha += 1;
                i += 1;
                continue;
            }
            b'"' => {
                let fim = fim_de_literal(bytes, i);
                linha += bytes[i..fim].iter().filter(|b| **b == b'\n').count();
                i = fim;
                continue;
            }
            _ => {}
        }

        // O nome da macro tem de comecar aqui: `ainfo!` nao e `info!`.
        let encontrada = MACROS.iter().find(|m| {
            bytes[i..].starts_with(m.as_bytes())
                && !matches!(
                    bytes.get(i.wrapping_sub(1)),
                    Some(b) if b.is_ascii_alphanumeric() || *b == b'_'
                )
        });
        let Some(macro_) = encontrada else {
            i += 1;
            continue;
        };

        let inicio = linha;
        let mut j = i + macro_.len();
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            if bytes[j] == b'\n' {
                linha += 1;
            }
            j += 1;
        }
        if !matches!(bytes.get(j), Some(b'(' | b'[' | b'{')) {
            let resto = codigo[i..].lines().next().unwrap_or("");
            saida.push((inicio, normaliza(resto)));
            i += macro_.len();
            continue;
        }

        let mut nivel = 0i32;
        let mut texto = String::from(*macro_);
        let mut k = j;
        while k < bytes.len() {
            match bytes[k] {
                b'\n' => {
                    linha += 1;
                    texto.push(' ');
                    k += 1;
                }
                b'"' => {
                    let fim = fim_de_literal(bytes, k);
                    linha += bytes[k..fim].iter().filter(|b| **b == b'\n').count();
                    texto.push_str(&codigo[k..fim]);
                    k = fim;
                }
                b'(' | b'[' | b'{' => {
                    nivel += 1;
                    texto.push(bytes[k] as char);
                    k += 1;
                }
                b')' | b']' | b'}' => {
                    nivel -= 1;
                    texto.push(bytes[k] as char);
                    k += 1;
                    if nivel == 0 {
                        break;
                    }
                }
                _ => {
                    let fim = fim_do_caractere(&codigo, k);
                    texto.push_str(&codigo[k..fim]);
                    k = fim;
                }
            }
        }
        saida.push((inicio, normaliza(&texto)));
        i = k;
    }
    saida
}

fn normaliza(texto: &str) -> String {
    texto.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O que de uma chamada de log pode carregar valor: o codigo **fora** dos
/// literais, mais os nomes capturados em linha dentro deles (`{texto}`).
///
/// A prosa do literal nao conta — `"remetente fora da allowlist"` e uma frase,
/// nao um telefone. Mas `"...{texto}"` conta, e e a forma mais facil de vazar
/// sem perceber.
pub fn parte_arriscada(chamada: &str) -> String {
    let bytes = chamada.as_bytes();
    let mut fora = String::new();
    let mut capturas = String::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'"' {
            let fim = fim_de_literal(bytes, i);
            for trecho in chamada[i..fim].split('{').skip(1) {
                if let Some((nome, _)) = trecho.split_once('}') {
                    capturas.push_str(nome);
                    capturas.push(' ');
                }
            }
            i = fim;
        } else {
            let fim = fim_do_caractere(chamada, i);
            fora.push_str(&chamada[i..fim]);
            i = fim;
        }
    }
    format!("{fora} {capturas}")
}

/// Nomes de binding que passam a valer como proibidos porque foram
/// **inicializados a partir** de um deles.
///
/// Sem isto, renomear o segredo desarma a varredura:
/// `let apelido = blob.expose().to_string();` seguido de `info!("{apelido}")`
/// nao casa com nenhum nome da lista original.
///
/// `antidotos` sao as chamadas que **quebram** a cadeia: uma redacao. `let
/// last4 = msg.sender_jid.last4();` toca `sender_jid`, mas o resultado e
/// justamente a forma que se pode logar — e sem esta lista o teste condenaria o
/// proprio remedio que ele existe para exigir.
///
/// # O antidoto e a chamada EXTERNA, e nao uma substring
///
/// A primeira versao perguntava `rhs.contains(antidoto)`, e isso desarmava a
/// varredura sem redigir nada: em
///
/// ```ignore
/// let vazamento = format!("{} {}", msg.sender_jid.as_str(), msg.sender_jid.last4());
/// ```
///
/// o `.last4()` aparece, o `contains` acha, e o JID inteiro — que esta no mesmo
/// RHS — segue para o log com a varredura verde. Quem calou o teste foi a
/// mencao ao remedio, nao o remedio.
///
/// Agora o antidoto precisa **terminar** o RHS (ignorando `;`, `?` e espaco ao
/// final), que e o unico caso em que ele de fato descreve o valor inteiro do
/// binding. `let last4 = msg.sender_jid.last4();` continua verde; o `format!`
/// misto volta a ser vermelho.
///
/// A direcao do erro que sobra e a segura: `let x = jid.last4().to_string();`
/// e condenado embora seja honesto. Falso positivo custa uma linha de
/// `#[allow]` conversada numa revisao; falso negativo custa um JID em disco.
///
/// Heuristica de linha, de proposito: `let <nome> = <rhs>` com `<rhs>` tocando
/// um gatilho e terminando fora de um antidoto. Nao e analise de fluxo — e um
/// portao que custa nada e fecha a forma que de fato aparece em codigo.
pub fn bindings_contaminados(fonte: &str, gatilhos: &[&str], antidotos: &[&str]) -> Vec<String> {
    let mut nomes = Vec::new();
    for linha in codigo_sem_comentario_nem_teste(fonte).lines() {
        let linha = linha.trim();
        let Some(resto) = linha.strip_prefix("let ") else {
            continue;
        };
        let resto = resto.strip_prefix("mut ").unwrap_or(resto);
        let Some((alvo, rhs)) = resto.split_once('=') else {
            continue;
        };
        if !gatilhos.iter().any(|g| rhs.contains(g)) {
            continue;
        }
        let terminal = rhs.trim().trim_end_matches([';', '?']).trim();
        if antidotos.iter().any(|a| terminal.ends_with(a)) {
            continue;
        }
        // `let x: T = ...` e `let Some(x) = ...`: fica so o identificador.
        let nome: String = alvo
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_')
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !nome.is_empty() && nome != "_" {
            nomes.push(nome);
        }
    }
    nomes.sort();
    nomes.dedup();
    nomes
}

/// O fonte sem comentario e sem corpo de `#[cfg(test)]`, **preservando a
/// numeracao das linhas** — cada linha apagada vira uma linha vazia.
///
/// A numeracao so aparece em mensagem de erro, mas e ela que decide se o
/// achado custa cinco segundos ou cinco minutos de quem for conferir.
fn codigo_sem_comentario_nem_teste(fonte: &str) -> String {
    sem_comentario(&sem_bloco_de_teste(fonte))
}

/// O fonte com os blocos `#[cfg(test)]` apagados, linha a linha e preservando
/// a contagem.
fn sem_bloco_de_teste(fonte: &str) -> String {
    let mut saida = String::with_capacity(fonte.len());
    let mut em_teste = false;
    let mut profundidade = 0i32;

    for raw in fonte.lines() {
        let linha = raw.trim();
        if linha.starts_with("#[cfg(test)]") {
            em_teste = true;
            profundidade = 0;
        }
        if em_teste {
            profundidade += linha.matches('{').count() as i32;
            profundidade -= linha.matches('}').count() as i32;
            if profundidade <= 0 && linha.contains('}') {
                em_teste = false;
            }
            saida.push('\n');
            continue;
        }
        saida.push_str(raw);
        saida.push('\n');
    }
    saida
}

/// O fonte sem comentario `//` — **inclusive o de fim de linha** — e sem tocar
/// num `//` que esteja dentro de um literal de string.
///
/// Cortar so a linha que **comeca** com `//` deixava um buraco barato: em
/// `let cru = msg.sender_jid.as_str(); // prefira .last4()` o comentario
/// entrava no RHS e valia como antidoto. Cortar sem saber onde estao as aspas
/// abriria o buraco simetrico, transformando `"https://exemplo"` em comeco de
/// comentario e engolindo o resto da linha.
fn sem_comentario(fonte: &str) -> String {
    let bytes = fonte.as_bytes();
    let mut saida = String::with_capacity(fonte.len());
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let fim = fim_de_literal(bytes, i);
                saida.push_str(&fonte[i..fim]);
                i = fim;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {
                let fim = fim_do_caractere(fonte, i);
                saida.push_str(&fonte[i..fim]);
                i = fim;
            }
        }
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    fn textos(fonte: &str) -> Vec<String> {
        chamadas_de_log(fonte).into_iter().map(|(_, c)| c).collect()
    }

    /// A mutacao que a varredura antiga, linha a linha, deixava passar.
    #[test]
    fn macro_multilinha_e_capturada_inteira() {
        let fonte =
            "fn f() {\n    tracing::info!(\n        %blob,\n        \"sessao\"\n    );\n}\n";
        let chamadas = textos(fonte);
        assert_eq!(chamadas.len(), 1, "{chamadas:?}");
        assert!(
            parte_arriscada(&chamadas[0]).contains("blob"),
            "o campo estruturado tem de sobrar na parte arriscada: {chamadas:?}"
        );
    }

    /// Parentese dentro de literal nao fecha a macro.
    #[test]
    fn parentese_em_literal_nao_fecha_a_chamada() {
        let fonte = "fn f() {\n    info!(\"abriu (e fechou)\", campo = %blob);\n}\n";
        let chamadas = textos(fonte);
        assert_eq!(chamadas.len(), 1);
        assert!(parte_arriscada(&chamadas[0]).contains("blob"));
    }

    /// A prosa do literal nao e valor; a captura em linha e.
    #[test]
    fn prosa_nao_conta_mas_captura_em_linha_conta() {
        let risco = parte_arriscada("info!(\"remetente fora da allowlist\")");
        assert!(!risco.contains("remetente"));
        let risco = parte_arriscada("info!(\"{remetente}\")");
        assert!(risco.contains("remetente"));
    }

    /// A segunda mutacao: renomear o segredo antes de loga-lo.
    #[test]
    fn binding_que_nasce_do_segredo_fica_contaminado() {
        let fonte = "fn f() {\n    let apelido = blob.expose().to_string();\n    info!(\"{apelido}\");\n}\n";
        let sujos = bindings_contaminados(fonte, &["expose", "blob"], &[]);
        assert_eq!(sujos, vec!["apelido".to_string()]);
    }

    /// Mas a redacao quebra a cadeia: sem isto o teste condenaria exatamente a
    /// forma que ele existe para exigir (`phone_last4 = %last4`).
    #[test]
    fn redacao_quebra_a_contaminacao() {
        let fonte =
            "fn f() {\n    let last4 = msg.sender_jid.last4();\n    info!(\"{last4}\");\n}\n";
        assert_eq!(
            bindings_contaminados(fonte, &["sender_jid"], &[".last4()"]),
            Vec::<String>::new()
        );
        assert_eq!(
            bindings_contaminados(fonte, &["sender_jid"], &[]),
            vec!["last4".to_string()],
            "premissa: sem o antidoto ele seria condenado"
        );
    }

    /// O antidoto tem de ser a chamada **externa**, e nao uma mencao em
    /// qualquer posicao do RHS.
    ///
    /// A forma vermelha aqui e a que passava verde antes: o mesmo `format!`
    /// carrega o JID cru *e* o `last4()`, e o `contains` bastava para desarmar
    /// a varredura. A forma verde e o remedio que a lista de antidotos existe
    /// para nao condenar — as duas no mesmo teste porque trocar o furo por um
    /// falso positivo nao seria conserto.
    #[test]
    fn antidoto_no_meio_do_rhs_nao_desarma_a_varredura() {
        let misto = "fn f() {\n    let vazamento = format!(\"{} {}\", msg.sender_jid.as_str(), msg.sender_jid.last4());\n    warn!(\"{vazamento}\");\n}\n";
        assert_eq!(
            bindings_contaminados(misto, &["sender_jid"], &[".last4()"]),
            vec!["vazamento".to_string()],
            "o RHS carrega o JID inteiro: mencionar `.last4()` nao pode absolver"
        );

        let remedio = "fn f() {\n    let last4 = jid.last4();\n}\n";
        assert_eq!(
            bindings_contaminados(remedio, &["jid"], &[".last4()"]),
            Vec::<String>::new(),
            "e o remedio continua verde"
        );
    }

    /// Comentario de **fim de linha** tambem nao e RHS.
    ///
    /// Variante ainda mais barata do furo anterior: sem cortar o comentario,
    /// `// prefira .last4()` entrava no RHS e valia como antidoto.
    #[test]
    fn comentario_de_fim_de_linha_nao_vale_como_antidoto() {
        let fonte = "fn f() {\n    let cru = msg.sender_jid.as_str(); // prefira .last4()\n}\n";
        assert_eq!(
            bindings_contaminados(fonte, &["sender_jid"], &[".last4()"]),
            vec!["cru".to_string()]
        );
    }

    /// E cortar comentario nao pode comer um `//` que mora dentro de aspas.
    #[test]
    fn barra_dupla_dentro_de_literal_nao_e_comentario() {
        let fonte = "fn f() {\n    info!(\"https://exemplo\", campo = %blob);\n}\n";
        let chamadas = textos(fonte);
        assert_eq!(chamadas.len(), 1, "{chamadas:?}");
        assert!(parte_arriscada(&chamadas[0]).contains("blob"));
    }

    /// String crua: dentro dela `\` nao escapa nada.
    ///
    /// A fixture termina o conteudo em `\`, que e onde a diferenca aparece: um
    /// parser que achasse que `\"` escapa pularia a aspa que **fecha** o
    /// literal e seguiria lendo o resto do arquivo como texto — engolindo, com
    /// ele, o `warn!` que carrega o segredo. Por isso o teste afirma as duas
    /// coisas: que sao duas chamadas, e que a segunda foi examinada.
    #[test]
    fn string_crua_termina_onde_deve() {
        let fonte = "fn f() {\n    info!(r#\"caminho C:\\\"#);\n    warn!(campo = %blob);\n}\n";
        let chamadas = textos(fonte);
        assert_eq!(
            chamadas.len(),
            2,
            "a string crua tem de fechar em `\"#`: {chamadas:?}"
        );
        assert!(parte_arriscada(&chamadas[1]).contains("blob"));
    }

    /// Macro sem delimitador conhecido: reporta a linha crua em vez de dizer
    /// "verde" sobre codigo que nao foi lido.
    #[test]
    fn macro_sem_delimitador_reporta_a_linha_crua() {
        let fonte = "fn f() {\n    let m = info!;\n}\n";
        let chamadas = chamadas_de_log(fonte);
        assert_eq!(chamadas.len(), 1, "{chamadas:?}");
        assert!(chamadas[0].1.contains("info!"), "{chamadas:?}");
    }

    /// A numeracao sobrevive ao apagamento do bloco de teste — o achado tem de
    /// apontar para a linha que existe no arquivo de verdade.
    #[test]
    fn a_linha_reportada_e_a_do_arquivo_original() {
        let fonte = "#[cfg(test)]\nmod t {\n    fn g() {}\n}\nfn f() {\n    info!(%blob);\n}\n";
        let chamadas = chamadas_de_log(fonte);
        assert_eq!(chamadas.len(), 1, "{chamadas:?}");
        assert_eq!(chamadas[0].0, 6, "a macro esta na linha 6 do fonte");
    }

    /// Bloco de teste nao conta: la o segredo e fixture, nao vazamento.
    #[test]
    fn corpo_de_cfg_test_e_ignorado() {
        let fonte = "fn f() {}\n#[cfg(test)]\nmod t {\n    fn g() { info!(\"{blob}\"); }\n}\n";
        assert!(chamadas_de_log(fonte).is_empty());
    }
}
