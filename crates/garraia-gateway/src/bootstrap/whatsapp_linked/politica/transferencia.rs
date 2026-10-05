//! Exportar e importar a Access Policy v2 **sem segredo** (#1435; ADR 0025
//! §4).
//!
//! O operador que roda varias instalacoes do GarraIA precisa replicar a
//! politica de acesso de uma para a outra sem copiar credencial, token,
//! material do cofre nem a sessao do WhatsApp. Isto e possivel aqui porque a
//! politica ja vive separada do segredo: `channels.whatsapp_linked.access` e
//! so estrutura (admissao, default, niveis, grupos, bloqueios), e o material
//! de sessao fica em disco (`<data_dir>/whatsapp/...`), nunca no `config.yml`.
//! O token da Cloud API vive em OUTRO canal (`whatsapp`), que este modulo nao
//! olha.
//!
//! # Exclusao de segredo por construcao
//!
//! [`exportar`] **nao** serializa um `ChannelConfig` nem um `LinkedSettings`:
//! ele monta o documento campo a campo, por uma allowlist fechada
//! ([`PoliticaDeAcesso`]). Uma chave de API que alguem plantasse na secao
//! `whatsapp_linked` (ou em qualquer outra) nunca entra no export porque o
//! export so le os campos que nomeia — e o teste varre a saida atras de
//! valores-sentinela plantados para provar isso.
//!
//! # PII opcional
//!
//! Com `redigir_pii`, as identidades (numero/JID) viram `…1234` (a mesma
//! mascara do audit, [`super::mutacao::mascarar`]): deterministica e
//! irreversivel. Um export redigido traz `pii_redacted: true` e **nao e
//! importavel** — [`parse_importacao`] o recusa, porque `…1234` nao reconstroi
//! uma identidade sem adivinhacao. Ele serve para inspecao e para compartilhar
//! a FORMA da politica, nao para replicar as pessoas.
//!
//! # Importar
//!
//! [`parse_importacao`] valida o `format` e a `version` (recusa formato
//! desconhecido e versao de um major futuro, com mensagem clara), recusa o
//! export redigido, e le a politica por allowlist (nivel/admissao
//! desconhecidos sao erro, nao normalizacao silenciosa). [`aplicar_importada`]
//! escreve a politica inteira em `access`, limpando o legado
//! (`allow`/`owners`/`reply_in_groups`) — cujo sentido ja foi capturado em
//! `access.users`/`access.groups` — e preservando o resto da secao (`enabled`,
//! `default_mode`, `audit_max_bytes`, a sessao em disco). Nada toca o disco
//! aqui: quem grava (atomico, `0600`, com backup) e quem audita e o chamador,
//! pelo mesmo caminho de [`super::mutacao`].

use std::fmt;

use garraia_agents::modes::Nivel;
use garraia_config::ChannelConfig;
use serde_json::{Map, Value, json};

use super::super::{CONFIG_KEY, LinkedSettings, chave_do_portao, normalizar_identidade};
use super::impacto::Diferenca;
use super::mutacao::{MutacaoInvalida, mascarar};
use super::{Admission, Alcance};

/// O `format` que marca um documento desta familia.
pub const FORMATO: &str = "garraia.access-policy";
/// A versao de schema que esta build escreve e sabe ler. Um documento de uma
/// versao MAIOR e recusado (foi exportado por um GarraIA mais novo).
pub const VERSAO_ATUAL: u64 = 1;

/// Uma identidade da politica, no formato do export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsuarioExportado {
    /// A identidade (chave do portao), ou `…1234` quando redigida.
    pub identidade: String,
    pub alcance: Alcance,
    pub dono: bool,
    pub bloqueado: bool,
}

/// A politica lida de um documento de importacao, ja validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoliticaImportada {
    pub admission: Admission,
    pub default: Alcance,
    pub groups_enabled: bool,
    pub groups_default: Alcance,
    /// `(jid, alcance)`, na ordem do documento.
    pub groups: Vec<(String, Alcance)>,
    /// `(identidade, alcance, dono, bloqueado)`.
    pub users: Vec<UsuarioExportado>,
}

/// Por que um documento de importacao foi recusado. O `Display` diz o que
/// fazer; [`Self::codigo`] da um codigo estavel para o cliente da API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroDeImportacao {
    /// O documento nao e JSON de objeto, ou nao tem `policy`.
    Estrutura(String),
    /// `format` ausente ou diferente de [`FORMATO`].
    FormatoDesconhecido(String),
    /// `version` de um major futuro: exportado por um GarraIA mais novo.
    VersaoFutura(u64),
    /// `version` ausente, zero ou nao-inteira.
    VersaoInvalida(String),
    /// `channel` presente e diferente de `whatsapp_linked`.
    CanalDiferente(String),
    /// `pii_redacted: true`: identidades mascaradas nao se reimportam.
    PiiRedigido,
    /// `level` ou `admission` com valor fora do dominio.
    Valor(String),
}

impl ErroDeImportacao {
    /// Codigo estavel para `error_code` da API.
    pub fn codigo(&self) -> &'static str {
        match self {
            Self::Estrutura(_) => "import_malformed",
            Self::FormatoDesconhecido(_) => "import_format_unknown",
            Self::VersaoFutura(_) => "import_version_future",
            Self::VersaoInvalida(_) => "import_version_invalid",
            Self::CanalDiferente(_) => "import_channel_mismatch",
            Self::PiiRedigido => "import_pii_redacted",
            Self::Valor(_) => "import_value_invalid",
        }
    }
}

impl fmt::Display for ErroDeImportacao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Estrutura(o) => write!(f, "documento invalido: {o}"),
            Self::FormatoDesconhecido(got) => write!(
                f,
                "`format` tem de ser `{FORMATO}` (veio `{got}`): este arquivo nao e um export de politica de acesso do GarraIA"
            ),
            Self::VersaoFutura(v) => write!(
                f,
                "`version` {v} e de um GarraIA mais novo do que este (esta build le ate a versao {VERSAO_ATUAL}): atualize antes de importar"
            ),
            Self::VersaoInvalida(o) => {
                write!(f, "`version` invalida: {o} (esperado um inteiro >= 1)")
            }
            Self::CanalDiferente(got) => {
                write!(f, "`channel` tem de ser `{CONFIG_KEY}` (veio `{got}`)")
            }
            Self::PiiRedigido => write!(
                f,
                "este export foi redigido (`pii_redacted: true`): as identidades viraram `…1234` e nao podem ser reimportadas — exporte sem `--redact-pii` para replicar as pessoas"
            ),
            Self::Valor(o) => write!(f, "valor invalido no documento: {o}"),
        }
    }
}

impl std::error::Error for ErroDeImportacao {}

/// `{ "level": ..., "write": ... }` de um [`Alcance`].
fn alcance_json(a: Alcance) -> Value {
    json!({ "level": a.nivel.as_str(), "write": a.write })
}

/// Monta o documento de export a partir dos settings vivos, por allowlist.
///
/// Le **apenas** [`LinkedSettings::access`] (ja com `allow`/`owners`
/// legados mesclados em `users`), campo a campo. Nada mais da secao — e
/// portanto nada de `default_mode`, `enabled`, caminho de sessao ou qualquer
/// chave que alguem tenha plantado — chega ao documento.
///
/// Com `redigir_pii`, cada identidade e cada JID viram `…1234`, e o documento
/// e marcado `pii_redacted: true` (e [`parse_importacao`] passa a recusa-lo).
pub fn exportar(settings: &LinkedSettings, redigir_pii: bool) -> Value {
    let identidade = |id: &str| -> String {
        if redigir_pii {
            mascarar(id)
        } else {
            id.to_string()
        }
    };

    // `access.users` e um `BTreeMap`: as chaves (do portao) ja vem em ordem
    // estavel, entao o export e deterministico sem ordenar de novo.
    let users: Vec<Value> = settings
        .access
        .users
        .iter()
        .map(|(chave, e)| {
            json!({
                "identity": identidade(chave),
                "level": e.alcance.nivel.as_str(),
                "write": e.alcance.write,
                "owner": e.dono,
                "blocked": e.bloqueado,
            })
        })
        .collect();

    let grupos: Vec<Value> = settings
        .access
        .groups
        .por_grupo
        .iter()
        .map(|(jid, alcance)| {
            json!({
                "jid": identidade(jid),
                "level": alcance.nivel.as_str(),
                "write": alcance.write,
            })
        })
        .collect();

    json!({
        "format": FORMATO,
        "version": VERSAO_ATUAL,
        "channel": CONFIG_KEY,
        "exported_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "pii_redacted": redigir_pii,
        "policy": {
            "admission": settings.access.admission.as_str(),
            "default": alcance_json(settings.access.default),
            "groups": {
                "enabled": settings.responde_em_grupo(),
                "default": alcance_json(settings.access.groups.default),
                "entries": grupos,
            },
            "users": users,
        },
    })
}

/// Le `{ level, write }` de um objeto (ou um nivel como string curta),
/// validando estritamente — nivel desconhecido e erro, nao normalizacao.
fn alcance_de(v: &Value, contexto: &str) -> Result<Alcance, ErroDeImportacao> {
    let (nivel_str, write) = match v {
        Value::String(s) => (s.as_str(), false),
        Value::Object(m) => {
            let nivel = m
                .get("level")
                .and_then(Value::as_str)
                .ok_or_else(|| ErroDeImportacao::Valor(format!("`{contexto}` sem `level`")))?;
            let write = match m.get("write") {
                None => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => {
                    return Err(ErroDeImportacao::Valor(format!(
                        "`{contexto}.write` nao e booleano"
                    )));
                }
            };
            (nivel, write)
        }
        _ => {
            return Err(ErroDeImportacao::Valor(format!(
                "`{contexto}` nao e um mapa nem um nivel"
            )));
        }
    };
    let nivel = Nivel::parse(nivel_str).ok_or_else(|| {
        ErroDeImportacao::Valor(format!(
            "`{contexto}.level` desconhecido: `{nivel_str}` (vale chat | read | full)"
        ))
    })?;
    Ok(Alcance { nivel, write })
}

fn bool_de(v: Option<&Value>, contexto: &str) -> Result<bool, ErroDeImportacao> {
    match v {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(ErroDeImportacao::Valor(format!(
            "`{contexto}` nao e booleano"
        ))),
    }
}

/// Valida o cabecalho (`format`, `version`, `channel`, `pii_redacted`) e le a
/// `policy` por allowlist. `Err` recusa o documento inteiro — nada e aplicado.
pub fn parse_importacao(doc: &Value) -> Result<PoliticaImportada, ErroDeImportacao> {
    let obj = doc.as_object().ok_or_else(|| {
        ErroDeImportacao::Estrutura("o documento nao e um objeto JSON".to_string())
    })?;

    match obj.get("format").and_then(Value::as_str) {
        Some(FORMATO) => {}
        Some(outro) => return Err(ErroDeImportacao::FormatoDesconhecido(outro.to_string())),
        None => {
            return Err(ErroDeImportacao::FormatoDesconhecido(
                "<ausente>".to_string(),
            ));
        }
    }

    match obj.get("version") {
        Some(Value::Number(n)) => match n.as_u64() {
            Some(0) | None => {
                return Err(ErroDeImportacao::VersaoInvalida(n.to_string()));
            }
            Some(v) if v > VERSAO_ATUAL => return Err(ErroDeImportacao::VersaoFutura(v)),
            Some(_) => {}
        },
        Some(outro) => return Err(ErroDeImportacao::VersaoInvalida(outro.to_string())),
        None => return Err(ErroDeImportacao::VersaoInvalida("<ausente>".to_string())),
    }

    if let Some(canal) = obj.get("channel").and_then(Value::as_str)
        && canal != CONFIG_KEY
    {
        return Err(ErroDeImportacao::CanalDiferente(canal.to_string()));
    }

    if bool_de(obj.get("pii_redacted"), "pii_redacted")? {
        return Err(ErroDeImportacao::PiiRedigido);
    }

    let policy = obj
        .get("policy")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ErroDeImportacao::Estrutura("sem `policy` (ou nao e um mapa)".to_string())
        })?;

    let admission = match policy.get("admission").and_then(Value::as_str) {
        None => Admission::Restricted,
        Some(s) => match s.trim().to_ascii_lowercase().as_str() {
            "restricted" => Admission::Restricted,
            "open" => Admission::Open,
            outro => {
                return Err(ErroDeImportacao::Valor(format!(
                    "`policy.admission` desconhecida: `{outro}` (vale restricted | open)"
                )));
            }
        },
    };

    let default = match policy.get("default") {
        None => Alcance::CHAT,
        Some(v) => alcance_de(v, "policy.default")?,
    };

    let (groups_enabled, groups_default, groups) = match policy.get("groups") {
        None => (false, Alcance::COMPLETO, Vec::new()),
        Some(Value::Object(g)) => {
            let enabled = bool_de(g.get("enabled"), "policy.groups.enabled")?;
            let gdefault = match g.get("default") {
                None => Alcance::COMPLETO,
                Some(v) => alcance_de(v, "policy.groups.default")?,
            };
            let mut entries = Vec::new();
            match g.get("entries") {
                None => {}
                Some(Value::Array(arr)) => {
                    for item in arr {
                        let o = item.as_object().ok_or_else(|| {
                            ErroDeImportacao::Valor(
                                "`policy.groups.entries[]` nao e um mapa".to_string(),
                            )
                        })?;
                        let jid = o
                            .get("jid")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                ErroDeImportacao::Valor(
                                    "`policy.groups.entries[].jid` ausente ou vazio".to_string(),
                                )
                            })?;
                        let alcance = alcance_de(item, "policy.groups.entries[]")?;
                        entries.push((jid.to_string(), alcance));
                    }
                }
                Some(_) => {
                    return Err(ErroDeImportacao::Valor(
                        "`policy.groups.entries` nao e uma lista".to_string(),
                    ));
                }
            }
            (enabled, gdefault, entries)
        }
        Some(_) => {
            return Err(ErroDeImportacao::Valor(
                "`policy.groups` nao e um mapa".to_string(),
            ));
        }
    };

    let mut users = Vec::new();
    match policy.get("users") {
        None => {}
        Some(Value::Array(arr)) => {
            for item in arr {
                let o = item.as_object().ok_or_else(|| {
                    ErroDeImportacao::Valor("`policy.users[]` nao e um mapa".to_string())
                })?;
                let identidade = o
                    .get("identity")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        ErroDeImportacao::Valor(
                            "`policy.users[].identity` ausente ou vazio".to_string(),
                        )
                    })?;
                let dono = bool_de(o.get("owner"), "policy.users[].owner")?;
                let bloqueado = bool_de(o.get("blocked"), "policy.users[].blocked")?;
                // O nivel e opcional: um usuario so com `blocked` nao precisa
                // de `level`. Sem `level` e sem ser dono, vale `chat`.
                let alcance = if o.contains_key("level") {
                    alcance_de(item, "policy.users[]")?
                } else if dono {
                    Alcance::COMPLETO
                } else {
                    Alcance::CHAT
                };
                users.push(UsuarioExportado {
                    identidade: identidade.to_string(),
                    alcance,
                    dono,
                    bloqueado,
                });
            }
        }
        Some(_) => {
            return Err(ErroDeImportacao::Valor(
                "`policy.users` nao e uma lista".to_string(),
            ));
        }
    }

    Ok(PoliticaImportada {
        admission,
        default,
        groups_enabled,
        groups_default,
        groups,
        users,
    })
}

/// Grava a politica importada na secao `access`, substituindo a politica
/// inteira: limpa `access` e o legado (`allow`, `owners`, `reply_in_groups`),
/// e reescreve admissao, default, grupos e usuarios. Preserva tudo o que NAO e
/// politica (`enabled`, `default_mode`, `audit_max_bytes`, a sessao em disco).
///
/// Pura quanto ao disco: muda a secao em memoria; quem grava e audita e o
/// chamador. O impacto (`impacto::diferencas`) e o alargamento sao calculados
/// por fora, sobre o `antes`/`depois` desta escrita.
pub fn aplicar_importada(
    secao: &mut ChannelConfig,
    imp: &PoliticaImportada,
) -> Result<(), MutacaoInvalida> {
    if secao.channel_type != CONFIG_KEY {
        return Err(MutacaoInvalida::SecaoDeOutroCanal(
            secao.channel_type.clone(),
        ));
    }

    // O legado some: tudo o que ele significava ja esta em `access`.
    secao.settings.remove("allow");
    secao.settings.remove("owners");
    secao.settings.remove("reply_in_groups");

    let mut access = Map::new();
    access.insert("admission".to_string(), json!(imp.admission.as_str()));
    access.insert("default".to_string(), alcance_json(imp.default));

    let mut groups = Map::new();
    groups.insert("enabled".to_string(), json!(imp.groups_enabled));
    groups.insert("default".to_string(), alcance_json(imp.groups_default));
    for (jid, alcance) in &imp.groups {
        groups.insert(jid.trim().to_string(), alcance_json(*alcance));
    }
    access.insert("groups".to_string(), Value::Object(groups));

    let mut users = Map::new();
    for u in &imp.users {
        let chave = chave_do_portao(&normalizar_identidade(&u.identidade));
        if chave.is_empty() {
            continue;
        }
        let mut entrada = Map::new();
        if u.dono {
            // Dono e sem teto: `role: owner` ja o faz `COMPLETO`; nivel/write
            // seriam redundantes (e `aplicar` recusaria nivel no dono).
            entrada.insert("role".to_string(), json!("owner"));
        } else {
            entrada.insert("level".to_string(), json!(u.alcance.nivel.as_str()));
            entrada.insert("write".to_string(), json!(u.alcance.write));
        }
        if u.bloqueado {
            entrada.insert("blocked".to_string(), json!(true));
        }
        users.insert(chave, Value::Object(entrada));
    }
    access.insert("users".to_string(), Value::Object(users));

    secao
        .settings
        .insert("access".to_string(), Value::Object(access));
    Ok(())
}

/// A importacao alarga a politica de algum principal? Alargar e um principal
/// GANHAR capacidade (ou passar a ser admitido) — o que exige confirmacao
/// explicita (#1435). Estreitar (so perdas) nunca exige.
pub fn ha_alargamento(diferencas: &[Diferenca]) -> bool {
    diferencas.iter().any(|d| !d.ganha.is_empty())
}

/// Os principais que ganham algo, cada um com o que ganha — para a mensagem de
/// "isto alarga a politica; confirme".
pub fn alargamentos(diferencas: &[Diferenca]) -> Vec<&Diferenca> {
    diferencas.iter().filter(|d| !d.ganha.is_empty()).collect()
}
