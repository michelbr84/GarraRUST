//! `garra skills` — pipeline de skills pela CLI (#1901).
//!
//! O minerador (`garraia_learning::miner`) e o registro
//! (`garraia_learning::registry`) ja existiam. A CLI agora expoe a mineracao
//! e a inspecao de candidatos/skills sem precisar iniciar o gateway.
//!
//! Caminhos: a raiz global vem de [`registry::global_skills_dir`]
//! (`$HOME/.garra/skills`, com `USERPROFILE` como fallback no Windows) e a de
//! projeto de `registry::project_skills_dir()`. O minerador grava candidatos na
//! raiz GLOBAL, entao a listagem de candidatos le os dois escopos pelo mesmo
//! helper do gateway (`registry::scan_candidates_scoped`) e rotula cada item
//! com o escopo onde ele esta (#1903).
//!
//! Este grupo e o pipeline de aprendizado; `garra skill` (singular) e o
//! catalogo operacional de skills instaladas para o agente — comandos
//! diferentes, mesmo substantivo.
//!
//! Interface: o criterio do ROADMAP pede `mine --from session-log.json`; o que
//! existe e `--sessions-dir <diretorio>` (varios `*.json` no formato
//! SessionRecord). Nao ha equivalencia: `--from` recebe UM arquivo e continua
//! pendente no checkbox do ROADMAP; `--sessions-dir` e a interface entregue.
//!
//! Codigos de saida (sysexits, iguais aos do `garra memory`):
//!
//! - `0`  — ok.
//! - `2`  — `EX_USAGE`: argumento invalido.
//! - `66` — `EX_NOINPUT`: o diretorio de sessoes nao existe, nao tem `*.json`
//!   ou nenhum arquivo tem sessao valida.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use garraia_learning::generator::Candidate;
use garraia_learning::miner::{self, MineOptions, MinedPattern};
use garraia_learning::registry::{self, CandidateScan, RegistryOptions};
use garraia_learning::{Skill, SkillScope, SkillSource};

/// Tudo certo.
const EXIT_OK: i32 = 0;
/// `EX_USAGE` — argumento invalido.
const EXIT_USAGE: i32 = 2;
/// `EX_NOINPUT` — o diretorio de sessoes nao existe ou nao tem sessao valida.
const EXIT_NOINPUT: i32 = 66;

/// Diretorios que o pipeline usa.
///
/// Injetavel para testes: nada aqui le `$HOME` por conta propria, e
/// [`Dirs::from_env`] e a unica porta que resolve o ambiente.
pub struct Dirs {
    /// `$HOME/.garra/sessions` — logs de sessao observados.
    pub sessions: PathBuf,
    /// `$HOME/.garra/skills/_candidates` — onde o minerador grava `mined-*.md`.
    pub candidates: PathBuf,
    /// `$HOME/.garra/skills` — skills globais.
    pub global: PathBuf,
    /// `./.garra/skills` — skills versionadas com o projeto.
    pub project: PathBuf,
    /// `./.garra/skills/_candidates` — candidatos do escopo de projeto.
    pub project_candidates: PathBuf,
}

impl Dirs {
    /// Resolve os diretorios do ambiente pelo registro compartilhado.
    pub fn from_env() -> Result<Self> {
        let global =
            registry::global_skills_dir().context("HOME/USERPROFILE — sem raiz de skills")?;
        Ok(Self::with_global(global))
    }

    fn with_global(global: PathBuf) -> Self {
        // O minerador documenta a arvore `<base>/sessions` + `<base>/skills/_candidates`
        // (`MineOptions::with_base`); a base e o pai do diretorio global de skills,
        // entao os dois lados nunca divergem de `$HOME/.garra`.
        let base = global
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| global.clone());
        let project = registry::project_skills_dir();
        Self {
            sessions: base.join("sessions"),
            candidates: global.join("_candidates"),
            global,
            project_candidates: project.join("_candidates"),
            project,
        }
    }

    #[cfg(test)]
    fn for_test(root: &Path) -> Self {
        let mut dirs = Self::with_global(root.join(".garra").join("skills"));
        dirs.project = root.join("project/.garra/skills");
        dirs.project_candidates = dirs.project.join("_candidates");
        dirs
    }
}

/// `garra skills mine` — minera os logs de sessao e grava candidatos.
///
/// `sessions_dir` sobrepoe o diretorio de sessoes (util para minerar um dump
/// fora de `$HOME`). Os candidatos vao SEMPRE para a raiz de skills resolvida
/// do ambiente, nunca para um diretorio derivado do override.
pub fn run_mine(sessions_dir: Option<&Path>, threshold: usize, json: bool) -> Result<i32> {
    let dirs = Dirs::from_env()?;
    run_mine_in(&dirs, sessions_dir, threshold, json)
}

fn run_mine_in(
    dirs: &Dirs,
    sessions_dir: Option<&Path>,
    threshold: usize,
    json: bool,
) -> Result<i32> {
    if threshold == 0 {
        eprintln!("--threshold precisa ser >= 1");
        return Ok(EXIT_USAGE);
    }

    let sessions = sessions_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs.sessions.clone());

    if !sessions.is_dir() {
        eprintln!(
            "sem diretorio de sessoes em {} — nenhuma sessao observada ainda; \
             passe --sessions-dir com logs JSON no formato SessionRecord",
            sessions.display()
        );
        return Ok(EXIT_NOINPUT);
    }

    // Arquivos e sessoes validas sao contagens diferentes (#1903): o
    // minerador ignora JSON malformado com um aviso de tracing que, sem
    // subscriber, ninguem ve. Aqui o aviso vai para o stderr e o numero de
    // sessoes que alimentaram a mineracao e o de verdade.
    let scan = miner::scan_sessions(&sessions).context("leitura dos logs de sessao falhou")?;
    if !scan.unparseable.is_empty() {
        eprintln!(
            "{} arquivo(s) de sessao ignorado(s) por JSON invalido em {}: {}",
            scan.unparseable.len(),
            sessions.display(),
            scan.unparseable.join(", ")
        );
    }
    if scan.files == 0 {
        eprintln!("nenhum arquivo de sessao JSON em {}", sessions.display());
        return Ok(EXIT_NOINPUT);
    }
    if scan.sessions.is_empty() {
        eprintln!(
            "nenhuma sessao valida em {} ({} arquivo(s) JSON, todos ilegiveis)",
            sessions.display(),
            scan.files
        );
        return Ok(EXIT_NOINPUT);
    }

    let before = registry::scan_candidates(&dirs.candidates)
        .context("leitura dos candidatos existentes falhou")?;
    let options = MineOptions {
        sessions_dir: sessions.clone(),
        candidates_dir: dirs.candidates.clone(),
        threshold,
    };
    let patterns = miner::mine(&options).context("mineracao de sessoes falhou")?;
    let after = registry::scan_candidates(&dirs.candidates)
        .context("leitura dos candidatos gravados falhou")?;
    // Idempotencia: o minerador nao reescreve candidato existente, entao o
    // numero de detectados pode ser maior que o de arquivos novos.
    let new_files = after.files.saturating_sub(before.files);
    let existing = patterns.len().saturating_sub(new_files);

    if json {
        let report = mine_report(
            dirs,
            &sessions,
            threshold,
            &scan,
            MineOutcome {
                detected: patterns.len(),
                new_files,
                existing,
            },
            &patterns,
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if patterns.is_empty() {
        println!(
            "nenhum padrao repetido em {} sessao(oes) valida(s) com limiar {threshold} \
             (nada gravado em {})",
            scan.sessions.len(),
            dirs.candidates.display()
        );
    } else {
        println!(
            "{} arquivo(s) de sessao, {} sessao(oes) valida(s) minerada(s) de {}",
            scan.files,
            scan.sessions.len(),
            sessions.display()
        );
        println!(
            "{} padrao(oes) detectado(s) — {new_files} candidato(s) novo(s), {existing} ja existente(s) — em {}:",
            patterns.len(),
            dirs.candidates.display()
        );
        for pattern in &patterns {
            println!(
                "- {} ({} sessoes): {}",
                miner::redact(&pattern.slug),
                pattern.occurrence_count,
                redacted_sequence(pattern).join(" -> ")
            );
        }
    }

    Ok(EXIT_OK)
}

/// O que a mineracao produziu, separando deteccao de gravacao.
struct MineOutcome {
    detected: usize,
    new_files: usize,
    existing: usize,
}

/// `garra skills candidates` — lista os candidatos minerados pendentes.
pub fn run_candidates(json: bool) -> Result<i32> {
    let dirs = Dirs::from_env()?;
    run_candidates_in(&dirs, json)
}

fn run_candidates_in(dirs: &Dirs, json: bool) -> Result<i32> {
    // Mesmo helper do gateway: escopo global (onde o minerador grava) e
    // escopo de projeto, cada um com o seu resultado (#1903).
    let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
    let scans = registry::scan_candidates_scoped(&opts).context("leitura dos candidatos falhou")?;

    let mut warnings = Vec::new();
    let mut total_files = 0usize;
    let mut all: Vec<(String, Candidate)> = Vec::new();
    for (scope, scan) in &scans {
        total_files += scan.files;
        let scope_name = scope_label(scope);
        if !scan.unparseable.is_empty() {
            warnings.push(format!(
                "{} arquivo(s) de candidato ignorado(s) no escopo {scope_name} ({}): {}",
                scan.unparseable.len(),
                candidate_dir_for(dirs, scope).display(),
                scan.unparseable.join(", ")
            ));
        }
        all.extend(
            scan.candidates
                .iter()
                .cloned()
                .map(|c| (scope_name.to_string(), c)),
        );
    }

    if json {
        let report = candidates_report(dirs, &scans);
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if all.is_empty() {
        println!(
            "nenhum candidato pendente (global: {}, projeto: {}) — minere com `garra skills mine`",
            dirs.candidates.display(),
            dirs.project_candidates.display()
        );
    } else {
        println!(
            "{} candidato(s) em {} arquivo(s) — global: {}, projeto: {}:",
            all.len(),
            total_files,
            dirs.candidates.display(),
            dirs.project_candidates.display()
        );
        for (scope, candidate) in &all {
            println!(
                "- [{scope}] {} ({} sessoes): {}",
                candidate.slug,
                candidate.occurrence_count,
                candidate.normalized_commands.join(" -> ")
            );
        }
    }
    for warning in warnings {
        eprintln!("{warning}");
    }

    Ok(EXIT_OK)
}

fn candidate_dir_for(dirs: &Dirs, scope: &SkillScope) -> PathBuf {
    match scope {
        SkillScope::Global => dirs.candidates.clone(),
        SkillScope::Project => dirs.project_candidates.clone(),
    }
}

/// `garra skills list` — lista as skills instaladas (global + projeto).
pub fn run_list(json: bool) -> Result<i32> {
    let dirs = Dirs::from_env()?;
    run_list_in(&dirs, json)
}

fn run_list_in(dirs: &Dirs, json: bool) -> Result<i32> {
    let options = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
    let skills = registry::list_skills(&options, None).context("leitura das skills falhou")?;

    if json {
        let report = list_report(dirs, &skills);
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if skills.is_empty() {
        println!(
            "nenhuma skill instalada (global: {}, projeto: {})",
            dirs.global.display(),
            dirs.project.display()
        );
    } else {
        println!(
            "{} skill(s) instalada(s) — global: {}, projeto: {}",
            skills.len(),
            dirs.global.display(),
            dirs.project.display()
        );
        for skill in &skills {
            let fm = &skill.frontmatter;
            println!(
                "- {} {} [{}/{}] score {:.2}{}{}",
                fm.name,
                fm.version,
                scope_label(&fm.scope),
                source_label(&fm.source),
                fm.score,
                if fm.locked { " locked" } else { "" },
                if fm.deprecated { " deprecated" } else { "" }
            );
            // O `scope` acima vem do frontmatter; o diretorio onde o arquivo
            // esta e o escopo de INSTALACAO. Quando divergem, o arquivo esta
            // num escopo e se declara em outro (#1903) — quem moveu o arquivo
            // a mao precisa ver isso.
            if let Some(installed) = installed_scope(skill, dirs)
                && installed != scope_label(&fm.scope)
            {
                println!(
                    "  (declarado {}, instalado em {})",
                    scope_label(&fm.scope),
                    installed
                );
            }
        }
    }

    Ok(EXIT_OK)
}

/// Escopo deduzido do diretorio do arquivo, nao do frontmatter.
fn installed_scope(skill: &Skill, dirs: &Dirs) -> Option<&'static str> {
    let path = skill.source_path.as_ref()?;
    if path.starts_with(&dirs.global) {
        Some("global")
    } else if path.starts_with(&dirs.project) {
        Some("project")
    } else {
        None
    }
}

// ──────────────────────────────────────────────
// Relatorios JSON
// ──────────────────────────────────────────────

fn mine_report(
    dirs: &Dirs,
    sessions: &Path,
    threshold: usize,
    scan: &miner::SessionScan,
    outcome: MineOutcome,
    patterns: &[MinedPattern],
) -> serde_json::Value {
    serde_json::json!({
        "sessions_dir": sessions.display().to_string(),
        "candidates_dir": dirs.candidates.display().to_string(),
        "threshold": threshold,
        // Nomes honestos: arquivos no diretorio != sessoes que o minerador
        // conseguiu ler (#1903).
        "session_files": scan.files,
        "sessions_valid": scan.sessions.len(),
        "sessions_skipped": scan.unparseable,
        "patterns_detected": outcome.detected,
        "candidates_new": outcome.new_files,
        "candidates_existing": outcome.existing,
        "patterns": patterns
            .iter()
            .map(|pattern| serde_json::json!({
                "slug": miner::redact(&pattern.slug),
                "occurrences": pattern.occurrence_count,
                "sequence": redacted_sequence(pattern),
            }))
            .collect::<Vec<_>>(),
    })
}

fn candidates_report(dirs: &Dirs, scans: &[(SkillScope, CandidateScan)]) -> serde_json::Value {
    let mut files = 0usize;
    let mut skipped: Vec<serde_json::Value> = Vec::new();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for (scope, scan) in scans {
        let scope_name = scope_label(scope);
        files += scan.files;
        for name in &scan.unparseable {
            skipped.push(serde_json::json!({ "scope": scope_name, "file": name }));
        }
        entries.extend(
            scan.candidates
                .iter()
                .map(|candidate| {
                    serde_json::json!({
                        "scope": scope_name,
                        "slug": candidate.slug,
                        "occurrences": candidate.occurrence_count,
                        "sequence": candidate.normalized_commands,
                    })
                })
                .collect::<Vec<_>>(),
        );
    }
    serde_json::json!({
        "global_candidates_dir": dirs.candidates.display().to_string(),
        "project_candidates_dir": dirs.project_candidates.display().to_string(),
        "files": files,
        "candidates": entries,
        "unparseable": skipped,
    })
}

fn list_report(dirs: &Dirs, skills: &[Skill]) -> serde_json::Value {
    serde_json::json!({
        "global_dir": dirs.global.display().to_string(),
        "project_dir": dirs.project.display().to_string(),
        "skills": skills
            .iter()
            .map(|skill| {
                let fm = &skill.frontmatter;
                serde_json::json!({
                    "name": fm.name,
                    "version": fm.version,
                    "scope": scope_label(&fm.scope),
                    // Escopo de instalacao (diretorio), distinto do declarado acima.
                    "installed_scope": installed_scope(skill, dirs),
                    "source": source_label(&fm.source),
                    "score": fm.score,
                    "locked": fm.locked,
                    "deprecated": fm.deprecated,
                    "path": skill.source_path.as_ref().map(|p| p.display().to_string()),
                })
            })
            .collect::<Vec<_>>(),
    })
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

fn redacted_sequence(pattern: &MinedPattern) -> Vec<String> {
    pattern
        .normalized_sequence
        .iter()
        .map(|cmd| miner::redact(cmd))
        .collect()
}

fn scope_label(scope: &SkillScope) -> &'static str {
    match scope {
        SkillScope::Project => "project",
        SkillScope::Global => "global",
    }
}

fn source_label(source: &SkillSource) -> &'static str {
    match source {
        SkillSource::Mined => "mined",
        SkillSource::Authored => "authored",
        SkillSource::Imported => "imported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn seed_session(dir: &Path, id: &str) {
        std::fs::create_dir_all(dir).expect("cria diretorio de sessoes");
        let body = serde_json::json!({
            "session_id": id,
            "intent": "preparar release",
            "task_family": "release",
            "commands": [
                {"cmd": "git status --porcelain", "exit_code": 0},
                {"cmd": "cargo test -p garraia --lib", "exit_code": 0},
            ],
            "created_at": "2026-11-15T00:00:00Z",
        });
        std::fs::write(dir.join(format!("{id}.json")), body.to_string()).expect("grava sessao");
    }

    fn candidate_files(dirs: &Dirs) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&dirs.candidates) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("mined-") && name.ends_with(".md"))
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn mine_grava_candidato_com_o_padrao_detectado() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");

        let code = run_mine_in(&dirs, None, 2, false).unwrap();

        assert_eq!(code, EXIT_OK);
        let files = candidate_files(&dirs);
        assert_eq!(
            files.len(),
            1,
            "um padrao repetido = um candidato: {files:?}"
        );
        let conteudo = std::fs::read_to_string(&files[0]).unwrap();
        assert!(
            conteudo.contains("## Commands"),
            "candidato sem corpo: {conteudo}"
        );
        assert!(conteudo.contains("git status --porcelain"));
        assert!(conteudo.contains("Detected in 2 session(s)"));
    }

    #[test]
    fn mine_e_idempotente_nao_reescreve_candidato_existente() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");

        run_mine_in(&dirs, None, 2, false).unwrap();
        let files = candidate_files(&dirs);
        assert_eq!(files.len(), 1);
        std::fs::write(&files[0], "SENTINELA").unwrap();

        let code = run_mine_in(&dirs, None, 2, false).unwrap();

        assert_eq!(code, EXIT_OK);
        assert_eq!(
            candidate_files(&dirs).len(),
            1,
            "nao pode duplicar o candidato"
        );
        assert_eq!(
            std::fs::read_to_string(&files[0]).unwrap(),
            "SENTINELA",
            "segunda mineracao nao pode reescrever o arquivo existente"
        );
    }

    #[test]
    fn mine_sem_diretorio_de_sessoes_sai_com_noinput() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());

        let code = run_mine_in(&dirs, None, 2, false).unwrap();

        assert_eq!(code, EXIT_NOINPUT);
        assert!(candidate_files(&dirs).is_empty());
    }

    #[test]
    fn mine_diretorio_vazio_sai_com_noinput() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        std::fs::create_dir_all(&dirs.sessions).unwrap();
        assert_eq!(run_mine_in(&dirs, None, 2, false).unwrap(), EXIT_NOINPUT);
        assert!(!dirs.candidates.exists());
    }

    #[test]
    fn relatorio_mine_redige_dados_pessoais_como_o_candidato() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        let email = ["fixture", "example.invalid"].join("@");
        let patterns = vec![MinedPattern {
            normalized_sequence: vec![
                format!("git -c user.email={email} status"),
                "cargo test".into(),
            ],
            occurrence_count: 2,
            slug: email.clone(),
            content_hash: "fixture".into(),
        }];
        let scan = miner::SessionScan::default();
        let report = mine_report(
            &dirs,
            &dirs.sessions,
            2,
            &scan,
            MineOutcome {
                detected: patterns.len(),
                new_files: patterns.len(),
                existing: 0,
            },
            &patterns,
        );
        assert!(!report.to_string().contains(&email));
    }

    #[test]
    fn mine_com_threshold_zero_sai_com_usage() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");

        let code = run_mine_in(&dirs, None, 0, false).unwrap();

        assert_eq!(code, EXIT_USAGE);
        assert!(candidate_files(&dirs).is_empty());
    }

    #[test]
    fn mine_respeita_sessions_dir_explicito() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        let externo = tmp.path().join("dump").join("sessoes");
        seed_session(&externo, "x");
        seed_session(&externo, "y");

        let code = run_mine_in(&dirs, Some(&externo), 2, false).unwrap();

        assert_eq!(code, EXIT_OK);
        // Candidatos sempre na raiz de skills do ambiente, nunca sob o override.
        assert_eq!(candidate_files(&dirs).len(), 1);
        assert!(!dirs.sessions.exists());
    }

    #[test]
    fn candidates_lista_o_que_o_minerador_gravou() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");
        run_mine_in(&dirs, None, 2, false).unwrap();

        let code = run_candidates_in(&dirs, false).unwrap();

        assert_eq!(code, EXIT_OK);
        let candidates = registry::list_candidates(&dirs.candidates).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].occurrence_count, 2);
    }

    #[test]
    fn candidates_vazio_e_ok() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());

        let code = run_candidates_in(&dirs, false).unwrap();

        assert_eq!(code, EXIT_OK);
    }

    #[test]
    fn list_sem_skills_e_ok() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());

        let code = run_list_in(&dirs, false).unwrap();

        assert_eq!(code, EXIT_OK);
    }

    #[test]
    fn list_e_candidates_json_cobrem_os_escopos_isolados() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        for (dir, name) in [
            (&dirs.global, "global-fixture"),
            (&dirs.project, "project-fixture"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{name}.md")), "fixture body").unwrap();
        }
        assert_eq!(run_list_in(&dirs, true).unwrap(), EXIT_OK);
        let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
        let skills = registry::list_skills(&opts, None).unwrap();
        let report = list_report(&dirs, &skills);
        let parsed: serde_json::Value = serde_json::from_str(&report.to_string()).unwrap();
        let names: Vec<_> = parsed["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"global-fixture"));
        assert!(names.contains(&"project-fixture"));
        assert_eq!(names.len(), 2);
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");
        assert_eq!(run_mine_in(&dirs, None, 2, true).unwrap(), EXIT_OK);
        assert_eq!(run_candidates_in(&dirs, true).unwrap(), EXIT_OK);
        let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
        let scans = registry::scan_candidates_scoped(&opts).unwrap();
        let report = candidates_report(&dirs, &scans);
        let parsed: serde_json::Value = serde_json::from_str(&report.to_string()).unwrap();
        assert_eq!(parsed["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["candidates"][0]["occurrences"], 2);
    }

    #[test]
    fn relatorio_json_do_mine_tem_o_contrato_esperado() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");

        let options = MineOptions {
            sessions_dir: dirs.sessions.clone(),
            candidates_dir: dirs.candidates.clone(),
            threshold: 2,
        };
        let scan = miner::scan_sessions(&dirs.sessions).unwrap();
        let patterns = miner::mine(&options).unwrap();
        let report = mine_report(
            &dirs,
            &dirs.sessions,
            2,
            &scan,
            MineOutcome {
                detected: patterns.len(),
                new_files: 1,
                existing: 0,
            },
            &patterns,
        );

        assert_eq!(report["threshold"], 2);
        // Nomes honestos: dois arquivos, duas sessoes validas (#1903).
        assert_eq!(report["session_files"], 2);
        assert_eq!(report["sessions_valid"], 2);
        assert_eq!(report["sessions_skipped"].as_array().unwrap().len(), 0);
        assert_eq!(report["patterns_detected"], 1);
        assert_eq!(report["candidates_new"], 1);
        assert_eq!(report["candidates_existing"], 0);
        let primeiro = &report["patterns"][0];
        assert_eq!(primeiro["occurrences"], 2);
        assert_eq!(primeiro["sequence"].as_array().unwrap().len(), 2);
        assert!(
            !primeiro["slug"].as_str().unwrap().is_empty(),
            "slug do padrao nao pode ser vazio"
        );
    }

    #[test]
    fn mine_conta_sessoes_validas_e_avisa_as_ilegiveis() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");
        std::fs::write(dirs.sessions.join("quebrada.json"), b"{ nao e json").unwrap();

        let scan = miner::scan_sessions(&dirs.sessions).unwrap();
        assert_eq!(scan.files, 3, "conta todo *.json no diretorio");
        assert_eq!(scan.sessions.len(), 2, "so as sessoes que parseiam");
        assert_eq!(scan.unparseable, vec!["quebrada.json".to_string()]);

        // A mineracao continua com as sessoes validas.
        assert_eq!(run_mine_in(&dirs, None, 2, false).unwrap(), EXIT_OK);
        assert_eq!(candidate_files(&dirs).len(), 1);
    }

    #[test]
    fn mine_com_todas_as_sessoes_ilegiveis_sai_com_noinput() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        std::fs::create_dir_all(&dirs.sessions).unwrap();
        std::fs::write(dirs.sessions.join("ruim.json"), b"[]").unwrap();

        let code = run_mine_in(&dirs, None, 2, false).unwrap();

        assert_eq!(code, EXIT_NOINPUT, "arquivo sem sessao valida nao e input");
        assert!(candidate_files(&dirs).is_empty());
    }

    #[test]
    fn candidates_le_os_dois_escopos_e_rotula_cada_um() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");
        run_mine_in(&dirs, None, 2, false).unwrap();
        // Candidato do escopo de projeto, escrito a mao (nada no minerador
        // grava la hoje, mas o gateway e a CLI precisam enxergar o escopo).
        std::fs::create_dir_all(&dirs.project_candidates).unwrap();
        std::fs::write(
            dirs.project_candidates.join("mined-projeto-abc12345.md"),
            "## Commands\n\n```\ncargo fmt\n```\n",
        )
        .unwrap();

        let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
        let scans = registry::scan_candidates_scoped(&opts).unwrap();
        let report = candidates_report(&dirs, &scans);
        let parsed: serde_json::Value = serde_json::from_str(&report.to_string()).unwrap();

        assert_eq!(parsed["files"], 2);
        let escopos: Vec<&str> = parsed["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["scope"].as_str().unwrap())
            .collect();
        assert!(
            escopos.contains(&"global"),
            "faltou o candidato global: {escopos:?}"
        );
        assert!(
            escopos.contains(&"project"),
            "faltou o candidato de projeto: {escopos:?}"
        );
        assert_eq!(
            escopos.len(),
            2,
            "escopos nao podem se misturar: {escopos:?}"
        );
    }

    #[test]
    fn candidates_reporta_arquivo_ilegivel_como_ignorado() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        std::fs::create_dir_all(&dirs.candidates).unwrap();
        // Bytes invalidos em UTF-8: o registro nao consegue ler o arquivo.
        std::fs::write(dirs.candidates.join("mined-quebrado.md"), [0xff, 0xfe]).unwrap();

        let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
        let scans = registry::scan_candidates_scoped(&opts).unwrap();
        let report = candidates_report(&dirs, &scans);
        let parsed: serde_json::Value = serde_json::from_str(&report.to_string()).unwrap();

        assert_eq!(parsed["files"], 1);
        assert_eq!(parsed["candidates"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["unparseable"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["unparseable"][0]["scope"], "global");
        assert_eq!(parsed["unparseable"][0]["file"], "mined-quebrado.md");
    }

    #[test]
    fn mine_reporta_candidato_existente_sem_contar_como_novo() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        seed_session(&dirs.sessions, "a");
        seed_session(&dirs.sessions, "b");
        run_mine_in(&dirs, None, 2, false).unwrap();

        let before = registry::scan_candidates(&dirs.candidates).unwrap();
        let options = MineOptions {
            sessions_dir: dirs.sessions.clone(),
            candidates_dir: dirs.candidates.clone(),
            threshold: 2,
        };
        let scan = miner::scan_sessions(&dirs.sessions).unwrap();
        let patterns = miner::mine(&options).unwrap();
        let after = registry::scan_candidates(&dirs.candidates).unwrap();
        let report = mine_report(
            &dirs,
            &dirs.sessions,
            2,
            &scan,
            MineOutcome {
                detected: patterns.len(),
                new_files: after.files.saturating_sub(before.files),
                existing: 1,
            },
            &patterns,
        );

        assert_eq!(report["patterns_detected"], 1);
        assert_eq!(
            report["candidates_new"], 0,
            "idempotencia: nada novo gravado"
        );
        assert_eq!(report["candidates_existing"], 1);
    }

    #[test]
    fn list_report_distingue_escopo_declarado_do_escopo_instalado() {
        let tmp = TempDir::new().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        std::fs::create_dir_all(&dirs.global).unwrap();
        // Arquivo instalado no escopo GLOBAL se declarando `project` no frontmatter.
        std::fs::write(
            dirs.global.join("moved-skill.md"),
            "---\nname: moved-skill\nversion: 0.1.0\nsource: mined\nscope: project\nscore: 0.8\n---\n\n## Steps\n\nbody\n",
        )
        .unwrap();

        let opts = RegistryOptions::new(dirs.global.clone(), dirs.project.clone());
        let skills = registry::list_skills(&opts, None).unwrap();
        let report = list_report(&dirs, &skills);
        let parsed: serde_json::Value = serde_json::from_str(&report.to_string()).unwrap();

        assert_eq!(parsed["skills"][0]["scope"], "project");
        assert_eq!(
            parsed["skills"][0]["installed_scope"], "global",
            "o diretorio e a prova do escopo de instalacao"
        );
    }
}
