//! `garra desktop` — localiza e lança o GarraIA Desktop instalado.
//!
//! M1 do épico #1181, decidido no [ADR 0021]. Três modos:
//!
//! ```text
//! garra desktop              # localiza o aplicativo e lança
//! garra desktop --status     # diz se está instalado e onde
//! garra desktop --no-launch  # só imprime o caminho resolvido (scriptável)
//! ```
//!
//! # Por que a CLI não embute nada de GUI
//!
//! Driver #5 do ADR 0021, inegociável: uma dependência de Tauri aqui quebraria
//! `cargo build --workspace --exclude garraia-desktop` e, com ele, toda
//! instalação headless — Termux, RunPod, Docker. Por isso este módulo só
//! resolve um caminho e chama `spawn`. A resolução em si mora em
//! `garraia-desktop-core`, que não conhece Tauri e entra nos gates de CI.
//!
//! # O que este comando deliberadamente não responde
//!
//! **Se o aplicativo já está rodando.** Responder isso de verdade exige ou
//! enumerar processos nas três plataformas — coisa que a CLI nunca fez e que
//! traria dependência nova e uma heurística por nome de processo, frágil por
//! construção — ou um canal de instância única na casca Tauri, que é onde a
//! resposta pode ser confiável. O segundo caminho é o do M2. Até lá o
//! `--status` diz que não sabe, em vez de inventar um "não" que seria falso
//! toda vez que o aplicativo estivesse aberto.
//!
//! [ADR 0021]: ../../../docs/adr/0021-garraia-desktop-control-center.md

use garraia_desktop_core::locate::{DesktopApp, Locator};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Aplicativo não encontrado (`EX_UNAVAILABLE`, sysexits).
///
/// Mesma família de exit codes que o `config check` usa desde o plan 0035:
/// script que chama `garra desktop --no-launch` consegue distinguir "não
/// instalado" de "instalado mas não abriu".
const EX_UNAVAILABLE: i32 = 69;

/// Encontrado, mas o lançamento falhou (`EX_SOFTWARE`, sysexits).
const EX_SOFTWARE: i32 = 70;

/// Abstrai o lançamento para o teste não abrir janela nenhuma.
pub trait DesktopLauncher {
    fn launch(&self, exe: &Path) -> io::Result<()>;
}

/// Lançamento real: processo solto, sem herdar o terminal.
pub struct RealLauncher;

impl DesktopLauncher for RealLauncher {
    fn launch(&self, exe: &Path) -> io::Result<()> {
        // `spawn` e não `status`: a CLI não fica pendurada esperando o usuário
        // fechar a janela. Os três descritores vão para `null` porque um app
        // gráfico não deve escrever no terminal de quem o lançou — e porque um
        // stdout herdado mantém o pipe aberto depois que a CLI sai.
        Command::new(exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

/// Instrução acionável para quem não tem o aplicativo instalado.
fn print_not_installed() {
    eprintln!("GarraIA Desktop nao encontrado nesta maquina.");
    eprintln!();
    eprintln!("Procurei, nesta ordem:");
    eprintln!("  1. o diretorio onde o instalador da plataforma o deixa");
    eprintln!("  2. os diretorios da PATH");
    eprintln!("  3. o diretorio desta CLI (instalacao lado-a-lado)");
    eprintln!();
    eprintln!("Baixe o instalador do desktop em:");
    eprintln!("  https://github.com/michelbr84/GarraRUST/releases/latest");
    eprintln!();
    eprintln!("A CLI continua funcionando sozinha — `garra chat`, `garra start`.");
}

fn print_status(found: Option<&DesktopApp>) {
    println!("GarraIA Desktop");
    match found {
        Some(app) => {
            println!("  instalado: sim");
            println!("  caminho:   {}", app.path.display());
            println!("  origem:    {}", app.source.describe());
        }
        None => {
            println!("  instalado: nao");
        }
    }
    // Ver a nota no topo do módulo: dizer "nao" aqui seria mentir sempre que o
    // aplicativo estivesse aberto.
    println!("  execucao:  nao detectavel nesta versao (#1181, M2)");
}

/// `garra desktop [--status] [--no-launch]`.
///
/// `found` é injetado para o teste não depender do que está instalado na
/// máquina. `main` passa o resultado de [`Locator::from_env`].
///
/// Devolve o exit code; nunca lança quando `status` ou `no_launch` estão
/// ligados — os dois são modos de leitura, e um deles lançar uma janela por
/// engano num script seria surpresa cara.
pub fn run(
    status: bool,
    no_launch: bool,
    found: Option<DesktopApp>,
    launcher: &dyn DesktopLauncher,
) -> i32 {
    if status {
        print_status(found.as_ref());
        return if found.is_some() { 0 } else { EX_UNAVAILABLE };
    }

    let Some(app) = found else {
        print_not_installed();
        return EX_UNAVAILABLE;
    };

    if no_launch {
        // Só o caminho, em stdout: é o modo scriptável.
        println!("{}", app.path.display());
        return 0;
    }

    // O comando fica visível antes de rodar — regra 4 do §detecção do ADR 0021.
    eprintln!("Abrindo {}", app.path.display());
    match launcher.launch(&app.path) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: falha ao lancar {}: {e}", app.path.display());
            EX_SOFTWARE
        }
    }
}

/// Resolve o aplicativo no ambiente real.
pub fn locate() -> Option<DesktopApp> {
    Locator::from_env().locate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_desktop_core::locate::Source;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct SpyLauncher {
        launched: RefCell<Vec<PathBuf>>,
        fail: bool,
    }

    impl DesktopLauncher for SpyLauncher {
        fn launch(&self, exe: &Path) -> io::Result<()> {
            self.launched.borrow_mut().push(exe.to_path_buf());
            if self.fail {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"))
            } else {
                Ok(())
            }
        }
    }

    fn app() -> DesktopApp {
        DesktopApp {
            path: PathBuf::from("/usr/bin/garraia-desktop"),
            source: Source::Install,
        }
    }

    #[test]
    fn lanca_o_aplicativo_encontrado() {
        let spy = SpyLauncher::default();
        assert_eq!(run(false, false, Some(app()), &spy), 0);
        assert_eq!(
            spy.launched.borrow().as_slice(),
            &[PathBuf::from("/usr/bin/garraia-desktop")]
        );
    }

    #[test]
    fn sem_aplicativo_instalado_instrui_em_vez_de_estourar() {
        let spy = SpyLauncher::default();
        assert_eq!(
            run(false, false, None, &spy),
            EX_UNAVAILABLE,
            "nao instalado e EX_UNAVAILABLE, nao um erro generico"
        );
        assert!(spy.launched.borrow().is_empty());
    }

    #[test]
    fn no_launch_resolve_sem_abrir_janela() {
        let spy = SpyLauncher::default();
        assert_eq!(run(false, true, Some(app()), &spy), 0);
        assert!(
            spy.launched.borrow().is_empty(),
            "--no-launch existe justamente para nao lancar"
        );
    }

    #[test]
    fn status_nunca_lanca_nem_com_o_aplicativo_instalado() {
        let spy = SpyLauncher::default();
        assert_eq!(run(true, false, Some(app()), &spy), 0);
        assert!(spy.launched.borrow().is_empty());
    }

    #[test]
    fn status_sem_aplicativo_sai_com_ex_unavailable() {
        // Assim `garra desktop --status` serve de teste em script.
        let spy = SpyLauncher::default();
        assert_eq!(run(true, false, None, &spy), EX_UNAVAILABLE);
    }

    #[test]
    fn status_tem_prioridade_sobre_no_launch() {
        let spy = SpyLauncher::default();
        assert_eq!(run(true, true, Some(app()), &spy), 0);
        assert!(spy.launched.borrow().is_empty());
    }

    #[test]
    fn falha_de_lancamento_e_distinguivel_de_nao_instalado() {
        let spy = SpyLauncher {
            fail: true,
            ..Default::default()
        };
        let code = run(false, false, Some(app()), &spy);
        assert_eq!(code, EX_SOFTWARE);
        assert_ne!(
            code, EX_UNAVAILABLE,
            "script precisa separar `nao instalado` de `nao abriu`"
        );
    }

    /// Driver #5 do ADR 0021: nenhuma dependência de Tauri pode entrar na CLI
    /// por este caminho, senão `cargo build --workspace --exclude
    /// garraia-desktop` quebra e com ele toda instalação headless.
    #[test]
    fn a_cli_nao_encosta_em_tauri() {
        let fonte = include_str!("desktop.rs");
        // Ignora este próprio teste, que precisa citar o nome proibido.
        let ate_o_teste = fonte
            .split("fn a_cli_nao_encosta_em_tauri")
            .next()
            .unwrap_or(fonte);
        let corpo: String = ate_o_teste
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!corpo.contains("tauri"), "a CLI nao pode depender de Tauri");
    }
}
