//! `garra skills` — pipeline de skills pela CLI (#1901).
//!
//! O minerador (`garraia_learning::miner`) e o registro
//! (`garraia_learning::registry`) ja existiam. A CLI agora expoe a mineracao
//! e a inspecao de candidatos/skills sem precisar iniciar o gateway.
//!
//! Caminhos: todos derivam de [`registry::global_skills_dir`]
//! (`$HOME/.garra/skills`). A listagem de skills usa os mesmos dois escopos
//! do gateway; candidatos seguem o minerador global. O endpoint de candidatos
//! do gateway ainda consulta o escopo de projeto, nao o global.
//!
//! Codigos de saida (sysexits, iguais aos do `garra memory`):
//!
//! - `0`  — ok.
//! - `2`  — `EX_USAGE`: argumento invalido.
//! - `66` — `EX_NOINPUT`: o diretorio de sessoes nao existe.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use garraia_learning::generator::Candidate;
use garraia_learning::miner::{self, MineOptions, MinedPattern};
use garraia_learning::registry::{self, RegistryOptions};
use garraia_learning::{Skill, SkillScope, SkillSource};

/// Tudo certo.
const EXIT_OK: i32 = 0;
/// `EX_USAGE` — argumento invalido.
const EXIT_USAGE: i32 = 2;
/// `EX_NOINPUT` — o diretorio de sessoes nao existe.
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
}

impl Dirs {
    /// Resolve os diretorios do ambiente, na mesma convencao do gateway.
    pub fn from_env() -> Result<Self> {
        let global =
            registry::global_skills_dir().context("HOME nao definido — sem raiz de skills")?;
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
        Self {
            sessions: base.join("sessions"),
            candidates: global.join("_candidates"),
            global,
            project: registry::project_skills_dir(),
        }
    }

    #[cfg(test)]
    fn for_test(root: &Path) -> Self {
        let mut dirs = Self::with_global(root.join(".garra").join("skills"));
        dirs.project = root.join("project/.garra/skills");
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

    let session_count = count_sessions(&sessions)?;
    if session_count == 0 {
        eprintln!("nenhum arquivo de sessao JSON em {}", sessions.display());
        return Ok(EXIT_NOINPUT);
    }
    let options = MineOptions {
        sessions_dir: sessions.clone(),
        candidates_dir: dirs.candidates.clone(),
        threshold,
    };
    let patterns = miner::mine(&options).context("mineracao de sessoes falhou")?;

    if json {
        let report = mine_report(dirs, &sessions, threshold, session_count, &patterns);
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if patterns.is_empty() {
        println!(
            "nenhum padrao repetido em {session_count} sessao(oes) com limiar {threshold} \
             (nada gravado em {})",
            dirs.candidates.display()
        );
    } else {
        println!(
            "{session_count} sessao(oes) minerada(s) de {}",
            sessions.display()
        );
        println!(
            "{} padrao(oes) gravado(s) em {}:",
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

/// `garra skills candidates` — lista os candidatos minerados pendentes.
pub fn run_candidates(json: bool) -> Result<i32> {
    let dirs = Dirs::from_env()?;
    run_candidates_in(&dirs, json)
}

fn run_candidates_in(dirs: &Dirs, json: bool) -> Result<i32> {
    let candidates = registry::list_candidates(&dirs.candidates)
        .context("leitura dos candidatos minerados falhou")?;

    if json {
        let report = candidates_report(dirs, &candidates);
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if candidates.is_empty() {
        println!(
            "nenhum candidato pendente em {} — minere com `garra skills mine`",
            dirs.candidates.display()
        );
    } else {
        println!(
            "{} candidato(s) em {}:",
            candidates.len(),
            dirs.candidates.display()
        );
        for candidate in &candidates {
            println!(
                "- {} ({} sessoes): {}",
                candidate.slug,
                candidate.occurrence_count,
                candidate.normalized_commands.join(" -> ")
            );
        }
    }

    Ok(EXIT_OK)
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
        }
    }

    Ok(EXIT_OK)
}

// ──────────────────────────────────────────────
// Relatorios JSON
// ──────────────────────────────────────────────

fn mine_report(
    dirs: &Dirs,
    sessions: &Path,
    threshold: usize,
    session_count: usize,
    patterns: &[MinedPattern],
) -> serde_json::Value {
    serde_json::json!({
        "sessions_dir": sessions.display().to_string(),
        "candidates_dir": dirs.candidates.display().to_string(),
        "threshold": threshold,
        "sessions": session_count,
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

fn candidates_report(dirs: &Dirs, candidates: &[Candidate]) -> serde_json::Value {
    serde_json::json!({
        "candidates_dir": dirs.candidates.display().to_string(),
        "candidates": candidates
            .iter()
            .map(|candidate| serde_json::json!({
                "slug": candidate.slug,
                "occurrences": candidate.occurrence_count,
                "sequence": candidate.normalized_commands,
            }))
            .collect::<Vec<_>>(),
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

fn count_sessions(dir: &Path) -> Result<usize> {
    let mut total = 0;
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("nao consegui ler {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("erro de entrada em {}", dir.display()))?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("json") {
            total += 1;
        }
    }
    Ok(total)
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
        let report = mine_report(&dirs, &dirs.sessions, 2, 2, &patterns);
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
        let candidates = registry::list_candidates(&dirs.candidates).unwrap();
        let report = candidates_report(&dirs, &candidates);
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
        let patterns = miner::mine(&options).unwrap();
        let report = mine_report(&dirs, &dirs.sessions, 2, 2, &patterns);

        assert_eq!(report["threshold"], 2);
        assert_eq!(report["sessions"], 2);
        let primeiro = &report["patterns"][0];
        assert_eq!(primeiro["occurrences"], 2);
        assert_eq!(primeiro["sequence"].as_array().unwrap().len(), 2);
        assert!(
            !primeiro["slug"].as_str().unwrap().is_empty(),
            "slug do padrao nao pode ser vazio"
        );
    }
}
