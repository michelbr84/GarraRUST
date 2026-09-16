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

/// Nomes que, no lugar onde um valor cabe, significam "o segredo foi logado".
///
/// Sao **identificadores**, e nao os padroes `%blob`/`{blob}`/`blob = ` de
/// antes: a busca nao acontece mais na linha crua, e sim na parte da invocacao
/// que pode carregar valor ([`log_audit::parte_arriscada`]), onde `%`, `?`, `=`
/// e `{}` ja foram descartados. Procurar por `"%blob"` ali nunca casaria.
const PROIBIDOS: &[&str] = &["expose", "blob", "session", "secret", "passphrase"];

/// O que contamina um binding: se um `let` nasce disto, o nome dele passa a
/// valer como proibido tambem.
const GATILHOS: &[&str] = &["expose", "SessionBlob", ".blob"];

/// O que **quebra** a contaminacao: uma redacao. Aqui nao ha nenhuma hoje — o
/// blob de sessao nao tem forma resumida logavel, ao contrario do `Jid`, que
/// tem `last4()`. A constante existe para que a proxima tenha onde entrar em
/// vez de virar excecao solta no meio do teste.
const ANTIDOTOS: &[&str] = &[];

/// Nenhuma macro de log pode carregar o valor da sessao.
///
/// # O que mudou, e por que
///
/// A versao anterior casava padrao **linha a linha**, e o `rustfmt` quebra toda
/// macro `tracing!` com campos estruturados em varias linhas. Duas mutacoes
/// sobreviviam a ela — `tracing::info!(\n %blob,\n "...")` e
/// `let apelido = blob.expose().to_string(); info!("{apelido}")` — enquanto as
/// mesmas duas numa linha so eram pegas. O teste era funcao da formatacao.
///
/// Agora ele usa o mesmo par do gateway ([`log_audit`]): a invocacao inteira,
/// atravessando linhas, e so entao a parte que pode carregar valor. E
/// [`log_audit::bindings_contaminados`] fecha o rename.
#[test]
fn no_log_line_mentions_the_session_value() {
    use super::log_audit;

    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        let sujos = log_audit::bindings_contaminados(source, GATILHOS, ANTIDOTOS);
        let mut proibidos: Vec<String> = PROIBIDOS.iter().map(|s| s.to_string()).collect();
        proibidos.extend(sujos.iter().cloned());

        for chamada in log_audit::chamadas_de_log(source) {
            let risco = log_audit::parte_arriscada(&chamada);
            for proibido in &proibidos {
                if risco.contains(proibido.as_str()) {
                    offenders.push(format!(
                        "{name}: `{proibido}` em: {}",
                        chamada.split_whitespace().collect::<Vec<_>>().join(" ")
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "log carregando material de sessao:\n{}",
        offenders.join("\n")
    );
}

/// O proprio detector, contra as duas mutacoes que a varredura antiga deixava
/// passar. Sem isto, trocar o corpo de `no_log_line_mentions_the_session_value`
/// por `assert!(true)` seria invisivel.
#[test]
fn a_varredura_pega_as_duas_formas_que_a_antiga_deixava_passar() {
    use super::log_audit;

    let multilinha =
        "fn f() {\n    tracing::info!(\n        %blob,\n        \"sessao\"\n    );\n}\n";
    let renomeado =
        "fn f() {\n    let apelido = blob.expose().to_string();\n    info!(\"{apelido}\");\n}\n";

    for (rotulo, fonte) in [("multi-linha", multilinha), ("renomeado", renomeado)] {
        let sujos = log_audit::bindings_contaminados(fonte, GATILHOS, ANTIDOTOS);
        let mut proibidos: Vec<String> = PROIBIDOS.iter().map(|s| s.to_string()).collect();
        proibidos.extend(sujos);

        let pegou = log_audit::chamadas_de_log(fonte).iter().any(|c| {
            let risco = log_audit::parte_arriscada(c);
            proibidos.iter().any(|p| risco.contains(p.as_str()))
        });
        assert!(pegou, "a varredura precisa pegar o vazamento {rotulo}");
    }
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
