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
//!
//! # O que estas varreduras NAO alcancam
//!
//! `SessionBlob` e `#[serde(transparent)]` no `Serialize`. Um
//! `serde_json::to_string(&blob)` futuro carrega o valor **cru** sem a palavra
//! `expose` aparecer em lugar nenhum do fonte — e o unico caminho que nem a
//! allowlist de [`EXPOSE_ALLOWED`] nem a varredura de macro veem. Fecha-lo e
//! trabalho de tipo (`Serialize` manual, ou um wrapper), nao de texto; ver
//! `the_transparent_serialize_is_a_known_and_documented_hole`.
//!
//! Desde a rodada 5, `SessionBlob::expose()` e `pub(crate)`: o rustc impede de
//! graca que a crate vizinha chame, e estas varreduras cobrem o que sobra —
//! de dentro da propria crate.

const SOURCES: &[(&str, &str)] = &[
    ("mod.rs", include_str!("mod.rs")),
    ("protocol.rs", include_str!("protocol.rs")),
    ("state.rs", include_str!("state.rs")),
    ("session.rs", include_str!("session.rs")),
    ("qr.rs", include_str!("qr.rs")),
    ("bridge.rs", include_str!("bridge.rs")),
    ("runner.rs", include_str!("runner.rs")),
];

/// Macros que levam texto para fora do processo.
///
/// A lista e propositalmente maior que a do `tracing`: o terminal aceita muito
/// mais do que cinco macros, e a auditoria R4 plantou um `eprintln!` com
/// `blob.expose()` que passou verde por nao estar aqui. Ela continua sendo,
/// porem, a **segunda** linha de defesa — enumerar macros e uma corrida que
/// nao se ganha, e quem fecha o buraco de verdade e a allowlist de
/// [`EXPOSE_ALLOWED`]. Esta lista pega o que a outra nao pega: um campo
/// chamado `session`/`blob`/`creds` que nunca passou por `expose()`.
const LOG_MACROS: &[&str] = &[
    "info!",
    "warn!",
    "debug!",
    "error!",
    "trace!",
    "event!",
    "info_span!",
    "warn_span!",
    "debug_span!",
    "error_span!",
    "trace_span!",
    "span!",
    "println!",
    "eprintln!",
    "print!",
    "eprint!",
    "dbg!",
    "panic!",
];

/// **Os unicos call sites de `SessionBlob::expose()` que existem.**
///
/// # Por que a regra e invertida
///
/// A varredura de macro — a que existia antes desta linha — reprova o que ela
/// conhece. A auditoria R4 mostrou o custo disso plantando `eprintln!` e
/// `tracing::event!` com `blob.expose()`: ambos passaram verdes. E registrou a
/// limitacao de fundo, que nenhuma lista de macros resolve:
///
/// ```ignore
/// let s = blob.expose();
/// let t = s;
/// tracing::info!(dado = %t);   // nenhuma varredura de macro ve isto
/// ```
///
/// A linha de defesa certa e o call site do `expose()`, porque e ali que a
/// protecao de tipo acaba: a partir dali o valor e um `&str` como outro
/// qualquer. Entao a regra passou a ser fechada — **toda** ocorrencia de
/// `expose` no codigo de producao destes arquivos precisa estar nomeada aqui,
/// e acrescentar uma linha nova e uma decisao consciente de quem escreve, em
/// vez de um silencio.
///
/// A checagem e sobre a linha de CODIGO (comentario fora), no mesmo espirito
/// de [`the_store_exposes_a_single_validating_constructor`]: ela pergunta se a
/// palavra continua onde deve, e quem de fato impede a chamada e o rustc.
const EXPOSE_ALLOWED: &[(&str, &str)] = &[
    // A declaracao. Ela precisa existir: o bridge e o store leem o blob.
    ("session.rs", "pub(crate) fn expose(&self) -> &str {"),
    // O unico uso: o que vai para o AES-GCM dentro de `SessionStore::save`.
    (
        "session.rs",
        "let mut in_out = blob.expose().as_bytes().to_vec();",
    ),
];

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
    // L4: a string crua do QR e uma credencial de ~20 s. Ela e impressa
    // literalmente em `Style::Raw` — por design, e e o que o usuario le com a
    // camera —, mas nada dela pertence a um log. Sem estes padroes um
    // `tracing::debug!(qr = %data)` futuro passaria mesmo com a allowlist de
    // `expose()` no lugar, porque o QR nunca passa por `SessionBlob`.
    "%qr",
    "?qr",
    "{qr}",
    "qr = ",
    "%data",
    "?data",
    "{data}",
    "data = ",
];

/// Consome UM pedaco do item que segue um `#[cfg(test)]` e diz ao cortador o
/// que fazer em seguida.
///
/// Existe como funcao, e nao inline no laco, porque ela roda em DOIS lugares:
/// sobre o resto da linha do proprio atributo (`#[cfg(test)] use std::fmt;`) e
/// sobre as linhas seguintes (`#[cfg(test)]` sozinho, item na linha de baixo).
/// Eram esses dois lugares que a rodada 5 tratou de formas diferentes — e a
/// forma "mesma linha" ficou sem tratamento nenhum.
///
/// - Abriu chave e ela continua aberta: comeca o modo "apaga".
/// - Abriu e fechou no mesmo pedaco (`mod tests { fn t() {} }`): o item acabou
///   ali.
/// - Sem chave e terminando em `;` (`mod tests;`, `use …;`, `const …;`): o
///   item acabou ali.
/// - Comentario ou outro atributo: nao decide nada, o item continua pendente.
fn absorb_cfg_test_item(
    text: &str,
    pending: &mut bool,
    in_tests: &mut bool,
    brace_depth: &mut i32,
) {
    // Comentario entre o atributo e o item nao decide nada: um `// veja foo;`
    // nao encerra o item.
    if text.starts_with("//") {
        return;
    }
    let opens = text.matches('{').count() as i32;
    let closes = text.matches('}').count() as i32;
    *brace_depth += opens - closes;
    if opens > 0 {
        *pending = false;
        if *brace_depth > 0 {
            *in_tests = true;
        } else {
            *brace_depth = 0;
        }
    } else if text.ends_with(';') {
        *pending = false;
        *brace_depth = 0;
    }
}

/// O fonte com os blocos `#[cfg(test)]` apagados, preservando a numeracao das
/// linhas (cada linha removida vira uma linha vazia).
///
/// # O furo de uma linha que isto fecha
///
/// A versao anterior ligava o modo "apaga" no instante em que via
/// `#[cfg(test)]` e so o desligava na primeira linha que contivesse `}`. Sobre
/// um item **sem chave** — `#[cfg(test)] mod tests;`, `use …;`, `const …;`,
/// `static …;` — o `brace_depth` nunca subia, e o modo seguia apagando
/// **producao** ate topar com uma chave de fechamento qualquer, la adiante.
/// Como as duas varreduras (a contagem crua de macros e o parser de blocos)
/// leem este mesmo texto, elas caiam JUNTAS: a coincidencia "verde com bug".
///
/// Nao e hipotetico. `mod.rs` ja declara `#[cfg(test)] mod source_scan;` — o
/// dano e zero **so** porque a declaracao esta na ultima linha do arquivo.
/// Move-la para o topo, onde declaracoes `mod` convencionalmente ficam,
/// cegaria `mod.rs` inteiro sem nenhum teste piscar. A auditoria R5 plantou
/// um `#[cfg(test)] const` seguido de um `tracing::info!` com
/// `blob.expose()` e teve 99/99 verdes.
///
/// A regra nova: o `#[cfg(test)]` fica **pendente**, e so vira modo "apaga"
/// quando uma chave de verdade abre um bloco. Um item que fecha em `;` apaga
/// exatamente a propria linha.
fn production_source(source: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(source.len());
    // Vimos `#[cfg(test)]` e ainda nao sabemos se o item abre bloco.
    let mut pending = false;
    let mut in_tests = false;
    let mut brace_depth = 0i32;
    for raw in source.lines() {
        let line = raw.trim();
        if !in_tests && !pending && line.starts_with(ATTR) {
            brace_depth = 0;
            pending = true;
            // **O resto da linha e parte do item**, e nao lixo a ignorar.
            // Setar `pending` e seguir em frente era o furo da rodada 5 pela
            // metade: em `#[cfg(test)] use std::fmt;` o item ja acabou nesta
            // linha, mas o cortador ia julgar a PROXIMA linha como se ela
            // fosse o item — e a primeira linha de producao que abrisse chave
            // ligava o modo "apaga".
            let rest = line[ATTR.len()..].trim();
            if !rest.is_empty() {
                absorb_cfg_test_item(rest, &mut pending, &mut in_tests, &mut brace_depth);
            }
            out.push('\n');
            continue;
        }
        if pending {
            absorb_cfg_test_item(line, &mut pending, &mut in_tests, &mut brace_depth);
            out.push('\n');
            continue;
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

/// Fim de um literal de char que comeca em `at` (o proprio `'`), ou `None`
/// quando aquele `'` e um tempo de vida (`&'a str`) ou um rotulo (`'outer:`).
///
/// # O furo que isto fecha
///
/// Sem este tratamento, um `'"'` ou um `b'"'` no fonte — e `bridge.rs`, o
/// arquivo de enquadramento NDJSON, e o candidato mais natural do repositorio
/// a ganhar um — deixava uma aspa desemparelhada. O parser lia dali ate a
/// proxima aspa do arquivo como se fosse uma string, e **todas** as fronteiras
/// de literal ficavam invertidas: o arquivo inteiro passava a render zero
/// bloco, em silencio. A auditoria R4 plantou
/// `fn is_quote(b: u8) -> bool { b == b'"' }` mais um `tracing::info!`
/// multilinha com `%blob.expose()` em `bridge.rs` e teve 7/7 verdes.
fn end_of_char(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = at + 1;
    if bytes.get(i) == Some(&b'\\') {
        // `\n`, `\'`, `\\`, `\x1b`, `\u{1b}`: todos fecham na proxima aspa.
        i += 1;
        while i < bytes.len() && bytes[i] != b'\'' {
            i += 1;
        }
        return (i < bytes.len()).then_some(i + 1);
    }
    // Um unico char (possivelmente multibyte) seguido da aspa que fecha. Se
    // nao fechar ali, aquele `'` era tempo de vida ou rotulo.
    let ch = src.get(i..)?.chars().next()?;
    i += ch.len_utf8();
    (bytes.get(i) == Some(&b'\'')).then_some(i + 1)
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
            // Um literal de char com aspa dentro (`'"'`, `b'"'`) inverteria
            // todas as fronteiras de string daqui para baixo. Ver
            // [`end_of_char`].
            b'\'' => {
                i = end_of_char(&src, i).unwrap_or(i + 1);
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
                b'\'' => {
                    let end = end_of_char(&src, k).unwrap_or(k + 1);
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

/// O fonte sem comentarios, preservando a numeracao das linhas.
///
/// Literal de string e literal de char sao pulados inteiros: um `//` dentro de
/// `"http://x"` nao comeca comentario nenhum, e um `'"'` nao abre string.
fn without_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let end = end_of_string(bytes, i);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'\'' => {
                let end = end_of_char(src, i).unwrap_or(i + 1);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    if bytes[i] == b'\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => {
                let end = (i + 1..=bytes.len())
                    .find(|e| src.is_char_boundary(*e))
                    .unwrap_or(bytes.len());
                out.push_str(&src[i..end]);
                i = end;
            }
        }
    }
    out
}

/// Quantas macros de log ha neste texto, pela mesma regra de fronteira do
/// [`log_blocks`] — `debug_span!` nao conta como `span!`.
fn count_log_macros(code: &str) -> usize {
    let bytes = code.as_bytes();
    (0..bytes.len())
        .filter(|i| {
            LOG_MACROS.iter().any(|m| {
                bytes[*i..].starts_with(m.as_bytes())
                    && !matches!(bytes.get(i.wrapping_sub(1)), Some(b) if b.is_ascii_alphanumeric() || *b == b'_')
            })
        })
        .count()
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

/// Chamadas de `expose` no codigo de producao que a allowlist nao declara,
/// ja formatadas para a mensagem de falha.
///
/// Funcao separada do `#[test]` pelo mesmo motivo de [`offending_log_blocks`]:
/// numa arvore limpa nao ha violacao nenhuma, entao um teste que so varre
/// `SOURCES` nao distingue **regra viva** de **regra ausente** — arrancar a
/// allowlist inteira o deixava verde. Com a funcao separada, o caso negativo
/// entra como TEXTO plantado e a mutacao fica vermelha.
fn exposes_outside_the_allowlist(name: &str, source: &str) -> Vec<String> {
    let code = without_comments(&production_source(source));
    let mut out = Vec::new();
    for (i, raw) in code.lines().enumerate() {
        let line = normalize(raw);
        if !line.contains("expose") {
            continue;
        }
        if EXPOSE_ALLOWED
            .iter()
            .any(|(f, allowed)| *f == name && line.contains(allowed))
        {
            continue;
        }
        out.push(format!("{name}:{}: {line}", i + 1));
    }
    out
}

/// **A regra invertida: `expose()` so pode aparecer onde a allowlist diz.**
///
/// A varredura de macro reprova o que conhece; esta reprova tudo o que nao foi
/// declarado. E a unica das duas que sobrevive a um `eprintln!`, a um
/// `tracing::event!`, a um `#[tracing::instrument(fields(…))]` e ao
/// `let s = blob.expose(); let t = s;` que nenhuma leitura de macro alcanca.
///
/// Acrescentar uma linha a [`EXPOSE_ALLOWED`] e o ponto: vira uma decisao
/// consciente, com nome e diff, em vez de um silencio.
#[test]
fn expose_is_only_called_where_the_allowlist_says() {
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        offenders.extend(exposes_outside_the_allowlist(name, source));
    }
    assert!(
        offenders.is_empty(),
        "`SessionBlob::expose()` fora da allowlist — a partir do call site o valor \
e um `&str` e a protecao de tipo acabou. Se o uso e legitimo, declare-o em \
EXPOSE_ALLOWED:\n{}",
        offenders.join("\n")
    );
}

/// A allowlist tem de continuar **descrevendo** a arvore, e nao so existir.
///
/// Uma allowlist cujas linhas ja nao casam com nada nao reprova nada: o teste
/// acima passaria verde com `EXPOSE_ALLOWED` apontando para codigo que foi
/// renomeado, e ninguem saberia. Aqui se exige o inverso — toda linha
/// declarada precisa ser encontrada.
#[test]
fn every_allowed_expose_call_site_still_exists() {
    for (name, allowed) in EXPOSE_ALLOWED {
        let (_, source) = SOURCES
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} nao esta em SOURCES"));
        let code = without_comments(&production_source(source));
        assert!(
            code.lines().any(|l| normalize(l).contains(allowed)),
            "{name}: a allowlist declara `{allowed}`, que nao existe mais no fonte de \
producao — allowlist morta nao guarda nada"
        );
    }
}

/// **Cada macro de log em producao tem de virar exatamente um bloco.**
///
/// Ancora de um arquivo so nao serve: hoje `runner.rs` tem 3 blocos e os
/// outros seis tem 0 cada, entao um `b'"'` plantado em `mod.rs`, `protocol.rs`,
/// `state.rs`, `session.rs`, `qr.rs` ou `bridge.rs` cegaria o parser **em
/// silencio** — nao ha ancora possivel num arquivo sem log. Esta asserção e
/// global e nao depende de um log especifico existir: ela compara o que o
/// parser devolveu com uma contagem crua do mesmo texto.
///
/// A contagem crua e deliberadamente ingenua (so comentario e removido, por
/// [`without_comments`]). Se um dia uma string literal de producao contiver
/// `"info!"`, este teste fica vermelho — ruidoso, mas visivel, que e o oposto
/// do que se esta consertando aqui.
#[test]
fn every_log_macro_in_production_yields_exactly_one_block() {
    let mut total_esperado = 0usize;
    for (name, source) in SOURCES {
        let code = without_comments(&production_source(source));
        let esperado = count_log_macros(&code);
        total_esperado += esperado;
        let blocos = log_blocks(source);
        assert_eq!(
            blocos.len(),
            esperado,
            "{name}: o texto de producao tem {esperado} macro(s) de log e o parser \
devolveu {} bloco(s). Uma contagem menor significa que as fronteiras de literal \
se inverteram — e dai em diante nenhum log deste arquivo e examinado. Blocos \
vistos:\n{}",
            blocos.len(),
            blocos
                .iter()
                .map(|(l, b)| format!("{l}: {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    // Sem isto, a comparacao acima degrada para `0 == 0` em toda a tabela se
    // `count_log_macros` e `log_blocks` pararem de achar qualquer coisa —
    // exatamente o modo de falha que ela existe para pegar.
    assert!(
        total_esperado > 0,
        "nenhuma macro de log em producao nos {} arquivos: ou o repo mudou muito, \
ou a contagem crua parou de funcionar e esta comparacao virou `0 == 0`",
        SOURCES.len()
    );
}

/// Um literal de char com aspa dentro nao pode cegar o arquivo inteiro.
///
/// `bridge.rs` e o arquivo de enquadramento NDJSON — o candidato mais natural
/// do repositorio a ganhar um `b'"'`. Antes desta rodada, uma aspa
/// desemparelhada fazia o parser ler dali ate a proxima aspa do arquivo como
/// "string", invertia todas as fronteiras e devolvia **zero** blocos.
#[test]
fn a_char_literal_holding_a_quote_does_not_blind_the_scan() {
    let mutacao = r#"
fn is_quote(b: u8) -> bool {
    b == b'"'
}

fn persistir(blob: &SessionBlob) {
    tracing::info!(
        sessao = %blob.expose(),
        "sessao persistida"
    );
}
"#;
    assert!(
        !offending_log_blocks("mutacao.rs", mutacao).is_empty(),
        "um `b'\"'` antes do log nao pode esconder o log"
    );

    // E o `'a` de um tempo de vida continua sendo tempo de vida, nao literal:
    // trata-lo como literal engoliria tudo ate a proxima aspa simples.
    let com_lifetime = r#"
impl<'a> Guarda<'a> {
    fn fala(&self, blob: &'a SessionBlob) {
        tracing::warn!(sessao = %blob.expose(), "vazou");
    }
}
"#;
    assert!(
        !offending_log_blocks("lifetime.rs", com_lifetime).is_empty(),
        "um tempo de vida nao pode ser lido como literal de char"
    );

    // Um log honesto depois de um literal de char nao pode virar falso
    // positivo por causa dele.
    let ok = r#"
fn separador() -> char {
    '"'
}

fn reconectar(attempt: u32) {
    tracing::info!(attempt, "caiu; reconectando");
}
"#;
    assert!(
        offending_log_blocks("ok.rs", ok).is_empty(),
        "log sem material de sessao nao pode ser reprovado"
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
///
/// Usa [`production_source`] em vez de reimplementar o corte do
/// `#[cfg(test)]`: a copia que morava aqui tinha o mesmo furo de uma linha —
/// um item sem chave apagava producao ate a proxima `}` — e uma regra de
/// seguranca com tres implementacoes e tres oportunidades de errar.
#[test]
fn production_code_has_no_unwrap_or_expect() {
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        for (i, raw) in production_source(source).lines().enumerate() {
            let line = raw.trim();
            if line.starts_with("//") {
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

/// **Um `#[cfg(test)]` sobre item sem chave nao pode cegar o arquivo.**
///
/// Este e o furo de uma linha que a rodada 5 fechou. `mod.rs:76` ja tem
/// `#[cfg(test)] mod source_scan;` na arvore — o dano hoje e zero so porque a
/// declaracao esta na ultima linha do arquivo. As tres varreduras que usam
/// [`production_source`] (macro de log, `expose` fora da allowlist,
/// `unwrap`/`expect`) leem o MESMO texto, entao elas ficavam cegas juntas: a
/// contagem crua e a do parser caiam no mesmo numero e o
/// `every_log_macro_in_production_yields_exactly_one_block` seguia verde.
#[test]
fn a_braceless_cfg_test_item_does_not_blind_the_rest_of_the_file() {
    // Exatamente a forma que existe em `mod.rs`, seguida de producao que
    // vaza. O `}` da funcao e o que a versao anterior usava para desligar o
    // modo "apaga" — tarde demais.
    let mutacao = r#"
#[cfg(test)]
mod tests;

fn vaza(blob: &SessionBlob) {
    let s = blob.expose();
    tracing::info!(dado = %s, "vazou a sessao");
    eprintln!("sessao: {}", blob.expose());
}
"#;
    let producao = production_source(mutacao);
    assert!(
        producao.contains("tracing::info!"),
        "a producao depois de um `#[cfg(test)] mod tests;` foi apagada:\n{producao}"
    );
    assert!(
        !offending_log_blocks("mutacao.rs", &producao).is_empty(),
        "o log com material de sessao precisa ser reprovado"
    );
    assert!(
        !exposes_outside_the_allowlist("mutacao.rs", mutacao).is_empty(),
        "o `expose()` fora da allowlist precisa ser reprovado"
    );

    // As outras formas sem chave da mesma familia.
    for item in [
        "use std::fmt;",
        "const SO_NO_TESTE: u8 = 1;",
        "static SO_NO_TESTE: u8 = 1;",
        "type Alias = u8;",
        // Com chave, mas fechando na mesma linha: tambem acaba ali.
        "static S: Foo = Foo { a: 1 };",
    ] {
        let fonte = format!("#[cfg(test)]\n{item}\n\nfn prod() {{\n    let x = 1;\n}}\n");
        let producao = production_source(&fonte);
        assert!(
            producao.contains("fn prod()"),
            "`#[cfg(test)] {item}` apagou a producao seguinte:\n{producao}"
        );
        assert!(
            !producao.contains(item),
            "`{item}` e codigo de teste e precisa sair do texto de producao"
        );
    }

    // **A forma que a rodada 5 nao cobriu: atributo e item na MESMA linha.**
    // O primeiro ramo consumia a linha inteira e deixava `pending = true`; a
    // proxima linha nao-comentario era avaliada como se fosse o item, e a
    // primeira linha de producao que abrisse chave ligava o modo "apaga".
    for item in [
        "#[cfg(test)] use std::fmt;",
        "#[cfg(test)] mod tests;",
        "#[cfg(test)] const SO_NO_TESTE: u8 = 1;",
    ] {
        let fonte = format!("{item}\n\nfn prod() {{\n    let x = 1;\n}}\n");
        let producao = production_source(&fonte);
        assert!(
            producao.contains("fn prod()"),
            "`{item}` (atributo e item na mesma linha) apagou a producao seguinte:\n{producao}"
        );
        assert!(
            !producao.contains("SO_NO_TESTE") && !producao.contains("std::fmt"),
            "`{item}` e codigo de teste e precisa sair do texto de producao:\n{producao}"
        );
    }

    // E a mesma forma COM bloco na mesma linha: o item acaba ali.
    let uma_linha = "#[cfg(test)] mod tests { fn t() {} }\n\nfn prod() {}\n";
    let producao = production_source(uma_linha);
    assert!(
        !producao.contains("fn t()"),
        "`#[cfg(test)] mod tests {{ … }}` numa linha so precisa sair:\n{producao}"
    );
    assert!(
        producao.contains("fn prod()"),
        "e o que vem depois precisa sobreviver:\n{producao}"
    );

    // **Aninhado, que e a forma mais funda do mesmo furo.** Em coluna 0 o
    // guarda de topo de arquivo ainda pegaria; dentro de um `mod`, nao — e
    // as tres varreduras ficam cegas juntas, a mesma coincidencia
    // "verde com bug".
    let aninhado = r#"
mod interno {
    #[cfg(test)] use std::fmt;
    pub fn vaza(blob: &SessionBlob) {
        let s = blob.expose();
        tracing::info!(dado = %s, "vazou a sessao inteira");
        eprintln!("sessao: {}", blob.expose());
    }
}
"#;
    let producao = production_source(aninhado);
    assert!(
        producao.contains("tracing::info!"),
        "a producao dentro do `mod` foi apagada por um `#[cfg(test)]` de uma \
linha so:\n{producao}"
    );
    assert!(
        !offending_log_blocks("aninhado.rs", &producao).is_empty(),
        "o log com material de sessao precisa ser reprovado mesmo aninhado"
    );
    assert!(
        !exposes_outside_the_allowlist("aninhado.rs", aninhado).is_empty(),
        "o `expose()` fora da allowlist precisa ser reprovado mesmo aninhado"
    );

    // E o bloco de verdade continua sendo apagado inteiro.
    let com_bloco =
        "#[cfg(test)]\nmod tests {\n    fn t() {\n        let x = 1;\n    }\n}\n\nfn prod() {}\n";
    let producao = production_source(com_bloco);
    assert!(
        !producao.contains("fn t()"),
        "o bloco `#[cfg(test)] mod tests {{ … }}` precisa continuar apagado:\n{producao}"
    );
    assert!(
        producao.contains("fn prod()"),
        "e o que vem depois dele precisa sobreviver:\n{producao}"
    );

    // Atributo entre o `#[cfg(test)]` e o item nao pode encerrar o item cedo.
    let com_atributo =
        "#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    fn t() {}\n}\n\nfn prod() {}\n";
    let producao = production_source(com_atributo);
    assert!(
        !producao.contains("fn t()"),
        "bloco com atributo extra:\n{producao}"
    );
    assert!(
        producao.contains("fn prod()"),
        "producao depois dele:\n{producao}"
    );
}

/// Indentacao da linha, ou `None` se ela e vazia/so espaco.
fn indent_of(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    (!trimmed.is_empty()).then(|| line.len() - trimmed.len())
}

/// O item que ABRE o bloco que contem a linha `i`: a linha mais proxima acima
/// com indentacao estritamente menor **que termina em `{`**.
///
/// O `ends_with('{')` nao e cosmetico. Sem ele, a continuacao de um literal de
/// string multilinha — que fica encostada na coluna 0 mesmo dentro de um `mod`
/// — passava por "bloco que me contem", e todo o `mod tests` abaixo dela
/// deixava de ser reconhecido como teste. `session.rs:1247` e uma dessas.
fn enclosing_opener(lines: &[&str], i: usize) -> Option<usize> {
    let ind = indent_of(lines[i])?;
    (0..i).rev().find(|&j| {
        indent_of(lines[j]).is_some_and(|x| x < ind) && lines[j].trim_end().ends_with('{')
    })
}

/// A linha `i` carrega `#[cfg(test)]` — nela mesma, ou nos atributos e
/// comentarios imediatamente acima, na mesma indentacao?
fn attributed_with_cfg_test(lines: &[&str], i: usize) -> bool {
    let Some(ind) = indent_of(lines[i]) else {
        return false;
    };
    if lines[i].trim_start().starts_with("#[cfg(test)]") {
        return true;
    }
    for j in (0..i).rev() {
        let Some(jind) = indent_of(lines[j]) else {
            continue; // linha em branco nao separa atributo de item
        };
        if jind != ind {
            return false;
        }
        let t = lines[j].trim_start();
        if let Some(rest) = t.strip_prefix("#[cfg(test)]") {
            // So atribui a NOSSA linha se estiver sozinho. Com item na mesma
            // linha (`#[cfg(test)] use std::fmt;`) aquela linha e um item
            // completo, que acabou nela — e e justamente essa forma que
            // cegava o cortador. Trata-la como atributo daria a resposta
            // errada com a mesma cara de certa.
            return rest.trim().is_empty();
        }
        // Outro atributo ou comentario entre o `#[cfg(test)]` e o item nao
        // encerra a busca — `#[allow(…)]` e doc-comment sao normais ali.
        if t.starts_with("#[") || t.starts_with("//") {
            continue;
        }
        return false;
    }
    false
}

/// A linha `i` esta dentro de algum item `#[cfg(test)]`, em qualquer
/// profundidade?
///
/// # Por que por indentacao, e nao contando chaves
///
/// Esta e a **segunda opiniao** sobre a mesma pergunta que [`production_source`]
/// responde. Ela so vale como guarda se chegar a resposta por outro caminho:
/// se as duas contassem chaves, o mesmo erro de contagem cegaria as duas
/// juntas — que e exatamente a falha que esta suite ja viu duas vezes. Aqui a
/// estrutura vem da INDENTACAO, e ela e confiavel porque `cargo fmt --check`
/// e gate de CI neste repositorio.
///
/// Sobe de bloco em bloco: basta que ALGUM ancestral seja `#[cfg(test)]` para
/// a linha ser codigo de teste. Chegar ao topo sem encontrar nenhum significa
/// producao.
fn is_under_cfg_test(lines: &[&str], i: usize) -> bool {
    let mut idx = i;
    loop {
        if attributed_with_cfg_test(lines, idx) {
            return true;
        }
        match enclosing_opener(lines, idx) {
            Some(parent) => idx = parent,
            None => return false,
        }
    }
}

/// **Nenhum item de producao pode desaparecer, em nenhuma profundidade.**
///
/// A asserção barata que faltava: se [`production_source`] apagar um item que
/// nao e de teste, apagou producao, e nao importa por qual caminho. Ela e
/// global e nao depende de haver violacao nenhuma na arvore, que e o que
/// distingue regra viva de regra ausente.
///
/// Foi isto que a versao anterior nao tinha: mover `#[cfg(test)] mod
/// source_scan;` de `mod.rs:76` para o topo do arquivo cegava `mod.rs`
/// inteiro — incluindo `pub const CONFIG_KEY` — sem nenhum teste piscar.
///
/// # Por que "em qualquer profundidade", e nao so coluna 0
///
/// A primeira versao olhava so a coluna 0, com o argumento de que o conteudo
/// de um `#[cfg(test)] mod tests { … }` e indentado. O argumento e verdadeiro
/// e a guarda era fraca do mesmo jeito: um `#[cfg(test)] use std::fmt;`
/// DENTRO de um `mod` interno apagava o resto daquele `mod` sem tocar em
/// nenhuma linha de coluna 0 — as tres varreduras cegas ao mesmo tempo, e a
/// guarda de topo acusando `[]`. Quem decide o que e teste aqui e
/// [`is_under_cfg_test`], que chega a resposta pela indentacao.
#[test]
fn production_source_never_drops_a_production_item() {
    const ITEM_STARTS: &[&str] = &[
        "pub ",
        "impl ",
        "impl<",
        "fn ",
        "async fn ",
        "struct ",
        "enum ",
        "trait ",
        "const ",
        "static ",
        "type ",
        "macro_rules!",
    ];
    for (name, source) in SOURCES {
        let texto = production_source(source);
        let producao: Vec<&str> = texto.lines().collect();
        let linhas: Vec<&str> = source.lines().collect();
        let mut vistos = 0usize;
        for (i, raw) in linhas.iter().enumerate() {
            let t = raw.trim_start();
            if !ITEM_STARTS.iter().any(|p| t.starts_with(p)) {
                continue;
            }
            if is_under_cfg_test(&linhas, i) {
                continue;
            }
            vistos += 1;
            assert_eq!(
                producao.get(i).copied(),
                Some(*raw),
                "{name}:{}: `{raw}` e um item de PRODUCAO e sumiu do texto \
varrido. A partir dali nenhuma das tres varreduras deste modulo ve mais nada.",
                i + 1
            );
        }
        // Sem isto a guarda seria vazia num arquivo em que `is_under_cfg_test`
        // resolvesse tudo como teste — ela passaria sem ter olhado nada.
        assert!(
            vistos > 0,
            "{name}: nenhum item de producao foi examinado; a guarda nao mediu nada"
        );
    }
}

/// A regra invertida do `expose` precisa reprovar de verdade.
///
/// Sem este teste, `expose_is_only_called_where_the_allowlist_says` e **vazio**:
/// numa arvore limpa nao ha violacao, entao "zero achados" nao distingue regra
/// viva de regra ausente — arrancar [`EXPOSE_ALLOWED`] inteira o deixava
/// verde. Aqui o caso negativo entra como TEXTO, e nao como estado da arvore.
#[test]
fn an_expose_outside_the_allowlist_is_actually_reported() {
    // O caso que justifica a regra invertida existir: o binding renomeado
    // duas vezes, que nenhuma leitura de macro alcanca.
    let renomeado = r#"
fn vaza(blob: &SessionBlob) {
    let s = blob.expose();
    let t = s;
    tracing::info!(dado = %t, "vazou");
}
"#;
    assert!(
        !exposes_outside_the_allowlist("mutacao.rs", renomeado).is_empty(),
        "`let s = blob.expose(); let t = s;` precisa ser reprovado pelo call site"
    );

    // Um `eprintln!` e um `tracing::event!` — as duas macros que a auditoria
    // R4 plantou e que passaram verdes na varredura de macro.
    for fonte in [
        r#"fn f(blob: &SessionBlob) { eprintln!("{}", blob.expose()); }"#,
        r#"fn f(blob: &SessionBlob) { tracing::event!(Level::INFO, s = %blob.expose()); }"#,
        r#"#[tracing::instrument(fields(s = %blob.expose()))] fn f() {}"#,
    ] {
        assert!(
            !exposes_outside_the_allowlist("mutacao.rs", fonte).is_empty(),
            "nao reprovado: {fonte}"
        );
    }

    // A allowlist precisa de fato isentar o que declara — se ela parasse de
    // isentar, `session.rs` ficaria vermelho e ninguem acrescentaria linha
    // nenhuma conscientemente.
    let permitido = "fn save(blob: &SessionBlob) {\n    let mut in_out = blob.expose().as_bytes().to_vec();\n}\n";
    assert!(
        exposes_outside_the_allowlist("session.rs", permitido).is_empty(),
        "o call site declarado na allowlist nao pode ser reprovado"
    );
    // ...e so para o arquivo declarado: a mesma linha em outro arquivo e
    // violacao.
    assert!(
        !exposes_outside_the_allowlist("runner.rs", permitido).is_empty(),
        "a allowlist e por arquivo — a mesma linha em `runner.rs` e violacao"
    );

    // Comentario nao e chamada.
    let comentario = "fn f() {\n    // nada de blob.expose() aqui\n    let x = 1;\n}\n";
    assert!(
        exposes_outside_the_allowlist("ok.rs", comentario).is_empty(),
        "um comentario citando expose() nao e um call site"
    );
}

/// `SessionBlob` e `#[serde(transparent)]`: um `serde_json::to_string(&blob)`
/// futuro carrega o valor **cru** sem a palavra `expose` aparecer no fonte.
///
/// E o unico caminho que nem a allowlist de [`EXPOSE_ALLOWED`] nem a varredura
/// de macro alcancam — esta nota existe para que quem mexer aqui saiba que a
/// serializacao e o buraco conhecido, e que fecha-lo e trabalho de tipo
/// (`Serialize` manual, ou um wrapper), nao de varredura de texto.
#[test]
fn the_transparent_serialize_is_a_known_and_documented_hole() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("#[serde(transparent)]"),
        "se a serializacao transparente saiu, atualize esta nota"
    );
}
