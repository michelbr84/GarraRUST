//! A categoria `hardware/` dos skills (#1131) — o manifesto, e só o manifesto.
//!
//! O epic [#1124](https://github.com/michelbr84/GarraRUST/issues/1124) fechou a
//! arquitetura em camadas **Garra Core → `garraia-hardware` → Skill/Adapter →
//! Device**: o core não conhece fabricante nem protocolo, e cada integração
//! chega empacotada como skill. Este módulo é o degrau de baixo dessa camada —
//! ele **descreve** o que um skill de hardware declara, sem saber executar nada:
//!
//! - `kind: hardware-adapter` — o skill declara um transporte que o
//!   `garraia-hardware` já sabe falar (`mqtt`, `home_assistant`, `serial`,
//!   `gpio`) e as capabilities que ele expõe;
//! - `kind: hardware-preset` — o skill só mapeia entidade → capability e traz
//!   sinônimos pt/en, para o usuário dizer "acende a luz da sala" em vez de
//!   `light.sala_teto` + `power`.
//!
//! # Por que os tipos daqui não conhecem `RiskClass`
//!
//! `garraia-skills` é uma crate de dados: ela lê YAML e devolve struct. Quem
//! transforma o manifesto em decisão — e quem aplica a regra de que **um skill
//! só pode SUBIR o risco de uma capability, nunca baixar** — é
//! `garraia_hardware::skills`, que vive ao lado da tabela R0–R5 (#1129). A
//! separação é de propósito: um skill de comunidade é conteúdo não confiável,
//! e a classificação de risco não pode nascer do próprio conteúdo que ela
//! restringe. Aqui o risco é validado só como *forma* (`r0`..`r5`).

use garraia_common::{Error, Result};
use serde::Deserialize;

/// O teto de sinônimos por preset — um manifesto hostil não transforma a
/// resolução por linguagem natural num varredor linear gigante.
const MAX_SINONIMOS: usize = 32;

/// Comprimento máximo de um identificador de entidade/sinônimo. Entity ids
/// reais (`light.sala_teto`, `esp32-varanda`) cabem folgadamente.
const MAX_TEXTO: usize = 128;

/// Os riscos que um preset pode declarar, na forma textual do manifesto.
/// Espelha `garraia_hardware::RiskClass` — validado aqui só como forma.
const RISCOS_VALIDOS: [&str; 6] = ["r0", "r1", "r2", "r3", "r4", "r5"];

/// A categoria de um skill. `instruction` é o skill clássico (markdown que o
/// agente lê); as duas de hardware carregam um bloco [`HardwareProvides`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillKind {
    /// O skill de sempre: instruções em markdown, sem hardware.
    #[default]
    Instruction,
    /// Declara um transporte que o core já sabe falar + as capabilities dele.
    HardwareAdapter,
    /// Só mapeia entidade → capability, com sinônimos pt/en.
    HardwarePreset,
}

impl SkillKind {
    /// `true` para as duas categorias de hardware.
    pub fn e_hardware(self) -> bool {
        matches!(self, Self::HardwareAdapter | Self::HardwarePreset)
    }

    /// Forma canônica, a mesma que aparece no frontmatter.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Instruction => "instruction",
            Self::HardwareAdapter => "hardware-adapter",
            Self::HardwarePreset => "hardware-preset",
        }
    }
}

impl std::fmt::Display for SkillKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// O bloco `provides:` de um skill de hardware.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HardwareProvides {
    /// O transporte que este skill usa (`mqtt`, `home_assistant`, `serial`,
    /// `gpio`). A lista **fechada** de transportes que o core sabe ativar vive
    /// em `garraia_hardware::skills` — aqui o campo é validado só como forma,
    /// e um transporte desconhecido é carregado inerte, nunca ativado.
    pub transport: String,
    /// As capabilities que o transporte expõe (`light`, `climate`, `sensor`).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Os mapeamentos entidade → capability com sinônimos de descoberta.
    #[serde(default)]
    pub presets: Vec<PresetEntry>,
}

/// Um mapeamento entidade → capability, com os sinônimos que o usuário usa
/// para falar dele.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PresetEntry {
    /// A entidade no vocabulário do transporte (`light.sala_teto` no HA,
    /// `varanda-esp32` no serial). Vira id de registry sob o prefixo do
    /// adapter (`ha:`, `serial:`, …) — por isso `:` é recusado aqui.
    pub entity: String,
    /// A capability do dispositivo (`power`, `temperature`, `door_unlock`).
    pub capability: String,
    /// Risco declarado pelo skill, forma textual (`r0`..`r5`). Opcional: o
    /// risco base vem do adapter. Quando presente, só é honrado se for
    /// **maior** que o do adapter (o clamp mora em `garraia-hardware`).
    #[serde(default)]
    pub risk: Option<String>,
    /// Sinônimos pt/en para descoberta por linguagem natural
    /// ("luz da sala", "living room light").
    #[serde(default)]
    pub synonyms: Vec<String>,
}

/// Valida o par (`kind`, `provides`) de um frontmatter já desserializado.
///
/// As regras, todas fail-closed:
///
/// 1. `kind: instruction` **não** pode carregar `provides` — um skill que
///    declara hardware sem se declarar de hardware seria invisível para a
///    triagem por categoria;
/// 2. os dois kinds de hardware **exigem** `provides` com transporte;
/// 3. `hardware-adapter` precisa de pelo menos uma capability;
/// 4. `hardware-preset` precisa de pelo menos um preset;
/// 5. entidade, capability, transporte e sinônimos passam por charset e
///    comprimento — eles viram chave de registry, log e inventário do agente.
pub fn validate_hardware(kind: SkillKind, provides: Option<&HardwareProvides>) -> Result<()> {
    let provides = match (kind, provides) {
        (SkillKind::Instruction, None) => return Ok(()),
        (SkillKind::Instruction, Some(_)) => {
            return Err(Error::Skill(
                "'provides' exige kind 'hardware-adapter' ou 'hardware-preset'".into(),
            ));
        }
        (_, None) => {
            return Err(Error::Skill(format!(
                "kind '{kind}' exige o bloco 'provides'"
            )));
        }
        (_, Some(p)) => p,
    };

    validar_identificador("transport", &provides.transport)?;

    if kind == SkillKind::HardwareAdapter && provides.capabilities.is_empty() {
        return Err(Error::Skill(
            "kind 'hardware-adapter' exige ao menos uma capability em 'provides.capabilities'"
                .into(),
        ));
    }
    if kind == SkillKind::HardwarePreset && provides.presets.is_empty() {
        return Err(Error::Skill(
            "kind 'hardware-preset' exige ao menos uma entrada em 'provides.presets'".into(),
        ));
    }

    for cap in &provides.capabilities {
        validar_identificador("capability", cap)?;
    }

    for preset in &provides.presets {
        validar_texto_simples("entity", &preset.entity)?;
        validar_identificador("capability", &preset.capability)?;

        if let Some(risco) = &preset.risk {
            let normalizado = risco.trim().to_ascii_lowercase();
            if !RISCOS_VALIDOS.contains(&normalizado.as_str()) {
                return Err(Error::Skill(format!(
                    "risco '{risco}' invalido em '{}': use r0..r5",
                    preset.entity
                )));
            }
        }

        if preset.synonyms.len() > MAX_SINONIMOS {
            return Err(Error::Skill(format!(
                "preset '{}' tem {} sinonimos (maximo {MAX_SINONIMOS})",
                preset.entity,
                preset.synonyms.len()
            )));
        }
        for sinonimo in &preset.synonyms {
            validar_texto_simples("synonym", sinonimo)?;
        }
    }

    Ok(())
}

/// Identificador de vocabulário fechado: `^[a-z][a-z0-9_]*$`. É o formato de
/// `transport` e de nome de capability em todo o `garraia-hardware`.
fn validar_identificador(campo: &str, valor: &str) -> Result<()> {
    if valor.is_empty() {
        return Err(Error::Skill(format!("'{campo}' nao pode ser vazio")));
    }
    if valor.len() > MAX_TEXTO {
        return Err(Error::Skill(format!(
            "'{campo}' excede {MAX_TEXTO} caracteres"
        )));
    }
    let mut chars = valor.chars();
    let primeiro_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let resto_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !primeiro_ok || !resto_ok {
        return Err(Error::Skill(format!(
            "'{campo}' invalido: '{valor}' (use minusculas, digitos e _)"
        )));
    }
    Ok(())
}

/// Texto que vira chave/rótulo: sem controle, sem espaço em branco exótico e
/// sem `:` (o separador de namespace de id do registry, #1168).
fn validar_texto_simples(campo: &str, valor: &str) -> Result<()> {
    let limpo = valor.trim();
    if limpo.is_empty() {
        return Err(Error::Skill(format!("'{campo}' nao pode ser vazio")));
    }
    if limpo.len() > MAX_TEXTO {
        return Err(Error::Skill(format!(
            "'{campo}' excede {MAX_TEXTO} caracteres"
        )));
    }
    if limpo.chars().any(|c| c.is_control()) {
        return Err(Error::Skill(format!(
            "'{campo}' contem caractere de controle"
        )));
    }
    if campo == "entity" && limpo.contains(':') {
        return Err(Error::Skill(format!(
            "'entity' nao pode conter ':' (prefixo de transporte e do adapter): '{valor}'"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provides(transport: &str) -> HardwareProvides {
        HardwareProvides {
            transport: transport.to_string(),
            capabilities: vec!["light".into()],
            presets: vec![],
        }
    }

    fn preset(entity: &str, capability: &str) -> PresetEntry {
        PresetEntry {
            entity: entity.to_string(),
            capability: capability.to_string(),
            risk: None,
            synonyms: vec!["luz da sala".into(), "living room light".into()],
        }
    }

    #[test]
    fn instruction_sem_provides_e_o_caminho_comum() {
        assert!(validate_hardware(SkillKind::Instruction, None).is_ok());
        assert!(SkillKind::default() == SkillKind::Instruction);
        assert!(!SkillKind::Instruction.e_hardware());
        assert!(SkillKind::HardwareAdapter.e_hardware());
        assert!(SkillKind::HardwarePreset.e_hardware());
    }

    /// Um skill que declara hardware sem se declarar de hardware escaparia da
    /// triagem por categoria — recusado na entrada.
    #[test]
    fn instruction_com_provides_e_recusado() {
        let err = validate_hardware(SkillKind::Instruction, Some(&provides("mqtt")))
            .expect_err("instruction nao carrega provides");
        assert!(err.to_string().contains("provides"), "{err}");
    }

    #[test]
    fn hardware_sem_provides_e_recusado() {
        for kind in [SkillKind::HardwareAdapter, SkillKind::HardwarePreset] {
            let err = validate_hardware(kind, None).expect_err("hardware exige provides");
            assert!(err.to_string().contains("provides"), "{err}");
        }
    }

    #[test]
    fn adapter_exige_capabilities_e_preset_exige_presets() {
        let mut p = provides("mqtt");
        p.capabilities.clear();
        let err = validate_hardware(SkillKind::HardwareAdapter, Some(&p))
            .expect_err("adapter sem capability");
        assert!(err.to_string().contains("capability"), "{err}");

        let p = provides("home_assistant");
        let err =
            validate_hardware(SkillKind::HardwarePreset, Some(&p)).expect_err("preset sem presets");
        assert!(err.to_string().contains("presets"), "{err}");
    }

    #[test]
    fn manifesto_valido_passa() {
        let mut p = provides("home_assistant");
        p.presets = vec![preset("light.sala_teto", "power")];
        validate_hardware(SkillKind::HardwareAdapter, Some(&p)).expect("valido");
        validate_hardware(SkillKind::HardwarePreset, Some(&p)).expect("valido");
    }

    /// `entity` vira chave de registry sob o prefixo do adapter (`ha:`), e um
    /// `:` embutido deixaria um skill escrever numa chave de outro transporte.
    #[test]
    fn entity_com_dois_pontos_e_recusada() {
        let mut p = provides("home_assistant");
        p.presets = vec![preset("mqtt:vitima", "power")];
        let err = validate_hardware(SkillKind::HardwarePreset, Some(&p))
            .expect_err("entity com ':' e recusada");
        assert!(err.to_string().contains("':'"), "{err}");
    }

    #[test]
    fn transporte_e_capability_seguem_charset_fechado() {
        for ruim in ["Home Assistant", "mqtt!", "", "1mqtt", "home-assistant"] {
            let p = provides(ruim);
            assert!(
                validate_hardware(SkillKind::HardwareAdapter, Some(&p)).is_err(),
                "transporte '{ruim}' deveria ser recusado"
            );
        }
        let mut p = provides("mqtt");
        p.capabilities = vec!["Light Switch".into()];
        assert!(validate_hardware(SkillKind::HardwareAdapter, Some(&p)).is_err());
    }

    #[test]
    fn risco_fora_de_r0_r5_e_recusado() {
        let mut p = provides("mqtt");
        let mut entrada = preset("light.sala", "power");
        entrada.risk = Some("R9".into());
        p.presets = vec![entrada];
        let err = validate_hardware(SkillKind::HardwarePreset, Some(&p))
            .expect_err("risco invalido e recusado");
        assert!(err.to_string().contains("r0..r5"), "{err}");

        // A forma maiuscula do manifesto e aceita (normalizada).
        let mut entrada = preset("light.sala", "power");
        entrada.risk = Some("R3".into());
        p.presets = vec![entrada];
        validate_hardware(SkillKind::HardwarePreset, Some(&p)).expect("R3 e valido");
    }

    #[test]
    fn sinonimos_tem_teto_e_charset() {
        let mut p = provides("mqtt");
        let mut entrada = preset("light.sala", "power");
        entrada.synonyms = (0..MAX_SINONIMOS + 1).map(|i| format!("n{i}")).collect();
        p.presets = vec![entrada];
        let err =
            validate_hardware(SkillKind::HardwarePreset, Some(&p)).expect_err("teto de sinonimos");
        assert!(err.to_string().contains("sinonimos"), "{err}");

        let mut entrada = preset("light.sala", "power");
        entrada.synonyms = vec!["luz\u{0}da sala".into()];
        p.presets = vec![entrada];
        assert!(validate_hardware(SkillKind::HardwarePreset, Some(&p)).is_err());
    }

    #[test]
    fn kind_desserializa_do_frontmatter() {
        let k: SkillKind = serde_yaml::from_str("hardware-adapter").expect("kebab-case");
        assert_eq!(k, SkillKind::HardwareAdapter);
        let k: SkillKind = serde_yaml::from_str("hardware-preset").expect("kebab-case");
        assert_eq!(k, SkillKind::HardwarePreset);
        assert!(serde_yaml::from_str::<SkillKind>("hardware_adapter").is_err());
    }
}
