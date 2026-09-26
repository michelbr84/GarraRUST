//! O documento da politica efetiva (#1400, #1402): **um** JSON, produzido
//! aqui, que a CLI (`garraia whatsapp access --json`), a API admin
//! (`GET /admin/api/whatsapp/access`) e o Web Console mostram. Nenhum dos
//! tres monta o seu — e assim que "CLI, API e Web expoem a mesma politica
//! efetiva" (#1388) deixa de ser promessa.
//!
//! Identidades so por `…1234`. `revelar` acrescenta `identity` (o valor da
//! config) e existe para a CLI local (`--reveal`); a API nunca o liga.

use garraia_config::{AppConfig, ExecutionProfile};
use serde_json::{Map, Value, json};

use super::super::{LinkedSettings, settings_from_config};
use super::impacto;
use super::mutacao::mascarar;
use super::{Alcance, Principal};

/// `standard` | `isolated-pod`, como a config escreve.
pub fn nome_do_perfil(perfil: ExecutionProfile) -> &'static str {
    if perfil.is_isolated_pod() {
        "isolated-pod"
    } else {
        "standard"
    }
}

fn alcance_json(a: Option<Alcance>) -> (Value, Value) {
    match a {
        Some(a) => (json!(a.nivel.as_str()), json!(a.write)),
        None => (Value::Null, Value::Null),
    }
}

/// A identidade inteira por `…1234`, para `revelar` (valores da config).
pub fn mapa_de_revelacao(s: &LinkedSettings) -> std::collections::HashMap<String, String> {
    let mut mapa = std::collections::HashMap::new();
    for id in s.allow.iter().chain(s.owners.iter()) {
        mapa.entry(mascarar(id)).or_insert_with(|| id.clone());
    }
    for chave in s.access.users.keys() {
        mapa.entry(mascarar(chave)).or_insert_with(|| chave.clone());
    }
    for jid in s.access.groups.por_grupo.keys() {
        mapa.entry(mascarar(jid)).or_insert_with(|| jid.clone());
    }
    mapa
}

/// O documento, a partir dos settings ja lidos.
pub fn documento_de(s: &LinkedSettings, perfil: ExecutionProfile, revelar: bool) -> Value {
    let revelacao = mapa_de_revelacao(s);
    let principals: Vec<Value> = impacto::matriz(s, perfil)
        .into_iter()
        .map(|l| {
            let (level, write) = alcance_json(l.efetivo.alcance);
            let c = l.efetivo.capacidades;
            let mut obj = Map::new();
            obj.insert("principal".into(), json!(l.principal));
            obj.insert("last4".into(), json!(l.alvo));
            if revelar
                && let Some(alvo) = &l.alvo
                && let Some(id) = revelacao.get(alvo)
            {
                obj.insert("identity".into(), json!(id));
            }
            obj.insert("level".into(), level);
            obj.insert("write".into(), write);
            obj.insert("mode".into(), json!(l.efetivo.modo));
            obj.insert(
                "admitted".into(),
                json!(
                    l.principal != Principal::Bloqueado.as_str()
                        && l.principal != Principal::Estranho.as_str()
                ),
            );
            obj.insert("capabilities".into(), json!(c.ligadas()));
            obj.insert(
                "can".into(),
                json!({
                    "read": c.leitura, "write": c.escrita, "shell": c.shell,
                    "device_execute": c.dispositivo, "message_send": c.mensagem,
                    "mcp_read": c.mcp_leitura, "mcp_write": c.mcp_escrita,
                }),
            );
            Value::Object(obj)
        })
        .collect();
    let bloqueados = s.access.users.values().filter(|u| u.bloqueado).count();
    let piso_do_dono = s.modo_padrao_efetivo(if perfil.is_isolated_pod() {
        ExecutionProfile::IsolatedPod
    } else {
        ExecutionProfile::Standard
    });
    json!({
        "channel": super::super::CONFIG_KEY,
        "enabled": s.enabled,
        "execution_profile": nome_do_perfil(perfil),
        "owner_floor_mode": piso_do_dono,
        "default_mode": s.modo_padrao_efetivo(ExecutionProfile::Standard),
        "admission": s.access.admission.as_str(),
        "default": { "level": s.access.default.nivel.as_str(), "write": s.access.default.write },
        "groups": {
            "enabled": s.responde_em_grupo(),
            "default": { "level": s.access.groups.default.nivel.as_str(), "write": s.access.groups.default.write },
            "declared": s.access.groups.por_grupo.len(),
        },
        "counts": { "authorized": s.autorizados(), "owners": s.donos(), "blocked": bloqueados },
        "warnings": s.access.avisos,
        "principals": principals,
    })
}

/// O documento, a partir do `AppConfig` (a mesma leitura do turno).
pub fn documento(config: &AppConfig, perfil: ExecutionProfile, revelar: bool) -> Value {
    documento_de(&settings_from_config(config), perfil, revelar)
}
