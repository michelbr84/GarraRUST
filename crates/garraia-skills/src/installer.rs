use std::path::{Path, PathBuf};
use std::time::Duration;

use garraia_common::ssrf::{self, UrlPolicy};
use garraia_common::{Error, Result};

use crate::parser::{self, SkillDefinition};

/// Skills are markdown documents; a megabyte is already generous. Without a cap
/// a hostile or broken upstream can stream until the process is out of memory.
const SKILL_BODY_CAP_BYTES: usize = 1024 * 1024;

/// Long enough for a slow CDN, short enough that a hanging host does not pin a
/// request handler open.
const SKILL_FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// Fetch policy for remote skill install.
///
/// https-only and public-addresses-only. Before 2026-08-29 this call site used
/// a bare `reqwest::get(url)`: any scheme, redirects followed, no timeout, no
/// body cap, and no IP filtering — so `POST /api/skills/import` with
/// `{"url":"http://169.254.169.254/latest/meta-data/"}` made the gateway probe
/// the cloud instance-metadata service and write the response into the skills
/// directory. CodeQL flags the shape as `rust/request-forgery` (9.1, Critical).
///
/// Plaintext http is refused rather than merely discouraged: a skill is
/// executable-adjacent content, so an on-path attacker rewriting it in transit
/// is a code-execution problem, not a confidentiality one. Nothing legitimate
/// is lost — a plaintext *localhost* URL was already refused by the IP block.
fn skill_url_policy() -> UrlPolicy {
    UrlPolicy::https_public(
        SKILL_FETCH_TIMEOUT,
        concat!("GarraIA/", env!("CARGO_PKG_VERSION"), " skill-installer"),
    )
}

pub struct SkillInstaller {
    skills_dir: PathBuf,
}

impl SkillInstaller {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
        }
    }

    /// Install a skill from a URL. Downloads the content, parses and validates it,
    /// then writes it to the skills directory.
    ///
    /// The URL is vetted by the shared SSRF guard before any connection is
    /// opened — see [`skill_url_policy`]. The guard is applied here rather than
    /// in the HTTP handler so the CLI (`garraia skills install <url>`) is
    /// covered by the same check.
    pub async fn install_from_url(&self, url: &str) -> Result<SkillDefinition> {
        let vetted = ssrf::vet_url(url, &skill_url_policy())
            .map_err(|e| Error::Skill(format!("refusing to fetch skill: {e}")))?;
        let client = ssrf::pinned_client(&vetted, &skill_url_policy())
            .map_err(|e| Error::Skill(format!("refusing to fetch skill: {e}")))?;

        let response = client
            .get(vetted.url.clone())
            .send()
            .await
            .map_err(|e| Error::Skill(format!("failed to download skill from {url}: {e}")))?;

        if !response.status().is_success() {
            return Err(Error::Skill(format!(
                "failed to download skill from {url}: HTTP {}",
                response.status()
            )));
        }

        let bytes = ssrf::read_capped(response, SKILL_BODY_CAP_BYTES)
            .await
            .map_err(|e| Error::Skill(format!("failed to read response body: {e}")))?;
        let content = String::from_utf8(bytes)
            .map_err(|e| Error::Skill(format!("skill body is not valid UTF-8: {e}")))?;

        let skill = parser::parse_skill(&content)?;
        parser::validate_skill(&skill)?;

        self.write_skill(&skill.frontmatter.name, &content)?;

        let mut skill = skill;
        skill.source_path = Some(self.skill_path(&skill.frontmatter.name));
        Ok(skill)
    }

    /// Install a skill from a local file path. Reads, parses, validates, and copies
    /// to the skills directory.
    pub fn install_from_path(&self, path: &Path) -> Result<SkillDefinition> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Skill(format!("failed to read {}: {e}", path.display())))?;

        let skill = parser::parse_skill(&content)?;
        parser::validate_skill(&skill)?;

        self.write_skill(&skill.frontmatter.name, &content)?;

        let mut skill = skill;
        skill.source_path = Some(self.skill_path(&skill.frontmatter.name));
        Ok(skill)
    }

    /// Remove a skill by name. Returns true if the file existed and was removed.
    pub fn remove(&self, name: &str) -> Result<bool> {
        let path = self.skill_path(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn skill_path(&self, name: &str) -> PathBuf {
        self.skills_dir.join(format!("{name}.md"))
    }

    fn write_skill(&self, name: &str, content: &str) -> Result<()> {
        if !self.skills_dir.exists() {
            std::fs::create_dir_all(&self.skills_dir)?;
        }
        let path = self.skill_path(name);
        std::fs::write(&path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod ssrf_tests {
    use super::*;

    /// Regression for the SSRF that `POST /api/skills/import` exposed: the URL
    /// was handed straight to `reqwest::get`. Every case below is refused by
    /// `vet_url` before a socket is opened, so the test is offline and
    /// deterministic. Literal IPs only — a hostname would need DNS.
    #[tokio::test]
    async fn install_from_url_refuses_internal_and_non_https_targets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let installer = SkillInstaller::new(dir.path());

        for url in [
            // Cloud instance metadata — the payoff case.
            "http://169.254.169.254/latest/meta-data/",
            "https://169.254.169.254/latest/meta-data/",
            // Internal services on the box and on the LAN.
            "https://127.0.0.1/skill.md",
            "https://10.0.0.1/skill.md",
            "https://192.168.1.1/skill.md",
            "https://[::1]/skill.md",
            // The v4-mapped IPv6 bypass.
            "https://[::ffff:127.0.0.1]/skill.md",
            // Plaintext http: a skill is executable-adjacent content.
            "http://1.1.1.1/skill.md",
            // Non-HTTP schemes.
            "file:///etc/passwd",
            "gopher://1.1.1.1/_x",
            "not a url at all",
        ] {
            let err = installer
                .install_from_url(url)
                .await
                .expect_err("{url} must be refused");
            let msg = err.to_string();
            assert!(
                msg.contains("refusing to fetch skill"),
                "{url} failed for the wrong reason: {msg}"
            );
            assert!(
                std::fs::read_dir(dir.path())
                    .expect("read skills dir")
                    .next()
                    .is_none(),
                "{url} must not have written anything"
            );
        }
    }
}
