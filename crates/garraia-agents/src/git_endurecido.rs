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
                stderr_para_o_turno(String::from_utf8_lossy(&saida.stderr).trim())
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
/// absoluto vira [`crate::tools::tool_context::CAMINHO_FORA_DO_TURNO`]; o
/// resto da mensagem (a CAUSA, que e o que o modelo precisa) fica intacto. No
/// turno aberto o texto sai como o git escreveu.
///
/// So o STDERR passa por aqui: o stdout de um `git diff` e conteudo do
/// repositorio (`a/src/x.rs`), nao diagnostico, e nao e tocado.
pub(crate) fn stderr_para_o_turno(stderr: &str) -> String {
    if crate::tools::tool_context::pode_revelar_caminho_do_host() {
        stderr.to_string()
    } else {
        oculta_caminhos_absolutos(stderr)
    }
}

/// Troca cada caminho absoluto de `texto` pelo marcador do turno. Funcao pura.
///
/// Um caminho comeca em inicio de token (inicio do texto, depois de espaco,
/// aspas, crase, `(` ou `=`) com `/x`, `X:\`, `X:/` ou `\\` (UNC). Entre
/// aspas ele vai ate a aspa que fecha na mesma linha (o git cita assim, e e o
/// que cobre caminho com espaco); fora delas, ate o proximo espaco, deixando
/// de fora a pontuacao final (`:`, `,`, `;`, `)`, `.`). `://` de URL nao e
/// inicio de token, entao `https://host/x` nao e tocado.
fn oculta_caminhos_absolutos(texto: &str) -> String {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO;

    fn comeca_absoluto(resto: &[char]) -> bool {
        match resto {
            ['/', c, ..] => !c.is_whitespace() && *c != '/',
            ['\\', '\\', c, ..] => !c.is_whitespace(),
            [d, ':', '/' | '\\', ..] => d.is_ascii_alphabetic(),
            _ => false,
        }
    }

    let chars: Vec<char> = texto.chars().collect();
    let mut saida = String::with_capacity(texto.len());
    let mut i = 0;
    while i < chars.len() {
        let anterior = i.checked_sub(1).map(|j| chars[j]);
        let inicio_de_token =
            anterior.is_none_or(|p| p.is_whitespace() || matches!(p, '\'' | '"' | '`' | '(' | '='));
        if !(inicio_de_token && comeca_absoluto(&chars[i..])) {
            saida.push(chars[i]);
            i += 1;
            continue;
        }
        let resto = &chars[i..];
        let fecha_aspas = anterior
            .filter(|p| matches!(p, '\'' | '"' | '`'))
            .and_then(|aspa| {
                resto
                    .iter()
                    .position(|c| *c == aspa || *c == '\n')
                    .filter(|pos| resto[*pos] == aspa)
            });
        let fim = match fecha_aspas {
            Some(pos) => pos,
            None => {
                let mut pos = resto
                    .iter()
                    .position(|c| c.is_whitespace())
                    .unwrap_or(resto.len());
                while pos > 1 && matches!(resto[pos - 1], ':' | ',' | ';' | ')' | '.' | '\'' | '"')
                {
                    pos -= 1;
                }
                pos
            }
        };
        saida.push_str(CAMINHO_FORA_DO_TURNO);
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
            oculta_caminhos_absolutos(bare),
            format!("fatal: cannot use bare repository '{M}' (safe.bareRepository is 'explicit')")
        );

        let dubia = "fatal: detected dubious ownership in repository at '/srv/garra/wt'\n\
                     To add an exception for this directory, call:\n\n\
                     \tgit config --global --add safe.directory /srv/garra/wt\n";
        let saida = oculta_caminhos_absolutos(dubia);
        assert!(!saida.contains("/srv"), "{saida}");
        assert!(saida.contains("dubious ownership"), "{saida}");
        assert!(saida.contains(&format!("safe.directory {M}\n")), "{saida}");

        // Sem aspas, com pontuacao colada: o `:` da mensagem sobrevive.
        assert_eq!(
            oculta_caminhos_absolutos("error: could not lock /a/b/.git/config: Permission denied"),
            format!("error: could not lock {M}: Permission denied")
        );
        // Windows e UNC.
        assert_eq!(
            oculta_caminhos_absolutos(r"fatal: not a git repository: 'C:\Users\dono\x'"),
            format!("fatal: not a git repository: '{M}'")
        );
        assert_eq!(
            oculta_caminhos_absolutos(r"em \\servidor\share\x agora"),
            format!("em {M} agora")
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
            assert_eq!(oculta_caminhos_absolutos(texto), texto);
        }
    }

    /// O portao: turno restrito oculta, turno aberto mostra, e fora de um
    /// turno (sem escopo) e fail-closed.
    #[tokio::test]
    async fn stderr_para_o_turno_segue_o_portao() {
        use crate::tools::turn_tools::com_restricao_do_turno;
        let texto = "fatal: cannot use bare repository '/home/alguem/x'";
        let aberto = com_restricao_do_turno(false, async { stderr_para_o_turno(texto) }).await;
        assert_eq!(aberto, texto);
        let restrito = com_restricao_do_turno(true, async { stderr_para_o_turno(texto) }).await;
        assert!(!restrito.contains("/home/alguem"), "{restrito}");
        assert!(!stderr_para_o_turno(texto).contains("/home/alguem"));
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
