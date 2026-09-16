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

/// Cada invocacao de macro de log do fonte, inteira, atravessando linhas.
///
/// Ignora linha de comentario e o corpo de `#[cfg(test)]` — nos dois os termos
/// aparecem como prosa ou como fixture, e nao como valor logado.
///
/// A contagem de parenteses e **ciente de literal**: um `)` dentro de
/// `"foo (bar)"` nao fecha a macro.
pub fn chamadas_de_log(fonte: &str) -> Vec<String> {
    const MACROS: &[&str] = &["info!(", "warn!(", "error!(", "debug!(", "trace!("];

    let codigo = codigo_sem_comentario_nem_teste(fonte);
    let chars: Vec<char> = codigo.chars().collect();
    let mut saida = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let janela: String = chars[i..].iter().take(7).collect();
        let Some(macro_) = MACROS.iter().find(|m| janela.starts_with(**m)) else {
            i += 1;
            continue;
        };
        let mut j = i + macro_.len();
        let mut nivel = 1usize;
        let mut em_literal = false;
        let mut escapado = false;
        while j < chars.len() && nivel > 0 {
            let c = chars[j];
            if em_literal {
                if escapado {
                    escapado = false;
                } else if c == '\\' {
                    escapado = true;
                } else if c == '"' {
                    em_literal = false;
                }
            } else {
                match c {
                    '"' => em_literal = true,
                    '(' => nivel += 1,
                    ')' => nivel -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        saida.push(chars[i..j].iter().collect::<String>());
        i = j;
    }
    saida
}

/// O que de uma chamada de log pode carregar valor: o codigo **fora** dos
/// literais, mais os nomes capturados em linha dentro deles (`{texto}`).
///
/// A prosa do literal nao conta — `"remetente fora da allowlist"` e uma frase,
/// nao um telefone. Mas `"...{texto}"` conta, e e a forma mais facil de vazar
/// sem perceber.
pub fn parte_arriscada(chamada: &str) -> String {
    let mut fora = String::new();
    let mut capturas = String::new();
    let mut dentro = false;
    let mut escapado = false;
    let mut literal = String::new();

    for c in chamada.chars() {
        if dentro {
            if escapado {
                escapado = false;
            } else if c == '\\' {
                escapado = true;
            } else if c == '"' {
                dentro = false;
                for trecho in literal.split('{').skip(1) {
                    if let Some((nome, _)) = trecho.split_once('}') {
                        capturas.push_str(nome);
                        capturas.push(' ');
                    }
                }
                literal.clear();
                continue;
            }
            literal.push(c);
        } else if c == '"' {
            dentro = true;
        } else {
            fora.push(c);
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
/// Heuristica de linha, de proposito: `let <nome> = <rhs>` com `<rhs>` tocando
/// um gatilho e nenhum antidoto. Nao e analise de fluxo — e um portao que custa
/// nada e fecha a forma que de fato aparece em codigo.
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
        if antidotos.iter().any(|a| rhs.contains(a)) {
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

/// O fonte sem linha de comentario e sem corpo de `#[cfg(test)]`.
fn codigo_sem_comentario_nem_teste(fonte: &str) -> String {
    let mut saida = Vec::new();
    let mut em_teste = false;
    let mut profundidade = 0i32;

    for raw in fonte.lines() {
        let linha = raw.trim();
        if linha.starts_with("#[cfg(test)]") {
            em_teste = true;
            profundidade = 0;
            continue;
        }
        if em_teste {
            profundidade += linha.matches('{').count() as i32;
            profundidade -= linha.matches('}').count() as i32;
            if profundidade <= 0 && linha.contains('}') {
                em_teste = false;
            }
            continue;
        }
        if linha.starts_with("//") {
            continue;
        }
        saida.push(raw);
    }
    saida.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mutacao que a varredura antiga, linha a linha, deixava passar.
    #[test]
    fn macro_multilinha_e_capturada_inteira() {
        let fonte =
            "fn f() {\n    tracing::info!(\n        %blob,\n        \"sessao\"\n    );\n}\n";
        let chamadas = chamadas_de_log(fonte);
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
        let chamadas = chamadas_de_log(fonte);
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

    /// Bloco de teste nao conta: la o segredo e fixture, nao vazamento.
    #[test]
    fn corpo_de_cfg_test_e_ignorado() {
        let fonte = "fn f() {}\n#[cfg(test)]\nmod t {\n    fn g() { info!(\"{blob}\"); }\n}\n";
        assert!(chamadas_de_log(fonte).is_empty());
    }
}
