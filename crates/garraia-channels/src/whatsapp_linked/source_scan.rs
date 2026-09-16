//! Varredura do proprio fonte: garantias que o compilador nao expressa.
//!
//! Mesmo padrao do teste de `garraia-desktop-core::detect`, que varre o fonte
//! atras de `Command::new`/`.spawn()`. A regra principal aqui e uma so — **o
//! blob de sessao nao pode aparecer num log** — e as outras pegam carona no
//! mesmo mecanismo: `Debug` do blob, `unwrap`/`expect` em producao, ANSI no
//! renderizador de QR e a visibilidade do construtor sem guarda do store.
//!
//! Por que um teste de texto e nao um tipo: o `RedactingWriter` de
//! `garraia-security` redige por prefixo conhecido (`sk-`, `xoxb-`, …) e nao
//! tem como reconhecer um base64 generico. `SessionBlob` ja tem `Debug`
//! redigido — mas `SessionBlob::expose()` existe, e precisa existir, para o
//! bridge e o store. O que este teste fixa e que ninguem passe o resultado de
//! `expose()` (ou um `blob`/`session` cru) para uma macro de log.

const SOURCES: &[(&str, &str)] = &[
    ("mod.rs", include_str!("mod.rs")),
    ("protocol.rs", include_str!("protocol.rs")),
    ("state.rs", include_str!("state.rs")),
    ("session.rs", include_str!("session.rs")),
    ("qr.rs", include_str!("qr.rs")),
    ("bridge.rs", include_str!("bridge.rs")),
    ("runner.rs", include_str!("runner.rs")),
];

const LOG_MACROS: &[&str] = &["info!", "warn!", "debug!", "error!", "trace!"];

/// Padroes que significam "o valor da sessao foi para o log": o `expose()`
/// cru, o `Display`/`Debug` de um binding chamado `session`/`blob`/`creds`, o
/// campo homonimo de um evento e o `.0` de um newtype.
const FORBIDDEN: &[&str] = &[
    "expose",
    "%session",
    "?session",
    "{session}",
    "session = ",
    "%blob",
    "?blob",
    "{blob}",
    "blob = ",
    "%creds",
    "?creds",
    "{creds}",
    "creds = ",
    ".0",
];

/// O fonte com os blocos `#[cfg(test)]` apagados, preservando a numeracao das
/// linhas (cada linha removida vira uma linha vazia).
fn production_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_tests = false;
    let mut brace_depth = 0i32;
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with("#[cfg(test)]") {
            in_tests = true;
            brace_depth = 0;
        }
        if in_tests {
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 && line.contains('}') {
                in_tests = false;
            }
            out.push('\n');
            continue;
        }
        out.push_str(raw);
        out.push('\n');
    }
    out
}

/// Fim de um literal de string que comeca em `at` (o proprio `"`), incluindo
/// o `"` que fecha. Trata escape (`\"`) e string crua (`r"…"`, `r#"…"#`).
fn end_of_string(bytes: &[u8], at: usize) -> usize {
    // Quantos `#` vieram antes da aspa? So conta como string crua se houver um
    // `r` logo antes deles.
    let mut hashes = 0usize;
    while at > hashes && bytes[at - 1 - hashes] == b'#' {
        hashes += 1;
    }
    let raw = at > hashes && bytes[at - 1 - hashes] == b'r';

    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if !raw => i += 2,
            b'"' => {
                if !raw {
                    return i + 1;
                }
                // Crua: fecha na aspa seguida do mesmo numero de `#`.
                let closing = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if closing >= hashes {
                    return i + 1 + hashes;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Blocos de log completos (`info!(…)` ate o delimitador que fecha), com o
/// numero da linha em que a macro comeca e o texto ja normalizado.
///
/// # Por que bloco e nao linha
///
/// A versao anterior coletava **so a linha que continha** `info!`, e os campos
/// das linhas seguintes nunca eram examinados. Isso nao e um caso exotico: e a
/// forma que o proprio `rustfmt` produz para qualquer log com mais de um
/// campo, e `runner.rs` ja tem uma macro assim em producao. A mutacao
///
/// ```ignore
/// tracing::info!(
///     sessao = %blob.expose(),
///     "sessao persistida"
/// );
/// ```
///
/// passava verde enquanto a mesma coisa numa linha so ficava vermelha — ou
/// seja, a unica prova automatizada do requisito "sem log de secret" nao via a
/// forma normal de escrever um log.
///
/// # Como
///
/// Balanceando delimitadores ate o que fecha a chamada, a mesma tecnica de
/// `account_arguments` no scan da CLI. Literal de string e pulado inteiro (um
/// parentese dentro de uma mensagem nao desbalanceia nada) e comentario e
/// descartado (uma nota `// expose()` nao e um log). O conteudo do literal
/// **fica** no bloco de proposito: `info!("session = {}", x)` tem de ser
/// reprovado.
fn log_blocks(source: &str) -> Vec<(usize, String)> {
    let src = production_source(source);
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                let end = end_of_string(bytes, i);
                line += bytes[i..end].iter().filter(|b| **b == b'\n').count();
                i = end;
                continue;
            }
            _ => {}
        }

        let macro_here = LOG_MACROS.iter().find(|m| {
            bytes[i..].starts_with(m.as_bytes())
                && !matches!(bytes.get(i.wrapping_sub(1)), Some(b) if b.is_ascii_alphanumeric() || *b == b'_')
        });
        let Some(mac) = macro_here else {
            i += 1;
            continue;
        };

        let start_line = line;
        let mut j = i + mac.len();
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            if bytes[j] == b'\n' {
                line += 1;
            }
            j += 1;
        }
        let Some(open) = bytes
            .get(j)
            .copied()
            .filter(|b| matches!(b, b'(' | b'[' | b'{'))
        else {
            // `info!` sem delimitador: nao e a forma que conhecemos, entao o
            // scan reporta a linha crua em vez de fingir que esta tudo bem.
            let rest = src[i..].lines().next().unwrap_or("");
            out.push((start_line, normalize(rest)));
            i += mac.len();
            continue;
        };

        let mut depth = 0i32;
        let mut text = String::new();
        let mut k = j;
        while k < bytes.len() {
            match bytes[k] {
                b'\n' => {
                    line += 1;
                    text.push(' ');
                    k += 1;
                }
                b'/' if bytes.get(k + 1) == Some(&b'/') => {
                    while k < bytes.len() && bytes[k] != b'\n' {
                        k += 1;
                    }
                }
                b'"' => {
                    let end = end_of_string(bytes, k);
                    line += bytes[k..end].iter().filter(|b| **b == b'\n').count();
                    text.push_str(&src[k..end]);
                    k = end;
                }
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    text.push(bytes[k] as char);
                    k += 1;
                }
                b')' | b']' | b'}' => {
                    depth -= 1;
                    text.push(bytes[k] as char);
                    k += 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {
                    let ch_end = (k + 1..=bytes.len())
                        .find(|e| src.is_char_boundary(*e))
                        .unwrap_or(bytes.len());
                    text.push_str(&src[k..ch_end]);
                    k = ch_end;
                }
            }
        }
        let _ = open;
        out.push((start_line, normalize(&text)));
        i = k;
    }
    out
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Blocos de log que carregam material de sessao, ja formatados para a
/// mensagem de falha. Funcao separada do `#[test]` porque e ela que o teste do
/// proprio parser exercita, com as mutacoes como entrada.
fn offending_log_blocks(name: &str, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (line_no, block) in log_blocks(source) {
        for pattern in FORBIDDEN {
            if block.contains(pattern) {
                out.push(format!("{name}:{line_no}: {block} ({pattern})"));
            }
        }
    }
    out
}

/// Nenhuma macro de log pode carregar o valor da sessao.
#[test]
fn no_log_line_mentions_the_session_value() {
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        offenders.extend(offending_log_blocks(name, source));
    }
    assert!(
        offenders.is_empty(),
        "linha de log carregando material de sessao:\n{}",
        offenders.join("\n")
    );
}

/// O parser enxerga a macro que o `rustfmt` quebrou em varias linhas.
///
/// Mesmo padrao do `account_arguments` na CLI: as mutacoes que derrubaram a
/// versao anterior entram como *entrada do parser*, e nao como codigo plantado
/// na arvore de verdade.
#[test]
fn the_scan_reads_the_whole_macro_even_broken_across_lines() {
    // A mutacao que sobreviveu a varredura anterior.
    let multiline = r#"
fn persistir(blob: &SessionBlob) {
    tracing::info!(
        sessao = %blob.expose(),
        "sessao persistida"
    );
}
"#;
    assert!(
        !offending_log_blocks("mutacao.rs", multiline).is_empty(),
        "a macro quebrada em varias linhas precisa ser reprovada"
    );

    // A mesma coisa numa linha so — ja era pega antes, e continua sendo.
    let single = r#"tracing::info!(sessao = %blob.expose(), "sessao persistida");"#;
    assert!(
        !offending_log_blocks("mutacao.rs", single).is_empty(),
        "a macro numa linha so continua reprovada"
    );

    // Um log honesto nao pode virar falso positivo so por ser multilinha.
    let ok = r#"
fn reconectar(attempt: u32, delay: u64) {
    tracing::info!(
        attempt,
        delay_ms = delay,
        "caiu; reconectando"
    );
}
"#;
    assert!(
        offending_log_blocks("ok.rs", ok).is_empty(),
        "log sem material de sessao nao pode ser reprovado"
    );

    // Comentario dentro do bloco nao e um log.
    let commented = r#"
tracing::info!(
    // nada de blob.expose() aqui
    attempt,
    "ok"
);
"#;
    assert!(
        offending_log_blocks("comentario.rs", commented).is_empty(),
        "um comentario dentro do bloco nao e material logado"
    );

    // Parentese dentro da mensagem nao pode desbalancear a leitura: se
    // desbalanceasse, o bloco seguinte seria engolido e nunca examinado.
    let paren_in_message = r#"
tracing::info!("aviso :-) nao fecha nada");
tracing::warn!(sessao = %blob.expose(), "depois");
"#;
    assert!(
        !offending_log_blocks("paren.rs", paren_in_message).is_empty(),
        "um parentese dentro da mensagem nao pode esconder o log seguinte"
    );
}

/// Ancora na arvore de verdade: o `tracing::info!` multilinha que ja existe em
/// producao e lido INTEIRO. Se a varredura voltar ao modo linha, este teste
/// morre junto com a garantia.
#[test]
fn the_scan_sees_the_multiline_log_that_already_exists_in_production() {
    let blocks = log_blocks(include_str!("runner.rs"));
    let found = blocks.iter().any(|(_, block)| {
        block.contains("attempt")
            && block.contains("delay_ms = delay")
            && block.contains("WhatsApp vinculado caiu; reconectando")
    });
    assert!(
        found,
        "o scan precisa ler o bloco inteiro do `tracing::info!` de `serve`; \
blocos vistos:\n{}",
        blocks
            .iter()
            .map(|(l, b)| format!("{l}: {b}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// `SessionStore` so pode expor UM construtor para fora da crate, e e o que
/// valida.
///
/// # O que este teste pina, e por que ele e uma checagem de DECLARACAO
///
/// A supressao CodeQL 173 afirma que a contencao do caminho no data dir "fecha
/// por construcao, sem depender de call site". Isso e verdade enquanto
/// `for_data_dir` — que valida o segmento de conta — for o unico construtor
/// visivel de fora. No dia em que `new` voltar a ser `pub`, a afirmacao vira
/// falsa em silencio: `new` aceita qualquer `PathBuf`, e o scan da CLI, que
/// chaveia no nome `for_data_dir`, nao enxerga nada.
///
/// Note o que esta sendo lido: a linha de DECLARACAO, e nao os call sites.
/// Uma varredura que tenta entender chamadas ja foi furada tres vezes neste
/// modulo; esta aqui so pergunta se a palavra `pub` continua onde deve, e quem
/// de fato impede a chamada de fora e o rustc.
#[test]
fn the_store_exposes_a_single_validating_constructor() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("pub(crate) fn new(dir: impl Into<PathBuf>)"),
        "SessionStore::new precisa continuar pub(crate) — ver o docstring dela"
    );
    assert!(
        !src.contains("pub fn new(dir: impl Into<PathBuf>)"),
        "SessionStore::new nao pode ser pub: seria um segundo construtor sem \
guarda, e a supressao CodeQL 173 depende de nao haver um"
    );
    assert!(
        src.contains("pub fn for_data_dir("),
        "e `for_data_dir` — o que valida — continua sendo o construtor publico"
    );
}

/// `SessionBlob` nao pode derivar `Debug`: o `Debug` dele e manual e redigido.
#[test]
fn session_blob_never_derives_debug() {
    let src = include_str!("session.rs");
    let idx = src
        .find("pub struct SessionBlob")
        .expect("SessionBlob precisa existir");
    let header = &src[idx.saturating_sub(300)..idx];
    let derive = header
        .rfind("#[derive(")
        .map(|i| &header[i..])
        .unwrap_or("");
    assert!(
        !derive.contains("Debug"),
        "SessionBlob nao pode derivar Debug — o Debug dele imprime <redacted>. Derive: {derive}"
    );
}

/// Nenhum `unwrap()`/`expect()` fora de teste: regra 4 do CLAUDE.md.
#[test]
fn production_code_has_no_unwrap_or_expect() {
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        let mut in_tests = false;
        let mut brace_depth = 0i32;
        for (i, raw) in source.lines().enumerate() {
            let line = raw.trim();
            if line.starts_with("#[cfg(test)]") {
                in_tests = true;
                brace_depth = 0;
            }
            if in_tests {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;
                if brace_depth <= 0 && line.contains('}') {
                    in_tests = false;
                }
                continue;
            }
            if line.starts_with("//") || line.starts_with("///") || line.starts_with("//!") {
                continue;
            }
            if line.contains(".unwrap()") || line.contains(".expect(") {
                offenders.push(format!("{name}:{}: {line}", i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "unwrap/expect em codigo de producao:\n{}",
        offenders.join("\n")
    );
}

/// O renderizador de QR nunca pode esconder o cursor nem emitir ANSI — a
/// mesma invariante do `spinner.rs`.
#[test]
fn the_qr_renderer_never_touches_the_terminal_state() {
    let src = include_str!("qr.rs");
    for forbidden in ["?25l", "\\x1b[", "\\u{1b}", "\\x1B["] {
        assert!(
            !src.contains(forbidden),
            "qr.rs contem {forbidden:?} — nada aqui pode mexer no terminal"
        );
    }
}
