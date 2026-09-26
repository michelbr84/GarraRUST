//! O caminho **unico** de mutacao da Access Policy v2 (ADR 0025 §4; #1412,
//! #1413).
//!
//! CLI (`garraia whatsapp access ...`), API admin e Web Console chamam
//! [`aplicar`] sobre a secao `channels.whatsapp_linked` e recebem o antes e o
//! depois **pelo mesmo leitor que o turno usa**
//! ([`super::super::settings_da_secao`]). Quem grava o arquivo e quem chama (a
//! escrita atomica `0600` do `ConfigLoader::save`); quem audita e
//! [`super::auditoria`]; quem diz o impacto e [`super::impacto`]. Nada aqui
//! toca disco.
//!
//! Validacao vem **antes** de qualquer escrita: uma mutacao invalida deixa a
//! secao exatamente como estava e explica o que vale (#1399).

use std::fmt;

use garraia_agents::modes::Nivel;
use garraia_config::ChannelConfig;
use serde_json::{Map, Value, json};

use super::super::{
    CONFIG_KEY, LinkedSettings, chave_do_portao, normalizar_identidade, settings_da_secao,
};
use super::{Admission, Alcance, PoliticaDeAcesso};

/// O que se pode mudar. Cada variante e um comando da CLI e uma acao da API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutacao {
    /// `access.admission` (#1396).
    Admissao(Admission),
    /// `access.default` — o desconhecido em `open` (#1399). Nunca `full`.
    DefaultDesconhecido(Alcance),
    /// `access.users.<id>.level` (#1398).
    Nivel { identidade: String, nivel: Nivel },
    /// `access.users.<id>.write` (#1397).
    Write { identidade: String, on: bool },
    /// `access.users.<id>.blocked = true`.
    Bloquear(String),
    /// Tira o `blocked`; entrada que fica vazia some.
    Desbloquear(String),
    /// `access.groups.enabled` (e o `reply_in_groups` legado, ao desligar).
    Grupos(bool),
    /// `access.groups.default`.
    DefaultDeGrupo(Alcance),
    /// `access.groups.<jid>`.
    Grupo { jid: String, alcance: Alcance },
    /// Volta ao seguro (#1401): `restricted`, default `chat`, grupos
    /// desligados e sem politica, overrides de nivel/write removidos.
    /// Preserva donos e bloqueios.
    Reset,
    /// `role: owner` liga/desliga (#1404). Desligar o dono LEGADO (so em
    /// `owners`) o move para `allow`: o acesso e preservado, como
    /// `garraia whatsapp unowner` faz.
    Papel { identidade: String, dono: bool },
    /// Tira a identidade de `allow`, `owners` e `access.users` (#1404).
    Remover(String),
}

impl Mutacao {
    /// O nome da acao no audit e no `--json` (contrato de script).
    pub fn acao(&self) -> &'static str {
        match self {
            Self::Admissao(Admission::Open) => "open",
            Self::Admissao(Admission::Restricted) => "restricted",
            Self::DefaultDesconhecido(_) => "default",
            Self::Nivel { .. } => "level",
            Self::Write { .. } => "write",
            Self::Bloquear(_) => "block",
            Self::Desbloquear(_) => "unblock",
            Self::Grupos(_) => "groups",
            Self::DefaultDeGrupo(_) => "group-default",
            Self::Grupo { .. } => "group",
            Self::Reset => "reset",
            Self::Papel { dono: true, .. } => "owner",
            Self::Papel { dono: false, .. } => "unowner",
            Self::Remover(_) => "remove",
        }
    }

    /// A identidade (ou o JID de grupo) que a mutacao mira, crua.
    pub fn alvo(&self) -> Option<&str> {
        match self {
            Self::Nivel { identidade, .. }
            | Self::Write { identidade, .. }
            | Self::Bloquear(identidade)
            | Self::Desbloquear(identidade)
            | Self::Papel { identidade, .. }
            | Self::Remover(identidade) => Some(identidade),
            Self::Grupo { jid, .. } => Some(jid),
            _ => None,
        }
    }
}

/// Por que a mutacao nao foi aplicada. O `Display` diz o que fazer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutacaoInvalida {
    /// `default: full` para desconhecido (#1390).
    DefaultFull,
    /// `chat` com `write: true`: nao ha onde escrever.
    ChatComWrite,
    /// `write` em quem ninguem conhece: autorize (ou de um nivel) primeiro.
    Desconhecida,
    /// Nivel/write no dono: o dono nao tem teto; mexer nele e `unowner`.
    EDono,
    /// Identidade ou JID em branco.
    IdentidadeVazia,
    /// Tornar dono quem esta bloqueado: desbloqueie antes.
    Bloqueada,
    /// A secao `channels.whatsapp_linked` tem `type:` de outro canal.
    SecaoDeOutroCanal(String),
    /// Uma chave que devia ser mapa nao e (`access`, `access.users`,
    /// `access.groups`): o arquivo precisa de correcao manual.
    NaoEMapa(String),
}

impl fmt::Display for MutacaoInvalida {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefaultFull => write!(
                f,
                "`default` do desconhecido nao pode ser `full` (#1390): o maximo e `read` — quem precisa de mais e declarado em `access.users`"
            ),
            Self::ChatComWrite => write!(
                f,
                "`chat` nao tem onde escrever: para `write on`, de antes o nivel `read` (ou `full`)"
            ),
            Self::Desconhecida => write!(
                f,
                "esta identidade nao esta autorizada: autorize com `whatsapp allow <numero>` ou de um nivel com `whatsapp level <numero> read|full` antes de mexer em `write`"
            ),
            Self::EDono => write!(
                f,
                "esta identidade e DONO e o dono nao tem teto: para limitar, tire o papel com `whatsapp unowner <numero>` e depois de um nivel"
            ),
            Self::IdentidadeVazia => write!(f, "identidade em branco"),
            Self::Bloqueada => write!(
                f,
                "esta identidade esta bloqueada: `whatsapp unblock <numero>` antes de torna-la dono"
            ),
            Self::SecaoDeOutroCanal(tipo) => write!(
                f,
                "`channels.{CONFIG_KEY}` existe com `type: {tipo}` — corrija para `type: {CONFIG_KEY}` no config.yml"
            ),
            Self::NaoEMapa(chave) => write!(
                f,
                "`channels.{CONFIG_KEY}.{chave}` nao e um mapa — corrija o config.yml"
            ),
        }
    }
}

impl std::error::Error for MutacaoInvalida {}

/// O resultado de [`aplicar`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aplicada {
    /// A secao ficou diferente? `false` e idempotencia, nao erro.
    pub mudou: bool,
    pub antes: PoliticaDeAcesso,
    pub depois: PoliticaDeAcesso,
    /// O que mudou, em linhas legiveis **sem identidade** (so `…1234`).
    pub mudancas: Vec<String>,
}

/// Aplica uma mutacao a secao, validando antes de escrever.
pub fn aplicar(secao: &mut ChannelConfig, mutacao: &Mutacao) -> Result<Aplicada, MutacaoInvalida> {
    if secao.channel_type != CONFIG_KEY {
        return Err(MutacaoInvalida::SecaoDeOutroCanal(
            secao.channel_type.clone(),
        ));
    }
    let antes = settings_da_secao(secao);

    // ── Validacao, antes de tocar em qualquer coisa ──
    let identidade = match mutacao {
        Mutacao::Nivel { identidade, .. }
        | Mutacao::Write { identidade, .. }
        | Mutacao::Bloquear(identidade)
        | Mutacao::Desbloquear(identidade)
        | Mutacao::Papel { identidade, .. }
        | Mutacao::Remover(identidade) => {
            let id = normalizar_identidade(identidade);
            if id.is_empty() {
                return Err(MutacaoInvalida::IdentidadeVazia);
            }
            Some(id)
        }
        Mutacao::Grupo { jid, .. } if jid.trim().is_empty() => {
            return Err(MutacaoInvalida::IdentidadeVazia);
        }
        _ => None,
    };
    let id = identidade.as_deref().unwrap_or_default();
    match mutacao {
        Mutacao::DefaultDesconhecido(a) => {
            if a.nivel == Nivel::Full {
                return Err(MutacaoInvalida::DefaultFull);
            }
            if a.nivel == Nivel::Chat && a.write {
                return Err(MutacaoInvalida::ChatComWrite);
            }
        }
        Mutacao::DefaultDeGrupo(a) | Mutacao::Grupo { alcance: a, .. } => {
            if a.nivel == Nivel::Chat && a.write {
                return Err(MutacaoInvalida::ChatComWrite);
            }
        }
        Mutacao::Nivel { .. } => {
            if antes.e_dono(id) {
                return Err(MutacaoInvalida::EDono);
            }
        }
        Mutacao::Papel { dono: true, .. } => {
            if antes.entrada_de(id).is_some_and(|e| e.bloqueado) {
                return Err(MutacaoInvalida::Bloqueada);
            }
        }
        Mutacao::Write { on, .. } => {
            if antes.e_dono(id) {
                return Err(MutacaoInvalida::EDono);
            }
            let Some(entrada) = antes.entrada_de(id) else {
                return Err(MutacaoInvalida::Desconhecida);
            };
            if *on && entrada.alcance.nivel == Nivel::Chat {
                return Err(MutacaoInvalida::ChatComWrite);
            }
        }
        _ => {}
    }
    if secao.settings.get("access").is_some_and(|v| !v.is_object()) {
        return Err(MutacaoInvalida::NaoEMapa("access".to_string()));
    }

    // ── Escrita ──
    // O nivel que `write` herda quando a entrada ainda nao declara um: o do
    // legado (`allow` = full sem teto, por isso `full`).
    let nivel_herdado = antes
        .entrada_de(id)
        .map_or(Nivel::Chat, |e| e.alcance.nivel);
    let mut desligar_legado_de_grupo = false;
    let mut tirar_de_owners = false;
    let mut remover_das_listas = false;
    let chave = chave_do_portao(id);
    {
        let access = secao
            .settings
            .entry("access".to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| MutacaoInvalida::NaoEMapa("access".to_string()))?;
        match mutacao {
            Mutacao::Admissao(a) => {
                access.insert("admission".to_string(), json!(a.as_str()));
            }
            Mutacao::DefaultDesconhecido(a) => {
                access.insert("default".to_string(), alcance_json(*a));
            }
            Mutacao::Nivel { nivel, .. } => {
                let entrada = entrada_mut(users_mut(access)?, id)?;
                entrada.insert("level".to_string(), json!(nivel.as_str()));
                if *nivel == Nivel::Chat {
                    entrada.insert("write".to_string(), json!(false));
                }
            }
            Mutacao::Write { on, .. } => {
                let entrada = entrada_mut(users_mut(access)?, id)?;
                if !entrada.contains_key("level") {
                    entrada.insert("level".to_string(), json!(nivel_herdado.as_str()));
                }
                entrada.insert("write".to_string(), json!(*on));
            }
            Mutacao::Bloquear(_) => {
                entrada_mut(users_mut(access)?, id)?.insert("blocked".to_string(), json!(true));
            }
            Mutacao::Desbloquear(_) => {
                let users = users_mut(access)?;
                let chave = chave_do_portao(id);
                if let Some(k) = users
                    .keys()
                    .find(|k| chave_do_portao(&normalizar_identidade(k)) == chave)
                    .cloned()
                {
                    let vazia = users
                        .get_mut(&k)
                        .and_then(Value::as_object_mut)
                        .map(|obj| {
                            obj.remove("blocked");
                            obj.is_empty()
                        })
                        .unwrap_or(false);
                    if vazia {
                        users.remove(&k);
                    }
                }
            }
            Mutacao::Grupos(ligados) => {
                groups_mut(access)?.insert("enabled".to_string(), json!(*ligados));
                desligar_legado_de_grupo = !*ligados;
            }
            Mutacao::DefaultDeGrupo(a) => {
                groups_mut(access)?.insert("default".to_string(), alcance_json(*a));
            }
            Mutacao::Grupo { jid, alcance } => {
                groups_mut(access)?.insert(jid.trim().to_string(), alcance_json(*alcance));
            }
            Mutacao::Papel { dono: true, .. } => {
                entrada_mut(users_mut(access)?, id)?.insert("role".to_string(), json!("owner"));
            }
            Mutacao::Papel { dono: false, .. } => {
                let users = users_mut(access)?;
                if let Some(k) = users
                    .keys()
                    .find(|k| chave_do_portao(&normalizar_identidade(k)) == chave)
                    .cloned()
                {
                    let vazia = users
                        .get_mut(&k)
                        .and_then(Value::as_object_mut)
                        .map(|obj| {
                            obj.remove("role");
                            obj.is_empty()
                        })
                        .unwrap_or(false);
                    if vazia {
                        users.remove(&k);
                    }
                }
                tirar_de_owners = true;
            }
            Mutacao::Remover(_) => {
                users_mut(access)?
                    .retain(|k, _| chave_do_portao(&normalizar_identidade(k)) != chave);
                remover_das_listas = true;
            }
            Mutacao::Reset => {
                access.insert(
                    "admission".to_string(),
                    json!(Admission::Restricted.as_str()),
                );
                access.insert("default".to_string(), alcance_json(Alcance::CHAT));
                let groups = groups_mut(access)?;
                groups.retain(|k, _| k == "enabled");
                groups.insert("enabled".to_string(), json!(false));
                let users = users_mut(access)?;
                for valor in users.values_mut() {
                    if let Some(obj) = valor.as_object_mut() {
                        obj.remove("level");
                        obj.remove("write");
                    }
                }
                users.retain(|_, v| v.as_object().is_some_and(|o| !o.is_empty()));
                desligar_legado_de_grupo = true;
            }
        }
    }
    if desligar_legado_de_grupo && let Some(v) = secao.settings.get_mut("reply_in_groups") {
        *v = json!(false);
    }
    if tirar_de_owners {
        // O dono legado sai de `owners`; se so existia la, entra em `allow`:
        // o acesso e preservado, como `garraia whatsapp unowner` faz. Quem
        // ainda tem entrada em `access.users` ja continua admitido por ela.
        let saiu = retirar_da_lista(&mut secao.settings, "owners", &chave);
        let ainda_declarado = lista_contem(&secao.settings, "allow", &chave)
            || secao
                .settings
                .get("access")
                .and_then(|a| a.get("users"))
                .and_then(Value::as_object)
                .is_some_and(|users| {
                    users
                        .keys()
                        .any(|k| chave_do_portao(&normalizar_identidade(k)) == chave)
                });
        if saiu && !ainda_declarado {
            secao
                .settings
                .entry("allow".to_string())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| MutacaoInvalida::NaoEMapa("allow".to_string()))?
                .push(Value::String(id.to_string()));
        }
    }
    if remover_das_listas {
        retirar_da_lista(&mut secao.settings, "allow", &chave);
        retirar_da_lista(&mut secao.settings, "owners", &chave);
    }

    let depois = settings_da_secao(secao);
    let mudou = antes != depois;
    let mudancas = if mudou {
        descrever(&antes, &depois)
    } else {
        Vec::new()
    };
    Ok(Aplicada {
        mudou,
        antes: antes.access,
        depois: depois.access,
        mudancas,
    })
}

/// Tira toda grafia de `chave` da lista legada (`allow` | `owners`).
/// Devolve se algo saiu. Lista ausente ou que nao e lista: nada sai.
fn retirar_da_lista(
    settings: &mut std::collections::HashMap<String, Value>,
    lista: &str,
    chave: &str,
) -> bool {
    let Some(itens) = settings.get_mut(lista).and_then(Value::as_array_mut) else {
        return false;
    };
    let antes = itens.len();
    itens.retain(|v| {
        v.as_str()
            .map(|s| chave_do_portao(&normalizar_identidade(s)) != chave)
            .unwrap_or(true)
    });
    itens.len() != antes
}

fn lista_contem(
    settings: &std::collections::HashMap<String, Value>,
    lista: &str,
    chave: &str,
) -> bool {
    settings
        .get(lista)
        .and_then(Value::as_array)
        .is_some_and(|itens| {
            itens
                .iter()
                .filter_map(Value::as_str)
                .any(|s| chave_do_portao(&normalizar_identidade(s)) == chave)
        })
}

/// `…1234`: os quatro ultimos digitos de um numero, do id de um `@lid` ou do
/// JID de um grupo. Nunca a identidade inteira.
pub fn mascarar(identidade: &str) -> String {
    let base = identidade.split(['@', ':']).next().unwrap_or_default();
    let digitos: Vec<char> = base.chars().filter(|c| c.is_ascii_digit()).collect();
    let fim: String = digitos.iter().rev().take(4).rev().collect();
    if fim.is_empty() {
        "…????".to_string()
    } else {
        format!("…{fim}")
    }
}

fn alcance_json(a: Alcance) -> Value {
    json!({ "level": a.nivel.as_str(), "write": a.write })
}

fn users_mut(access: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, MutacaoInvalida> {
    access
        .entry("users")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| MutacaoInvalida::NaoEMapa("access.users".to_string()))
}

fn groups_mut(access: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, MutacaoInvalida> {
    access
        .entry("groups")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| MutacaoInvalida::NaoEMapa("access.groups".to_string()))
}

/// A entrada de `access.users` desta identidade — a que ja existe com a mesma
/// chave de portao (qualquer grafia), ou uma nova sob a identidade
/// normalizada. Nunca duas para a mesma pessoa.
fn entrada_mut<'a>(
    users: &'a mut Map<String, Value>,
    identidade: &str,
) -> Result<&'a mut Map<String, Value>, MutacaoInvalida> {
    let chave = chave_do_portao(identidade);
    let existente = users
        .keys()
        .find(|k| chave_do_portao(&normalizar_identidade(k)) == chave)
        .cloned()
        .unwrap_or_else(|| identidade.to_string());
    users
        .entry(existente)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| MutacaoInvalida::NaoEMapa("access.users.<identidade>".to_string()))
}

/// O papel de uma entrada, para o diff: `dono` | `bloqueado` | `usuario (a)`.
fn papel_de(entrada: super::EntradaDeUsuario) -> String {
    if entrada.bloqueado {
        "bloqueado".to_string()
    } else if entrada.dono {
        "dono".to_string()
    } else if entrada.alcance == Alcance::COMPLETO {
        "usuario (sem teto)".to_string()
    } else {
        format!("usuario ({})", entrada.alcance)
    }
}

/// As linhas de [`Aplicada::mudancas`]: um diff legivel entre dois estados,
/// sem identidade (so `…1234`).
fn descrever(antes: &LinkedSettings, depois: &LinkedSettings) -> Vec<String> {
    let mut linhas = Vec::new();
    if antes.access.admission != depois.access.admission {
        linhas.push(format!(
            "admission: {} → {}",
            antes.access.admission.as_str(),
            depois.access.admission.as_str()
        ));
    }
    if antes.access.default != depois.access.default {
        linhas.push(format!(
            "default do desconhecido: {} → {}",
            antes.access.default, depois.access.default
        ));
    }
    if antes.responde_em_grupo() != depois.responde_em_grupo() {
        linhas.push(format!(
            "grupos: {}",
            if depois.responde_em_grupo() {
                "ligados"
            } else {
                "desligados"
            }
        ));
    }
    if antes.access.groups.default != depois.access.groups.default {
        linhas.push(format!(
            "default de grupo: {} → {}",
            antes.access.groups.default, depois.access.groups.default
        ));
    }
    let grupos: std::collections::BTreeSet<&String> = antes
        .access
        .groups
        .por_grupo
        .keys()
        .chain(depois.access.groups.por_grupo.keys())
        .collect();
    for jid in grupos {
        let a = antes.access.groups.por_grupo.get(jid);
        let d = depois.access.groups.por_grupo.get(jid);
        match (a, d) {
            (Some(x), Some(y)) if x == y => {}
            (_, Some(y)) => linhas.push(format!("grupo {}: {y}", mascarar(jid))),
            (Some(_), None) => linhas.push(format!("grupo {}: politica removida", mascarar(jid))),
            (None, None) => {}
        }
    }
    let mut chaves: Vec<String> = antes
        .chaves_declaradas()
        .into_iter()
        .chain(depois.chaves_declaradas())
        .collect();
    chaves.sort();
    chaves.dedup();
    for chave in chaves {
        let a = antes.entrada_de(&chave);
        let d = depois.entrada_de(&chave);
        if a == d {
            continue;
        }
        let quem = mascarar(&chave);
        match (a, d) {
            (None, Some(y)) => linhas.push(format!("{quem}: novo, {}", papel_de(y))),
            (Some(x), None) => linhas.push(format!("{quem}: removido (era {})", papel_de(x))),
            (Some(x), Some(y)) => linhas.push(format!("{quem}: {} → {}", papel_de(x), papel_de(y))),
            (None, None) => {}
        }
    }
    linhas
}
