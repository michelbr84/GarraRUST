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
//! que este modulo corrige — e cada conserto do parser (string crua, literal
//! de char, comentario de bloco, macro sem delimitador) chega as duas de uma
//! vez por isso.
//!
//! # Duas regras, e a ordem importa
//!
//! 1. [`EXPOSE_ALLOWED`] — regra **fechada**: `expose()` so pode aparecer onde
//!    a lista diz. E a linha de defesa principal, porque e no call site que a
//!    protecao de tipo acaba: dali em diante o valor e um `&str` como outro
//!    qualquer, e nenhuma leitura de macro alcanca
//!    `let s = blob.expose(); let t = s; info!(dado = %t)`.
//! 2. [`PROIBIDOS`]/[`PADROES_PROIBIDOS`] — regra **aberta**: reprova o que
//!    conhece. Segunda linha, e por construcao incompleta (enumerar macro e
//!    uma corrida que nao se ganha). Ela pega o que a primeira nao pega: um
//!    campo chamado `session`/`blob`/`creds`/`qr` que nunca passou por
//!    `expose()`.

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

/// **Os unicos call sites de `SessionBlob::expose()` que existem.**
///
/// # Por que a regra e invertida
///
/// A varredura de macro reprova o que ela conhece. Uma auditoria mostrou o
/// custo disso plantando `eprintln!` e `tracing::event!` com `blob.expose()`:
/// os dois passaram verdes. E registrou a limitacao de fundo, que nenhuma
/// lista de macros resolve:
///
/// ```ignore
/// let s = blob.expose();
/// let t = s;
/// tracing::info!(dado = %t);   // nenhuma varredura de macro ve isto
/// ```
///
/// A linha de defesa certa e o call site do `expose()`, porque e ali que a
/// protecao de tipo acaba. Entao a regra passou a ser fechada: **toda**
/// ocorrencia de `expose` no codigo de producao destes arquivos precisa estar
/// nomeada aqui, e acrescentar uma linha e uma decisao consciente, com nome e
/// diff, em vez de um silencio.
///
/// A checagem e sobre a linha de CODIGO (comentario fora), no mesmo espirito
/// de [`the_store_exposes_a_single_validating_constructor`]: ela pergunta se a
/// palavra continua onde deve, e quem de fato impede a chamada e o rustc.
const EXPOSE_ALLOWED: &[(&str, &str)] = &[
    // A declaracao. Ela precisa existir: o bridge e o store leem o blob.
    ("session.rs", "pub fn expose(&self) -> &str {"),
    // O unico uso: o que vai para o AES-GCM dentro de `SessionStore::save`.
    (
        "session.rs",
        "let mut in_out = blob.expose().as_bytes().to_vec();",
    ),
];

/// Nomes que, no lugar onde um valor cabe, significam "o segredo foi logado".
///
/// Sao **identificadores**, e nao os padroes `%blob`/`{blob}`/`blob = `: a
/// busca acontece na parte da invocacao que pode carregar valor
/// ([`log_audit::parte_arriscada`]), onde `%`, `?`, `=` e `{}` ja foram
/// descartados. Procurar por `"%blob"` ali nunca casaria.
const PROIBIDOS: &[&str] = &["expose", "blob", "session", "secret", "passphrase", "creds"];

/// Padroes conferidos no texto **cru** do bloco, e nao na parte arriscada.
///
/// Existem para os nomes genericos demais para virar identificador proibido:
/// `data` casaria `data_dir` e `metadata`, e `qr` casaria qualquer coisa. Com
/// o sigilo na frente (`%qr`, `{qr}`, `qr = `) a busca volta a ser precisa.
///
/// L4: a string crua do QR e uma credencial de ~20 s. Ela e impressa
/// literalmente em `Style::Raw` — por design, e e o que o usuario le com a
/// camera —, mas nada dela pertence a um log. O QR nunca passa por
/// `SessionBlob`, entao a allowlist de `expose()` nao o cobre.
const PADROES_PROIBIDOS: &[&str] = &[
    "%qr", "?qr", "{qr}", "qr = ", "%data", "?data", "{data}", "data = ", ".0",
];

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
        for padrao in PADROES_PROIBIDOS {
            if chamada.contains(padrao) {
                saida.push(format!("{name}:{linha}: `{padrao}` em: {chamada}"));
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

/// **A regra invertida: `expose()` so pode aparecer onde a allowlist diz.**
///
/// A varredura de macro reprova o que conhece; esta reprova tudo o que nao foi
/// declarado. E a unica das duas que sobrevive a um `eprintln!`, a um
/// `tracing::event!`, a um `#[tracing::instrument(fields(…))]` e ao
/// `let s = blob.expose(); let t = s;` que nenhuma leitura de macro alcanca.
/// Os `expose()` de um fonte que a allowlist nao declara.
///
/// Funcao separada do `#[test]` pelo mesmo motivo de [`blocos_ofensivos`]: e
/// ela que o teste do proprio detector exercita, com as mutacoes como entrada.
/// Uma allowlist so tem caso negativo quando alguem o planta — na arvore
/// limpa, por construcao, nao ha nenhum, e um teste que so varre a arvore
/// passaria verde com a regra inteira arrancada.
fn exposes_fora_da_allowlist(name: &str, source: &str) -> Vec<String> {
    let mut saida = Vec::new();
    let code = log_audit::codigo_de_producao(source);
    for (i, raw) in code.lines().enumerate() {
        let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.contains("expose") {
            continue;
        }
        if EXPOSE_ALLOWED
            .iter()
            .any(|(f, allowed)| *f == name && line.contains(allowed))
        {
            continue;
        }
        saida.push(format!("{name}:{}: {line}", i + 1));
    }
    saida
}

#[test]
fn expose_is_only_called_where_the_allowlist_says() {
    let mut offenders = Vec::new();
    for (name, source) in SOURCES {
        offenders.extend(exposes_fora_da_allowlist(name, source));
    }
    assert!(
        offenders.is_empty(),
        "`SessionBlob::expose()` fora da allowlist — a partir do call site o valor \
e um `&str` e a protecao de tipo acabou. Se o uso e legitimo, declare-o em \
EXPOSE_ALLOWED:\n{}",
        offenders.join("\n")
    );
}

/// **O detector da regra fechada, contra o que a regra aberta nao alcanca.**
///
/// Sem este teste, `expose_is_only_called_where_the_allowlist_says` passaria
/// verde com a allowlist arrancada: na arvore limpa nao ha violacao, entao
/// "zero achados" nao distingue regra viva de regra ausente. As entradas aqui
/// sao plantadas como TEXTO, e nao na arvore de verdade.
#[test]
fn a_regra_fechada_pega_o_que_nenhuma_leitura_de_macro_alcanca() {
    // O caso que motivou a inversao: dois renames antes do log. Nenhuma
    // varredura de macro ve `%t` como segredo — mas o `expose()` esta ali.
    let renomeado_duas_vezes = r#"
fn vaza(blob: &SessionBlob) {
    let s = blob.expose();
    let t = s;
    tracing::info!(dado = %t);
}
"#;
    assert!(
        !exposes_fora_da_allowlist("mutacao.rs", renomeado_duas_vezes).is_empty(),
        "o call site do `expose()` e a linha de defesa: dali em diante o valor \
         e um `&str` e nenhuma leitura de macro alcanca o rename"
    );
    assert!(
        blocos_ofensivos("mutacao.rs", renomeado_duas_vezes)
            .iter()
            .all(|o| !o.contains("%t")),
        "premissa: e justamente o que a regra aberta NAO pega — se ela passar a \
         pegar, esta asserção vira ruido e deve ser reescrita, nao apagada"
    );

    // Saida que nenhuma lista de macros cobre por completo.
    let atributo = r#"
#[tracing::instrument(fields(sessao = %blob.expose()))]
fn persistir(blob: &SessionBlob) {}
"#;
    assert!(
        !exposes_fora_da_allowlist("atributo.rs", atributo).is_empty(),
        "`#[instrument(fields(…))]` nao e macro de log, e leva o valor ao span"
    );

    // E o que a allowlist declara continua passando, senao a regra condenaria
    // o unico uso legitimo que existe.
    let legitimo = "fn save() {\n    let mut in_out = blob.expose().as_bytes().to_vec();\n}\n";
    assert!(
        exposes_fora_da_allowlist("session.rs", legitimo).is_empty(),
        "o call site declarado do AES-GCM nao pode ser reprovado"
    );
    // ... e so no arquivo em que foi declarado.
    assert!(
        !exposes_fora_da_allowlist("runner.rs", legitimo).is_empty(),
        "a allowlist e por ARQUIVO: a mesma linha noutro arquivo nao esta declarada"
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
        let code = log_audit::codigo_de_producao(source);
        assert!(
            code.lines().any(|l| l
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains(allowed)),
            "{name}: a allowlist declara `{allowed}`, que nao existe mais no fonte de \
producao — allowlist morta nao guarda nada"
        );
    }
}

/// **Cada macro de log em producao tem de virar exatamente um bloco.**
///
/// Ancora de um arquivo so nao serve: hoje `runner.rs` tem os blocos e a
/// maioria dos outros tem zero cada, entao um `b'"'` plantado em `mod.rs`,
/// `protocol.rs`, `state.rs`, `session.rs`, `qr.rs`, `bridge.rs` ou `health.rs`
/// cegaria o parser **em silencio** — nao ha ancora possivel num arquivo sem
/// log. Esta asserção e global e nao depende de um log especifico existir: ela
/// compara o que o parser devolveu com uma contagem crua do mesmo texto.
#[test]
fn every_log_macro_in_production_yields_exactly_one_block() {
    let total: usize = SOURCES
        .iter()
        .map(|(_, source)| log_audit::conta_macros_de_log(source))
        .sum();
    assert!(
        total > 0,
        "premissa: o canal TEM log em producao. Se a contagem total zerar, esta \
         asserção passa a comparar zero com zero em todo arquivo e nao guarda \
         mais nada — que e o modo de falha que ela existe para pegar"
    );

    for (name, source) in SOURCES {
        let esperado = log_audit::conta_macros_de_log(source);
        let blocos = log_audit::chamadas_de_log(source);
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

/// Um literal de char com aspa dentro nao pode cegar o arquivo inteiro.
///
/// `bridge.rs` e o arquivo de enquadramento NDJSON — o candidato mais natural
/// do repositorio a ganhar um `b'"'`. Antes, uma aspa desemparelhada fazia o
/// parser ler dali ate a proxima aspa do arquivo como "string", invertia todas
/// as fronteiras e devolvia **zero** blocos.
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
        !blocos_ofensivos("mutacao.rs", mutacao).is_empty(),
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
        !blocos_ofensivos("lifetime.rs", com_lifetime).is_empty(),
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
        blocos_ofensivos("ok.rs", ok).is_empty(),
        "log sem material de sessao nao pode ser reprovado"
    );
}

/// Ancora na arvore de verdade: o `tracing::info!` multilinha que ja existe em
/// producao e lido INTEIRO. Se a varredura voltar ao modo linha, este teste
/// morre junto com a garantia — e ele nao depende de ninguem ter imaginado a
/// mutacao certa.
///
/// Complementa, e nao substitui,
/// [`every_log_macro_in_production_yields_exactly_one_block`]: aquela pega a
/// cegueira em arquivo **sem** log, que ancora nenhuma consegue cobrir; esta
/// pega o conteudo do bloco, que a contagem nao olha.
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
