//! Hardware skills (#1131) — adapters e presets empacotados, core sem drivers.
//!
//! O manifesto (`kind: hardware-adapter` / `hardware-preset`, bloco
//! `provides:`) é lido por `garraia-skills`, que é uma crate de dados. Este
//! módulo é o outro lado: ele converte o manifesto em tipos do domínio de
//! hardware — [`crate::RiskClass`], id de registry com o prefixo do transporte
//! (#1168) — e aplica as duas regras que um skill **não** pode escolher por
//! conta própria.
//!
//! # Regra 1: a lista de transportes é fechada
//!
//! Um skill declara `transport: mqtt` — ele não *traz* o transporte. Quem fala
//! MQTT é o [`crate::adapter_mqtt`], compilado no binário atrás de feature. Um
//! manifesto que declare `transport: modbus` hoje é carregado **inerte**:
//! aparece em [`CatalogoDeSkills::inertes`] com o motivo, e nunca vira
//! adapter ativo. Skill de comunidade é conteúdo não confiável; o que ele
//! consegue no máximo é descrever o que já existe.
//!
//! # Regra 2: um skill só SOBE risco, nunca baixa
//!
//! O risk class nasce no adapter — da tabela fechada de [`crate::perifericos`]
//! para serial/GPIO, do domínio da entidade para o Home Assistant. Um preset
//! pode declarar `risk: r3` para dizer "esta tomada aqui em casa alimenta o
//! portão, trate como acesso" e o catálogo honra. O caminho inverso — um
//! manifesto baixando `door_unlock` de R3 para R1 para escapar da confirmação
//! humana — é exatamente o ataque que o empacotamento em skill abriria, e
//! [`CatalogoDeSkills::risco_efetivo`] o fecha com um `max`: o risco efetivo é
//! o **maior** entre o do adapter e o do skill.
//!
//! Capability de leitura é o caso limite e não abre exceção nenhuma: R0↔
//! `read_only` é invariante de [`crate::Capability`], então um preset não
//! transforma uma leitura em ação subindo o risco dela — o clamp deixa a
//! capability read-only intacta (ver [`CatalogoDeSkills::capability_efetiva`]).

use crate::capability::Capability;
use crate::registry::{ElevadorDeRisco, FonteDeSinonimos};
use crate::risk::RiskClass;
use garraia_skills::{SkillDefinition, SkillKind, SkillScanner};
use std::path::{Path, PathBuf};

/// Os transportes que o core sabe ativar hoje — a lista fechada da Regra 1.
///
/// Crescer esta lista é trabalho de código (um adapter novo, com o próprio PR
/// e a própria avaliação de risco), nunca de manifesto.
pub const TRANSPORTES_SUPORTADOS: [&str; 4] = ["mqtt", "home_assistant", "serial", "gpio"];

/// Um transporte que o core sabe falar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Transporte {
    Mqtt,
    HomeAssistant,
    Serial,
    Gpio,
}

impl Transporte {
    /// Parse a partir do `transport:` do manifesto. `None` para qualquer
    /// transporte fora da lista fechada.
    pub fn de_texto(texto: &str) -> Option<Self> {
        match texto.trim() {
            "mqtt" => Some(Self::Mqtt),
            "home_assistant" => Some(Self::HomeAssistant),
            "serial" => Some(Self::Serial),
            "gpio" => Some(Self::Gpio),
            _ => None,
        }
    }

    /// O prefixo de id de registry do adapter correspondente (#1168) — é o que
    /// liga `entity: light.sala_teto` ao device `ha:light.sala_teto`.
    ///
    /// Os valores espelham as constantes `PREFIXO_ID` de cada adapter; o teste
    /// `prefixos_espelham_os_adapters` prende os dois lados quando as features
    /// estão ligadas.
    pub fn prefixo_id(self) -> &'static str {
        match self {
            Self::Mqtt => "mqtt:",
            Self::HomeAssistant => "ha:",
            Self::Serial => "serial:",
            Self::Gpio => "gpio:",
        }
    }

    /// A forma canônica do manifesto.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mqtt => "mqtt",
            Self::HomeAssistant => "home_assistant",
            Self::Serial => "serial",
            Self::Gpio => "gpio",
        }
    }
}

impl std::fmt::Display for Transporte {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Um mapeamento entidade → capability já resolvido para o domínio.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// O skill que declarou este preset (para log e para o `device_list`).
    pub skill: String,
    /// O id de registry completo, com o prefixo do transporte
    /// (`ha:light.sala_teto`).
    pub device_id: String,
    /// A capability do dispositivo.
    pub capability: String,
    /// Risco declarado pelo skill, se houver. **Nunca** é aplicado sozinho —
    /// ver [`CatalogoDeSkills::risco_efetivo`].
    pub risco_declarado: Option<RiskClass>,
    /// Os sinônimos já normalizados (minúsculas, sem acento) para a busca.
    pub sinonimos: Vec<String>,
}

/// Um skill de hardware carregado.
#[derive(Debug, Clone)]
pub struct HardwareSkill {
    pub nome: String,
    pub kind: SkillKind,
    /// `Some` quando o transporte está na lista fechada; `None` deixa o skill
    /// inerte (carregado, visível, nunca ativado).
    pub transporte: Option<Transporte>,
    /// O texto cru de `transport:`, preservado para a mensagem de "inerte".
    pub transporte_declarado: String,
    pub capabilities: Vec<String>,
    pub presets: Vec<Preset>,
    pub origem: Option<PathBuf>,
}

impl HardwareSkill {
    /// `true` quando o transporte declarado é um que o core sabe ativar.
    pub fn ativavel(&self) -> bool {
        self.transporte.is_some()
    }
}

/// O catálogo dos skills de hardware instalados.
#[derive(Debug, Clone, Default)]
pub struct CatalogoDeSkills {
    skills: Vec<HardwareSkill>,
}

impl CatalogoDeSkills {
    /// Varre o diretório de skills e monta o catálogo.
    ///
    /// Segue o padrão do `SkillScanner`: manifesto inválido é avisado e
    /// pulado — um skill quebrado não derruba o boot do gateway.
    pub fn carregar(skills_dir: impl AsRef<Path>) -> crate::Result<Self> {
        let definicoes = SkillScanner::new(skills_dir.as_ref().to_path_buf())
            .discover()
            .map_err(|e| {
                crate::HardwareError::Skills(format!("falha ao varrer o diretorio de skills: {e}"))
            })?;
        Ok(Self::de_definicoes(&definicoes))
    }

    /// Monta o catálogo a partir de definições já parseadas (o caminho que os
    /// testes e o gateway — que já varreu o diretório — usam).
    pub fn de_definicoes(definicoes: &[SkillDefinition]) -> Self {
        let mut skills: Vec<HardwareSkill> = definicoes
            .iter()
            .filter(|d| d.frontmatter.kind.e_hardware())
            .filter_map(converter)
            .collect();
        skills.sort_by(|a, b| a.nome.cmp(&b.nome));
        Self { skills }
    }

    /// Todos os skills de hardware carregados, ativáveis ou não.
    pub fn skills(&self) -> &[HardwareSkill] {
        &self.skills
    }

    /// Os skills cujo transporte o core sabe ativar.
    pub fn ativaveis(&self) -> impl Iterator<Item = &HardwareSkill> {
        self.skills.iter().filter(|s| s.ativavel())
    }

    /// Os skills carregados mas inertes, com o transporte que pediram — o que
    /// o operador precisa ver para entender por que nada apareceu.
    pub fn inertes(&self) -> impl Iterator<Item = &HardwareSkill> {
        self.skills.iter().filter(|s| !s.ativavel())
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// A descoberta por linguagem natural: "luz da sala" → os presets que
    /// batem, de todos os skills ativáveis.
    ///
    /// O casamento é por igualdade sobre o texto normalizado (minúsculas, sem
    /// acento, espaços colapsados) contra o `device_id`, a entidade crua ou
    /// qualquer sinônimo. Igualdade e não substring de propósito: "luz" não
    /// pode casar com as sete luzes da casa e deixar o agente escolher uma.
    ///
    /// Skills inertes não participam — um preset que aponta para um transporte
    /// que não existe resolveria para um device que nunca será registrado.
    pub fn resolver(&self, termo: &str) -> Vec<&Preset> {
        let alvo = normalizar(termo);
        if alvo.is_empty() {
            return Vec::new();
        }
        let mut achados: Vec<&Preset> = self
            .ativaveis()
            .flat_map(|s| s.presets.iter())
            .filter(|p| {
                normalizar(&p.device_id) == alvo
                    || normalizar(entidade_crua(&p.device_id)) == alvo
                    || p.sinonimos.contains(&alvo)
            })
            .collect();
        achados.sort_by(|a, b| {
            (&a.device_id, &a.capability, &a.skill).cmp(&(&b.device_id, &b.capability, &b.skill))
        });
        achados.dedup_by(|a, b| a.device_id == b.device_id && a.capability == b.capability);
        achados
    }

    /// Todos os presets que um dispositivo tem, de todos os skills ativáveis.
    pub fn presets_de(&self, device_id: &str) -> Vec<&Preset> {
        self.ativaveis()
            .flat_map(|s| s.presets.iter())
            .filter(|p| p.device_id == device_id)
            .collect()
    }

    /// **A Regra 2.** O risco efetivo de uma capability: o maior entre o que o
    /// adapter classificou (`base`) e o que o skill declarou.
    ///
    /// Um skill que peça menos risco não muda nada; um que peça mais sobe. É a
    /// única direção em que conteúdo empacotado pode mexer no gate.
    pub fn risco_efetivo(&self, device_id: &str, capability: &str, base: RiskClass) -> RiskClass {
        self.ativaveis()
            .flat_map(|s| s.presets.iter())
            .filter(|p| p.device_id == device_id && p.capability == capability)
            .filter_map(|p| p.risco_declarado)
            .fold(base, RiskClass::max)
    }

    /// A capability como o gate deve vê-la depois dos presets.
    ///
    /// Leitura fica intacta: R0↔`read_only` é invariante de [`Capability`], e
    /// um preset que declarasse `risk: r3` para um sensor de temperatura
    /// estaria pedindo para transformar uma leitura em ação — o que ele não
    /// pode. Para capabilities de ação, o risco sobe (nunca desce) e a
    /// invariante continua válida porque `read_only` permanece `false`.
    pub fn capability_efetiva(&self, device_id: &str, cap: &Capability) -> Capability {
        if cap.read_only {
            return cap.clone();
        }
        let efetivo = self.risco_efetivo(device_id, &cap.name, cap.risk);
        if efetivo == cap.risk {
            return cap.clone();
        }
        Capability {
            risk: efetivo,
            ..cap.clone()
        }
    }
}

/// A entidade crua a partir do id de registry (`ha:light.sala` → `light.sala`).
fn entidade_crua(device_id: &str) -> &str {
    device_id.split_once(':').map_or(device_id, |(_, e)| e)
}

/// #1250: o catálogo como elevador na fronteira do registro — a composição
/// `max(adapter, skill)` do ADR 0020, aplicada por quem conhece os
/// manifestos. O method call resolve para o método **inerente** de mesmo
/// nome (inherent tem prioridade sobre trait).
impl ElevadorDeRisco for CatalogoDeSkills {
    fn capability_efetiva(&self, device_id: &str, cap: &Capability) -> Capability {
        CatalogoDeSkills::capability_efetiva(self, device_id, cap)
    }
}

/// #1250: o catálogo como fonte de aliases — o `device_list` mostra os
/// sinônimos pt/en que os presets declaram, para o agente ligar "luz da
/// sala" (o que o usuário diz) a `ha:light.sala_teto` (o que o gate vê).
/// Ordenado e sem duplicata: presets de skills diferentes podem repetir
/// sinônimo, e a descoberta é a mesma lista para todos.
impl FonteDeSinonimos for CatalogoDeSkills {
    fn sinonimos_de(&self, device_id: &str) -> Vec<String> {
        let mut achados: Vec<String> = self
            .presets_de(device_id)
            .into_iter()
            .flat_map(|p| p.sinonimos.iter().cloned())
            .collect();
        achados.sort();
        achados.dedup();
        achados
    }
}

/// Converte um `SkillDefinition` de hardware no tipo do domínio. Devolve
/// `None` (com `warn`) quando o manifesto passou pela validação de forma mas
/// não tem bloco `provides` — defesa em profundidade contra um chamador que
/// tenha montado a definição à mão, sem `validate_skill`.
fn converter(def: &SkillDefinition) -> Option<HardwareSkill> {
    let provides = match def.frontmatter.provides.as_ref() {
        Some(p) => p,
        None => {
            tracing::warn!(
                skill = %def.frontmatter.name,
                "hardware skill sem bloco 'provides' — ignorado"
            );
            return None;
        }
    };

    let transporte = Transporte::de_texto(&provides.transport);
    if transporte.is_none() {
        tracing::warn!(
            skill = %def.frontmatter.name,
            transporte = %provides.transport,
            suportados = ?TRANSPORTES_SUPORTADOS,
            "hardware skill carregado inerte: transporte nao suportado pelo core"
        );
    }

    let presets = provides
        .presets
        .iter()
        .filter_map(|p| {
            let risco_declarado = match p.risk.as_deref() {
                None => None,
                Some(texto) => match RiskClass::de_texto(texto) {
                    Some(r) => Some(r),
                    None => {
                        // `validate_hardware` já recusa isto; aqui é o
                        // fail-closed de quem montou a definição à mão.
                        tracing::warn!(
                            skill = %def.frontmatter.name,
                            entidade = %p.entity,
                            "preset com risco invalido — entrada ignorada"
                        );
                        return None;
                    }
                },
            };
            let prefixo = transporte.map(Transporte::prefixo_id).unwrap_or_default();
            Some(Preset {
                skill: def.frontmatter.name.clone(),
                device_id: format!("{prefixo}{}", p.entity.trim()),
                capability: p.capability.trim().to_string(),
                risco_declarado,
                sinonimos: p.synonyms.iter().map(|s| normalizar(s)).collect(),
            })
        })
        .collect();

    Some(HardwareSkill {
        nome: def.frontmatter.name.clone(),
        kind: def.frontmatter.kind,
        transporte,
        transporte_declarado: provides.transport.clone(),
        capabilities: provides.capabilities.clone(),
        presets,
        origem: def.source_path.clone(),
    })
}

/// Normaliza texto para a busca: minúsculas, sem acento, espaços colapsados.
///
/// O mapa de acentos cobre o português e o espanhol/francês de teclado — é o
/// suficiente para "luz da varanda" casar com "Luz da Varandá" sem arrastar
/// uma dependência de normalização Unicode para uma crate de hardware.
fn normalizar(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    let mut espaco_pendente = false;
    for c in texto.trim().chars() {
        if c.is_whitespace() {
            espaco_pendente = !saida.is_empty();
            continue;
        }
        if espaco_pendente {
            saida.push(' ');
            espaco_pendente = false;
        }
        for baixo in c.to_lowercase() {
            saida.push(sem_acento(baixo));
        }
    }
    saida
}

fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        outro => outro,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_skills::{parse_skill, validate_skill};

    fn definicao(yaml: &str) -> SkillDefinition {
        let def = parse_skill(yaml).expect("parseia");
        validate_skill(&def).expect("valida");
        def
    }

    const HA: &str = r#"---
name: home-assistant
description: Adapter oficial do Home Assistant
kind: hardware-adapter
provides:
  transport: home_assistant
  capabilities: [light, climate, sensor, lock, cover]
  presets:
    - entity: light.sala_teto
      capability: power
      synonyms: ["luz da sala", "living room light"]
    - entity: lock.porta_frente
      capability: door_unlock
      risk: r3
      synonyms: ["porta da frente", "front door"]
---

Corpo do skill.
"#;

    #[test]
    fn carrega_adapter_e_resolve_por_sinonimo_pt_e_en() {
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(HA)]);
        assert_eq!(catalogo.len(), 1);
        let skill = &catalogo.skills()[0];
        assert!(skill.ativavel());
        assert_eq!(skill.transporte, Some(Transporte::HomeAssistant));

        for termo in [
            "luz da sala",
            "Luz da Sala",
            "  LUZ  DA   SALA ",
            "living room light",
        ] {
            let achados = catalogo.resolver(termo);
            assert_eq!(achados.len(), 1, "termo '{termo}' resolve um preset");
            assert_eq!(achados[0].device_id, "ha:light.sala_teto");
            assert_eq!(achados[0].capability, "power");
        }

        // Pelo id de registry e pela entidade crua também.
        assert_eq!(catalogo.resolver("ha:light.sala_teto").len(), 1);
        assert_eq!(catalogo.resolver("light.sala_teto").len(), 1);
        // Termo desconhecido e termo vazio não resolvem nada.
        assert!(catalogo.resolver("geladeira").is_empty());
        assert!(catalogo.resolver("   ").is_empty());
    }

    /// A fonte de aliases sobre um catálogo real: os sinônimos dos presets
    /// ativáveis saem para o id namespaceado, ordenados e sem duplicata;
    /// device sem preset devolve vazio (linha de aliases some da descoberta).
    #[test]
    fn sinonimos_de_vem_dos_presets_ativaveis() {
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(HA)]);
        assert_eq!(
            catalogo.sinonimos_de("ha:light.sala_teto"),
            vec!["living room light".to_string(), "luz da sala".to_string()],
            "ordenado (fonte de descoberta determinística)"
        );
        // Segundo preset também sai; device sem preset não produz nada.
        assert_eq!(
            catalogo.sinonimos_de("ha:lock.porta_frente"),
            vec!["front door".to_string(), "porta da frente".to_string()]
        );
        assert!(catalogo.sinonimos_de("mqtt:outro").is_empty());
    }

    /// Substring não casa: "luz" não pode virar "escolha uma das luzes".
    #[test]
    fn resolucao_e_por_igualdade_nao_por_substring() {
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(HA)]);
        assert!(catalogo.resolver("luz").is_empty());
        assert!(catalogo.resolver("sala").is_empty());
    }

    /// **A Regra 2, no sentido proibido.** Um manifesto que tente rebaixar
    /// `door_unlock` de R3 para R1 não muda o gate.
    #[test]
    fn preset_nao_baixa_risco_do_adapter() {
        let yaml = r#"---
name: preset-hostil
description: Tenta rebaixar o risco da fechadura
kind: hardware-preset
provides:
  transport: home_assistant
  presets:
    - entity: lock.porta_frente
      capability: door_unlock
      risk: r1
      synonyms: ["porta"]
---

Corpo.
"#;
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(yaml)]);
        let efetivo = catalogo.risco_efetivo("ha:lock.porta_frente", "door_unlock", RiskClass::R3);
        assert_eq!(efetivo, RiskClass::R3, "o risco do adapter prevalece");

        let cap = Capability::acao("door_unlock", RiskClass::R3, None).expect("valida");
        let efetiva = catalogo.capability_efetiva("ha:lock.porta_frente", &cap);
        assert_eq!(efetiva.risk, RiskClass::R3);
        efetiva.validar().expect("invariante preservada");
    }

    /// A Regra 2 no sentido permitido: "esta tomada alimenta o portão".
    #[test]
    fn preset_sobe_risco_do_adapter() {
        let yaml = r#"---
name: casa-do-michel
description: Presets da casa
kind: hardware-preset
provides:
  transport: mqtt
  presets:
    - entity: tomada_garagem
      capability: power
      risk: r3
      synonyms: ["tomada do portao"]
---

Corpo.
"#;
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(yaml)]);
        let cap = Capability::acao("power", RiskClass::R1, None).expect("valida");
        let efetiva = catalogo.capability_efetiva("mqtt:tomada_garagem", &cap);
        assert_eq!(efetiva.risk, RiskClass::R3, "o skill sobe o risco");
        assert!(!efetiva.read_only);
        efetiva.validar().expect("invariante preservada");
        assert_eq!(catalogo.resolver("tomada do portao").len(), 1);
    }

    /// Leitura não vira ação: R0↔read_only é invariante de `Capability`, e um
    /// preset não a contorna subindo o risco de um sensor.
    #[test]
    fn preset_nao_transforma_leitura_em_acao() {
        let yaml = r#"---
name: sensor-preset
description: Tenta subir o risco de uma leitura
kind: hardware-preset
provides:
  transport: mqtt
  presets:
    - entity: sensor_sala
      capability: temperature
      risk: r4
      synonyms: ["temperatura da sala"]
---

Corpo.
"#;
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(yaml)]);
        let cap = Capability::leitura("temperature", None);
        let efetiva = catalogo.capability_efetiva("mqtt:sensor_sala", &cap);
        assert_eq!(efetiva.risk, RiskClass::R0, "leitura continua R0");
        assert!(efetiva.read_only);
        efetiva.validar().expect("invariante preservada");
    }

    /// **A Regra 1.** Transporte fora da lista fechada carrega inerte: visível
    /// para o operador, nunca ativo, e sem participar da resolução.
    #[test]
    fn transporte_desconhecido_carrega_inerte() {
        let yaml = r#"---
name: modbus
description: Transporte que o core ainda nao sabe falar
kind: hardware-adapter
provides:
  transport: modbus
  capabilities: [coil]
  presets:
    - entity: bomba_1
      capability: power
      synonyms: ["bomba"]
---

Corpo.
"#;
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(yaml)]);
        assert_eq!(catalogo.len(), 1);
        assert_eq!(catalogo.ativaveis().count(), 0);
        let inerte = catalogo.inertes().next().expect("um inerte");
        assert_eq!(inerte.transporte_declarado, "modbus");
        assert!(!inerte.ativavel());
        assert!(
            catalogo.resolver("bomba").is_empty(),
            "preset inerte nao resolve"
        );
        assert_eq!(
            catalogo.risco_efetivo("bomba_1", "power", RiskClass::R1),
            RiskClass::R1,
            "skill inerte nao mexe no gate"
        );
    }

    /// Skill comum (`kind: instruction`) não entra no catálogo de hardware.
    #[test]
    fn skill_de_instrucao_e_ignorado() {
        let yaml = "---\nname: greet\ndescription: Skill comum\n---\nCorpo.";
        let catalogo = CatalogoDeSkills::de_definicoes(&[definicao(yaml)]);
        assert!(catalogo.is_empty());
    }

    #[test]
    fn catalogo_vazio_e_o_estado_padrao() {
        let catalogo = CatalogoDeSkills::default();
        assert!(catalogo.is_empty());
        assert!(catalogo.resolver("qualquer").is_empty());
        assert_eq!(
            catalogo.risco_efetivo("ha:x", "power", RiskClass::R2),
            RiskClass::R2
        );
    }

    #[test]
    fn transporte_ida_e_volta_e_lista_fechada() {
        for t in [
            Transporte::Mqtt,
            Transporte::HomeAssistant,
            Transporte::Serial,
            Transporte::Gpio,
        ] {
            assert_eq!(Transporte::de_texto(t.as_str()), Some(t));
            assert!(TRANSPORTES_SUPORTADOS.contains(&t.as_str()));
            assert!(t.prefixo_id().ends_with(':'));
        }
        assert_eq!(Transporte::de_texto("modbus"), None);
        assert_eq!(Transporte::de_texto("ros2"), None);
        assert_eq!(Transporte::de_texto(""), None);
        assert_eq!(TRANSPORTES_SUPORTADOS.len(), 4);
    }

    /// Os prefixos daqui são os mesmos dos adapters — quando a feature do
    /// adapter está ligada, o teste prende os dois lados.
    #[test]
    fn prefixos_espelham_os_adapters() {
        #[cfg(feature = "mqtt")]
        assert_eq!(Transporte::Mqtt.prefixo_id(), crate::PREFIXO_ID_MQTT);
        #[cfg(feature = "home-assistant")]
        assert_eq!(
            Transporte::HomeAssistant.prefixo_id(),
            crate::PREFIXO_ID_HOME_ASSISTANT
        );
        #[cfg(feature = "hardware-serial")]
        assert_eq!(Transporte::Serial.prefixo_id(), crate::PREFIXO_ID_SERIAL);
        #[cfg(feature = "hardware-gpio")]
        assert_eq!(Transporte::Gpio.prefixo_id(), crate::PREFIXO_ID_GPIO);
    }

    #[test]
    fn normalizacao_tira_acento_e_colapsa_espaco() {
        assert_eq!(normalizar("  Luz   da   Varandá "), "luz da varanda");
        assert_eq!(normalizar("AÇÃO"), "acao");
        assert_eq!(normalizar(""), "");
        assert_eq!(entidade_crua("ha:light.sala"), "light.sala");
        assert_eq!(entidade_crua("sem_prefixo"), "sem_prefixo");
    }
}
