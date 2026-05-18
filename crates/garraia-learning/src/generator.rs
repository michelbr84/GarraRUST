use garraia_common::{Error, Result};
use std::path::Path;

use crate::{LearningSkillFrontmatter, Skill, SkillScope, SkillSource};

// ──────────────────────────────────────────────
// Provider trait
// ──────────────────────────────────────────────

/// Synchronous LLM provider for skill drafting.
///
/// Implementations must be `Send + Sync` so they can be passed across threads.
/// In async contexts, bridge via `tokio::task::block_in_place`.
pub trait SkillDraftProvider: Send + Sync {
    fn draft(&self, prompt: &str) -> Result<String>;
}

// ──────────────────────────────────────────────
// Input / output types
// ──────────────────────────────────────────────

/// In-memory representation of a mined candidate, ready for generation.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// URL-safe kebab slug (e.g. `"merge-delete"`).
    pub slug: String,
    /// Normalized commands forming the detected pattern.
    pub normalized_commands: Vec<String>,
    /// Number of distinct sessions in which the pattern was seen.
    pub occurrence_count: usize,
}

impl Candidate {
    /// Construct a `Candidate` directly from a [`crate::miner::MinedPattern`].
    pub fn from_mined_pattern(pattern: &crate::miner::MinedPattern) -> Self {
        Candidate {
            slug: pattern.slug.clone(),
            normalized_commands: pattern.normalized_sequence.clone(),
            occurrence_count: pattern.occurrence_count,
        }
    }
}

/// Options for the generation step.
#[derive(Default)]
pub struct GenerateOptions {
    /// Skill names already in the active registry.
    /// Used to detect collisions and append `-v2`, `-v3`, … suffixes.
    pub existing_skill_names: Vec<String>,
}

// ──────────────────────────────────────────────
// Prompt template
// ──────────────────────────────────────────────

const SKILL_DRAFT_PROMPT: &str = r#"You are a DevOps automation assistant. Generate a reusable skill document in Markdown with YAML frontmatter based on the detected command pattern below.

## Detected pattern

Slug: {SLUG}
Seen in {COUNT} session(s).

Commands:
{COMMANDS}

## Required output format

Produce a document that begins with a YAML frontmatter block enclosed in `---` delimiters, followed by Markdown content. Example structure:

---
name: example-skill-name
version: "0.1.0"
description: "A brief one-sentence description."
source: mined
score: 0.5
---

## Overview

Brief explanation of what this skill automates.

## Steps

1. Step one.
2. Step two.

## Expected outcomes

What happens when the skill runs successfully.

Rules:
- `name` must be kebab-case and describe the automation.
- `description` must be one sentence, <= 120 characters.
- Do NOT include any email addresses, absolute file paths, or API tokens.
- Output ONLY the document -- no explanation or commentary outside it.
"#;

// ──────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────

/// Generate a polished skill document from a mined candidate.
///
/// 1. Builds a prompt from the candidate.
/// 2. Calls the provider.
/// 3. Parses the LLM response into a [`Skill`].
/// 4. Applies PII redaction to the body.
/// 5. Deduplicates the skill name against `opts.existing_skill_names`.
pub fn generate(
    candidate: &Candidate,
    provider: &dyn SkillDraftProvider,
    opts: &GenerateOptions,
) -> Result<Skill> {
    let prompt = build_prompt(candidate);
    let raw = provider.draft(&prompt)?;
    let mut skill = parse_llm_response(&raw, &candidate.slug)?;
    skill = apply_pii_redaction(skill);
    let unique_name = make_unique_name(&skill.frontmatter.name, &opts.existing_skill_names);
    skill.frontmatter.name = unique_name;
    Ok(skill)
}

/// Parse a `mined-<slug>-<hash>.md` file written by the Skill Miner into a `Candidate`.
pub fn load_candidate_file(path: &Path) -> Result<Candidate> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Other(format!("read {}: {e}", path.display())))?;

    let (fm_str, _body) = split_frontmatter(&content);

    // Try to extract the slug from the `name` field; fall back to the filename stem.
    let slug = if !fm_str.is_empty() {
        if let Ok(fm) = serde_yaml::from_str::<LearningSkillFrontmatter>(fm_str) {
            to_kebab(&fm.name)
        } else {
            stem_slug(path)
        }
    } else {
        stem_slug(path)
    };

    // Extract the commands list from the Markdown body (```...``` block).
    let commands = parse_commands_block(&content);

    Ok(Candidate {
        slug,
        normalized_commands: commands,
        occurrence_count: 1, // file-only parse; occurrence_count not recoverable
    })
}

// ──────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────

fn stem_slug(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mined")
        .to_string()
}

fn build_prompt(candidate: &Candidate) -> String {
    let cmds = candidate
        .normalized_commands
        .iter()
        .map(|c| format!("  {c}"))
        .collect::<Vec<_>>()
        .join("\n");

    SKILL_DRAFT_PROMPT
        .replace("{SLUG}", &candidate.slug)
        .replace("{COUNT}", &candidate.occurrence_count.to_string())
        .replace("{COMMANDS}", &cmds)
}

/// Parse the LLM response into a `Skill`.
///
/// If the response contains a valid YAML frontmatter block, parse it.
/// Otherwise fall back to constructing a minimal `Skill` from the fallback slug.
fn parse_llm_response(raw: &str, fallback_slug: &str) -> Result<Skill> {
    let (fm_str, body) = split_frontmatter(raw);

    if fm_str.is_empty() {
        tracing::warn!(
            fallback_slug,
            "LLM response missing frontmatter — using fallback"
        );
        return Ok(make_fallback_skill(fallback_slug, raw));
    }

    match serde_yaml::from_str::<serde_yaml::Value>(fm_str) {
        Ok(yaml) => {
            let name_raw = yaml["name"].as_str().unwrap_or(fallback_slug).to_string();
            let description = yaml["description"].as_str().unwrap_or("").to_string();

            let fm = LearningSkillFrontmatter {
                name: to_kebab(&name_raw),
                version: yaml["version"].as_str().unwrap_or("0.1.0").to_string(),
                source: SkillSource::Mined,
                scope: SkillScope::Project,
                score: yaml["score"].as_f64().unwrap_or(0.5) as f32,
                locked: false,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            };

            // Embed description in body if not already present.
            let body_text = if !description.is_empty() && !body.contains(&description) {
                format!("> {description}\n\n{body}")
            } else {
                body.to_string()
            };

            Ok(Skill {
                frontmatter: fm,
                body: body_text.trim().to_string(),
                source_path: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse LLM frontmatter — using fallback");
            Ok(make_fallback_skill(fallback_slug, raw))
        }
    }
}

fn make_fallback_skill(slug: &str, raw_body: &str) -> Skill {
    Skill {
        frontmatter: LearningSkillFrontmatter {
            name: to_kebab(slug),
            version: "0.1.0".to_string(),
            source: SkillSource::Mined,
            scope: SkillScope::Project,
            score: 0.5,
            locked: false,
            critical_paths_touched: vec![],
            fail_count: 0,
            deprecated: false,
        },
        body: raw_body.trim().to_string(),
        source_path: None,
    }
}

/// Apply PII redaction to the skill body using [`crate::miner::redact`].
fn apply_pii_redaction(mut skill: Skill) -> Skill {
    skill.body = crate::miner::redact(&skill.body);
    skill
}

/// Convert an arbitrary string to kebab-case.
///
/// Rules: lowercase, whitespace/punctuation → `-`, collapse multiple `-`,
/// trim trailing `-`. Empty result → `"generated"`.
pub fn to_kebab(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true; // treat start as if preceded by `-` to avoid leading dash

    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }

    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "generated".to_string()
    } else {
        out
    }
}

/// Make a skill name unique by appending `-v2`, `-v3`, … when `base` collides.
pub fn make_unique_name(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|n| n == base) {
        return base.to_string();
    }
    let mut version = 2usize;
    loop {
        let candidate = format!("{base}-v{version}");
        if !existing.iter().any(|n| n == &candidate) {
            return candidate;
        }
        version += 1;
    }
}

/// Split a document into `(frontmatter_str, body_str)`.
///
/// Looks for the pattern `---\n...\n---`. Returns `("", full_text)` if not found.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    if !text.starts_with("---") {
        return ("", text);
    }
    // Skip past the opening `---` line
    let after_open = match text.find('\n') {
        Some(pos) => &text[pos + 1..],
        None => return ("", text),
    };
    // Find closing `---`
    if let Some(close_pos) = after_open.find("\n---") {
        let fm = after_open[..close_pos].trim_end();
        let body_start = close_pos + 4; // past `\n---`
        let body = after_open[body_start..].trim_start_matches('\n');
        (fm, body)
    } else {
        ("", text)
    }
}

/// Extract lines from the first fenced code block (``` ... ```) in the document.
fn parse_commands_block(text: &str) -> Vec<String> {
    let mut in_block = false;
    let mut cmds = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block && !trimmed.is_empty() {
            cmds.push(trimmed.to_string());
        }
    }
    cmds
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── MockDraftProvider ─────────────────────

    struct MockDraftProvider {
        response: String,
    }

    impl MockDraftProvider {
        fn new(response: impl Into<String>) -> Self {
            MockDraftProvider {
                response: response.into(),
            }
        }
    }

    impl SkillDraftProvider for MockDraftProvider {
        fn draft(&self, _prompt: &str) -> Result<String> {
            Ok(self.response.clone())
        }
    }

    struct ErrorProvider;

    impl SkillDraftProvider for ErrorProvider {
        fn draft(&self, _prompt: &str) -> Result<String> {
            Err(Error::Other("provider unavailable".into()))
        }
    }

    // ── to_kebab ─────────────────────────────

    #[test]
    fn kebab_lowercases_and_replaces_spaces() {
        assert_eq!(
            to_kebab("Cleanup Merged Branches"),
            "cleanup-merged-branches"
        );
    }

    #[test]
    fn kebab_strips_punctuation() {
        assert_eq!(
            to_kebab("Cleanup Merged Branches!"),
            "cleanup-merged-branches"
        );
    }

    #[test]
    fn kebab_collapses_multiple_separators() {
        assert_eq!(to_kebab("foo--bar  baz"), "foo-bar-baz");
    }

    #[test]
    fn kebab_empty_returns_generated() {
        assert_eq!(to_kebab("!!!"), "generated");
    }

    #[test]
    fn kebab_already_kebab_unchanged() {
        assert_eq!(to_kebab("merge-delete"), "merge-delete");
    }

    // ── make_unique_name ─────────────────────

    #[test]
    fn unique_name_no_collision() {
        let existing = vec!["bar".to_string()];
        assert_eq!(make_unique_name("foo", &existing), "foo");
    }

    #[test]
    fn unique_name_first_collision() {
        let existing = vec!["foo".to_string()];
        assert_eq!(make_unique_name("foo", &existing), "foo-v2");
    }

    #[test]
    fn unique_name_multiple_collisions() {
        let existing = vec!["foo".to_string(), "foo-v2".to_string()];
        assert_eq!(make_unique_name("foo", &existing), "foo-v3");
    }

    #[test]
    fn unique_name_empty_existing() {
        assert_eq!(make_unique_name("foo", &[]), "foo");
    }

    // ── split_frontmatter ────────────────────

    #[test]
    fn split_finds_frontmatter() {
        let doc = "---\nname: foo\n---\n\nbody text";
        let (fm, body) = split_frontmatter(doc);
        assert_eq!(fm, "name: foo");
        assert_eq!(body, "body text");
    }

    #[test]
    fn split_no_frontmatter() {
        let doc = "just plain text";
        let (fm, body) = split_frontmatter(doc);
        assert_eq!(fm, "");
        assert_eq!(body, "just plain text");
    }

    // ── build_prompt ─────────────────────────

    #[test]
    fn prompt_contains_slug_and_count() {
        let c = Candidate {
            slug: "merge-delete".to_string(),
            normalized_commands: vec!["gh pr merge --squash".to_string()],
            occurrence_count: 5,
        };
        let prompt = build_prompt(&c);
        assert!(prompt.contains("merge-delete"), "prompt missing slug");
        assert!(prompt.contains('5'), "prompt missing occurrence count");
        assert!(prompt.contains("gh pr merge"), "prompt missing command");
    }

    // ── parse_llm_response ───────────────────

    const VALID_RESPONSE: &str = "---\nname: cleanup-merged-branches\nversion: \"0.1.0\"\ndescription: \"Squash-merge a PR and delete its remote branch.\"\nsource: mined\nscore: 0.5\n---\n\n## Overview\n\nThis skill squash-merges a pull request and removes the remote branch.\n\n## Steps\n\n1. Run `gh pr merge --squash --delete-branch`.\n2. Delete the remote tracking ref.\n";

    #[test]
    fn parse_valid_response() {
        let skill = parse_llm_response(VALID_RESPONSE, "fallback").unwrap();
        assert_eq!(skill.frontmatter.name, "cleanup-merged-branches");
        assert!((skill.frontmatter.score - 0.5).abs() < f32::EPSILON);
        assert!(skill.body.contains("squash-merges"));
    }

    #[test]
    fn parse_missing_frontmatter_falls_back_to_slug() {
        let raw = "No frontmatter here, just prose.";
        let skill = parse_llm_response(raw, "my-slug").unwrap();
        assert_eq!(skill.frontmatter.name, "my-slug");
        assert!(skill.body.contains("No frontmatter"));
    }

    #[test]
    fn parse_invalid_yaml_falls_back() {
        // Construct a document with `---` delimiters but invalid YAML inside.
        let bad = "---\nkey: [unclosed\n---\nbody";
        let skill = parse_llm_response(bad, "fallback-slug").unwrap();
        assert_eq!(skill.frontmatter.name, "fallback-slug");
    }

    // ── apply_pii_redaction ──────────────────

    #[test]
    fn pii_email_is_redacted() {
        let skill = Skill {
            frontmatter: LearningSkillFrontmatter {
                name: "test".to_string(),
                version: "0.1.0".to_string(),
                source: SkillSource::Mined,
                scope: SkillScope::Project,
                score: 0.5,
                locked: false,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            },
            body: "Contact developer@example.com for help.".to_string(),
            source_path: None,
        };
        let cleaned = apply_pii_redaction(skill);
        assert!(
            !cleaned.body.contains("developer@example.com"),
            "email not redacted: {}",
            cleaned.body
        );
        assert!(cleaned.body.contains("<EMAIL>"));
    }

    // ── generate ─────────────────────────────

    #[test]
    fn generate_returns_skill_with_correct_name() {
        let candidate = Candidate {
            slug: "merge-delete".to_string(),
            normalized_commands: vec![
                "gh pr merge --squash --delete-branch".to_string(),
                "git push origin --delete <BRANCH>".to_string(),
            ],
            occurrence_count: 3,
        };
        let provider = MockDraftProvider::new(VALID_RESPONSE);
        let opts = GenerateOptions::default();
        let skill = generate(&candidate, &provider, &opts).unwrap();
        assert_eq!(skill.frontmatter.name, "cleanup-merged-branches");
        assert_eq!(skill.frontmatter.source, SkillSource::Mined);
    }

    #[test]
    fn generate_deduplicates_name() {
        let candidate = Candidate {
            slug: "cleanup-merged-branches".to_string(),
            normalized_commands: vec!["gh pr merge --squash".to_string()],
            occurrence_count: 3,
        };
        let provider = MockDraftProvider::new(VALID_RESPONSE);
        let opts = GenerateOptions {
            existing_skill_names: vec!["cleanup-merged-branches".to_string()],
        };
        let skill = generate(&candidate, &provider, &opts).unwrap();
        assert_eq!(skill.frontmatter.name, "cleanup-merged-branches-v2");
    }

    #[test]
    fn generate_propagates_provider_error() {
        let candidate = Candidate {
            slug: "test".to_string(),
            normalized_commands: vec![],
            occurrence_count: 1,
        };
        let provider = ErrorProvider;
        let opts = GenerateOptions::default();
        assert!(generate(&candidate, &provider, &opts).is_err());
    }

    // ── load_candidate_file ──────────────────

    #[test]
    fn load_candidate_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mined-merge-delete-abcd1234.md");
        let content = "---\nname: mined-merge-delete\nversion: \"0.1.0\"\nsource: mined\nscope: project\nscore: 0.0\nlocked: false\ncritical_paths_touched: []\nfail_count: 0\n---\n\n## Commands\n\nDetected in 3 session(s).\n\n```\ngh pr merge --squash --delete-branch\ngit push origin --delete <BRANCH>\n```\n";
        std::fs::write(&path, content).unwrap();
        let candidate = load_candidate_file(&path).unwrap();
        assert_eq!(candidate.slug, "mined-merge-delete");
        assert_eq!(candidate.normalized_commands.len(), 2);
        assert!(candidate.normalized_commands[0].contains("gh pr merge"));
    }

    #[test]
    fn load_candidate_file_missing_returns_err() {
        let result = load_candidate_file(Path::new("/nonexistent/path/skill.md"));
        assert!(result.is_err());
    }
}
