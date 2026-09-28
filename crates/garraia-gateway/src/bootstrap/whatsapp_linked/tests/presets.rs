//! Presets nomeados de permissao (#1434; ADR 0025 §3-4).
//!
//! Um preset e um apelido para um [`Alcance`] canonico — nada de novo fica no
//! `config.yml`, e a autoridade continua sendo `level` + `write`. O que se
//! prova aqui sao os seis criterios da issue: definicao documentada, `chat_only`
//! e `read` seguros para contexto remoto/nao confiavel, `developer` e `full_pod`
//! limitados pelo perfil de execucao (porque compilam para o mesmo `full`, que
//! por ADR 0025 §3 nao tem restricao propria), atribuicao por principal, politica
//! fora dos presets exibida como `custom`, e — o golden abaixo — nenhuma
//! evolucao silenciosa do que cada preset libera.

use std::collections::HashMap;

use garraia_agents::capacidades::Capacidade;
use garraia_agents::modes::{Nivel, ToolPolicy, politica_do_nivel};
use garraia_config::{ChannelConfig, ExecutionProfile};
use serde_json::json;

use crate::bootstrap::whatsapp_linked::politica::Alcance;
use crate::bootstrap::whatsapp_linked::politica::presets::{NOMES, Preset, rotulo_efetivo};
use crate::bootstrap::whatsapp_linked::politica::visao;
use crate::bootstrap::whatsapp_linked::{LinkedSettings, settings_from_config};

const USUARIO: &str = "5511999998888";
const DONO: &str = "5511977776666";

/// As classes que um `ToolPolicy` deixa passar para uma ferramenta de classe
/// unica — a mesma leitura que o `ToolGate` faz: `no_tools` e `denied` vencem
/// tudo, e num whitelist so passa quem esta em `allowed_capabilities`.
fn classes_liberadas(p: &ToolPolicy) -> Vec<&'static str> {
    Capacidade::TODAS
        .iter()
        .copied()
        .filter(|c| {
            if p.no_tools {
                return false;
            }
            if p.denied_capabilities.contains(c) {
                return false;
            }
            !p.whitelist_mode || p.allowed_capabilities.contains(c)
        })
        .map(Capacidade::as_str)
        .collect()
}

fn politica_do_preset(preset: Preset) -> ToolPolicy {
    let a = preset.alcance();
    politica_do_nivel(a.nivel, a.write)
}

fn settings(secao: serde_json::Value) -> LinkedSettings {
    let mapa: HashMap<String, serde_json::Value> = secao
        .as_object()
        .expect("objeto")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut config = garraia_config::AppConfig::default();
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings: mapa,
        },
    );
    settings_from_config(&config)
}

// ---------------------------------------------------------------------------
// Nome <-> preset
// ---------------------------------------------------------------------------

#[test]
fn nome_e_parse_fecham_o_ciclo_e_recusam_o_resto() {
    for preset in Preset::ALL {
        assert_eq!(
            Preset::parse(preset.nome()),
            Some(preset),
            "{} nao volta pelo proprio nome",
            preset.nome()
        );
        // `-` no lugar de `_`, espaco em volta e caixa alta tambem entram.
        assert_eq!(
            Preset::parse(&format!(
                "  {}  ",
                preset.nome().replace('_', "-").to_uppercase()
            )),
            Some(preset)
        );
        assert!(
            NOMES.contains(preset.nome()),
            "a mensagem de erro precisa listar {}",
            preset.nome()
        );
    }
    // Fail-closed: nada de adivinhar preset.
    for nao in [
        "", "   ", "full", "chat", "dev", "fullpod", "owner", "admin",
    ] {
        assert_eq!(Preset::parse(nao), None, "`{nao}` nao e preset");
    }
    // Quatro nomes distintos.
    let mut nomes: Vec<&str> = Preset::ALL.iter().map(|p| p.nome()).collect();
    nomes.sort_unstable();
    nomes.dedup();
    assert_eq!(nomes.len(), Preset::ALL.len());
}

// ---------------------------------------------------------------------------
// GOLDEN: o que cada preset libera, hoje, classe por classe
// ---------------------------------------------------------------------------

/// Este teste e o guarda-costas do criterio "evolucao de preset nao concede
/// capacidade perigosa em silencio": ele fixa o conjunto EXATO de classes que
/// `chat_only` e `read` deixam passar. Qualquer mudanca em `politica_do_nivel`
/// que alargue `chat` ou `read` quebra este teste em vez de embarcar calada.
#[test]
fn golden_das_classes_de_cada_preset() {
    assert_eq!(
        classes_liberadas(&politica_do_preset(Preset::ChatOnly)),
        Vec::<&str>::new(),
        "chat_only e `no_tools`: nenhuma classe, de nenhuma forma"
    );
    assert_eq!(
        classes_liberadas(&politica_do_preset(Preset::Read)),
        vec![
            "filesystem.read",
            "network.read",
            "device.read",
            "memory.read",
            "runtime.inspect",
            "mcp.read",
        ],
        "read e a whitelist de LEITURA, e so ela"
    );
    // `developer` e `full_pod` nao restringem por conta propria (ADR 0025 §3):
    // quem decide e o piso de modo e o perfil de execucao.
    let todas: Vec<&str> = Capacidade::TODAS.iter().map(|c| c.as_str()).collect();
    for preset in [Preset::Developer, Preset::FullPod] {
        assert_eq!(
            classes_liberadas(&politica_do_preset(preset)),
            todas,
            "{} nao pode ter teto proprio",
            preset.nome()
        );
    }
}

/// O criterio "Chat Only e Read sao seguros para contexto remoto/nao
/// confiavel": nenhuma classe perigosa passa em nenhum dos dois.
#[test]
fn chat_only_e_read_negam_toda_classe_perigosa() {
    const PERIGOSAS: &[Capacidade] = &[
        Capacidade::ProcessExecute,
        Capacidade::DeviceExecute,
        Capacidade::MessageSend,
        Capacidade::MemoryWrite,
        Capacidade::Scheduling,
        Capacidade::FilesystemWrite,
        Capacidade::McpWrite,
    ];
    for preset in [Preset::ChatOnly, Preset::Read] {
        let liberadas = classes_liberadas(&politica_do_preset(preset));
        for perigosa in PERIGOSAS {
            assert!(
                !liberadas.contains(&perigosa.as_str()),
                "{} liberou {}: {liberadas:?}",
                preset.nome(),
                perigosa.as_str()
            );
        }
        // Alem de nao passar, sao negadas de forma explicita — `denied` vence
        // tudo, inclusive um `allowed` por nome vindo do modo.
        let p = politica_do_preset(preset);
        assert!(
            p.no_tools || PERIGOSAS.iter().all(|c| p.denied_capabilities.contains(c)),
            "{}: negacao precisa ser explicita, nao so ausencia do whitelist",
            preset.nome()
        );
        assert!(
            !preset.alcance().write,
            "{} nunca escreve arquivo",
            preset.nome()
        );
    }
}

/// O criterio "Developer e Full Pod respeitam o perfil de execucao": os dois
/// compilam para a MESMA `Alcance`, logo nenhum dos dois carrega capacidade
/// propria — o que os separa na pratica e `execution.profile` (ADR 0024), nao
/// o nome do preset.
#[test]
fn developer_e_full_pod_sao_o_mesmo_alcance() {
    assert_eq!(Preset::Developer.alcance(), Alcance::COMPLETO);
    assert_eq!(Preset::FullPod.alcance(), Alcance::COMPLETO);
    assert_eq!(Preset::Developer.alcance(), Preset::FullPod.alcance());
    assert_eq!(
        politica_do_preset(Preset::Developer),
        politica_do_preset(Preset::FullPod),
        "o teto aplicado e identico; a diferenca e o perfil, nunca o nome"
    );
    // E os dois estreitos continuam estreitos.
    assert_eq!(Preset::ChatOnly.alcance(), Alcance::CHAT);
    assert_eq!(Preset::Read.alcance(), Alcance::LEITURA);
}

// ---------------------------------------------------------------------------
// rotulo_efetivo: o `custom` continua possivel
// ---------------------------------------------------------------------------

#[test]
fn rotulo_efetivo_nomeia_o_preset_e_chama_o_resto_de_custom() {
    assert_eq!(rotulo_efetivo(Alcance::CHAT), "chat_only");
    assert_eq!(rotulo_efetivo(Alcance::LEITURA), "read");
    assert_eq!(rotulo_efetivo(Alcance::COMPLETO), "full");
    // Combinacoes validas que nenhum preset cobre continuam existindo — e
    // aparecem como `custom`, nunca como o preset mais proximo.
    for custom in [
        Alcance {
            nivel: Nivel::Read,
            write: true,
        },
        Alcance {
            nivel: Nivel::Full,
            write: false,
        },
        Alcance {
            nivel: Nivel::Chat,
            write: true,
        },
    ] {
        assert_eq!(rotulo_efetivo(custom), "custom", "{custom}");
    }
}

// ---------------------------------------------------------------------------
// visao: o rotulo no documento efetivo (CLI, API e console leem o mesmo)
// ---------------------------------------------------------------------------

#[test]
fn o_documento_efetivo_traz_o_preset_calculado_ao_vivo() {
    let s = settings(json!({
        "access": {
            "admission": "open",
            "default": { "level": "read", "write": false },
            "users": {
                USUARIO: { "level": "read", "write": true },
                DONO: { "role": "owner" },
            },
            "groups": { "enabled": true, "default": { "level": "chat", "write": false } },
        }
    }));
    let doc = visao::documento_de(&s, ExecutionProfile::Standard, false);
    // `access.default` e `access.groups.default`, ao lado de level/write.
    assert_eq!(doc["default"]["preset"], json!("read"));
    assert_eq!(doc["default"]["level"], json!("read"), "chave antiga fica");
    assert_eq!(doc["groups"]["default"]["preset"], json!("chat_only"));
    // Por principal: o usuario declarado tem `read` + `write on`, que nenhum
    // preset cobre — `custom`.
    let principais = doc["principals"].as_array().expect("lista");
    let usuario = principais
        .iter()
        .find(|p| p["principal"] == json!("usuario"))
        .expect("usuario");
    assert_eq!(usuario["preset"], json!("custom"), "{usuario}");
    // O dono nao tem teto: `preset` e nulo, como `level`/`write`.
    let dono = principais
        .iter()
        .find(|p| p["principal"] == json!("dono"))
        .expect("dono");
    assert_eq!(dono["preset"], serde_json::Value::Null);
    assert_eq!(dono["level"], serde_json::Value::Null);
    // Todo principal tem a chave (contrato de leitura para o console).
    for p in principais {
        assert!(p.get("preset").is_some(), "sem `preset`: {p}");
    }
}

#[test]
fn o_documento_efetivo_nomeia_cada_preset_atribuido() {
    for (preset, esperado) in [
        (Preset::ChatOnly, "chat_only"),
        (Preset::Read, "read"),
        (Preset::Developer, "full"),
        (Preset::FullPod, "full"),
    ] {
        let a = preset.alcance();
        let s = settings(json!({
            "access": {
                "users": { USUARIO: { "level": a.nivel.as_str(), "write": a.write } },
            }
        }));
        let doc = visao::documento_de(&s, ExecutionProfile::Standard, false);
        let usuario = doc["principals"]
            .as_array()
            .expect("lista")
            .iter()
            .find(|p| p["principal"] == json!("usuario"))
            .cloned()
            .expect("usuario");
        assert_eq!(
            usuario["preset"],
            json!(esperado),
            "{} deveria aparecer como {esperado}: {usuario}",
            preset.nome()
        );
    }
}
