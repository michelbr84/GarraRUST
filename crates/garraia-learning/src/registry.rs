use crate::safety::{self, SafetyIntent};
use crate::{LearningSkillFrontmatter, Skill, SkillScope, SkillSource};
use garraia_common::{Error, Result};
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

/// Paths for both registry scopes.
#[derive(Debug, Clone)]
pub struct RegistryOptions {
    /// `~/.garra/skills/` — shared across projects.
    pub global_dir: PathBuf,
    /// `.garra/skills/` — versioned with the project.
    pub project_dir: PathBuf,
}

impl RegistryOptions {
    pub fn new(global_dir: PathBuf, project_dir: PathBuf) -> Self {
        Self {
            global_dir,
            project_dir,
        }
    }

    /// Build options using the real HOME dir + an explicit project root.
    pub fn default_with_cwd(project_root: &Path) -> Result<Self> {
        let home =
            std::env::var("HOME").map_err(|_| Error::Other("HOME env var not set".into()))?;
        Ok(Self {
            global_dir: PathBuf::from(home).join(".garra").join("skills"),
            project_dir: project_root.join(".garra").join("skills"),
        })
    }
}

// ──────────────────────────────────────────────
// Lock-file guard
// ──────────────────────────────────────────────

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn acquire_lock(locks_dir: &Path, name: &str) -> Result<LockGuard> {
    fs::create_dir_all(locks_dir)?;
    let lock_path = locks_dir.join(format!("{name}.lock"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Other(format!(
                    "skill '{name}' is locked by another process; try again shortly"
                ))
            } else {
                Error::Io(e)
            }
        })?;
    Ok(LockGuard(lock_path))
}

// ──────────────────────────────────────────────
// File helpers
// ──────────────────────────────────────────────

fn skill_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.md"))
}

fn read_skill_file(path: &Path) -> Result<Skill> {
    let raw = fs::read_to_string(path)?;

    // Expected format: `---\n<yaml>\n---\n<body>`
    let without_leading = raw.trim_start_matches("---\n");
    let Some(sep) = without_leading.find("\n---") else {
        // No closing delimiter — treat whole content as body with empty frontmatter.
        tracing::warn!(
            path = %path.display(),
            "skill file missing YAML frontmatter delimiters; using defaults"
        );
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        return Ok(Skill {
            frontmatter: minimal_frontmatter(name),
            body: raw,
            source_path: Some(path.to_path_buf()),
        });
    };

    let yaml_src = &without_leading[..sep];
    let body_start = sep + "\n---".len();
    let body = without_leading[body_start..]
        .trim_start_matches('\n')
        .to_string();

    let frontmatter: LearningSkillFrontmatter =
        serde_yaml::from_str(yaml_src).map_err(|e| Error::Other(e.to_string()))?;

    Ok(Skill {
        frontmatter,
        body,
        source_path: Some(path.to_path_buf()),
    })
}

fn write_skill_file(skill: &Skill, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let yaml =
        serde_yaml::to_string(&skill.frontmatter).map_err(|e| Error::Other(e.to_string()))?;
    let mut file = fs::File::create(path)?;
    writeln!(file, "---")?;
    file.write_all(yaml.as_bytes())?;
    writeln!(file, "---")?;
    if !skill.body.is_empty() {
        writeln!(file)?;
        file.write_all(skill.body.as_bytes())?;
        if !skill.body.ends_with('\n') {
            writeln!(file)?;
        }
    }
    Ok(())
}

fn minimal_frontmatter(name: String) -> LearningSkillFrontmatter {
    LearningSkillFrontmatter {
        name,
        version: "0.1.0".into(),
        source: SkillSource::Mined,
        scope: SkillScope::Project,
        score: 0.0,
        locked: false,
        critical_paths_touched: vec![],
        fail_count: 0,
        deprecated: false,
    }
}

/// Scan `dir` for `*.md` files, skipping entries whose names start with `_`.
fn list_skills_in_dir(dir: &Path) -> Result<Vec<Skill>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut skills = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let fname = entry.file_name();
        let name = fname.to_string_lossy();
        // Skip reserved directories/files (_locks/, _candidates/, _deprecated/, etc.)
        if name.starts_with('_') {
            continue;
        }
        if !name.ends_with(".md") {
            continue;
        }
        let path = entry.path();
        match read_skill_file(&path) {
            Ok(skill) => skills.push(skill),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping unreadable skill file");
            }
        }
    }
    Ok(skills)
}

fn scope_dir<'a>(opts: &'a RegistryOptions, scope: &SkillScope) -> &'a Path {
    match scope {
        SkillScope::Global => &opts.global_dir,
        SkillScope::Project => &opts.project_dir,
    }
}

// ──────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────

/// Lists skills from one or both scopes.
///
/// If `scope` is `None`, returns all skills from both global and project dirs.
pub fn list_skills(opts: &RegistryOptions, scope: Option<SkillScope>) -> Result<Vec<Skill>> {
    match scope {
        Some(s) => list_skills_in_dir(scope_dir(opts, &s)),
        None => {
            let mut all = list_skills_in_dir(&opts.global_dir)?;
            all.extend(list_skills_in_dir(&opts.project_dir)?);
            Ok(all)
        }
    }
}

/// Returns the first skill whose `frontmatter.name` matches `name`, in the
/// given scope (or both if `scope` is `None`).
pub fn get_skill(
    opts: &RegistryOptions,
    name: &str,
    scope: Option<SkillScope>,
) -> Result<Option<Skill>> {
    let skills = list_skills(opts, scope)?;
    Ok(skills.into_iter().find(|s| s.frontmatter.name == name))
}

/// Writes a skill to the appropriate scope directory after passing the
/// hard-wall Safety Gate. Acquires a lock-file first to prevent concurrent
/// corruption.
///
/// GAR-649: every promotion path must traverse `safety::gate_with_intent`.
/// Use [`promote_with_intent`] to provide approval labels (e.g.
/// `security-audit-passed`); this entrypoint applies the default intent.
///
/// Returns the path where the skill was written.
pub fn promote(skill: &Skill, opts: &RegistryOptions) -> Result<PathBuf> {
    promote_with_intent(skill, opts, &SafetyIntent::default())
}

/// Like [`promote`] but lets callers attach `SafetyIntent::labels` — in
/// particular `security-audit-passed` to waive the `CriticalPath` denial.
///
/// All other Safety Gate categories (dangerous commands, score threshold,
/// anti-flap, PII) remain hard walls regardless of labels.
pub fn promote_with_intent(
    skill: &Skill,
    opts: &RegistryOptions,
    intent: &SafetyIntent,
) -> Result<PathBuf> {
    safety::gate_with_intent(skill, intent).map_err(|denial| {
        tracing::warn!(
            skill = %skill.frontmatter.name,
            denial = %denial,
            "skill.safety_denial during promote"
        );
        Error::Other(format!(
            "safety denial for skill '{}': {denial}",
            skill.frontmatter.name
        ))
    })?;

    let dir = scope_dir(opts, &skill.frontmatter.scope);
    let locks_dir = dir.join("_locks");
    let _lock = acquire_lock(&locks_dir, &skill.frontmatter.name)?;
    let path = skill_path(dir, &skill.frontmatter.name);
    write_skill_file(skill, &path)?;
    Ok(path)
}

/// Marks the named skill as `deprecated = true` by rewriting its file.
///
/// Returns `true` if the skill was found and updated, `false` if not found.
pub fn deprecate(opts: &RegistryOptions, name: &str, scope: SkillScope) -> Result<bool> {
    let dir = scope_dir(opts, &scope);
    let path = skill_path(dir, name);
    if !path.exists() {
        return Ok(false);
    }
    let mut skill = read_skill_file(&path)?;
    if skill.frontmatter.deprecated {
        return Ok(true); // already deprecated, nothing to do
    }
    skill.frontmatter.deprecated = true;
    let locks_dir = dir.join("_locks");
    let _lock = acquire_lock(&locks_dir, name)?;
    write_skill_file(&skill, &path)?;
    Ok(true)
}

/// Toggles the `locked` flag on a skill file in-place.
///
/// `locked = true` prevents auto-update via the Updater; `false` re-enables it.
/// Returns `true` if the skill was found and updated, `false` if not found.
pub fn set_locked(opts: &RegistryOptions, name: &str, locked: bool) -> Result<bool> {
    for scope in [SkillScope::Project, SkillScope::Global] {
        let dir = scope_dir(opts, &scope);
        let path = skill_path(dir, name);
        if !path.exists() {
            continue;
        }
        let mut skill = read_skill_file(&path)?;
        if skill.frontmatter.locked == locked {
            return Ok(true);
        }
        skill.frontmatter.locked = locked;
        let locks_dir = dir.join("_locks");
        let _lock = acquire_lock(&locks_dir, name)?;
        write_skill_file(&skill, &path)?;
        return Ok(true);
    }
    Ok(false)
}

/// Lists all candidate skill files in `candidates_dir` (files matching `mined-*.md`).
///
/// Files that fail to parse are skipped with a warning.
pub fn list_candidates(candidates_dir: &Path) -> Result<Vec<crate::generator::Candidate>> {
    if !candidates_dir.exists() {
        return Ok(vec![]);
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(candidates_dir)? {
        let entry = entry?;
        let fname = entry.file_name();
        let name = fname.to_string_lossy();
        if !name.starts_with("mined-") || !name.ends_with(".md") {
            continue;
        }
        let path = entry.path();
        match crate::generator::load_candidate_file(&path) {
            Ok(c) => candidates.push(c),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "skipping unparseable candidate file"
                );
            }
        }
    }
    Ok(candidates)
}

/// Returns the global skills root: `~/.garra/skills/`.
pub fn global_skills_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::Other("HOME env not set".into()))?;
    Ok(PathBuf::from(home).join(".garra").join("skills"))
}

/// Returns the project skills root: `.garra/skills/` relative to CWD.
pub fn project_skills_dir() -> PathBuf {
    PathBuf::from(".garra").join("skills")
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_skill(name: &str, scope: SkillScope, body: &str) -> Skill {
        Skill {
            frontmatter: LearningSkillFrontmatter {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                source: SkillSource::Mined,
                scope,
                // Above MIN_PROMOTE_SCORE so the post-GAR-649 safety gate
                // does not refuse fixture-based promotions.
                score: 0.8,
                locked: false,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            },
            body: body.to_string(),
            source_path: None,
        }
    }

    fn opts_in(tmp: &TempDir) -> RegistryOptions {
        RegistryOptions::new(tmp.path().join("global"), tmp.path().join("project"))
    }

    // ── read/write round-trip ──────────────────

    #[test]
    fn round_trip_write_read_preserves_fields() {
        let tmp = TempDir::new().unwrap();
        let skill = make_skill(
            "test-skill",
            SkillScope::Project,
            "## Steps\n\nDo the thing.\n",
        );
        let path = tmp.path().join("test-skill.md");

        write_skill_file(&skill, &path).unwrap();
        let loaded = read_skill_file(&path).unwrap();

        assert_eq!(loaded.frontmatter.name, "test-skill");
        assert_eq!(loaded.frontmatter.version, "0.1.0");
        assert_eq!(loaded.frontmatter.score, 0.8);
        assert!(!loaded.frontmatter.deprecated);
        assert!(loaded.body.contains("Do the thing."));
    }

    #[test]
    fn round_trip_preserves_deprecated_flag() {
        let tmp = TempDir::new().unwrap();
        let mut skill = make_skill("old-skill", SkillScope::Global, "body");
        skill.frontmatter.deprecated = true;
        let path = tmp.path().join("old-skill.md");

        write_skill_file(&skill, &path).unwrap();
        let loaded = read_skill_file(&path).unwrap();

        assert!(loaded.frontmatter.deprecated);
    }

    #[test]
    fn read_file_missing_delimiters_uses_fallback() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bare.md");
        fs::write(&path, "No frontmatter here.").unwrap();

        let skill = read_skill_file(&path).unwrap();
        assert_eq!(skill.frontmatter.name, "bare");
    }

    // ── list_skills ───────────────────────────

    #[test]
    fn list_skills_empty_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let result = list_skills(&opts, Some(SkillScope::Project)).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_skills_returns_skill_in_dir() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("my-skill", SkillScope::Project, "body");
        promote(&skill, &opts).unwrap();

        let listed = list_skills(&opts, Some(SkillScope::Project)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].frontmatter.name, "my-skill");
    }

    #[test]
    fn list_skills_none_scope_merges_both() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let global_skill = make_skill("global-one", SkillScope::Global, "body");
        let project_skill = make_skill("project-one", SkillScope::Project, "body");
        promote(&global_skill, &opts).unwrap();
        promote(&project_skill, &opts).unwrap();

        let all = list_skills(&opts, None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_skills_skips_underscore_files() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        // promote a normal skill (creates dir)
        let skill = make_skill("real-skill", SkillScope::Project, "body");
        promote(&skill, &opts).unwrap();

        // manually create a _reserved file
        fs::write(opts.project_dir.join("_index.md"), "should be skipped").unwrap();

        let listed = list_skills(&opts, Some(SkillScope::Project)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].frontmatter.name, "real-skill");
    }

    // ── get_skill ─────────────────────────────

    #[test]
    fn get_skill_found() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("find-me", SkillScope::Project, "body");
        promote(&skill, &opts).unwrap();

        let found = get_skill(&opts, "find-me", Some(SkillScope::Project)).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().frontmatter.name, "find-me");
    }

    #[test]
    fn get_skill_not_found_returns_none() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let found = get_skill(&opts, "ghost", Some(SkillScope::Project)).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn get_skill_wrong_scope_returns_none() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("scoped-skill", SkillScope::Global, "body");
        promote(&skill, &opts).unwrap();

        // Looking in Project scope shouldn't find a Global skill
        let found = get_skill(&opts, "scoped-skill", Some(SkillScope::Project)).unwrap();
        assert!(found.is_none());
    }

    // ── promote ───────────────────────────────

    #[test]
    fn promote_creates_file_in_correct_dir() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("new-skill", SkillScope::Project, "## Do stuff\n");
        let path = promote(&skill, &opts).unwrap();

        assert!(path.exists());
        assert_eq!(path, opts.project_dir.join("new-skill.md"));
    }

    #[test]
    fn promote_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let mut skill = make_skill("idem-skill", SkillScope::Project, "v1");
        promote(&skill, &opts).unwrap();

        skill.body = "v2".into();
        promote(&skill, &opts).unwrap();

        let loaded = read_skill_file(&opts.project_dir.join("idem-skill.md")).unwrap();
        assert_eq!(loaded.body.trim(), "v2");
    }

    // ── deprecate ─────────────────────────────

    #[test]
    fn deprecate_sets_flag_on_existing_skill() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("old-skill", SkillScope::Project, "body");
        promote(&skill, &opts).unwrap();

        let updated = deprecate(&opts, "old-skill", SkillScope::Project).unwrap();
        assert!(updated);

        let loaded = read_skill_file(&opts.project_dir.join("old-skill.md")).unwrap();
        assert!(loaded.frontmatter.deprecated);
    }

    #[test]
    fn deprecate_unknown_skill_returns_false() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let updated = deprecate(&opts, "ghost", SkillScope::Project).unwrap();
        assert!(!updated);
    }

    #[test]
    fn deprecate_already_deprecated_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let mut skill = make_skill("dup-depr", SkillScope::Project, "body");
        skill.frontmatter.deprecated = true;
        promote(&skill, &opts).unwrap();

        let result = deprecate(&opts, "dup-depr", SkillScope::Project).unwrap();
        assert!(result);
    }

    // ── list_candidates ───────────────────────

    #[test]
    fn list_candidates_empty_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let result = list_candidates(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_candidates_parses_mined_files() {
        let tmp = TempDir::new().unwrap();

        // Write a fixture in the format emitted by miner::render_candidate.
        let candidate_path = tmp.path().join("mined-git-cleanup.md");
        fs::write(
            &candidate_path,
            "---\nname: mined-git-cleanup\nversion: '0.1.0'\nsource: mined\nscope: project\nscore: 0.0\nlocked: false\ncritical_paths_touched: []\nfail_count: 0\ndeprecated: false\n---\n\n## Commands\n\n```\ngit branch -d main\n```\n",
        )
        .unwrap();

        let candidates = list_candidates(tmp.path()).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].slug, "mined-git-cleanup");
    }

    #[test]
    fn list_candidates_skips_non_mined_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("other-file.md"), "not a candidate").unwrap();

        let result = list_candidates(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    // ── GAR-649 RED tests: promote MUST call safety::gate first ──────────

    #[test]
    fn promote_denied_when_skill_body_has_dangerous_command() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let skill = make_skill("evil-skill", SkillScope::Project, "## Cleanup\nrm -rf /\n");
        let err = promote(&skill, &opts).unwrap_err();
        assert!(
            err.to_string().contains("safety"),
            "promote should refuse and mention safety; got: {err}"
        );
        // And nothing must have been written.
        assert!(!opts.project_dir.join("evil-skill.md").exists());
    }

    #[test]
    fn promote_denied_when_skill_touches_critical_path_without_label() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let mut skill = make_skill("auth-skill", SkillScope::Project, "Innocent body");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        skill.frontmatter.score = 0.9;
        let err = promote(&skill, &opts).unwrap_err();
        assert!(err.to_string().contains("safety"));
        assert!(!opts.project_dir.join("auth-skill.md").exists());
    }

    #[test]
    fn promote_with_intent_allows_critical_path_when_audit_label_present() {
        let tmp = TempDir::new().unwrap();
        let opts = opts_in(&tmp);

        let mut skill = make_skill("audited-skill", SkillScope::Project, "Reviewed");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        skill.frontmatter.score = 0.9;

        let intent = crate::safety::SafetyIntent {
            labels: vec!["security-audit-passed".to_string()],
        };
        let path = promote_with_intent(&skill, &opts, &intent).unwrap();
        assert!(path.exists());
    }
}
