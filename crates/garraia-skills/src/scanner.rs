use garraia_common::Result;
use std::path::PathBuf;
use tracing;

use crate::parser::{self, SkillDefinition};

/// Profundidade maxima da varredura a partir do diretorio de skills.
///
/// A categoria `hardware/` (#1131) mora em `hardware/<slug>/SKILL.md`, que e
/// profundidade 2 — o teto de 3 deixa uma folga e, mais importante, impede
/// que uma arvore profunda (ou um link que alguem plantou ali) vire uma
/// varredura ilimitada no boot do gateway.
const PROFUNDIDADE_MAXIMA: usize = 3;

pub struct SkillScanner {
    skills_dir: PathBuf,
}

impl SkillScanner {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
        }
    }

    /// Discover all valid skill files in the skills directory.
    /// Invalid files are warned and skipped, following the PluginLoader::discover() pattern.
    ///
    /// A varredura desce em subdiretorios ate [`PROFUNDIDADE_MAXIMA`] porque a
    /// categoria de hardware (#1131) e empacotada como `hardware/<slug>/SKILL.md`
    /// — um diretorio por skill, que e o que permite versionar firmware de
    /// referencia e presets ao lado do manifesto. Skills soltos na raiz
    /// (`<slug>.md`) continuam funcionando exatamente como antes.
    ///
    /// Symlinks de diretorio sao ignorados: o diretorio de skills recebe
    /// conteudo instalado de fora (`garraia skill install <url>`), e seguir um
    /// link dali deixaria a varredura sair da arvore de skills.
    pub fn discover(&self) -> Result<Vec<SkillDefinition>> {
        let mut skills = Vec::new();
        if !self.skills_dir.exists() {
            return Ok(skills);
        }
        self.discover_em(&self.skills_dir, 1, &mut skills)?;
        // Ordem estavel: a varredura de diretorio nao tem ordem garantida, e o
        // catalogo de hardware e a CLI mostram esta lista ao usuario.
        skills.sort_by(|a, b| a.frontmatter.name.cmp(&b.frontmatter.name));
        Ok(skills)
    }

    fn discover_em(
        &self,
        dir: &std::path::Path,
        profundidade: usize,
        skills: &mut Vec<SkillDefinition>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            // `file_type()` da entrada nao segue o link — e assim que o
            // symlink de diretorio e reconhecido antes de ser percorrido.
            let tipo = entry.file_type()?;

            if tipo.is_dir() {
                if profundidade >= PROFUNDIDADE_MAXIMA {
                    tracing::debug!(
                        "skills: ignorando {} (profundidade maxima {PROFUNDIDADE_MAXIMA})",
                        path.display()
                    );
                    continue;
                }
                self.discover_em(&path, profundidade + 1, skills)?;
                continue;
            }

            if tipo.is_symlink() {
                tracing::debug!("skills: ignorando symlink {}", path.display());
                continue;
            }
            if !tipo.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str());
            if ext != Some("md") {
                continue;
            }

            match self.load_skill(&path) {
                Ok(skill) => {
                    tracing::info!("discovered skill: {}", skill.frontmatter.name);
                    skills.push(skill);
                }
                Err(e) => {
                    tracing::warn!("skipping invalid skill at {}: {}", path.display(), e);
                }
            }
        }
        Ok(())
    }

    fn load_skill(&self, path: &std::path::Path) -> Result<SkillDefinition> {
        let content = std::fs::read_to_string(path)?;
        let mut skill = parser::parse_skill(&content)?;
        parser::validate_skill(&skill)?;
        skill.source_path = Some(path.to_path_buf());
        Ok(skill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn empty_dir() {
        let dir = std::env::temp_dir().join("garraia_test_empty_skills");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let scanner = SkillScanner::new(&dir);
        let skills = scanner.discover().unwrap();
        assert!(skills.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonexistent_dir() {
        let scanner = SkillScanner::new("/tmp/garraia_nonexistent_skills_dir");
        let skills = scanner.discover().unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn discovers_valid_skill_file() {
        let dir = std::env::temp_dir().join("garraia_test_valid_skills");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("greet.md"),
            "---\nname: greet\ndescription: Greeting skill\n---\nSay hello to the user.",
        )
        .unwrap();

        let scanner = SkillScanner::new(&dir);
        let skills = scanner.discover().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].frontmatter.name, "greet");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_invalid_files() {
        let dir = std::env::temp_dir().join("garraia_test_skip_invalid");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Valid skill
        fs::write(
            dir.join("valid.md"),
            "---\nname: valid\ndescription: Valid skill\n---\nBody content.",
        )
        .unwrap();
        // Invalid skill (no frontmatter)
        fs::write(dir.join("invalid.md"), "Just plain markdown.").unwrap();
        // Non-md file (should be ignored)
        fs::write(dir.join("notes.txt"), "not a skill").unwrap();

        let scanner = SkillScanner::new(&dir);
        let skills = scanner.discover().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].frontmatter.name, "valid");

        let _ = fs::remove_dir_all(&dir);
    }

    /// A categoria `hardware/` (#1131) e um diretorio por skill — a varredura
    /// precisa descer ate `hardware/<slug>/SKILL.md`.
    #[test]
    fn descobre_skill_de_hardware_em_subdiretorio() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pacote = dir.path().join("hardware").join("mqtt");
        fs::create_dir_all(&pacote).expect("cria pacote");
        fs::write(
            pacote.join("SKILL.md"),
            "---\nname: mqtt\ndescription: Adapter MQTT\nkind: hardware-adapter\nprovides:\n  transport: mqtt\n  capabilities: [light]\n---\nCorpo.",
        )
        .expect("escreve");
        // Um skill solto na raiz continua sendo descoberto.
        fs::write(
            dir.path().join("greet.md"),
            "---\nname: greet\ndescription: Greeting skill\n---\nSay hello.",
        )
        .expect("escreve");

        let skills = SkillScanner::new(dir.path()).discover().expect("varre");
        let nomes: Vec<&str> = skills.iter().map(|s| s.frontmatter.name.as_str()).collect();
        assert_eq!(nomes, vec!["greet", "mqtt"], "ordem estavel por nome");
        let mqtt = skills
            .iter()
            .find(|s| s.frontmatter.name == "mqtt")
            .expect("mqtt");
        assert_eq!(
            mqtt.frontmatter.kind,
            crate::hardware::SkillKind::HardwareAdapter
        );
        assert!(
            mqtt.source_path
                .as_ref()
                .expect("path")
                .ends_with("SKILL.md")
        );
    }

    /// O diretorio de skills recebe conteudo instalado de fora; seguir um
    /// symlink dali deixaria a varredura sair da arvore de skills.
    #[cfg(unix)]
    #[test]
    fn ignora_symlink_de_diretorio_e_de_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fora = tempfile::tempdir().expect("tempdir fora");
        fs::write(
            fora.path().join("intruso.md"),
            "---\nname: intruso\ndescription: fora da arvore\n---\nCorpo.",
        )
        .expect("escreve");

        std::os::unix::fs::symlink(fora.path(), dir.path().join("link-dir")).expect("symlink dir");
        std::os::unix::fs::symlink(
            fora.path().join("intruso.md"),
            dir.path().join("link-arquivo.md"),
        )
        .expect("symlink arquivo");

        let skills = SkillScanner::new(dir.path()).discover().expect("varre");
        assert!(
            skills.is_empty(),
            "nenhum skill vem de symlink: {:?}",
            skills
                .iter()
                .map(|s| &s.frontmatter.name)
                .collect::<Vec<_>>()
        );
    }

    /// Profundidade maior que o teto nao e varrida — um `git clone` acidental
    /// dentro do diretorio de skills nao vira varredura ilimitada no boot.
    #[test]
    fn respeita_o_teto_de_profundidade() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fundo = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&fundo).expect("cria");
        fs::write(
            fundo.join("SKILL.md"),
            "---\nname: fundo\ndescription: fundo demais\n---\nCorpo.",
        )
        .expect("escreve");

        let skills = SkillScanner::new(dir.path()).discover().expect("varre");
        assert!(skills.is_empty(), "profundidade 4 nao e varrida");
    }
}
