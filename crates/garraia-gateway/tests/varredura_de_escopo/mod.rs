//! Varredura estatica de quem chama o runtime do agente (#1343 S4/S5).
//!
//! Um modulo, dois guardas: `crates/garraia-gateway/tests/approval_scope_coverage.rs`
//! e `crates/garraia-cli/tests/approval_scope_oneshot.rs` (este o inclui por
//! `#[path]`). Correcao no detector vale para os dois.
//!
//! O detector le o fonte com comentarios e conteudo de literais apagados —
//! mesmo tamanho em bytes, quebras de linha mantidas, entao posicao e numero
//! de linha continuam valendo. `process_message*` citado num comentario nao e
//! chamada, e um `"{"` numa string nao desalinha o casamento de chaves. Os
//! modulos `#[cfg(test)] mod x { .. }` sao apagados inteiros, e o codigo de
//! producao que vem DEPOIS deles continua sendo lido; os `#[cfg(test)] mod
//! x;` sao so declaracoes, e [`varrer`] pula o arquivo deles.
//!
//! Regra de ouro: na duvida, "sem escopo". Um falso "sem escopo" reprova o
//! teste e alguem olha; um falso "escopada" seria um buraco silencioso.

// Cada guarda usa uma parte do modulo.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Os pontos de entrada do `AgentRuntime` que rodam um turno.
pub const ENTRADAS: &[&str] = &[
    "process_message",
    "process_message_with_context",
    "process_message_with_agent_config",
    "process_message_streaming",
    "process_message_streaming_with_context",
    "process_message_streaming_with_agent_config",
    "process_message_streaming_with_events",
    "process_heartbeat",
];

/// Casam o prefixo `process_message` mas sao privados do runtime: ninguem de
/// fora consegue chama-los.
const PRIVADAS: &[&str] = &["process_message_impl"];

/// Nome usado quando a chamada nao esta dentro de nenhuma `fn`.
pub const FORA_DE_FN: &str = "<fora de fn>";

/// A funcao que monta o `ExecContext` com escopo de aprovacao.
pub struct Construtor<'a> {
    /// O nome dela (ultimo segmento do caminho).
    pub nome: &'a str,
    /// Os caminhos aceitos para chama-la. Outro caminho (um `use` e o nome
    /// solto, por exemplo) conta como "sem escopo".
    pub caminhos: &'a [&'a str],
}

/// Um arquivo pronto para a varredura.
pub struct Fonte {
    /// Comentarios e conteudo de literais apagados; modulos de teste
    /// apagados. E sobre este texto que a estrutura e lida.
    pub codigo: String,
    /// So comentarios e modulos de teste apagados: o texto de um argumento
    /// sai daqui (um canal `"discord"` continua legivel).
    pub texto: String,
    /// Os `#[cfg(test)] mod nome;` do arquivo.
    pub modulos_de_teste: Vec<String>,
}

/// Uma chamada a um ponto de entrada do runtime.
#[derive(Debug)]
pub struct Chamada {
    pub linha: usize,
    /// A `fn` que contem a chamada ([`FORA_DE_FN`] se nenhuma).
    pub funcao: String,
    /// Qual de [`ENTRADAS`].
    pub entrada: String,
    /// Os argumentos (normalizados) do construtor de escopo, quando o
    /// ultimo argumento da chamada comprovadamente passa por ele.
    pub escopo: Option<Vec<String>>,
}

impl Chamada {
    pub fn escopada(&self) -> bool {
        self.escopo.is_some()
    }
}

/// Uma chamada ao construtor de escopo, onde quer que esteja.
#[derive(Debug)]
pub struct Construcao {
    pub linha: usize,
    pub funcao: String,
    /// O caminho usado (`crate::approval_scope::com_escopo`).
    pub caminho: String,
    /// Os argumentos, normalizados (sem espacos, sem `&` na frente).
    pub args: Vec<String>,
}

impl Fonte {
    pub fn nova(fonte: &str) -> Self {
        let codigo = limpar(fonte, true);
        let (apagar, modulos_de_teste) = modulos_de_teste(&codigo);
        Self {
            codigo: apagar_trechos(codigo, &apagar),
            texto: apagar_trechos(limpar(fonte, false), &apagar),
            modulos_de_teste,
        }
    }

    fn linha(&self, pos: usize) -> usize {
        self.codigo[..pos].matches('\n').count() + 1
    }
}

fn e_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Troca por espaco cada byte dos trechos (menos `\n`). Os trechos caem em
/// fronteira de char, entao o resultado continua UTF-8.
fn apagar_trechos(s: String, trechos: &[(usize, usize)]) -> String {
    let mut b = s.into_bytes();
    for &(ini, fim) in trechos {
        for x in &mut b[ini.min(fim)..fim] {
            if *x != b'\n' {
                *x = b' ';
            }
        }
    }
    String::from_utf8(b).expect("so troca bytes por espaco ASCII")
}

/// Apaga comentarios e, com `literais`, o conteudo de strings e chars (as
/// aspas ficam: um argumento literal continua reconhecivel).
fn limpar(fonte: &str, literais: bool) -> String {
    let cs: Vec<(usize, char)> = fonte.char_indices().collect();
    let pos = |k: usize| cs.get(k).map_or(fonte.len(), |x| x.0);
    let c = |k: usize| cs.get(k).map(|x| x.1);
    let mut apagar = Vec::new();
    let mut k = 0;
    while k < cs.len() {
        let antes = k.checked_sub(1).and_then(c);
        match cs[k].1 {
            '/' if c(k + 1) == Some('/') => {
                let mut j = k;
                while j < cs.len() && cs[j].1 != '\n' {
                    j += 1;
                }
                apagar.push((pos(k), pos(j)));
                k = j;
            }
            '/' if c(k + 1) == Some('*') => {
                let mut prof = 0usize;
                let mut j = k;
                while j < cs.len() {
                    if cs[j].1 == '/' && c(j + 1) == Some('*') {
                        prof += 1;
                        j += 2;
                    } else if cs[j].1 == '*' && c(j + 1) == Some('/') {
                        prof -= 1;
                        j += 2;
                        if prof == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                apagar.push((pos(k), pos(j)));
                k = j;
            }
            '"' => {
                let mut j = k + 1;
                while j < cs.len() && cs[j].1 != '"' {
                    if cs[j].1 == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
                if literais {
                    apagar.push((pos(k + 1), pos(j)));
                }
                k = j + 1;
            }
            // `r"..."`, `r#"..."#`, `br"..."`. `r#ident` e identificador.
            'r' if !antes.is_some_and(e_ident)
                || (antes == Some('b') && !k.checked_sub(2).and_then(c).is_some_and(e_ident)) =>
            {
                let mut j = k + 1;
                let mut hashes = 0;
                while c(j) == Some('#') {
                    hashes += 1;
                    j += 1;
                }
                if c(j) != Some('"') {
                    k += 1;
                    continue;
                }
                let ini = j + 1;
                let mut m = ini;
                while m < cs.len()
                    && !(cs[m].1 == '"' && (1..=hashes).all(|h| c(m + h) == Some('#')))
                {
                    m += 1;
                }
                if literais {
                    apagar.push((pos(ini), pos(m)));
                }
                k = m + 1 + hashes;
            }
            // char literal; `'a` sem fechamento logo depois e lifetime.
            '\'' => {
                if c(k + 1) == Some('\\') {
                    let mut j = k + 3;
                    while j < cs.len() && cs[j].1 != '\'' && j < k + 12 {
                        j += 1;
                    }
                    if literais {
                        apagar.push((pos(k + 1), pos(j)));
                    }
                    k = j + 1;
                } else if c(k + 2) == Some('\'') {
                    if literais {
                        apagar.push((pos(k + 1), pos(k + 2)));
                    }
                    k += 3;
                } else {
                    k += 1;
                }
            }
            _ => k += 1,
        }
    }
    apagar_trechos(fonte.to_string(), &apagar)
}

fn pular_espaco(s: &str, mut p: usize) -> usize {
    while let Some(c) = s[p..].chars().next() {
        if !c.is_whitespace() {
            break;
        }
        p += c.len_utf8();
    }
    p
}

fn fim_do_ident(s: &str, p: usize) -> usize {
    s[p..]
        .find(|c: char| !e_ident(c))
        .map_or(s.len(), |n| p + n)
}

/// O identificador logo antes de `p` (pulando espaco).
fn palavra_antes(s: &str, p: usize) -> &str {
    let t = s[..p].trim_end();
    let ini = t.trim_end_matches(e_ident).len();
    &t[ini..]
}

/// Posicoes de `palavra` como palavra inteira.
fn palavras(s: &str, palavra: &str) -> Vec<usize> {
    let mut fora = Vec::new();
    let mut desde = 0;
    while let Some(rel) = s[desde..].find(palavra) {
        let p = desde + rel;
        let fim = p + palavra.len();
        if !s[..p].chars().next_back().is_some_and(e_ident)
            && !s[fim..].chars().next().is_some_and(e_ident)
        {
            fora.push(p);
        }
        desde = fim;
    }
    fora
}

/// O `)`/`]`/`}` que fecha o delimitador aberto em `abre`.
fn fecha(s: &str, abre: usize) -> Option<usize> {
    let mut prof = 0i32;
    for (i, c) in s[abre..].char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => {
                prof -= 1;
                if prof == 0 {
                    return Some(abre + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Os `#[cfg(test)] mod x { .. }` (trechos a apagar) e os `#[cfg(test)] mod
/// x;` (nomes) do arquivo. So o `#[cfg(test)]` exato conta: um
/// `#[cfg(any(test, feature = ".."))]` e producao com a feature ligada.
fn modulos_de_teste(codigo: &str) -> (Vec<(usize, usize)>, Vec<String>) {
    const MARCA: &str = "#[cfg(test)]";
    let mut apagar = Vec::new();
    let mut nomes = Vec::new();
    let mut desde = 0;
    while let Some(rel) = codigo[desde..].find(MARCA) {
        let ini = desde + rel;
        desde = ini + MARCA.len();
        let mut p = pular_espaco(codigo, desde);
        // outros atributos no mesmo item
        while codigo[p..].starts_with("#[") {
            let Some(f) = fecha(codigo, p + 1) else { break };
            p = pular_espaco(codigo, f + 1);
        }
        if palavra_em(codigo, p, "pub") {
            p = pular_espaco(codigo, p + 3);
            if codigo[p..].starts_with('(')
                && let Some(f) = fecha(codigo, p)
            {
                p = pular_espaco(codigo, f + 1);
            }
        }
        if !palavra_em(codigo, p, "mod") {
            continue;
        }
        let n = pular_espaco(codigo, p + 3);
        let n_fim = fim_do_ident(codigo, n);
        if n_fim == n {
            continue;
        }
        let q = pular_espaco(codigo, n_fim);
        if codigo[q..].starts_with(';') {
            nomes.push(codigo[n..n_fim].to_string());
        } else if codigo[q..].starts_with('{')
            && let Some(f) = fecha(codigo, q)
        {
            apagar.push((ini, f + 1));
            desde = f + 1;
        }
    }
    (apagar, nomes)
}

fn palavra_em(s: &str, p: usize, palavra: &str) -> bool {
    s[p..].starts_with(palavra) && !s[p + palavra.len()..].starts_with(e_ident)
}

/// A `fn` mais interna cujo corpo contem `pos`: nome e `(abre, fecha)` do
/// corpo.
fn funcao_que_contem(s: &str, pos: usize) -> Option<(String, usize, usize)> {
    for f in palavras(&s[..pos], "fn").into_iter().rev() {
        let n = pular_espaco(s, f + 2);
        let n_fim = fim_do_ident(s, n);
        if n_fim == n {
            continue; // `fn(..)`: tipo ponteiro de funcao
        }
        let mut prof = 0i32;
        let mut abre = None;
        for (i, c) in s[n_fim..].char_indices() {
            match c {
                '(' | '[' => prof += 1,
                ')' | ']' => prof -= 1,
                '{' if prof == 0 => {
                    abre = Some(n_fim + i);
                    break;
                }
                ';' if prof == 0 => break,
                _ => {}
            }
        }
        let Some(abre) = abre else { continue };
        let Some(fim) = fecha(s, abre) else { continue };
        if abre < pos && pos < fim {
            return Some((s[n..n_fim].to_string(), abre, fim));
        }
    }
    None
}

/// O bloco `{ .. }` mais interno que contem `pos`.
fn bloco_que_contem(s: &str, pos: usize) -> Option<(usize, usize)> {
    let mut prof = 0i32;
    for (i, c) in s[..pos].char_indices().rev() {
        match c {
            '}' => prof += 1,
            '{' => {
                if prof == 0 {
                    return Some((i, fecha(s, i)?));
                }
                prof -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Os argumentos de topo entre `abre` e `fim` (os parenteses), como
/// trechos.
fn argumentos(s: &str, abre: usize, fim: usize) -> Vec<(usize, usize)> {
    let mut fora = Vec::new();
    let mut prof = 0i32;
    let mut ini = abre + 1;
    for (i, c) in s[abre + 1..fim].char_indices() {
        let i = abre + 1 + i;
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => prof -= 1,
            ',' if prof == 0 => {
                fora.push((ini, i));
                ini = i + 1;
            }
            _ => {}
        }
    }
    fora.push((ini, fim));
    fora.retain(|&(a, b)| !s[a..b].trim().is_empty());
    fora
}

/// Sem espacos e sem o `&` da frente: `&session_id` e `session_id` sao o
/// mesmo argumento para a comparacao.
fn normalizar(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    t.trim_start_matches('&').to_string()
}

/// O trecho e EXATAMENTE uma chamada ao construtor por um caminho aceito
/// (com ou sem `&`)? Devolve os argumentos. `&{ ..com_escopo(..); outro }`
/// nao conta: contem o construtor, mas nao e ele.
fn escopo_inline(f: &Fonte, (ini, fim): (usize, usize), k: &Construtor) -> Option<Vec<String>> {
    let s = &f.codigo;
    let mut p = pular_espaco(s, ini);
    if s[p..fim].starts_with('&') {
        p = pular_espaco(s, p + 1);
    }
    let caminho = k.caminhos.iter().find(|c| {
        s[p..fim].starts_with(**c)
            && !s[p + c.len()..].starts_with(|x: char| e_ident(x) || x == ':')
    })?;
    let abre = pular_espaco(s, p + caminho.len());
    if !s[abre..].starts_with('(') {
        return None;
    }
    let fecha_ = fecha(s, abre)?;
    if fecha_ >= fim || !s[fecha_ + 1..fim].trim().is_empty() {
        return None;
    }
    Some(
        argumentos(s, abre, fecha_)
            .into_iter()
            .map(|(a, b)| normalizar(&f.texto[a..b]))
            .collect(),
    )
}

/// Fim do padrao de um `let` que comeca em `p`: o `=` do inicializador
/// (`true`) ou o `;` de um `let x;` (`false`).
fn fim_do_padrao(s: &str, p: usize) -> Option<(usize, bool)> {
    let b = s.as_bytes();
    let mut prof = 0i32;
    for (i, c) in s[p..].char_indices() {
        let i = p + i;
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => {
                prof -= 1;
                if prof < 0 {
                    return None;
                }
            }
            ';' if prof == 0 => return Some((i, false)),
            '=' if prof == 0 => {
                let prox = b.get(i + 1).copied();
                let ant = i.checked_sub(1).and_then(|j| b.get(j).copied());
                if !matches!(prox, Some(b'=' | b'>'))
                    && !matches!(ant, Some(b'=' | b'!' | b'<' | b'>'))
                {
                    return Some((i, true));
                }
            }
            _ => {}
        }
    }
    None
}

/// O `;` que fecha a instrucao comecada em `p`.
fn fim_da_instrucao(s: &str, p: usize) -> Option<usize> {
    let mut prof = 0i32;
    for (i, c) in s[p..].char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => {
                prof -= 1;
                if prof < 0 {
                    return None;
                }
            }
            ';' if prof == 0 => return Some(p + i),
            _ => {}
        }
    }
    None
}

/// `let` de instrucao (nao `if let`, `while let`, `&& let`).
fn let_de_instrucao(s: &str, l: usize) -> bool {
    match s[..l].trim_end().chars().next_back() {
        None => true,
        Some(c) => matches!(c, ';' | '{' | '}' | ']'),
    }
}

/// `nome`, `mut nome`, `nome: Tipo` ou `mut nome: Tipo`.
fn padrao_simples(padrao: &str, nome: &str) -> bool {
    let t = padrao.trim();
    let t = match t.strip_prefix("mut") {
        Some(r) if r.starts_with(char::is_whitespace) => r.trim_start(),
        _ => t,
    };
    let Some(resto) = t.strip_prefix(nome) else {
        return false;
    };
    let resto = resto.trim_start();
    resto.is_empty() || resto.starts_with(':')
}

/// Toda mencao a `nome` entre `ini` e `fim` e um `&nome` inteiro como
/// argumento (`&nome,` ou `&nome)`) — o outro ramo de um `if`/`else` que
/// passa o mesmo contexto. Qualquer outra (`nome.approval_scope = None`,
/// `|nome|`, `nome = ..`, `Some(nome)`) invalida o binding.
fn so_emprestimos(s: &str, nome: &str, ini: usize, fim: usize) -> bool {
    palavras(&s[ini..fim], nome).into_iter().all(|rel| {
        let p = ini + rel;
        let antes = s[..p].trim_end();
        let depois = s[p + nome.len()..].trim_start();
        antes.ends_with('&')
            && !antes.ends_with("&&")
            && (depois.starts_with(',') || depois.starts_with(')'))
    })
}

/// O identificador `nome` usado na chamada em `pos` vem de um `let` que e
/// exatamente o construtor? So vale o `let` visivel na chamada, dentro da
/// MESMA `fn`: parametro, binding de outra `fn`, padrao (`if let`,
/// `|nome|`) ou `let` com outra forma — tudo "sem escopo".
fn escopo_do_binding(f: &Fonte, nome: &str, pos: usize, k: &Construtor) -> Option<Vec<String>> {
    let s = &f.codigo;
    let (_, corpo, _) = funcao_que_contem(s, pos)?;
    for l in palavras(&s[..pos], "let").into_iter().rev() {
        if l <= corpo {
            break;
        }
        let Some((fim_padrao, tem_init)) = fim_do_padrao(s, l + 3) else {
            continue;
        };
        if fim_padrao >= pos || palavras(&s[l + 3..fim_padrao], nome).is_empty() {
            continue;
        }
        let Some((_, fim_bloco)) = bloco_que_contem(s, l) else {
            continue;
        };
        if fim_bloco < pos {
            continue; // o `let` estava num bloco que ja fechou
        }
        // Este e o binding que a chamada ve. Forma diferente da simples:
        // sem escopo.
        if !tem_init || !let_de_instrucao(s, l) || !padrao_simples(&s[l + 3..fim_padrao], nome) {
            return None;
        }
        let fim_init = fim_da_instrucao(s, fim_padrao + 1)?;
        if fim_init >= pos {
            return None;
        }
        let args = escopo_inline(f, (fim_padrao + 1, fim_init), k)?;
        if !so_emprestimos(s, nome, fim_init, pos) {
            return None;
        }
        return Some(args);
    }
    None
}

/// Toda chamada a um ponto de entrada do runtime, e os erros de uma
/// chamada que o detector nao sabe decidir (nome fora de [`ENTRADAS`],
/// referencia de metodo sem chamada).
pub fn chamadas(f: &Fonte, k: &Construtor) -> (Vec<Chamada>, Vec<String>) {
    let s = &f.codigo;
    let mut fora = Vec::new();
    let mut erros = Vec::new();
    let mut desde = 0;
    while let Some(rel) = s[desde..].find("process_") {
        let pos = desde + rel;
        let fim_nome = fim_do_ident(s, pos);
        desde = fim_nome.max(pos + 1);
        if s[..pos].chars().next_back().is_some_and(e_ident) {
            continue; // meio de outro identificador
        }
        let antes = s[..pos].trim_end();
        if !(antes.ends_with('.') || antes.ends_with("::")) {
            continue;
        }
        let nome = &s[pos..fim_nome];
        let linha = f.linha(pos);
        let funcao = funcao_que_contem(s, pos).map_or_else(|| FORA_DE_FN.to_string(), |x| x.0);
        if !ENTRADAS.contains(&nome) {
            if nome.starts_with("process_message") && !PRIVADAS.contains(&nome) {
                erros.push(format!(
                    "linha {linha} (`{funcao}`): `{nome}` parece um ponto de entrada do runtime \
                     que o guarda nao conhece; acrescente-o a ENTRADAS para a chamada ser decidida"
                ));
            }
            continue;
        }
        let abre = pular_espaco(s, fim_nome);
        if !s[abre..].starts_with('(') {
            erros.push(format!(
                "linha {linha} (`{funcao}`): `{nome}` citado sem ser chamado (referencia de \
                 metodo?); o guarda so decide chamada direta"
            ));
            continue;
        }
        let Some(fecha_) = fecha(s, abre) else {
            erros.push(format!(
                "linha {linha}: os parenteses de `{nome}(` nao fecham"
            ));
            continue;
        };
        let escopo = argumentos(s, abre, fecha_).last().and_then(|&ultimo| {
            escopo_inline(f, ultimo, k).or_else(|| {
                let t = s[ultimo.0..ultimo.1].trim();
                let t = t.strip_prefix('&').unwrap_or(t).trim();
                if !t.is_empty() && t.chars().all(e_ident) {
                    escopo_do_binding(f, t, pos, k)
                } else {
                    None
                }
            })
        });
        fora.push(Chamada {
            linha,
            funcao,
            entrada: nome.to_string(),
            escopo,
        });
    }
    (fora, erros)
}

/// Toda chamada ao construtor de escopo no arquivo, por qualquer caminho.
pub fn construcoes(f: &Fonte, k: &Construtor) -> Vec<Construcao> {
    let s = &f.codigo;
    let mut fora = Vec::new();
    for p in palavras(s, k.nome) {
        let abre = pular_espaco(s, p + k.nome.len());
        if !s[abre..].starts_with('(') || palavra_antes(s, p) == "fn" {
            continue;
        }
        let Some(fecha_) = fecha(s, abre) else {
            continue;
        };
        let ini_caminho = s[..p]
            .trim_end_matches(|c: char| e_ident(c) || c == ':')
            .len();
        fora.push(Construcao {
            linha: f.linha(p),
            funcao: funcao_que_contem(s, p).map_or_else(|| FORA_DE_FN.to_string(), |x| x.0),
            caminho: s[ini_caminho..p + k.nome.len()].to_string(),
            args: argumentos(s, abre, fecha_)
                .into_iter()
                .map(|(a, b)| normalizar(&f.texto[a..b]))
                .collect(),
        });
    }
    fora
}

/// O argumento e um literal de string?
pub fn e_literal(arg: &str) -> bool {
    let t = arg.trim_start_matches('b');
    t.starts_with('"') || t.starts_with("r\"") || t.starts_with("r#")
}

fn arquivos_rs(dir: &Path, fora: &mut Vec<PathBuf>) {
    let entradas = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("ler {}: {e}", dir.display()));
    for e in entradas.flatten() {
        let p = e.path();
        if p.is_dir() {
            arquivos_rs(&p, fora);
        } else if p.extension().is_some_and(|x| x == "rs") {
            fora.push(p);
        }
    }
}

/// Onde moram os filhos `mod x;` de um arquivo.
fn diretorio_dos_filhos(arquivo: &Path) -> PathBuf {
    let pai = arquivo.parent().unwrap_or_else(|| Path::new(""));
    match arquivo.file_name().and_then(|n| n.to_str()) {
        Some("mod.rs" | "lib.rs" | "main.rs") => pai.to_path_buf(),
        _ => pai.join(arquivo.file_stem().unwrap_or_default()),
    }
}

/// Todo `.rs` sob `raiz`, com o caminho relativo (separador `/`) e o fonte
/// pronto. O arquivo de um modulo declarado `#[cfg(test)] mod x;` (e o que
/// estiver embaixo de `x/`) fica de fora.
pub fn varrer(raiz: &Path) -> Vec<(String, Fonte)> {
    let mut arquivos = Vec::new();
    arquivos_rs(raiz, &mut arquivos);
    arquivos.sort();
    let lidos: Vec<(PathBuf, Fonte)> = arquivos
        .into_iter()
        .map(|p| {
            let t =
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("ler {}: {e}", p.display()));
            (p, Fonte::nova(&t))
        })
        .collect();
    let de_teste: Vec<PathBuf> = lidos
        .iter()
        .flat_map(|(p, f)| {
            let dir = diretorio_dos_filhos(p);
            f.modulos_de_teste
                .iter()
                .map(move |m| dir.join(m))
                .collect::<Vec<_>>()
        })
        .collect();
    lidos
        .into_iter()
        .filter(|(p, _)| {
            !de_teste
                .iter()
                .any(|t| *p == t.with_extension("rs") || p.starts_with(t))
        })
        .map(|(p, f)| {
            let rel = p
                .strip_prefix(raiz)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            (rel, f)
        })
        .collect()
}
