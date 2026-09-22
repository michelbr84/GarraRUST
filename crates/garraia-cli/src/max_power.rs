//! `garra max-power` — GarraMaxPower pipeline entry point (GAR-494 skeleton).
//!
//! Routes a goal description to the correct workflow by keyword matching.
//! The full state-machine execution (brainstorm → spec → plan → execute) is
//! implemented in GAR-495..GAR-501; this module is the entry-point skeleton.
//!
//! At startup the module loads `.garra-estado.md` (GAR-500) and prints a
//! one-line handoff summary so the operator knows where the previous session
//! left off. GAR-495 adds a capability summary showing available providers,
//! tools, channels, and MCP servers.

use garraia_agents::{
    AgentRuntime, ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart,
};
use garraia_common::handoff;
use garraia_skills::SkillCompleter;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::capability_prompt;
use crate::chat;
use crate::repo_workflow;
use crate::team::{AgentTeam, ReviewDecision, TeamSummary};

/// Default path for the handoff state file (relative to CWD).
const HANDOFF_FILE: &str = ".garra-estado.md";

/// Name-to-keyword mapping for route detection.
const ROUTES: &[(&str, &[&str])] = &[
    (
        "systematic-debugging",
        &[
            "bug",
            "fix",
            "crash",
            "error",
            "broken",
            "panic",
            "regression",
            "issue",
        ],
    ),
    (
        "brainstorm",
        &[
            "feature",
            "add",
            "implement",
            "build",
            "create",
            "new",
            "idea",
            "design",
        ],
    ),
    (
        "refactor-module",
        &[
            "refactor",
            "clean",
            "extract",
            "rename",
            "simplify",
            "restructure",
            "improve",
            "reorganize",
        ],
    ),
    (
        "tdd-loop",
        &[
            "test",
            "coverage",
            "spec",
            "unit",
            "integration",
            "tdd",
            "assertion",
        ],
    ),
    (
        "generate-docs",
        &[
            "docs",
            "document",
            "readme",
            "explain",
            "describe",
            "documentation",
        ],
    ),
    (
        "code-review",
        &[
            "review", "audit", "check", "inspect", "analyse", "analyze", "security",
        ],
    ),
];

/// Detect which pipeline to run based on keywords in the goal string.
/// Returns the route name and the matched keyword, or the default `brainstorm`.
pub fn detect_route(goal: &str) -> (&'static str, Option<&'static str>) {
    let lower = goal.to_lowercase();
    for (route, keywords) in ROUTES {
        for kw in *keywords {
            if lower.contains(kw) {
                return (route, Some(kw));
            }
        }
    }
    ("brainstorm", None)
}

/// Entry point for `garraia max-power`.
///
/// Async de proposito (#1228, achado do dogfood): o comando e despachado de
/// dentro do `async_main` da CLI, que ja roda num runtime tokio. A versao
/// anterior era sincrona e criava um segundo runtime com `block_on`, o que
/// faz o tokio entrar em panico ("Cannot start a runtime from within a
/// runtime") em todo `garraia max-power --goal`: o modo provider-backed
/// nunca rodou num binario publicado.
pub async fn run(goal: Option<String>, mode: String, config: &garraia_config::AppConfig) {
    print_handoff_summary();
    match goal {
        None => print_menu_with_capabilities(config),
        Some(g) => {
            print_capability_summary(config);
            let completer = build_completer(config).await;
            route_goal(&g, &mode, completer.as_ref()).await;
        }
    }
}

/// Provider-backed executor for native skills (GAR-498 follow-up).
///
/// Adapts the resolved default provider + `AgentRuntime` (retry/fallback
/// chain) to the dependency-inverted `SkillCompleter` seam. Raw model text
/// is filed into `SkillRunOutput.model_output`; the deterministic scaffold
/// always stays alongside for review.
struct RuntimeCompleter {
    runtime: AgentRuntime,
    provider: Arc<dyn LlmProvider>,
    model: String,
}

impl SkillCompleter for RuntimeCompleter {
    fn complete<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = garraia_common::Result<String>> + Send + 'a>> {
        let request = LlmRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text(prompt.to_string()),
            }],
            system: None,
            max_tokens: Some(2048),
            temperature: None,
            tools: vec![],
        };
        let runtime = &self.runtime;
        let provider = &self.provider;
        Box::pin(async move {
            let response = runtime.complete_with_fallback(provider, &request).await?;
            let text = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        })
    }
}

/// Resolve the default provider (same chain as `garra chat`) and wrap it
/// in a `RuntimeCompleter`. `None` → the pipeline runs deterministic
/// offline mode — never a hard failure, max-power stays usable without
/// any provider configured.
async fn build_completer(config: &garraia_config::AppConfig) -> Option<RuntimeCompleter> {
    let (_config_key, model, provider) = chat::detect_provider(config, None, None, true).await;
    let runtime = AgentRuntime::new();
    runtime.register_provider(Arc::clone(&provider));
    Some(RuntimeCompleter {
        runtime,
        provider,
        model,
    })
}

/// Load `.garra-estado.md` and print a one-line handoff summary if the file
/// exists and contains a previous action.  Silently skips on missing file or
/// parse error (fail-closed per design invariant).
fn print_handoff_summary() {
    let path = std::path::Path::new(HANDOFF_FILE);
    match handoff::load(path) {
        Ok(state) if state.last_action.is_some() || state.next_action.is_some() => {
            println!("  [handoff] {}", state.summary());
            println!();
        }
        _ => {}
    }
}

fn print_capability_summary(config: &garraia_config::AppConfig) {
    let snap = capability_prompt::build_snapshot_merged(config);
    println!(
        "  [capabilities] {}",
        capability_prompt::render_summary(&snap)
    );
    println!();
}

fn print_menu_with_capabilities(config: &garraia_config::AppConfig) {
    let snap = capability_prompt::build_snapshot_merged(config);
    println!();
    println!("  ╔══════════════════════════════════════════╗");
    println!("  ║          G A R R A  M A X  P O W E R    ║");
    println!("  ║     Autonomous AI Development Pipeline   ║");
    println!("  ╚══════════════════════════════════════════╝");
    println!();
    print!("{}", capability_prompt::render_prompt(&snap));
    println!("  Pipeline stages:");
    println!("    1. Brainstorm  — explore possibilities, generate ideas");
    println!("    2. Spec        — define acceptance criteria");
    println!("    3. Plan        — architecture + task breakdown");
    println!("    4. Execute     — TDD implementation loop");
    println!("    5. Review      — code review + security audit");
    println!("    6. Merge       — CI green → squash merge");
    println!();
    println!("  Entry points (pass --goal to skip this menu):");
    println!("    --goal \"fix bug X\"           → systematic-debugging");
    println!("    --goal \"add feature Y\"        → brainstorm");
    println!("    --goal \"refactor module Z\"    → refactor-module");
    println!("    --goal \"write tests for W\"    → tdd-loop");
    println!("    --goal \"document API V\"       → generate-docs");
    println!("    --goal \"review auth module\"   → code-review");
    println!();
    println!("  Modes: --mode new (fresh start) | existing (resume) | auto (detect)");
    println!();
    println!("{}", linha_de_exemplo(&crate::binario::nome()));
    println!();
}

/// A linha de exemplo do menu, com o nome do binario que de fato esta
/// rodando (#1228). O literal antigo dizia `garra`, que e so o alias do
/// instalador: numa imagem Docker ou num `cargo install` o comando impresso
/// nao existia na maquina.
fn linha_de_exemplo(binario: &str) -> String {
    format!("  Example: {binario} max-power --goal \"fix the login crash\" --mode new")
}

async fn route_goal(goal: &str, mode: &str, completer: Option<&RuntimeCompleter>) {
    let (route, matched_kw) = detect_route(goal);
    println!("route: {route}");
    match matched_kw {
        Some(kw) => println!("rationale: keyword '{kw}' matched in goal"),
        None => println!("rationale: no specific keyword matched — defaulting to brainstorm"),
    }
    println!("mode: {mode}");
    println!("goal: {goal}");
    println!(
        "execution: {}",
        if completer.is_some() {
            "provider-backed"
        } else {
            "deterministic (offline)"
        }
    );
    println!();
    print_repo_preflight();
    let team = AgentTeam::new();
    let summary = match completer {
        Some(c) => team.run_with_completer(goal, c).await,
        None => team.run(goal).await,
    };
    print_team_summary(&summary);
}

fn print_team_summary(summary: &TeamSummary) {
    println!(
        "  ── Agent Team Pipeline: {} ──────────────────────────────",
        summary.goal
    );
    for result in &summary.phases {
        let status = match &result.decision {
            ReviewDecision::Accepted => "✓",
            ReviewDecision::NeedsRevision { .. } => "~",
            ReviewDecision::Rejected { .. } => "✗",
        };
        let role_label = match result.role {
            crate::team::TeamRole::Orchestrator => "orch",
            crate::team::TeamRole::Reviewer => "reviewer",
            crate::team::TeamRole::Executor => "exec",
        };
        println!(
            "  {status} [{label}/{role_label}] {summary}",
            label = result.phase.label(),
            summary = result.output.summary,
        );
        if !result.output.next_steps.is_empty() {
            for step in &result.output.next_steps {
                println!("      → {step}");
            }
        }
    }
    println!();
    if summary.completed {
        println!("  Pipeline complete. Open a PR when ready.");
    } else {
        println!("  Pipeline halted — review the phase marked ✗ or ~ above.");
    }
    println!();
}

/// Print a git preflight summary (current branch + tree status).
///
/// Silently no-ops when the current directory is not inside a git repository
/// or when `git` is not in `PATH`.
fn print_repo_preflight() {
    if let Some(summary) = repo_workflow::preflight_summary() {
        println!("  [repo] {}", summary.display());
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1228 (dogfood): o comando roda de dentro do runtime da CLI. Antes, o
    /// `block_on` interno criava um segundo runtime e o tokio entrava em
    /// panico em todo `garraia max-power --goal`. Este teste roda o roteamento
    /// no mesmo cenario do `async_main` (runtime multi-thread ja ativo), sem
    /// provider, para nao depender do ambiente.
    #[tokio::test(flavor = "multi_thread")]
    async fn roteamento_roda_dentro_do_runtime_da_cli() {
        route_goal("fix the login crash", "new", None).await;
    }

    /// #1228 (dogfood): nenhum caminho de producao deste modulo cria runtime
    /// proprio. Foi um `block_on` num runtime novo, dentro do runtime do
    /// `async_main`, que derrubava todo `garraia max-power --goal`. Varre o
    /// fonte fora do bloco de testes.
    #[test]
    fn modulo_nao_cria_runtime_proprio() {
        for (nome, fonte) in [
            ("max_power.rs", include_str!("max_power.rs")),
            ("team.rs", include_str!("team.rs")),
        ] {
            let corte = fonte.find("#[cfg(test)]").unwrap_or(fonte.len());
            let producao: String = fonte[..corte]
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for proibido in ["block_on(", "tokio::runtime::", "block_in_place"] {
                assert!(
                    !producao.contains(proibido),
                    "{nome} nao pode usar `{proibido}` fora dos testes: o max-power ja roda dentro do runtime da CLI"
                );
            }
        }
    }

    #[test]
    fn routes_bug_keywords() {
        assert_eq!(
            detect_route("fix the login crash").0,
            "systematic-debugging"
        );
        assert_eq!(
            detect_route("there is a bug in auth").0,
            "systematic-debugging"
        );
        assert_eq!(
            detect_route("panic in thread main").0,
            "systematic-debugging"
        );
        assert_eq!(
            detect_route("regression on upload").0,
            "systematic-debugging"
        );
    }

    #[test]
    fn routes_feature_keywords() {
        assert_eq!(detect_route("add OAuth2 support").0, "brainstorm");
        assert_eq!(detect_route("implement rate limiting").0, "brainstorm");
        assert_eq!(detect_route("build a new dashboard").0, "brainstorm");
        assert_eq!(detect_route("create a plugin system").0, "brainstorm");
    }

    #[test]
    fn routes_refactor_keywords() {
        assert_eq!(
            detect_route("refactor the auth module").0,
            "refactor-module"
        );
        assert_eq!(
            detect_route("clean up the config code").0,
            "refactor-module"
        );
        assert_eq!(
            detect_route("extract a helper function").0,
            "refactor-module"
        );
        // "simplify the error types" would hit "error" → systematic-debugging first,
        // so we use a goal without that collision.
        assert_eq!(
            detect_route("simplify the config logic").0,
            "refactor-module"
        );
    }

    #[test]
    fn routes_test_keywords() {
        assert_eq!(
            detect_route("write tests for the signup flow").0,
            "tdd-loop"
        );
        // "improve test coverage" would hit "improve" → refactor-module first.
        assert_eq!(detect_route("increase test coverage").0, "tdd-loop");
        // "add integration spec" would hit "add" → brainstorm first.
        assert_eq!(detect_route("write integration tests").0, "tdd-loop");
    }

    #[test]
    fn routes_docs_keywords() {
        assert_eq!(detect_route("document the API").0, "generate-docs");
        assert_eq!(detect_route("update the README").0, "generate-docs");
        assert_eq!(detect_route("explain the auth flow").0, "generate-docs");
    }

    #[test]
    fn routes_review_keywords() {
        assert_eq!(detect_route("review the auth module").0, "code-review");
        assert_eq!(
            detect_route("security audit of the gateway").0,
            "code-review"
        );
        // "inspect the upload handler" would hit "spec" (substring of "inspect") →
        // tdd-loop first, so we use an unambiguous goal.
        assert_eq!(detect_route("run a security check").0, "code-review");
    }

    #[test]
    fn defaults_to_brainstorm_on_no_match() {
        let (route, kw) = detect_route("something completely unrelated");
        assert_eq!(route, "brainstorm");
        assert!(kw.is_none());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(detect_route("FIX THE BUG").0, "systematic-debugging");
        assert_eq!(detect_route("REFACTOR everything").0, "refactor-module");
    }

    #[test]
    fn matched_keyword_is_returned() {
        let (route, kw) = detect_route("fix the login crash");
        assert_eq!(route, "systematic-debugging");
        assert_eq!(kw, Some("fix"));
    }

    /// #1228: o exemplo do menu nomeia o binario em execucao, e o nome que
    /// sai no `cargo test` (e numa maquina so com `garraia`) e `garraia`.
    #[test]
    fn exemplo_do_menu_usa_o_binario_instalado() {
        let linha = linha_de_exemplo(&crate::binario::nome());
        assert!(linha.contains("garraia max-power --goal"), "{linha}");
        assert_eq!(
            linha_de_exemplo("garra"),
            "  Example: garra max-power --goal \"fix the login crash\" --mode new"
        );

        // Nenhum literal fixo com o alias pode voltar ao que o menu imprime.
        // `concat!` impede o teste de casar consigo mesmo.
        let src = include_str!("max_power.rs");
        let fim = src.find(concat!("#[cfg(", "test)]")).expect("testes");
        let producao = &src[..fim];
        for linha in producao
            .lines()
            .filter(|l| l.contains(concat!("print", "ln!")))
        {
            assert!(
                !linha.contains(concat!("garra ", "max-power")),
                "literal fixo com o alias: {linha}"
            );
        }
    }

    /// #1228: a ajuda do `max-power` descrevia a execucao como futura
    /// ("lands in GAR-495..GAR-501"), mas ela ja existe desde o PR #1218.
    #[test]
    fn ajuda_do_max_power_descreve_a_execucao_que_existe() {
        use clap::CommandFactory;
        let cmd = crate::Cli::command();
        let sub = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "max-power")
            .expect("subcomando max-power");
        let ajuda = format!(
            "{} {}",
            sub.get_about().map(|a| a.to_string()).unwrap_or_default(),
            sub.get_long_about()
                .map(|a| a.to_string())
                .unwrap_or_default()
        );
        assert!(!ajuda.contains("GAR-495"), "{ajuda}");
        assert!(!ajuda.contains("lands in"), "{ajuda}");
        assert!(ajuda.contains("provider-backed"), "{ajuda}");
        assert!(ajuda.contains("deterministic"), "{ajuda}");
    }
}
