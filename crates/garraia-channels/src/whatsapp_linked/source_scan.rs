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
//! # Uma implementacao, duas varreduras
//!
//! A leitura do texto nao mora aqui: mora em [`super::log_audit`], que o
//! gateway usa para varrer o **seu** fonte com as mesmas regras. Duas copias
//! que precisam ser mantidas iguais por disciplina foi exatamente o defeito
//! que este modulo corrige.

use super::log_audit;

const SOURCES: &[(&str, &str)] = &[
    ("mod.rs", include_str!("mod.rs")),
    ("protocol.rs", include_str!("protocol.rs")),
    ("state.rs", include_str!("state.rs")),
    ("session.rs", include_str!("session.rs")),
    ("qr.rs", include_str!("qr.rs")),
    ("bridge.rs", include_str!("bridge.rs")),
    ("runner.rs", include_str!("runner.rs")),
    ("health.rs", include_str!("health.rs")),
];

/// Nomes que, no lugar onde um valor cabe, significam "o segredo foi logado".
///
/// Sao **identificadores**, e nao os padroes `%blob`/`{blob}`/`blob = ` de
/// antes: a busca nao acontece mais na linha crua, e sim na parte da invocacao
/// que pode carregar valor ([`log_audit::parte_arriscada`]), onde `%`, `?`, `=`
/// e `{}` ja foram descartados. Procurar por `"%blob"` ali nunca casaria.
const PROIBIDOS: &[&str] = &["expose", "blob", "session", "secret", "passphrase", "creds"];

/// O que contamina um binding: se um `let` nasce disto, o nome dele passa a
/// valer como proibido tambem.
const GATILHOS: &[&str] = &["expose", "SessionBlob", ".blob"];

/// O que **quebra** a contaminacao: uma redacao. Aqui nao ha nenhuma hoje — o
/// blob de sessao nao tem forma resumida logavel, ao contrario do `Jid`, que
/// tem `last4()`. A constante existe para que a proxima tenha onde entrar em
/// vez de virar excecao solta no meio do teste.
const ANTIDOTOS: &[&str] = &[];

/// Chamadas de log que carregam material de sessao, ja formatadas para a
/// mensagem de falha.
///
/// Funcao separada do `#[test]` porque e ela que o teste do proprio parser
/// exercita, com as mutacoes como entrada — plantar a mutacao na arvore de
/// verdade so para prova-la seria plantar um vazamento.
fn blocos_ofensivos(name: &str, source: &str) -> Vec<String> {
    let mut proibidos: Vec<String> = PROIBIDOS.iter().map(|s| s.to_string()).collect();
    proibidos.extend(log_audit::bindings_contaminados(
        source, GATILHOS, ANTIDOTOS,
    ));

    let mut saida = Vec::new();
    for (linha, chamada) in log_audit::chamadas_de_log(source) {
        let risco = log_audit::parte_arriscada(&chamada);
        for proibido in &proibidos {
            if risco.contains(proibido.as_str()) {
                saida.push(format!("{name}:{linha}: `{proibido}` em: {chamada}"));
            }
        }
    }
    saida
}

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
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        offenders.extend(blocos_ofensivos(name, source));
    }
    assert!(
        offenders.is_empty(),
        "log carregando material de sessao:\n{}",
        offenders.join("\n")
    );
}

/// `SOURCES` e lista manual, e lista manual fica para tras: `health.rs` entrou
/// no modulo sem entrar aqui, no **mesmo PR** que criou o arquivo.
///
/// Este teste nao tenta adivinhar o conteudo — so exige que a contagem de
/// entradas bata com a de `mod`/`pub mod` declarados em `mod.rs`. Um arquivo
/// novo no modulo e um vermelho de uma linha, e nao um ponto cego silencioso.
#[test]
fn every_module_of_the_channel_is_scanned() {
    let mod_rs = include_str!("mod.rs");
    let mut declarados: Vec<&str> = Vec::new();
    for linha in mod_rs.lines() {
        let linha = linha.trim();
        let resto = linha
            .strip_prefix("pub mod ")
            .or_else(|| linha.strip_prefix("pub(crate) mod "))
            .or_else(|| linha.strip_prefix("mod "));
        let Some(resto) = resto else { continue };
        let Some(nome) = resto.strip_suffix(';') else {
            continue;
        };
        declarados.push(nome);
    }
    declarados.sort_unstable();

    // `source_scan` e `log_audit` sao a propria varredura: ela nao se varre.
    let esperados: Vec<&str> = declarados
        .iter()
        .copied()
        .filter(|m| *m != "source_scan" && *m != "log_audit")
        .collect();

    let mut vistos: Vec<String> = SOURCES
        .iter()
        .filter(|(n, _)| *n != "mod.rs")
        .map(|(n, _)| n.trim_end_matches(".rs").to_string())
        .collect();
    vistos.sort();

    assert_eq!(
        vistos, esperados,
        "SOURCES ficou para tras de `mod.rs` — todo modulo do canal tem de ser varrido"
    );
}

/// O proprio detector, contra as duas mutacoes que a varredura antiga deixava
/// passar. Sem isto, trocar o corpo de `no_log_line_mentions_the_session_value`
/// por `assert!(true)` seria invisivel.
#[test]
fn a_varredura_pega_as_duas_formas_que_a_antiga_deixava_passar() {
    let multilinha =
        "fn f() {\n    tracing::info!(\n        %blob,\n        \"sessao\"\n    );\n}\n";
    let renomeado =
        "fn f() {\n    let apelido = blob.expose().to_string();\n    info!(\"{apelido}\");\n}\n";

    for (rotulo, fonte) in [("multi-linha", multilinha), ("renomeado", renomeado)] {
        assert!(
            !blocos_ofensivos("mutacao.rs", fonte).is_empty(),
            "a varredura precisa pegar o vazamento {rotulo}"
        );
    }
}

/// O parser enxerga a macro que o `rustfmt` quebrou em varias linhas, e nao
/// inventa achado em cima de log honesto.
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
        !blocos_ofensivos("mutacao.rs", multiline).is_empty(),
        "a macro quebrada em varias linhas precisa ser reprovada"
    );

    // A mesma coisa numa linha so — ja era pega antes, e continua sendo.
    let single = r#"tracing::info!(sessao = %blob.expose(), "sessao persistida");"#;
    assert!(
        !blocos_ofensivos("mutacao.rs", single).is_empty(),
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
        blocos_ofensivos("ok.rs", ok).is_empty(),
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
        blocos_ofensivos("comentario.rs", commented).is_empty(),
        "um comentario dentro do bloco nao e material logado"
    );

    // Parentese dentro da mensagem nao pode desbalancear a leitura: se
    // desbalanceasse, o bloco seguinte seria engolido e nunca examinado.
    let paren_in_message = r#"
tracing::info!("aviso :-) nao fecha nada");
tracing::warn!(sessao = %blob.expose(), "depois");
"#;
    assert!(
        !blocos_ofensivos("paren.rs", paren_in_message).is_empty(),
        "um parentese dentro da mensagem nao pode esconder o log seguinte"
    );
}

/// Ancora na arvore de verdade: o `tracing::info!` multilinha que ja existe em
/// producao e lido INTEIRO. Se a varredura voltar ao modo linha, este teste
/// morre junto com a garantia — e ele nao depende de ninguem ter imaginado a
/// mutacao certa.
#[test]
fn the_scan_sees_the_multiline_log_that_already_exists_in_production() {
    let blocos = log_audit::chamadas_de_log(include_str!("runner.rs"));
    let achou = blocos.iter().any(|(_, bloco)| {
        bloco.contains("attempt")
            && bloco.contains("delay_ms = delay")
            && bloco.contains("WhatsApp vinculado caiu; reconectando")
    });
    assert!(
        achou,
        "o scan precisa ler o bloco inteiro do `tracing::info!` de `serve`; \
blocos vistos:\n{}",
        blocos
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
