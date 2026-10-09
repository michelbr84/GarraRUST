//! `garra uninstall` — desinstala a CLI desta maquina.
//!
//! Escopo padrao: os artefatos de instalacao — o binario em execucao
//! (canonico, nunca o alias), o alias `garra` quando e symlink nosso, os
//! wrappers do Termux e os backups `.old`/`.new` que o update deixa ao lado.
//! `--purge` acrescenta os diretorios de config e de dados, incluindo o
//! legado `~/.garraia`. App desktop, units do systemd, Ollama, a entrada de
//! PATH do Registro (Windows) e `~/.garra` ficam: sao de outro dono, e
//! remove-los e papel do desinstalador de cada um.
//!
//! Roda interceptado antes do `load()` global, como `doctor`/`verify`: tem
//! de funcionar com `config.yml` quebrado ou ausente. A config e lida, no
//! maximo, em melhor-esforco para achar um `data_dir` custom no `--purge` e
//! a porta do fallback do stop do daemon.
//!
//! Exit codes (sysexits): 0 feito (ou nada a fazer) · 1 cancelado no prompt ·
//! 64 sem terminal e sem `--yes` (EX_USAGE) · 70 alguma remocao falhou
//! (EX_SOFTWARE) · 78 o daemon pertence a uma unit systemd (EX_CONFIG, o
//! mesmo do bind) — desinstalar por fora deixaria a unit restartando em loop.

use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::systemd_guard::{Gerente, escotilha_ligada, unidade_do_processo};
use crate::update::installed_exe;
use crate::update_scan::{SYSTEM_BIN_DIRS, find_other_binaries};

/// sysexits `EX_USAGE`: desinstalacao destrutiva num pipe sem `--yes`.
const EX_USAGE: i32 = 64;
/// sysexits `EX_SOFTWARE`: alguma remocao falhou; o resto foi aplicado e o
/// item que falhou esta nomeado no stderr.
const EX_SOFTWARE: i32 = 70;
// `EX_CONFIG` (78) e `crate::EX_CONFIG` — daemon sob unit systemd.

pub(crate) struct Pedido {
    pub(crate) yes: bool,
    pub(crate) purge: bool,
    pub(crate) all_binaries: bool,
}

/// O que vai cair. `Diretorio` sai com `remove_dir_all`; o resto, com
/// `remove_file`, que tambem serve para symlink.
enum Alvo {
    Arquivo(PathBuf),
    Diretorio(PathBuf),
}

impl Alvo {
    fn caminho(&self) -> &Path {
        match self {
            Alvo::Arquivo(p) | Alvo::Diretorio(p) => p,
        }
    }
}

struct Plano {
    /// (alvo, rotulo). A ordem e a ordem da remocao: o binario e o ultimo.
    alvos: Vec<(Alvo, &'static str)>,
    /// Daemon vivo no pidfile, para ser encerrado antes dos arquivos.
    daemon: Option<u32>,
    /// Coisas de proposito mantidas, com o motivo — imprenso antes do prompt.
    mantidos: Vec<String>,
    /// Avisos nao-fatais (config ilegivel, residuo inerte no Windows).
    avisos: Vec<String>,
}

pub(crate) fn run(pedido: Pedido) -> Result<i32> {
    let exe = installed_exe().context("cannot locate the running binary")?;

    // O daemon e detectado ANTES do prompt: a recusa de systemd tem de virar
    // resposta, nao confirmacao desperdicada em cima de um plano que nao vai
    // rodar.
    let daemon = crate::read_pid().filter(|pid| crate::is_process_running(*pid));
    if let Some(pid) = daemon
        && let Some(unidade) = unidade_do_processo(pid)
        && !escotilha_ligada()
    {
        let disable = match unidade.gerente {
            Gerente::Usuario => format!("systemctl --user disable --now {}", unidade.nome),
            Gerente::Sistema => format!("sudo systemctl disable --now {}", unidade.nome),
        };
        eprintln!(
            "garra uninstall: recusando — o daemon (PID {pid}) pertence a unit \
             systemd `{nome}`.\n\n\
             Desinstalar agora mata o binario e deixa a unit orfa em crash-loop. \
             Quem gerencia a unit e o systemd — desligue-a primeiro:\n\n    \
             {stop}\n    \
             {disable}\n\n\
             Depois rode `garra uninstall` de novo. Se voce sabe o que esta fazendo: \
             {env}=1.",
            nome = unidade.nome,
            stop = unidade.comando_stop(),
            env = crate::systemd_guard::ENV_ESCOTILHA,
        );
        return Ok(crate::EX_CONFIG);
    }

    // Config em melhor-esforco: ausente/quebrada nao impede desinstalar, so
    // tira do `--purge` o achado de um `data_dir` fora do diretorio padrao.
    let cfg = garraia_config::ConfigLoader::new()
        .and_then(|loader| loader.load())
        .ok();
    let porta = cfg.as_ref().map(|c| c.gateway.port).unwrap_or(3888);

    let plano = monta_plano(&exe, &pedido, cfg.as_ref(), daemon);

    if plano.alvos.is_empty() && plano.daemon.is_none() {
        println!("Nada para remover — a CLI ja foi desinstalada.");
        return Ok(0);
    }

    imprimir_plano(&plano);

    if !pedido.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("error: desinstalacao sem terminal para confirmar — use --yes");
            return Ok(EX_USAGE);
        }
        let confirmado = dialoguer::Confirm::new()
            .with_prompt("Remover os itens acima?")
            .default(false)
            .interact()
            .context("failed to read confirmation")?;
        if !confirmado {
            println!("Cancelado.");
            return Ok(1);
        }
    }

    executar(&plano, &exe, porta)
}

/// Monta o plano a partir do binario em execucao. Funcao separada para o
/// teste de unidade dirigir o pedido sem processo nenhum no meio.
fn monta_plano(
    exe: &Path,
    pedido: &Pedido,
    cfg: Option<&garraia_config::AppConfig>,
    daemon: Option<u32>,
) -> Plano {
    let mut plano = Plano {
        alvos: Vec::new(),
        daemon,
        mantidos: Vec::new(),
        avisos: Vec::new(),
    };
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));

    // Alias e wrappers primeiro; o binario entra por ultimo e e o ultimo a
    // cair na execucao (no Windows o rename dele e o truque do update.rs).
    #[cfg(unix)]
    {
        let alias = dir.join("garra");
        match std::fs::symlink_metadata(&alias) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let e_o_nosso = std::fs::canonicalize(&alias)
                    .map(|p| p == exe)
                    .unwrap_or(false);
                if e_o_nosso {
                    plano.alvos.push((Alvo::Arquivo(alias), "alias"));
                } else {
                    plano.mantidos.push(format!(
                        "{} — symlink aponta para outro lugar",
                        alias.display()
                    ));
                }
            }
            Ok(_) => plano.mantidos.push(format!(
                "{} — arquivo real no lugar do alias (o instalador so cria symlink; \
                 `--all-binaries` varre a PATH se for sobra de instalacao antiga)",
                alias.display()
            )),
            Err(_) => {}
        }
        for wrapper in ["garra-mcp-server", "garra-mcp-server-linker"] {
            let w = dir.join(wrapper);
            if existe(&w) {
                plano.alvos.push((Alvo::Arquivo(w), "wrapper"));
            }
        }
    }
    #[cfg(windows)]
    {
        // O shim `garra.cmd` e criado pelo install.ps1 e e sempre nosso.
        let shim = dir.join("garra.cmd");
        if existe(&shim) {
            plano.alvos.push((Alvo::Arquivo(shim), "alias"));
        }
    }

    // Backups que o update deixa ao lado do binario.
    for ext in ["old", "new"] {
        let artefato = exe.with_extension(ext);
        if existe(&artefato) {
            plano.alvos.push((Alvo::Arquivo(artefato), "backup"));
        }
    }

    if pedido.all_binaries {
        let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let system_dirs: Vec<PathBuf> = SYSTEM_BIN_DIRS.iter().map(PathBuf::from).collect();
        for outro in find_other_binaries(&path_dirs, &system_dirs, exe) {
            // O alias `garra` ao lado do binario alheio e nosso tambem: o
            // `find_other_binaries` deduplica por canonico, entao o symlink
            // (unix) ou o shim `garra.cmd` (windows) que aponta para este
            // binario nunca aparece na lista — sem esta checagem a varredura
            // apaga o binario e deixa o alias pendurado.
            if let Some(alias) = alias_nosso(&outro.path) {
                plano.alvos.push((Alvo::Arquivo(alias), "alias"));
            }
            plano
                .alvos
                .push((Alvo::Arquivo(outro.path), "outro binario"));
        }
    }

    if pedido.purge {
        let config_dir = crate::garraia_dir();
        match cfg {
            Some(cfg) => {
                let dados = cfg.resolved_data_dir();
                if existe(&dados) && !dados.starts_with(&config_dir) {
                    plano.alvos.push((Alvo::Diretorio(dados), "dados"));
                }
            }
            None => plano.avisos.push(
                "config ilegivel ou ausente; o `--purge` cobre so o diretorio padrao de config"
                    .to_string(),
            ),
        }
        if existe(&config_dir) {
            plano
                .alvos
                .push((Alvo::Diretorio(config_dir.clone()), "config"));
        }
        if let Some(home) = dirs::home_dir() {
            let legado = home.join(".garraia");
            if existe(&legado) && legado != config_dir {
                plano.alvos.push((Alvo::Diretorio(legado), "legado"));
            }
        }
    }

    plano
        .alvos
        .push((Alvo::Arquivo(exe.to_path_buf()), "binario"));

    // O que de proposito fica — impreso no plano, para a decisao ser informada.
    if let Some(app) = crate::desktop::locate() {
        plano.mantidos.push(format!(
            "app desktop em {} — desinstale pelo instalador da plataforma",
            app.path.display()
        ));
    }
    if let Some(home) = dirs::home_dir() {
        let heranca = home.join(".garra");
        if existe(&heranca) {
            plano.mantidos.push(format!(
                "{} — sessoes e skills do Learning Agent; apague a mao se quiser",
                heranca.display()
            ));
        }
    }
    #[cfg(windows)]
    plano.mantidos.push(
        "a entrada de PATH do usuario no Registro nao e alterada; remova em \
         Variaveis de Ambiente se quiser"
            .to_string(),
    );

    plano
}

fn imprimir_plano(plano: &Plano) {
    println!("Alvos:");
    if let Some(pid) = plano.daemon {
        println!(
            "  {:<13} pid {pid} sera encerrado antes da remocao",
            "daemon"
        );
    }
    for (alvo, rotulo) in &plano.alvos {
        println!("  {rotulo:<13} {}", alvo.caminho().display());
    }
    if !plano.mantidos.is_empty() {
        println!("Nao sera removido:");
        for nota in &plano.mantidos {
            println!("  {nota}");
        }
    }
}

fn executar(plano: &Plano, exe: &Path, porta: u16) -> Result<i32> {
    let mut erros: Vec<String> = Vec::new();
    let mut avisos: Vec<String> = Vec::new();
    let mut removidos = 0usize;

    if let Some(pid) = plano.daemon {
        println!("Encerrando daemon (pid {pid})...");
        // O pidfile e a fonte daqui e do try_stop_daemon — o fallback por
        // porta so entra quando o arquivo sumiu no meio, e ai a porta da
        // config (ou o default 3888) e o melhor palpite.
        crate::try_stop_daemon(porta);
    }

    for (alvo, rotulo) in &plano.alvos {
        match remover(alvo, exe) {
            Ok(Some(nota)) => avisos.push(nota),
            Ok(None) => {}
            Err(e) => {
                erros.push(format!("{rotulo} {}: {e:#}", alvo.caminho().display()));
                continue;
            }
        }
        removidos += 1;
        println!("removido  {rotulo:<13} {}", alvo.caminho().display());
    }

    for nota in plano.avisos.iter().chain(avisos.iter()) {
        println!("aviso: {nota}");
    }

    if erros.is_empty() {
        println!("Desinstalacao concluida: {removidos} item(ns) removido(s).");
        return Ok(0);
    }
    for erro in &erros {
        eprintln!("error: {erro}");
    }
    eprintln!(
        "garra uninstall: {removidos} item(ns) removidos, {} falha(s). \
         Itens com permissao de escrita em `/usr/local/bin` pedem sudo.",
        erros.len()
    );
    Ok(EX_SOFTWARE)
}

/// Remove um alvo. No Windows o binario em execucao nao pode ser apagado de
/// uma vez: o rename para `.old` e o mesmo truque do `update.rs` — rename
/// funciona com o exe rodando, delete nao. Se o delete do `.old` falhar, o
/// binario ja sumiu da PATH e o residuo e inerte: vira aviso, nao erro.
fn remover(alvo: &Alvo, binario: &Path) -> Result<Option<String>> {
    match alvo {
        Alvo::Diretorio(p) => {
            std::fs::remove_dir_all(p)
                .with_context(|| format!("cannot remove directory {}", p.display()))?;
            Ok(None)
        }
        Alvo::Arquivo(p) => {
            #[cfg(windows)]
            {
                if p == binario {
                    let old = p.with_extension("old");
                    std::fs::rename(p, &old).with_context(|| {
                        format!("cannot rename {} to {}", p.display(), old.display())
                    })?;
                    return match std::fs::remove_file(&old) {
                        Ok(()) => Ok(None),
                        Err(_) => Ok(Some(format!(
                            "{} estava em uso; o residuo inerte {} pode ser apagado \
                             apos fechar os terminais",
                            p.display(),
                            old.display()
                        ))),
                    };
                }
            }
            #[cfg(not(windows))]
            let _ = binario;
            std::fs::remove_file(p).with_context(|| format!("cannot remove {}", p.display()))?;
            Ok(None)
        }
    }
}

fn existe(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// O alias nosso ao lado de `binario` (outro binario da varredura): no Unix,
/// o symlink `garra` que canoniza para ele; no Windows, o shim `garra.cmd` do
/// install.ps1, que e sempre nosso. So o alias — o binario entra no plano por
/// conta propria.
#[cfg(unix)]
fn alias_nosso(binario: &Path) -> Option<PathBuf> {
    let dir = binario.parent()?;
    let alias = dir.join("garra");
    let meta = std::fs::symlink_metadata(&alias).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    // Canonico dos dois lados: o binario da varredura pode vir de um diretorio
    // que e em si symlink (PATH com `/home/u/bin -> ...`).
    let aponta = std::fs::canonicalize(&alias).ok()? == std::fs::canonicalize(binario).ok()?;
    aponta.then_some(alias)
}

/// Variante Windows: o shim `garra.cmd` do install.ps1, que e sempre nosso.
#[cfg(windows)]
fn alias_nosso(binario: &Path) -> Option<PathBuf> {
    let shim = binario.parent()?.join("garra.cmd");
    existe(&shim).then_some(shim)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Regressao do E2E de uninstall em Linux: a varredura `--all-binaries`
    /// apagava o binario alheio e deixava o alias `garra` do mesmo diretorio
    /// pendurado apontando para o nada.
    #[test]
    fn alias_nosso_aponta_para_o_binario_e_nao_para_outro_lugar() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let binario = dir.join("garraia");
        std::fs::write(&binario, b"bin").expect("write");
        let alias = dir.join("garra");
        std::os::unix::fs::symlink(&binario, &alias).expect("symlink");

        assert_eq!(alias_nosso(&binario).as_deref(), Some(alias.as_path()));

        // Symlink para outro lugar nao e nosso: fica.
        let terceiro = dir.join("outra-coisa");
        std::fs::write(&terceiro, b"x").expect("write");
        std::fs::remove_file(&alias).expect("rm");
        std::os::unix::fs::symlink(&terceiro, &alias).expect("symlink");
        assert_eq!(alias_nosso(&binario), None);

        // Arquivo real no lugar do alias tambem nao e nosso.
        std::fs::remove_file(&alias).expect("rm");
        std::fs::write(&alias, b"nao-sou-symlink").expect("write");
        assert_eq!(alias_nosso(&binario), None);
    }

    /// O binario sem alias ao lado e so o binario: nada a acrescentar.
    #[test]
    fn alias_nosso_ausente_devolve_nada() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let binario = tmp.path().join("garraia");
        std::fs::write(&binario, b"bin").expect("write");
        assert_eq!(alias_nosso(&binario), None);
    }
}
