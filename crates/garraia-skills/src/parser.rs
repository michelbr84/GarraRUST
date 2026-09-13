use garraia_common::{Error, Result};
use serde::Deserialize;
use std::path::PathBuf;

use crate::hardware::{HardwareProvides, SkillKind, validate_hardware};

#[derive(Debug, Clone, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// A categoria do skill (#1131). Ausente = `instruction`, que e o skill
    /// classico — todo SKILL.md escrito antes da categoria `hardware/` existir
    /// continua valido sem tocar numa linha.
    #[serde(default)]
    pub kind: SkillKind,
    /// O bloco de hardware, obrigatorio (e exclusivo) dos kinds de hardware.
    #[serde(default)]
    pub provides: Option<HardwareProvides>,
}

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub source_path: Option<PathBuf>,
}

/// Parse a SKILL.md file: YAML frontmatter between `---` delimiters, followed by markdown body.
pub fn parse_skill(content: &str) -> Result<SkillDefinition> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(Error::Skill(
            "missing frontmatter: file must start with ---".into(),
        ));
    }

    // Skip the opening `---` line
    let after_open = &trimmed[3..];
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    let close_pos = after_open
        .find("\n---")
        .ok_or_else(|| Error::Skill("missing closing --- for frontmatter".into()))?;

    let yaml_block = &after_open[..close_pos];
    let body_start = close_pos + 4; // skip \n---
    let body = if body_start < after_open.len() {
        after_open[body_start..].trim().to_string()
    } else {
        String::new()
    };

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_block)
        .map_err(|e| Error::Skill(format!("invalid frontmatter YAML: {e}")))?;

    Ok(SkillDefinition {
        frontmatter,
        body,
        source_path: None,
    })
}

/// Validate a parsed skill definition.
pub fn validate_skill(skill: &SkillDefinition) -> Result<()> {
    let name = &skill.frontmatter.name;
    if name.is_empty() {
        return Err(Error::Skill("skill name must not be empty".into()));
    }
    // Name must be alphanumeric + hyphens
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err(Error::Skill(format!(
            "skill name '{}' contains invalid characters (only alphanumeric and hyphens allowed)",
            name
        )));
    }
    if skill.frontmatter.description.is_empty() {
        return Err(Error::Skill("skill description must not be empty".into()));
    }
    if skill.body.is_empty() {
        return Err(Error::Skill("skill body must not be empty".into()));
    }
    // A categoria de hardware (#1131) traz regras proprias — um manifesto de
    // adapter sem transporte, ou um skill comum declarando hardware por
    // acidente, para aqui e nao chega ao catalogo.
    validate_hardware(skill.frontmatter.kind, skill.frontmatter.provides.as_ref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SKILL: &str = r#"---
name: test-skill
description: A test skill
triggers:
  - hello
  - greet
dependencies:
  - other-skill
---

# Test Skill

This is the skill body with instructions.
"#;

    #[test]
    fn parse_valid_skill() {
        let skill = parse_skill(VALID_SKILL).unwrap();
        assert_eq!(skill.frontmatter.name, "test-skill");
        assert_eq!(skill.frontmatter.description, "A test skill");
        assert_eq!(skill.frontmatter.triggers, vec!["hello", "greet"]);
        assert_eq!(skill.frontmatter.dependencies, vec!["other-skill"]);
        assert!(skill.body.contains("# Test Skill"));
        assert!(skill.body.contains("This is the skill body"));
        validate_skill(&skill).unwrap();
    }

    #[test]
    fn missing_frontmatter() {
        let result = parse_skill("# Just a markdown file\nNo frontmatter here.");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing frontmatter"));
    }

    #[test]
    fn missing_name() {
        let content = "---\ndescription: test\n---\nBody here.";
        let result = parse_skill(content);
        // serde_yaml will error on missing required field
        assert!(result.is_err());
    }

    #[test]
    fn invalid_name_chars() {
        let content = "---\nname: bad name!\ndescription: test\n---\nBody here.";
        let skill = parse_skill(content).unwrap();
        let result = validate_skill(&skill);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid characters"));
    }

    #[test]
    fn empty_body() {
        let content = "---\nname: test\ndescription: test\n---\n";
        let skill = parse_skill(content).unwrap();
        let result = validate_skill(&skill);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("body must not be empty"));
    }

    const HARDWARE_SKILL: &str = r#"---
name: home-assistant
description: Adapter oficial do Home Assistant
kind: hardware-adapter
provides:
  transport: home_assistant
  capabilities: [light, climate, sensor]
  presets:
    - entity: light.sala_teto
      capability: power
      risk: r1
      synonyms: ["luz da sala", "living room light"]
---

# Home Assistant

Corpo do skill.
"#;

    #[test]
    fn parse_hardware_skill() {
        let skill = parse_skill(HARDWARE_SKILL).expect("parseia");
        validate_skill(&skill).expect("valida");
        assert_eq!(skill.frontmatter.kind, SkillKind::HardwareAdapter);
        let provides = skill.frontmatter.provides.as_ref().expect("bloco provides");
        assert_eq!(provides.transport, "home_assistant");
        assert_eq!(provides.capabilities, vec!["light", "climate", "sensor"]);
        assert_eq!(provides.presets.len(), 1);
        assert_eq!(provides.presets[0].entity, "light.sala_teto");
        assert_eq!(provides.presets[0].risk.as_deref(), Some("r1"));
    }

    /// O frontmatter de antes da #1131 continua valido — `kind` ausente e
    /// `instruction`, e nenhum SKILL.md instalado precisa ser reescrito.
    #[test]
    fn skill_sem_kind_e_instruction() {
        let skill = parse_skill(VALID_SKILL).expect("parseia");
        assert_eq!(skill.frontmatter.kind, SkillKind::Instruction);
        assert!(skill.frontmatter.provides.is_none());
        validate_skill(&skill).expect("valida");
    }

    /// A validacao de hardware corre dentro de `validate_skill`, que e o unico
    /// portao que o scanner, o installer e o gateway atravessam.
    #[test]
    fn validate_skill_reprova_manifesto_de_hardware_incoerente() {
        let content =
            "---\nname: quebrado\ndescription: sem provides\nkind: hardware-adapter\n---\nCorpo.";
        let skill = parse_skill(content).expect("parseia");
        let err = validate_skill(&skill).expect_err("adapter sem provides");
        assert!(err.to_string().contains("provides"), "{err}");
    }
}
