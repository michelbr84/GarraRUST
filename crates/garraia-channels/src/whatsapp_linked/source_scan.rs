//! Varredura do proprio fonte: garantias que o compilador nao expressa.
//!
//! Mesmo padrao do teste de `garraia-desktop-core::detect`, que varre o fonte
//! atras de `Command::new`/`.spawn()`. Aqui as regras sao tres, e todas tem a
//! mesma raiz: **o blob de sessao nao pode aparecer num log**.
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

/// Linhas de log (`info!`/`warn!`/`debug!`/`error!`/`trace!`), com o numero da
/// linha, ignorando comentario e bloco de teste.
fn log_lines(source: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
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
        if line.starts_with("//") {
            continue;
        }
        if ["info!", "warn!", "debug!", "error!", "trace!"]
            .iter()
            .any(|m| line.contains(m))
        {
            out.push((i + 1, raw));
        }
    }
    out
}

/// Nenhuma macro de log pode carregar o valor da sessao.
#[test]
fn no_log_line_mentions_the_session_value() {
    // Padroes que significariam "o valor foi para o log": o `expose()` cru, o
    // `Display`/`Debug` de um binding chamado `session`/`blob`, e o campo
    // `session` de um evento.
    const FORBIDDEN: &[&str] = &[
        "expose()",
        "%session",
        "?session",
        "{session}",
        "%blob",
        "?blob",
        "{blob}",
        "session = ",
        "blob = ",
        ".0",
    ];

    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        for (line_no, line) in log_lines(source) {
            for pattern in FORBIDDEN {
                if line.contains(pattern) {
                    offenders.push(format!("{name}:{line_no}: {} ({pattern})", line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "linha de log carregando material de sessao:\n{}",
        offenders.join("\n")
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
