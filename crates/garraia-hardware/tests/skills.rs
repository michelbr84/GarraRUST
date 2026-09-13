//! Os skills de hardware **versionados no repo** (#1131) carregam, são todos
//! ativáveis e não escondem nenhuma tentativa de rebaixar risco.
//!
//! O teste lê `skills/hardware/` de verdade: é o gate que impede um manifesto
//! oficial de entrar quebrado (transporte com typo, entidade com `:`, risco
//! fora de r0..r5) — e o que prende as duas regras do catálogo ao conteúdo
//! que o projeto distribui.
#![cfg(feature = "skills")]

use garraia_hardware::skills::{CatalogoDeSkills, Transporte};
use garraia_hardware::{Capability, RiskClass};
use std::path::PathBuf;

fn dir_dos_skills() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skills")
        .join("hardware")
}

fn catalogo() -> CatalogoDeSkills {
    CatalogoDeSkills::carregar(dir_dos_skills()).expect("carrega o diretorio de skills do repo")
}

#[test]
fn os_skills_oficiais_carregam_e_sao_todos_ativaveis() {
    let catalogo = catalogo();
    let mut nomes: Vec<&str> = catalogo.skills().iter().map(|s| s.nome.as_str()).collect();
    nomes.sort_unstable();
    assert_eq!(
        nomes,
        vec![
            "esp32",
            "home-assistant",
            "matter",
            "mqtt",
            "serial-arduino",
            "zigbee"
        ],
        "os seis skills oficiais do #1131"
    );

    assert_eq!(
        catalogo.inertes().count(),
        0,
        "nenhum skill oficial declara transporte fora da lista fechada: {:?}",
        catalogo
            .inertes()
            .map(|s| (&s.nome, &s.transporte_declarado))
            .collect::<Vec<_>>()
    );
}

/// Todo preset oficial vira id de registry namespaceado (#1168) — é o que
/// liga `entity:` ao dispositivo que o adapter registra.
#[test]
fn todo_preset_oficial_tem_id_namespaceado_pelo_transporte() {
    let catalogo = catalogo();
    let mut presets = 0;
    for skill in catalogo.skills() {
        let transporte = skill.transporte.expect("skill oficial e ativavel");
        for preset in &skill.presets {
            presets += 1;
            assert!(
                preset.device_id.starts_with(transporte.prefixo_id()),
                "preset '{}' do skill '{}' sem o prefixo '{}'",
                preset.device_id,
                skill.nome,
                transporte.prefixo_id()
            );
            assert!(
                !preset.capability.is_empty(),
                "preset '{}' sem capability",
                preset.device_id
            );
        }
    }
    assert!(
        presets >= 12,
        "os skills oficiais trazem presets: {presets}"
    );
}

/// O vocabulário pt/en é o ponto do empacotamento: o usuário fala como fala.
#[test]
fn resolve_vocabulario_em_portugues_e_ingles() {
    let catalogo = catalogo();

    let achados = catalogo.resolver("luz da sala");
    assert_eq!(achados.len(), 1, "'luz da sala' resolve um preset");
    assert_eq!(achados[0].device_id, "ha:light.sala_teto");
    assert_eq!(achados[0].capability, "power");

    let achados = catalogo.resolver("living room light");
    assert_eq!(achados.len(), 1);
    assert_eq!(achados[0].device_id, "ha:light.sala_teto");

    // Acento e caixa não importam.
    assert_eq!(catalogo.resolver("PORTÃO").len(), 1);
    assert_eq!(catalogo.resolver("portao").len(), 1);

    // Termo que ninguém declarou não inventa dispositivo.
    assert!(catalogo.resolver("geladeira").is_empty());
}

/// Nenhum preset oficial tenta rebaixar risco — e, se um tentasse, o clamp
/// devolveria o risco do adapter. O teste cobre as duas metades.
#[test]
fn nenhum_skill_oficial_rebaixa_risco() {
    let catalogo = catalogo();

    for skill in catalogo.skills() {
        for preset in &skill.presets {
            let Some(declarado) = preset.risco_declarado else {
                continue;
            };
            assert!(
                declarado > RiskClass::R0,
                "preset '{}' do skill '{}' declara R0: risco declarado so faz sentido para subir",
                preset.device_id,
                skill.nome
            );
        }
    }

    // A metade do clamp: um adapter que classificou a fechadura em R3
    // continua em R3 mesmo com o preset do repo por perto.
    let cap = Capability::acao("door_unlock", RiskClass::R3, None).expect("valida");
    let efetiva = catalogo.capability_efetiva("ha:lock.porta_frente", &cap);
    assert_eq!(efetiva.risk, RiskClass::R3);
    efetiva
        .validar()
        .expect("invariante leitura<->R0 preservada");

    // E a direção permitida, no preset do portão do ESP32: o adapter MQTT
    // classificaria a saída em R1; o skill sobe para R3.
    let cap = Capability::acao("power", RiskClass::R1, None).expect("valida");
    let efetiva = catalogo.capability_efetiva("mqtt:esp32_portao", &cap);
    assert_eq!(
        efetiva.risk,
        RiskClass::R3,
        "o preset do portao sobe o risco da saida"
    );
    efetiva.validar().expect("invariante preservada");
}

/// Leitura nunca vira ação, nem com preset oficial no meio.
#[test]
fn leitura_continua_r0_com_os_skills_do_repo() {
    let catalogo = catalogo();
    for id in ["mqtt:sensor_sala", "mqtt:esp32_varanda", "serial:bancada"] {
        let cap = Capability::leitura("temperature", None);
        let efetiva = catalogo.capability_efetiva(id, &cap);
        assert_eq!(efetiva.risk, RiskClass::R0, "{id} continua leitura");
        assert!(efetiva.read_only);
    }
}

/// Um transporte fora da lista fechada carrega inerte — e é por isso que o
/// repo não distribui skill de Modbus/ROS2 ainda.
#[test]
fn modbus_e_ros2_nao_sao_transportes_do_core() {
    assert_eq!(Transporte::de_texto("modbus"), None);
    assert_eq!(Transporte::de_texto("ros2"), None);
}
