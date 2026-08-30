//! `garra agents` — provisiona os agentes externos e o roteamento de LLM.
//!
//! Este comando é uma casca fina. O motor é o **AgentDeck**, que já tem
//! adapters completos para GarraIA, Hermes, OpenClaw e Claude Code (detecção,
//! instalação, upgrade, health check, backup e rollback), além do deck web e
//! dos grupos. Reimplementar isso em Rust seria manter duas cópias da mesma
//! lógica em duas linguagens.
//!
//! O que este módulo faz, portanto: descobre o binário `agentdeck`, oferece
//! instalá-lo se faltar, e repassa os argumentos. Nada mais — qualquer regra de
//! negócio aqui viraria drift em relação ao motor.

use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

/// Pacote npm que provê o binário `agentdeck`.
const AGENTDECK_NPM_PACKAGE: &str = "agentdeck";

/// Subcomandos repassados ao `agentdeck`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsAction {
    Setup,
    Status,
    Rollback,
    Link,
    Web,
}

impl AgentsAction {
    /// Argumentos que o `agentdeck` espera para esta ação.
    fn argv(self) -> &'static [&'static str] {
        match self {
            AgentsAction::Setup => &["agents", "setup"],
            AgentsAction::Status => &["agents", "status"],
            AgentsAction::Rollback => &["agents", "rollback"],
            AgentsAction::Link => &["agents", "link"],
            // `web` é comando de topo no AgentDeck, não subcomando de `agents`.
            AgentsAction::Web => &["web"],
        }
    }
}

/// Abstrai a descoberta e execução do binário, para os testes não dependerem
/// de haver um `agentdeck` instalado na máquina.
pub trait DeckProbe {
    /// Caminho do binário `agentdeck`, se existir.
    fn find_agentdeck(&self) -> Option<PathBuf>;
    /// Caminho do `npm`, se existir.
    fn find_npm(&self) -> Option<PathBuf>;
}

pub struct RealProbe;

impl DeckProbe for RealProbe {
    fn find_agentdeck(&self) -> Option<PathBuf> {
        which_in_path("agentdeck")
    }

    fn find_npm(&self) -> Option<PathBuf> {
        which_in_path("npm")
    }
}

/// `which` sem depender de um crate externo: varre `PATH` procurando um
/// arquivo executável com o nome dado.
fn which_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows: o executável carrega extensão.
        for ext in ["exe", "cmd", "bat"] {
            let with_ext = dir.join(format!("{bin}.{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

/// Mensagem impressa quando o AgentDeck não está instalado e não há como
/// perguntar (stdin não é TTY: CI, pipe, systemd).
fn print_missing_deck_hint() {
    eprintln!("AgentDeck não encontrado — é ele que provisiona os agentes.");
    eprintln!();
    eprintln!("Instale com:");
    eprintln!("  npm install -g {AGENTDECK_NPM_PACKAGE}");
    eprintln!();
    eprintln!("ou:");
    eprintln!(
        "  curl -fsSL https://raw.githubusercontent.com/michelbr84/AgentDeck/main/scripts/install.sh | bash"
    );
    eprintln!();
    eprintln!("Depois rode `garra agents setup` de novo.");
}

/// Decide o que fazer quando o `agentdeck` não foi encontrado.
///
/// Separado de `run` para ser testável: devolve `Ok(true)` quando a instalação
/// deve ser tentada, `Ok(false)` quando o usuário recusou ou não há como
/// perguntar.
fn should_offer_install(interactive: bool, assume_yes: bool) -> Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !interactive {
        print_missing_deck_hint();
        return Ok(false);
    }
    dialoguer::Confirm::new()
        .with_prompt("AgentDeck não está instalado. Instalar agora via npm?")
        .default(true)
        .interact()
        .context("confirmação cancelada")
}

/// `garra agents <ação> [args…]`.
///
/// `passthrough` recebe as flags do usuário verbatim (`--dry-run`, `--yes`,
/// `--provider …`), porque o contrato dessas flags pertence ao AgentDeck e
/// duplicá-lo aqui só criaria divergência.
pub fn run(
    action: AgentsAction,
    passthrough: &[String],
    assume_yes: bool,
    probe: &dyn DeckProbe,
) -> Result<i32> {
    let interactive = std::io::stdin().is_terminal();

    let bin = match probe.find_agentdeck() {
        Some(p) => p,
        None => {
            if !should_offer_install(interactive, assume_yes)? {
                return Ok(1);
            }
            let Some(npm) = probe.find_npm() else {
                eprintln!("error: `npm` não encontrado no PATH — instale o Node.js 20+ primeiro.");
                print_missing_deck_hint();
                return Ok(1);
            };
            println!("Instalando {AGENTDECK_NPM_PACKAGE} via npm...");
            let status = Command::new(npm)
                .args(["install", "-g", AGENTDECK_NPM_PACKAGE])
                .status()
                .context("falha ao executar npm")?;
            if !status.success() {
                eprintln!("error: `npm install -g {AGENTDECK_NPM_PACKAGE}` falhou.");
                return Ok(status.code().unwrap_or(1));
            }
            probe.find_agentdeck().ok_or_else(|| {
                anyhow::anyhow!(
                    "npm reportou sucesso mas o binário `agentdeck` não apareceu no PATH; \
                     confira o prefixo global do npm (`npm config get prefix`)"
                )
            })?
        }
    };

    let status = Command::new(&bin)
        .args(action.argv())
        .args(passthrough)
        .status()
        .with_context(|| format!("falha ao executar {}", bin.display()))?;

    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProbe {
        deck: Option<PathBuf>,
        npm: Option<PathBuf>,
    }

    impl DeckProbe for FakeProbe {
        fn find_agentdeck(&self) -> Option<PathBuf> {
            self.deck.clone()
        }
        fn find_npm(&self) -> Option<PathBuf> {
            self.npm.clone()
        }
    }

    #[test]
    fn action_argv_maps_to_the_agentdeck_command_tree() {
        assert_eq!(AgentsAction::Setup.argv(), &["agents", "setup"]);
        assert_eq!(AgentsAction::Status.argv(), &["agents", "status"]);
        assert_eq!(AgentsAction::Rollback.argv(), &["agents", "rollback"]);
        assert_eq!(AgentsAction::Link.argv(), &["agents", "link"]);
        // `web` é comando de topo, não subcomando de `agents`.
        assert_eq!(AgentsAction::Web.argv(), &["web"]);
    }

    #[test]
    fn non_interactive_without_deck_prints_a_hint_instead_of_hanging() {
        // Sem TTY não dá para perguntar; a resposta certa é instruir, não travar.
        assert!(!should_offer_install(false, false).expect("no prompt attempted"));
    }

    #[test]
    fn assume_yes_skips_the_prompt_even_without_a_tty() {
        assert!(should_offer_install(false, true).expect("no prompt attempted"));
    }

    #[test]
    fn missing_npm_is_reported_rather_than_panicking() {
        let probe = FakeProbe {
            deck: None,
            npm: None,
        };
        let code = run(AgentsAction::Status, &[], true, &probe).expect("must not error");
        assert_eq!(code, 1, "missing npm is a clean exit code, not a panic");
    }

    #[test]
    fn which_in_path_finds_a_real_binary() {
        // `sh` existe em qualquer unix onde estes testes rodam.
        assert!(which_in_path("sh").is_some());
        assert!(which_in_path("nao-existe-esse-binario-xyz").is_none());
    }
}
