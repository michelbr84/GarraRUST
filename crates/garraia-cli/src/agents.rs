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
use std::path::{Path, PathBuf};
use std::process::Command;

/// Instalador oficial do AgentDeck (michelbr84/AgentDeck).
///
/// **Não** use `npm install -g agentdeck`: o nome `agentdeck` no registro do npm
/// pertence a um projeto **diferente e sem relação** ("Mobile control for your
/// coding agents"). Instalá-lo traria um binário alheio com o nome certo — o
/// tipo de erro que só aparece num teste ponta a ponta, e que instalaria
/// software de terceiro na máquina do usuário sem ele pedir.
const AGENTDECK_INSTALL_URL: &str =
    "https://raw.githubusercontent.com/michelbr84/AgentDeck/main/scripts/install.sh";

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
    /// `true` quando o binário encontrado é o AgentDeck certo — isto é, quando
    /// ele conhece o grupo de comandos `agents`.
    fn supports_agents_command(&self, bin: &Path) -> bool;
}

pub struct RealProbe;

impl DeckProbe for RealProbe {
    fn find_agentdeck(&self) -> Option<PathBuf> {
        which_in_path("agentdeck")
    }

    fn supports_agents_command(&self, bin: &Path) -> bool {
        // Um binário chamado `agentdeck` na PATH não é prova de nada: existe um
        // pacote npm homônimo e sem relação. Perguntar pelo comando que nos
        // interessa é a checagem barata que distingue os dois.
        Command::new(bin)
            .args(["agents", "--help"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
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
    eprintln!("  curl -fsSL {AGENTDECK_INSTALL_URL} | bash");
    eprintln!();
    eprintln!("Depois rode `garra agents setup` de novo.");
    eprintln!();
    eprintln!("Nota: NÃO use `npm install -g agentdeck` — esse nome no npm é de");
    eprintln!("outro projeto, sem relação com o AgentDeck que este comando usa.");
}

/// Mensagem para quando existe um `agentdeck` na PATH que não é o nosso.
fn print_wrong_deck_hint(bin: &Path) {
    eprintln!(
        "O binário `agentdeck` em {} não reconhece o comando `agents`.",
        bin.display()
    );
    eprintln!();
    eprintln!("Provavelmente é o pacote npm homônimo, que é outro projeto.");
    eprintln!("Instale o AgentDeck correto e garanta que ele venha antes na PATH:");
    eprintln!("  curl -fsSL {AGENTDECK_INSTALL_URL} | bash");
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
        // Encontrado, mas pode ser o homônimo: confirmar antes de delegar.
        Some(p) if probe.supports_agents_command(&p) => p,
        Some(p) => {
            print_wrong_deck_hint(&p);
            return Ok(1);
        }
        None => {
            if !should_offer_install(interactive, assume_yes)? {
                return Ok(1);
            }
            println!("Instalando o AgentDeck a partir do instalador oficial...");
            let status = Command::new("sh")
                .arg("-c")
                .arg(format!("curl -fsSL {AGENTDECK_INSTALL_URL} | bash"))
                .status()
                .context("falha ao executar o instalador do AgentDeck")?;
            if !status.success() {
                eprintln!("error: o instalador do AgentDeck falhou.");
                return Ok(status.code().unwrap_or(1));
            }
            let found = probe.find_agentdeck().ok_or_else(|| {
                anyhow::anyhow!(
                    "o instalador reportou sucesso mas o binário `agentdeck` não apareceu \
                     no PATH; abra um terminal novo ou confira o diretório de instalação"
                )
            })?;
            if !probe.supports_agents_command(&found) {
                print_wrong_deck_hint(&found);
                return Ok(1);
            }
            found
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
        supports_agents: bool,
    }

    impl DeckProbe for FakeProbe {
        fn find_agentdeck(&self) -> Option<PathBuf> {
            self.deck.clone()
        }
        fn supports_agents_command(&self, _bin: &Path) -> bool {
            self.supports_agents
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
    fn a_homonymous_binary_is_refused_rather_than_driven() {
        // `agentdeck` on npm is an unrelated project. Finding *a* binary with
        // that name proves nothing, and delegating to the wrong one would fail
        // in a confusing way far from the cause.
        let probe = FakeProbe {
            deck: Some(PathBuf::from("/usr/bin/agentdeck")),
            supports_agents: false,
        };
        let code = run(AgentsAction::Status, &[], true, &probe).expect("must not error");
        assert_eq!(code, 1, "a wrong agentdeck exits cleanly with guidance");
    }

    #[test]
    fn the_install_url_points_at_the_right_project() {
        // Guard against anyone "simplifying" this back to `npm install -g`.
        assert!(AGENTDECK_INSTALL_URL.contains("michelbr84/AgentDeck"));
        assert!(!AGENTDECK_INSTALL_URL.contains("registry.npmjs"));
    }

    #[test]
    fn which_in_path_finds_a_real_binary() {
        // `sh` existe em qualquer unix onde estes testes rodam.
        assert!(which_in_path("sh").is_some());
        assert!(which_in_path("nao-existe-esse-binario-xyz").is_none());
    }
}
