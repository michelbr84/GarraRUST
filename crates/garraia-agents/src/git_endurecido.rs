//! #1272 S3: o `git` que as tools de leitura (`git_diff`, `code_review`)
//! executam nao pode virar execucao arbitraria por config do repositorio.
//!
//! `git diff`/`git status` sao "leitura" so no nome: o git le a config do
//! repositorio (`.git/config`) e os atributos da arvore (`.gitattributes`), e
//! dali roda PROGRAMAS — `core.fsmonitor`, `diff.<drv>.textconv`,
//! `filter.<drv>.clean`/`process` e `diff.external`. Quem escreve na arvore
//! (um modelo com `bash` sandboxado, que monta o workdir rw, ou uma tool de
//! escrita) planta um desses e o proximo `git_diff` executa o programa no
//! HOST, fora de qualquer sandbox.
//!
//! As defesas, todas por argv (nenhuma por texto de comando):
//!
//! - `-c core.fsmonitor=false` — o hook de fsmonitor nao roda;
//! - `-c safe.bareRepository=explicit` — um repositorio bare implicito (um
//!   `HEAD`+`config` plantados no diretorio) e recusado em vez de lido;
//! - `-c core.hooksPath=/dev/null` — nenhum hook;
//! - `--no-ext-diff` e `--no-textconv` no `diff`;
//! - nenhuma descida em submodulo: `-c submodule.recurse=false`,
//!   `diff.submodule=short`, `diff.ignoreSubmodules=all`,
//!   `status.submoduleSummary=false` e `--ignore-submodules=all` no `diff` e
//!   no `status`. Um submodulo tem config PROPRIA, que a listagem de filtros
//!   abaixo nao le;
//! - para cada driver `filter.<nome>` que a config declara, `-c
//!   filter.<nome>.{clean,smudge,process}=` vazios e `required=false` — o git
//!   trata comando vazio como "sem filtro" (verificado no git 2.43);
//! - `GIT_CONFIG_NOSYSTEM=1` e o env do filho reduzido a allowlist (#1075 R3).
//!
//! Listar os drivers com `git config --get-regexp` so LE config: `git config`
//! nao executa programa nenhum. Um nome de driver que nao cabe num `-c
//! chave=valor` (tem `=`) recusa a operacao inteira — fail-closed.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Vai antes do subcomando em toda invocacao das tools de leitura.
///
/// As quatro ultimas chaves fecham a recursao em submodulos: um submodulo
/// tem config PROPRIA (`.git/modules/<nome>/config`), que a listagem de
/// [`prefixo`] nao le, e com `diff.submodule=diff` (ou so por o `status`
/// checar a arvore de cada submodulo) o git entra nele e roda o
/// `filter.<drv>.clean` declarado LA. Nada aqui desce em submodulo.
pub(crate) const PREFIXO_FIXO: &[&str] = &[
    "-c",
    "core.fsmonitor=false",
    "-c",
    "safe.bareRepository=explicit",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "submodule.recurse=false",
    "-c",
    "diff.submodule=short",
    "-c",
    "diff.ignoreSubmodules=all",
    "-c",
    "status.submoduleSummary=false",
];

/// Opcoes do `git diff` que desligam programas externos e a descida em
/// submodulos (ver [`PREFIXO_FIXO`]).
pub(crate) const OPCOES_DO_DIFF: &[&str] =
    &["--no-ext-diff", "--no-textconv", "--ignore-submodules=all"];

/// Opcao do `git status` que nao desce em submodulo nenhum.
pub(crate) const OPCOES_DO_STATUS: &[&str] = &["--ignore-submodules=all"];

/// Os `-c` que anulam cada driver de filtro listado em `saida` (a saida de
/// `git config -z --name-only --get-regexp ^filter\.`). Pura.
pub(crate) fn neutraliza_filtros(saida: &[u8]) -> Result<Vec<String>, String> {
    let mut nomes: Vec<String> = Vec::new();
    for chave in saida.split(|b| *b == 0).filter(|c| !c.is_empty()) {
        let chave = String::from_utf8_lossy(chave);
        let chave = chave.trim_end_matches('\n');
        let Some(resto) = chave.strip_prefix("filter.") else {
            continue;
        };
        let Some((nome, _var)) = resto.rsplit_once('.') else {
            continue;
        };
        if nome.contains('=') || nome.chars().any(char::is_control) {
            return Err(
                "git recusado: a config do repositorio declara um filtro com nome que nao pode ser \
                 neutralizado (#1272)"
                    .to_string(),
            );
        }
        if !nomes.iter().any(|n| n == nome) {
            nomes.push(nome.to_string());
        }
    }
    let mut args = Vec::new();
    for nome in nomes {
        for var in ["clean=", "smudge=", "process=", "required=false"] {
            args.push("-c".to_string());
            args.push(format!("filter.{nome}.{var}"));
        }
    }
    Ok(args)
}

/// Env extra do git das tools: nada da config do sistema.
pub(crate) const ENV: &[(&str, &str)] = &[("GIT_CONFIG_NOSYSTEM", "1")];

/// Env do filho git: so a allowlist (#1075 R3) e nada da config do sistema.
pub(crate) fn aplica_env(cmd: &mut Command) {
    #[cfg(unix)]
    {
        cmd.env_clear();
        for (key, value) in garraia_common::safety_gate::allowed_child_env() {
            cmd.env(key, value);
        }
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
}

/// O prefixo completo (fixo + filtros anulados) para rodar git em `cwd`.
///
/// `Err` quando a listagem nao pode ser feita com seguranca: nome de driver
/// inneutralizavel, ou o git recusou o diretorio (ex.: repositorio bare
/// implicito com `safe.bareRepository=explicit`). Codigo 1 e "nenhuma chave",
/// o caso comum.
pub(crate) async fn prefixo(cwd: Option<&Path>, timeout: Duration) -> Result<Vec<String>, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(PREFIXO_FIXO)
        .args(["config", "-z", "--name-only", "--get-regexp", r"^filter\."])
        .stdin(std::process::Stdio::null());
    aplica_env(&mut cmd);
    cmd.kill_on_drop(true);
    let saida = match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(saida)) => saida,
        Ok(Err(e)) => return Err(format!("falha ao executar git: {e}")),
        Err(_) => return Err("git config excedeu o tempo limite".to_string()),
    };
    let filtros = match saida.status.code() {
        Some(0) => neutraliza_filtros(&saida.stdout)?,
        Some(1) => Vec::new(),
        _ => {
            // #1909: a recusa do git cita o diretorio por extenso (`cannot use
            // bare repository '/abs'`, `dubious ownership in repository at
            // '/abs'`). O texto vai para o modelo; passa pelo portao do turno.
            return Err(format!(
                "git recusou o diretorio: {}",
                stderr_para_o_turno(String::from_utf8_lossy(&saida.stderr).trim(), cwd)
            ));
        }
    };
    let mut args: Vec<String> = PREFIXO_FIXO.iter().map(|s| s.to_string()).collect();
    args.extend(filtros);
    Ok(args)
}

/// O stderr do git na forma que o turno pode ver (#1909).
///
/// O git nomeia caminhos ABSOLUTOS do host nas proprias recusas — `fatal:
/// cannot use bare repository '/abs' (safe.bareRepository is 'explicit')`,
/// `detected dubious ownership in repository at '/abs'` seguido de `git config
/// --global --add safe.directory /abs` — e esse texto sai no corpo de
/// `git_diff`/`code_review`, logo abaixo da linha `Repositorio:` que ja retem
/// o caminho. Num turno restrito
/// ([`crate::tools::tool_context::pode_revelar_caminho_do_host`]) cada caminho
/// absoluto passa por
/// [`crate::tools::tool_context::caminho_para_exibir_no_turno`]: relativo
/// quando cai dentro do `working_dir` da sessao (`.`, `.git/config` — a forma
/// que o modelo reconhece e que nao nomeia o host), e
/// [`crate::tools::tool_context::CAMINHO_FORA_DO_TURNO`] quando nao. O resto
/// da mensagem (a CAUSA, que e o que o modelo precisa) fica intacto. No turno
/// aberto o texto sai como o git escreveu.
///
/// So o STDERR passa por aqui: o stdout de um `git diff` e conteudo do
/// repositorio (`a/src/x.rs`), nao diagnostico, e nao e tocado.
pub(crate) fn stderr_para_o_turno(stderr: &str, working_dir: Option<&Path>) -> String {
    if crate::tools::tool_context::pode_revelar_caminho_do_host() {
        stderr.to_string()
    } else {
        oculta_caminhos_absolutos(stderr, working_dir)
    }
}

/// O stderr do git pronto para entrar no corpo de uma resposta, abaixo das
/// linhas que a propria tool escreve (`Repositório:`, `Intervalo:`,
/// `Revisão:` — #1909). O git ecoa o pathspec que recusou — quebra de linha
/// inclusive —, e o `file_path` e texto do modelo: cru, um
/// `:(bogus)x\nIntervalo: ...` escrevia uma linha `Intervalo:` forjada na
/// coluna zero. Cada linha sai recuada e sem caractere de controle, entao
/// nenhuma comeca onde as linhas da tool comecam.
pub(crate) fn stderr_recuado(stderr: &str) -> String {
    stderr
        .trim()
        .split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .map(|l| {
            let limpa: String = l
                .chars()
                .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
                .collect();
            format!("    {limpa}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Troca cada caminho absoluto de `texto` pela forma segura do turno
/// restrito: relativo ao `working_dir` quando cai dentro dele, o marcador
/// quando nao. Funcao pura (nao consulta o turno).
///
/// Um caminho comeca em inicio de token (inicio do texto, depois de espaco,
/// aspas, crase, `(`, `[`, `<`, `=`, `:` ou do `//` de `file://`) com `/x`,
/// `X:\`, `X:/` ou `\\` (UNC). Entre aspas ele vai ate a aspa que FECHA na
/// mesma linha — a seguida de espaco, pontuacao ou fim, para que o apostrofo
/// de `/home/o'brien` nao corte o caminho no meio; fora delas, ate o proximo
/// espaco, deixando de fora a pontuacao final — e estendido ate a ultima
/// palavra com separador de caminho antes da proxima pausa, que e o que cobre
/// o caminho sem aspas com espaco (`not a git repository: /abs com espaco/x`).
/// O `//` de `https://host/x` nao e inicio de caminho, entao URL de rede nao e
/// tocada.
///
/// Sem aspas, um ULTIMO componente com espaco e sem separador depois
/// (`/home/dono/Meus Projetos`) nao tem marca no texto. O sinal que cobre o
/// caso real (o git citando o repositorio da sessao) vem de fora do texto: o
/// `working_dir` da sessao, ou um ancestral dele, escrito por extenso ali.
/// O disco NAO e consultado — o nome a mais sumir ou nao viraria oraculo de
/// existencia de caminho alheio no turno restrito. Limite restante: um caminho
/// fora dessa linhagem com espaco no ultimo componente deixa esse nome solto
/// (`Projetos`) — um fragmento de nome, nao o caminho.
fn oculta_caminhos_absolutos(texto: &str, working_dir: Option<&Path>) -> String {
    use crate::tools::tool_context::{CAMINHO_FORA_DO_TURNO, caminho_para_exibir_no_turno};

    fn comeca_absoluto(resto: &[char]) -> bool {
        match resto {
            ['/', c, ..] => !c.is_whitespace() && *c != '/',
            ['\\', '\\', c, ..] => !c.is_whitespace(),
            [d, ':', '/' | '\\', ..] => d.is_ascii_alphabetic(),
            _ => false,
        }
    }
    fn e_aspa(c: char) -> bool {
        matches!(c, '\'' | '"' | '`')
    }
    fn e_pontuacao_final(c: char) -> bool {
        matches!(c, ':' | ',' | ';' | ')' | ']' | '>' | '.')
    }
    /// #1909 (residuo da auditoria da #1943): a partir de `pos`, estende o
    /// caminho palavra a palavra ate `pausa` e fica com o MAIOR candidato
    /// que e o `working_dir` da sessao ou um ancestral dele. Sem I/O: o
    /// disco viraria oraculo de existencia (ver o docblock acima).
    fn estende_ate_caminho_conhecido(
        resto: &[char],
        pos: usize,
        pausa: usize,
        working_dir: Option<&Path>,
    ) -> usize {
        let mut melhor = pos;
        let mut j = pos;
        while j < pausa {
            let Some(ini) = (j..pausa).find(|k| !resto[*k].is_whitespace()) else {
                break;
            };
            let fim = (ini..pausa)
                .find(|k| resto[*k].is_whitespace())
                .unwrap_or(pausa);
            let mut corte = fim;
            while corte > ini && (e_pontuacao_final(resto[corte - 1]) || e_aspa(resto[corte - 1])) {
                corte -= 1;
            }
            if corte > ini {
                let candidato: String = resto[..corte].iter().collect();
                let caminho = Path::new(&candidato);
                if working_dir.is_some_and(|wd| wd.starts_with(caminho)) {
                    melhor = corte;
                }
            }
            j = fim;
        }
        melhor
    }

    let chars: Vec<char> = texto.chars().collect();
    let mut saida = String::with_capacity(texto.len());
    let mut i = 0;
    while i < chars.len() {
        let anterior = i.checked_sub(1).map(|j| chars[j]);
        let depois_de_file_url = chars[..i].ends_with(&['f', 'i', 'l', 'e', ':', '/', '/']);
        let inicio_de_token = depois_de_file_url
            || anterior.is_none_or(|p| {
                p.is_whitespace() || e_aspa(p) || matches!(p, '(' | '[' | '<' | '=' | ':')
            });
        if !(inicio_de_token && comeca_absoluto(&chars[i..])) {
            saida.push(chars[i]);
            i += 1;
            continue;
        }
        let resto = &chars[i..];
        let linha = resto.iter().position(|c| *c == '\n').unwrap_or(resto.len());
        let fecha_aspas = anterior.filter(|p| e_aspa(*p)).and_then(|aspa| {
            (1..linha).find(|pos| {
                resto[*pos] == aspa
                    && resto
                        .get(pos + 1)
                        .is_none_or(|c| c.is_whitespace() || e_pontuacao_final(*c))
            })
        });
        let fim = match fecha_aspas {
            Some(pos) => pos,
            None => {
                let mut pos = resto
                    .iter()
                    .position(|c| c.is_whitespace())
                    .unwrap_or(resto.len());
                // Auditoria da #1943: o git tambem cita caminho SEM aspas
                // (`not a git repository: /abs`, `bad config line N in file
                // /abs`), e um diretorio com espaco deixaria o rabo do caminho
                // de fora. O token se estende ate a ultima palavra com
                // separador de caminho no mesmo trecho — o trecho acaba na
                // primeira pausa (`: `, `; `, `, `, ` (`) ou no fim da linha,
                // para a causa que vem depois do caminho continuar dita.
                let pausa = (pos..linha)
                    .find(|j| {
                        let c = resto[*j];
                        let seguinte = resto.get(j + 1).copied();
                        (matches!(c, ':' | ';' | ',') && seguinte.is_none_or(|n| n.is_whitespace()))
                            || (c.is_whitespace() && seguinte == Some('('))
                    })
                    .unwrap_or(linha);
                // Sem pausa nem fecho colado no proprio token (`/a/config: causa`,
                // `[/a/b] e ...`): ali o caminho ja acabou.
                if !(pos > 0 && matches!(resto[pos - 1], ':' | ';' | ',' | ')' | ']' | '>')) {
                    let mut j = pos;
                    while j < pausa {
                        let ini = j + resto[j..pausa]
                            .iter()
                            .position(|c| !c.is_whitespace())
                            .unwrap_or(pausa - j);
                        let fim_da_palavra = ini
                            + resto[ini..pausa]
                                .iter()
                                .position(|c| c.is_whitespace())
                                .unwrap_or(pausa - ini);
                        if resto[ini..fim_da_palavra]
                            .iter()
                            .any(|c| matches!(c, '/' | '\\'))
                        {
                            pos = fim_da_palavra;
                        }
                        j = fim_da_palavra.max(j + 1);
                    }
                    pos = estende_ate_caminho_conhecido(resto, pos, pausa, working_dir);
                }
                while pos > 1 && (e_pontuacao_final(resto[pos - 1]) || e_aspa(resto[pos - 1])) {
                    pos -= 1;
                }
                pos
            }
        };
        let token: String = resto[..fim].iter().collect();
        let exibido = caminho_para_exibir_no_turno(Path::new(&token), working_dir);
        // `caminho_para_exibir_no_turno` devolve o proprio caminho quando ele
        // nao e absoluto NESTA plataforma (um `C:\x` lido no Linux) ou quando
        // o turno pode ver. Aqui o turno e restrito por contrato: o que nao
        // virou relativo vira o marcador.
        if exibido == token {
            saida.push_str(CAMINHO_FORA_DO_TURNO);
        } else {
            saida.push_str(&exibido);
        }
        i += fim;
    }
    saida
}

/// Fixture da #1272 S3: planta no repositorio `dir` os tres programas que o
/// git executaria sozinho — `core.fsmonitor`, `diff.x.textconv` e
/// `filter.x.clean` (com `.gitattributes` mapeando `*.txt`) — cada um
/// criando um marcador em `marcas`. Devolve os caminhos dos marcadores.
#[cfg(all(test, unix))]
pub(crate) fn planta_programas_no_repo(dir: &Path, marcas: &Path) -> [std::path::PathBuf; 3] {
    use std::os::unix::fs::PermissionsExt;
    let fsm = marcas.join("M_FSMONITOR");
    let tc = marcas.join("M_TEXTCONV");
    let clean = marcas.join("M_CLEAN");
    let script = |nome: &str, marca: &Path, corpo: &str| {
        let p = marcas.join(nome);
        std::fs::write(
            &p,
            format!("#!/bin/sh\ntouch '{}'\n{corpo}\n", marca.display()),
        )
        .expect("script");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    };
    let s_fsm = script("fsm.sh", &fsm, "exit 0");
    let s_tc = script("tc.sh", &tc, "cat \"$1\"");
    let s_clean = script("clean.sh", &clean, "cat");
    std::fs::write(dir.join(".gitattributes"), "*.txt filter=x diff=x\n").expect("attrs");
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["config", "core.fsmonitor", &s_fsm.to_string_lossy()]);
    git(&["config", "diff.x.textconv", &s_tc.to_string_lossy()]);
    git(&["config", "filter.x.clean", &s_clean.to_string_lossy()]);
    [fsm, tc, clean]
}

/// Fixture da recursao em submodulo: `dir` (repositorio com um commit)
/// ganha um submodulo `sub` cuja config PROPRIA declara `filter.evil.clean`
/// (com `sub/.gitattributes` mapeando `*.txt`), o superprojeto ganha
/// `diff.submodule=diff`, e `sub/a.txt` fica modificado. Devolve o marcador
/// que o filtro cria se rodar.
#[cfg(all(test, unix))]
pub(crate) fn planta_filtro_em_submodulo(dir: &Path, marcas: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let git = |cwd: &Path, args: &[&str]| {
        let saida = std::process::Command::new("git")
            .current_dir(cwd)
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .output()
            .expect("git");
        assert!(
            saida.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&saida.stderr)
        );
    };
    let origem = marcas.join("origem-sub");
    std::fs::create_dir_all(&origem).expect("mkdir");
    git(&origem, &["init", "-q", "-b", "main"]);
    std::fs::write(origem.join("a.txt"), "original\n").expect("write");
    std::fs::write(origem.join(".gitattributes"), "*.txt filter=evil\n").expect("attrs");
    git(&origem, &["add", "."]);
    git(&origem, &["commit", "-q", "-m", "sub"]);
    let origem_txt = origem.to_string_lossy().into_owned();
    git(dir, &["submodule", "add", "-q", &origem_txt, "sub"]);
    git(dir, &["commit", "-q", "-m", "com submodulo"]);

    let marca = marcas.join("M_SUBMODULE_CLEAN");
    let script = marcas.join("evil.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat\n", marca.display()),
    )
    .expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let sub = dir.join("sub");
    git(
        &sub,
        &["config", "filter.evil.clean", &script.to_string_lossy()],
    );
    git(dir, &["config", "diff.submodule", "diff"]);
    std::fs::write(sub.join("a.txt"), "alterado\n").expect("write");
    marca
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1909: as duas recusas do git que nomeiam o diretorio por extenso. O
    /// caminho sai, a causa fica.
    #[test]
    fn oculta_o_caminho_das_recusas_do_git_e_preserva_a_causa() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        let bare = "fatal: cannot use bare repository '/home/alguem/proj secreto' \
                    (safe.bareRepository is 'explicit')";
        assert_eq!(
            oculta_caminhos_absolutos(bare, None),
            format!("fatal: cannot use bare repository '{M}' (safe.bareRepository is 'explicit')")
        );

        let dubia = "fatal: detected dubious ownership in repository at '/srv/garra/wt'\n\
                     To add an exception for this directory, call:\n\n\
                     \tgit config --global --add safe.directory /srv/garra/wt\n";
        let saida = oculta_caminhos_absolutos(dubia, None);
        assert!(!saida.contains("/srv"), "{saida}");
        assert!(saida.contains("dubious ownership"), "{saida}");
        assert!(saida.contains(&format!("safe.directory {M}\n")), "{saida}");

        // Sem aspas, com pontuacao colada: o `:` da mensagem sobrevive.
        assert_eq!(
            oculta_caminhos_absolutos(
                "error: could not lock /a/b/.git/config: Permission denied",
                None
            ),
            format!("error: could not lock {M}: Permission denied")
        );
        // Windows e UNC.
        assert_eq!(
            oculta_caminhos_absolutos(r"fatal: not a git repository: 'C:\Users\dono\x'", None),
            format!("fatal: not a git repository: '{M}'")
        );
        assert_eq!(
            oculta_caminhos_absolutos(r"em \\servidor\share\x agora", None),
            format!("em {M} agora")
        );
    }

    /// Revisao da #1943: o caminho que o git cita e, no caso real, o proprio
    /// `working_dir` da sessao. Ele sai RELATIVO (`.`, `.git/config`) — dizer
    /// "fora do diretorio da sessao" seria falso e apagaria o que o modelo
    /// precisa; so o que cai fora vira o marcador.
    #[cfg(unix)]
    #[test]
    fn caminho_dentro_do_working_dir_sai_relativo() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        let wd = Path::new("/srv/sessao/proj");
        assert_eq!(
            oculta_caminhos_absolutos(
                "fatal: cannot use bare repository '/srv/sessao/proj' (safe.bareRepository is 'explicit')",
                Some(wd)
            ),
            "fatal: cannot use bare repository '.' (safe.bareRepository is 'explicit')"
        );
        assert_eq!(
            oculta_caminhos_absolutos(
                "error: could not lock /srv/sessao/proj/.git/config: File exists; ver /etc/gitconfig",
                Some(wd)
            ),
            format!("error: could not lock .git/config: File exists; ver {M}")
        );
    }

    /// Revisao da #1943: formas que o tokenizer deixava passar — `file://`,
    /// caminho colado em `:` ou entre colchetes, e apostrofo dentro do caminho.
    #[test]
    fn formas_coladas_e_apostrofo_tambem_saem() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        for (entrada, esperado) in [
            (
                "fatal: '/home/o'brien/x' is not a repo".to_string(),
                format!("fatal: '{M}' is not a repo"),
            ),
            (
                "remote: file:///home/dono/x.git falhou".to_string(),
                format!("remote: file://{M} falhou"),
            ),
            (
                "hint: [/home/dono/x] e host:/home/dono/y".to_string(),
                format!("hint: [{M}] e host:{M}"),
            ),
        ] {
            let saida = oculta_caminhos_absolutos(&entrada, None);
            assert_eq!(saida, esperado);
            assert!(
                !saida.contains("dono") && !saida.contains("brien"),
                "{saida}"
            );
        }
    }

    /// Auditoria da #1943: caminho SEM aspas com espaco (formas reais do git
    /// 2.43) nao deixa o rabo de fora, e a causa depois da pausa fica.
    #[test]
    fn caminho_sem_aspas_com_espaco_sai_inteiro() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        for (entrada, esperado) in [
            (
                "fatal: not a git repository: /home/dono/Meus Projetos/garra/.git/worktrees/wt2",
                format!("fatal: not a git repository: {M}"),
            ),
            (
                "fatal: bad config line 3 in file /mnt/My Big Volume/x/.git/config\nhint: veja",
                format!("fatal: bad config line 3 in file {M}\nhint: veja"),
            ),
            (
                "error: could not lock /a b/c/.git/config: File exists; ver /etc/gitconfig",
                format!("error: could not lock {M}: File exists; ver {M}"),
            ),
            (
                "\tgit config --global --add safe.directory /srv/garra/wt\n",
                format!("\tgit config --global --add safe.directory {M}\n"),
            ),
            (
                "fatal: /a/b nao e um repositorio valido",
                format!("fatal: {M} nao e um repositorio valido"),
            ),
        ] {
            assert_eq!(oculta_caminhos_absolutos(entrada, None), esperado);
        }
    }

    /// #1909 (residuo da auditoria da #1943): sem aspas, o ULTIMO componente
    /// com espaco (`/home/dono/Meus Projetos`) deixava `Projetos` solto. Ele
    /// sai inteiro quando e o `working_dir` da sessao ou um ancestral dele; a
    /// causa depois da pausa continua dita, e o disco nao e consultado.
    #[cfg(unix)]
    #[test]
    fn ultimo_componente_com_espaco_sai_inteiro_na_linhagem_da_sessao() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        let wd = Path::new("/home/dono/Meus Projetos/garra wt");
        for (entrada, esperado) in [
            // O proprio working_dir: sai relativo.
            (
                "fatal: not a git repository: /home/dono/Meus Projetos/garra wt",
                "fatal: not a git repository: .".to_string(),
            ),
            // Um ancestral, com pausa e causa depois: marcador inteiro.
            (
                "fatal: bad config in /home/dono/Meus Projetos: Permission denied",
                format!("fatal: bad config in {M}: Permission denied"),
            ),
            // Pontuacao final colada continua fora do caminho.
            (
                "hint: veja /home/dono/Meus Projetos. Depois tente",
                format!("hint: veja {M}. Depois tente"),
            ),
            // Prefixo da linhagem nao engole a palavra seguinte que nao e dela.
            (
                "fatal: /home/dono nao e um repositorio",
                format!("fatal: {M} nao e um repositorio"),
            ),
        ] {
            let saida = oculta_caminhos_absolutos(entrada, Some(wd));
            assert_eq!(saida, esperado);
            assert!(!saida.contains("Projetos"), "{saida}");
        }
    }

    /// Sem consulta ao disco: um caminho FORA da linhagem da sessao que
    /// existe sai igual a um que nao existe — o nome solto e o limite
    /// documentado, e e o mesmo nos dois casos (sem oraculo de existencia).
    #[cfg(unix)]
    #[test]
    fn existencia_no_disco_nao_muda_a_saida() {
        use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tmp.path().join("Meus Projetos")).expect("mkdir");
        let existe = format!("fatal: em {}/Meus Projetos", tmp.path().display());
        let nao_existe = format!("fatal: em {}/Meus Outros", tmp.path().display());
        assert_eq!(
            oculta_caminhos_absolutos(&existe, None),
            format!("fatal: em {M} Projetos")
        );
        assert_eq!(
            oculta_caminhos_absolutos(&nao_existe, None),
            format!("fatal: em {M} Outros")
        );
    }

    /// O que NAO e caminho absoluto do host fica como esta: relativo, URL,
    /// barra solta, nome de ref.
    #[test]
    fn nao_toca_no_que_nao_e_caminho_absoluto() {
        for texto in [
            "fatal: Needed a single revision",
            "fatal: ambiguous argument 'origin/main': unknown revision",
            "warning: in the working copy of 'src/a.rs', LF will be replaced",
            "fatal: unable to access 'https://example.com/a/b.git/': timeout",
            "usage: git diff [<options>] [<commit>] [--] [<path>...]",
            "a / b // c",
            "",
        ] {
            assert_eq!(oculta_caminhos_absolutos(texto, None), texto);
        }
    }

    /// O portao: turno restrito oculta, turno aberto mostra, e fora de um
    /// turno (sem escopo) e fail-closed.
    #[tokio::test]
    async fn stderr_para_o_turno_segue_o_portao() {
        use crate::tools::turn_tools::com_restricao_do_turno;
        let texto = "fatal: cannot use bare repository '/home/alguem/x'";
        let aberto =
            com_restricao_do_turno(false, async { stderr_para_o_turno(texto, None) }).await;
        assert_eq!(aberto, texto);
        let restrito =
            com_restricao_do_turno(true, async { stderr_para_o_turno(texto, None) }).await;
        assert!(!restrito.contains("/home/alguem"), "{restrito}");
        assert!(!stderr_para_o_turno(texto, None).contains("/home/alguem"));
    }

    #[test]
    fn neutraliza_cada_driver_uma_vez() {
        let saida = b"filter.lfs.clean\0filter.lfs.smudge\0filter.My.Drv.process\0";
        let args = neutraliza_filtros(saida).expect("ok");
        assert!(args.contains(&"filter.lfs.clean=".to_string()));
        assert!(args.contains(&"filter.lfs.required=false".to_string()));
        assert!(args.contains(&"filter.My.Drv.process=".to_string()));
        assert_eq!(args.iter().filter(|a| *a == "filter.lfs.clean=").count(), 1);
        assert_eq!(args.len(), 2 * 4 * 2);
    }

    #[test]
    fn nome_com_igual_recusa() {
        assert!(neutraliza_filtros(b"filter.a=b.clean\0").is_err());
    }

    #[test]
    fn saida_vazia_nao_gera_nada() {
        assert!(neutraliza_filtros(b"").expect("ok").is_empty());
    }
}
