//! Access Policy v2 do WhatsApp pessoal (ADR 0025 §4).
//!
//! `channels.whatsapp_linked.access` descreve **quem** fala com o agente e
//! **ate onde** cada um vai — por principal, nao por lista unica:
//!
//! ```yaml
//! access:
//!   admission: restricted          # restricted | open
//!   default: { level: chat, write: false }   # o DESCONHECIDO, so com open
//!   users:
//!     "+5511999998888": { level: read, write: false }
//!     "+5511977776666": { role: owner }
//!     "+5511955554444": { blocked: true }
//!   groups:
//!     enabled: false
//!     default: { level: read, write: false }
//!     "120363@g.us": { level: chat }
//! ```
//!
//! Tres regras que a secao **nao** escolhe:
//!
//! - **Compatibilidade.** `allow` continua valendo (= usuario **sem teto**: o
//!   piso de modo decide, como sempre), `owners` continua valendo (= dono) e
//!   `reply_in_groups` continua ligando os grupos. Sem `access:`, o
//!   comportamento e o da v0.4.5. Nivel e `write` so existem onde foram
//!   **declarados** (`access.users`, `access.default`, `access.groups`);
//!   `access.users` vence o legado para a mesma identidade. A unica excecao
//!   e quem entrou por codigo (`/pair`): credencial fraca, teto `read`.
//! - **Fail-closed.** Nivel desconhecido e `chat`; `chat` com `write: true` e
//!   `chat`; `admission` desconhecida e `restricted`; o `default` de `open`
//!   nunca chega a `full` (#1390). Cada normalizacao deixa um aviso — sem
//!   numero — que o boot e o `config check` mostram.
//! - **Nivel e teto, nao piso.** O que sai daqui vira um
//!   [`TetoDeCapacidades`] composto por E com o modo da sessao (ADR 0025 §2):
//!   o teto so tira, nunca poe. O piso de modo (`search`; dono em pod, `code`)
//!   continua sendo decidido por [`super::perfil_do_turno`] e
//!   [`super::modo_do_piso`].
//!
//! Tudo aqui e puro. Quem le a config viva por turno e
//! [`super::admissao_vigente`]; quem aplica e o `turno` do sink.

use std::collections::BTreeMap;
use std::fmt;

use garraia_agents::modes::{Nivel, TetoDeCapacidades, politica_do_nivel};
use garraia_config::ChannelConfig;
use serde_json::Value;

use super::{LinkedSettings, chave_do_portao, normalizar_identidade};

/// A trilha local de mudancas (#1414).
pub mod auditoria;
/// O efetivo por principal e o preview de impacto, pelo `ToolGate` real
/// (#1400, #1413).
pub mod impacto;
/// O caminho unico de mutacao da secao (#1412).
pub mod mutacao;

/// Quem entra sem estar declarado nem pareado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Admission {
    /// So quem esta em `allow`/`owners`/`access.users` ou resgatou um codigo.
    #[default]
    Restricted,
    /// Qualquer remetente nao bloqueado, com [`PoliticaDeAcesso::default`].
    Open,
}

impl Admission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restricted => "restricted",
            Self::Open => "open",
        }
    }
}

/// Nivel + `write` de um principal: o que compila para o teto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alcance {
    pub nivel: Nivel,
    /// Escrita de arquivo (nativa e MCP) — e **so** isso (ADR 0025 §3).
    pub write: bool,
}

impl Alcance {
    /// Nenhuma ferramenta: o desconhecido em `open`, por default.
    pub const CHAT: Alcance = Alcance {
        nivel: Nivel::Chat,
        write: false,
    };
    /// O que `allow` sempre deu: leitura, sem escrita.
    pub const LEITURA: Alcance = Alcance {
        nivel: Nivel::Read,
        write: false,
    };
    /// Sem teto proprio: o modo e o perfil de execucao decidem.
    pub const COMPLETO: Alcance = Alcance {
        nivel: Nivel::Full,
        write: true,
    };
}

impl Default for Alcance {
    fn default() -> Self {
        Self::CHAT
    }
}

impl fmt::Display for Alcance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, write {}",
            self.nivel.as_str(),
            if self.write { "on" } else { "off" }
        )
    }
}

/// Uma linha de `access.users`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntradaDeUsuario {
    pub alcance: Alcance,
    /// `role: owner` — o mesmo papel de `owners`.
    pub dono: bool,
    /// `blocked: true` — recusado mesmo em `open`, mesmo em `allow`, mesmo
    /// pareado.
    pub bloqueado: bool,
}

/// `access.groups`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoliticaDeGrupos {
    /// `access.groups.enabled` **ou** o `reply_in_groups` legado.
    pub enabled: bool,
    /// O grupo sem entrada propria. Sem `access.groups.default`, sem teto
    /// (o piso de modo decide, como sempre).
    pub default: Alcance,
    /// Por JID de grupo (chave como veio, `trim`).
    pub por_grupo: BTreeMap<String, Alcance>,
}

impl Default for PoliticaDeGrupos {
    fn default() -> Self {
        Self {
            enabled: false,
            default: Alcance::COMPLETO,
            por_grupo: BTreeMap::new(),
        }
    }
}

/// A secao inteira, ja normalizada e fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PoliticaDeAcesso {
    pub admission: Admission,
    /// O que um desconhecido recebe em `open`. Nunca `full`.
    pub default: Alcance,
    /// Por [`chave_do_portao`] da identidade — legado (`allow`, `owners`)
    /// mais `access.users`, este vencendo.
    pub users: BTreeMap<String, EntradaDeUsuario>,
    pub groups: PoliticaDeGrupos,
    /// O que foi normalizado por estar invalido. Texto sem identidade.
    pub avisos: Vec<String>,
}

/// Quem fala neste turno, depois de admitido — e o que cada um pode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    /// Declarado em `owners` ou `role: owner`, em conversa 1:1. Sem teto.
    Dono,
    /// Declarado em `allow` (sem teto) ou `access.users` (com o alcance
    /// declarado).
    Usuario(Alcance),
    /// Resgatou um codigo do `/pair` neste processo: leitura, sem escrita.
    Pareado,
    /// Entrou por `admission: open`, com o `default`.
    Desconhecido(Alcance),
    /// Mensagem de grupo: a politica e do grupo, nunca do remetente — o dono
    /// no grupo e o grupo.
    Grupo(Alcance),
    /// `blocked: true`. Nao e admitido.
    Bloqueado,
    /// Ninguem o conhece e a admissao e `restricted`. Nao e admitido.
    Estranho,
}

impl Principal {
    /// A etiqueta do log e da recusa. Nunca carrega identidade.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dono => "dono",
            Self::Usuario(_) => "usuario",
            Self::Pareado => "pareado",
            Self::Desconhecido(_) => "desconhecido",
            Self::Grupo(_) => "grupo",
            Self::Bloqueado => "bloqueado",
            Self::Estranho => "estranho",
        }
    }

    /// Passa para o agente?
    pub fn admitido(self) -> bool {
        !matches!(self, Self::Bloqueado | Self::Estranho)
    }

    /// O alcance efetivo. `None` para o dono (sem teto) e para quem nao e
    /// admitido (nao ha turno).
    pub fn alcance(self) -> Option<Alcance> {
        match self {
            Self::Dono | Self::Bloqueado | Self::Estranho => None,
            Self::Usuario(a) | Self::Desconhecido(a) | Self::Grupo(a) => Some(a),
            Self::Pareado => Some(Alcance::LEITURA),
        }
    }
}

/// Onde `full` nao vale (#1390): o `default` de `open`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TetoDeNivel {
    Read,
    Full,
}

/// Le um `{ level, write }` (ou a forma curta, so o nivel). Fail-closed em
/// cada campo, com **um** aviso por entrada invalida e sem identidade.
fn alcance_de(v: &Value, contexto: &str, teto: TetoDeNivel, avisos: &mut Vec<String>) -> Alcance {
    let (nivel, write) = match v {
        Value::String(s) => (Some(s.as_str()), false),
        Value::Object(m) => (
            m.get("level").and_then(Value::as_str),
            m.get("write").and_then(Value::as_bool).unwrap_or(false),
        ),
        _ => {
            avisos.push(format!(
                "`{contexto}` nao e um mapa nem um nivel; vale `chat`"
            ));
            return Alcance::CHAT;
        }
    };
    let nivel = match nivel {
        None => Nivel::Chat,
        Some(s) => match Nivel::parse(s) {
            Some(n) => n,
            None => {
                avisos.push(format!(
                    "`{contexto}`: nivel desconhecido (vale `chat` | `read` | `full`); vale `chat`"
                ));
                return Alcance::CHAT;
            }
        },
    };
    let nivel = if nivel == Nivel::Full && teto == TetoDeNivel::Read {
        avisos.push(format!(
            "`{contexto}`: nivel `full` nao vale para desconhecido; vale `read`"
        ));
        Nivel::Read
    } else {
        nivel
    };
    if nivel == Nivel::Chat && write {
        avisos.push(format!(
            "`{contexto}`: `write: true` nao vale em `chat` (nao ha onde escrever); ignorado"
        ));
        return Alcance::CHAT;
    }
    Alcance { nivel, write }
}

/// Uma linha de `access.users`: `{ level, write, role, blocked }`, ou so o
/// nivel como string.
fn entrada_de_usuario(v: &Value, avisos: &mut Vec<String>) -> EntradaDeUsuario {
    const CONTEXTO: &str = "access.users.<identidade>";
    let Value::Object(m) = v else {
        return EntradaDeUsuario {
            alcance: alcance_de(v, CONTEXTO, TetoDeNivel::Full, avisos),
            dono: false,
            bloqueado: false,
        };
    };
    let bloqueado = m.get("blocked").and_then(Value::as_bool).unwrap_or(false);
    let dono = match m.get("role").and_then(Value::as_str) {
        None => false,
        Some(r) if r.trim().eq_ignore_ascii_case("owner") => true,
        Some(_) => {
            avisos.push(format!(
                "`{CONTEXTO}`: `role` desconhecido (so `owner`); ignorado"
            ));
            false
        }
    };
    let alcance = if m.contains_key("level") || m.contains_key("write") {
        alcance_de(v, CONTEXTO, TetoDeNivel::Full, avisos)
    } else if dono {
        Alcance::COMPLETO
    } else {
        if !bloqueado {
            avisos.push(format!("`{CONTEXTO}`: sem `level`; vale `chat`"));
        }
        Alcance::CHAT
    };
    EntradaDeUsuario {
        alcance,
        dono,
        bloqueado,
    }
}

impl PoliticaDeAcesso {
    /// Le `access` da secao **e** o legado (`allow`, `owners`,
    /// `reply_in_groups`) ja normalizado por quem chama.
    pub fn da_secao(
        section: &ChannelConfig,
        allow: &[String],
        owners: &[String],
        reply_in_groups: bool,
    ) -> Self {
        let mut avisos = Vec::new();
        let mut users: BTreeMap<String, EntradaDeUsuario> = BTreeMap::new();
        // `allow` legado: admitido, sem teto — o piso de modo decide, como
        // sempre decidiu. Nivel e `write` so existem onde foram declarados.
        for a in allow {
            users.insert(
                chave_do_portao(a),
                EntradaDeUsuario {
                    alcance: Alcance::COMPLETO,
                    dono: false,
                    bloqueado: false,
                },
            );
        }
        // `owners` vence `allow` para a mesma identidade: dono e admitido sem
        // se listar duas vezes (ADR 0024).
        for d in owners {
            users.insert(
                chave_do_portao(d),
                EntradaDeUsuario {
                    alcance: Alcance::COMPLETO,
                    dono: true,
                    bloqueado: false,
                },
            );
        }
        let mut politica = Self {
            admission: Admission::Restricted,
            default: Alcance::CHAT,
            users,
            groups: PoliticaDeGrupos {
                enabled: reply_in_groups,
                ..PoliticaDeGrupos::default()
            },
            avisos: Vec::new(),
        };

        let Some(access) = section.settings.get("access") else {
            return politica;
        };
        let Some(obj) = access.as_object() else {
            politica
                .avisos
                .push("`access` nao e um mapa; ignorado (vale so o legado)".to_string());
            return politica;
        };

        for chave in obj.keys() {
            if !matches!(chave.as_str(), "admission" | "default" | "users" | "groups") {
                avisos.push(format!(
                    "`access.{chave}` nao e uma chave conhecida; ignorada"
                ));
            }
        }

        politica.admission = match obj.get("admission") {
            None => Admission::Restricted,
            Some(v) => match v.as_str().map(|s| s.trim().to_ascii_lowercase()).as_deref() {
                Some("restricted") => Admission::Restricted,
                Some("open") => Admission::Open,
                _ => {
                    avisos.push(
                        "`access.admission` desconhecida (vale `restricted` | `open`); vale `restricted`"
                            .to_string(),
                    );
                    Admission::Restricted
                }
            },
        };

        if let Some(v) = obj.get("default") {
            politica.default = alcance_de(v, "access.default", TetoDeNivel::Read, &mut avisos);
        }

        match obj.get("users") {
            None => {}
            Some(Value::Object(m)) => {
                for (k, v) in m {
                    let chave = chave_do_portao(&normalizar_identidade(k));
                    if chave.is_empty() {
                        avisos.push("`access.users`: identidade vazia ignorada".to_string());
                        continue;
                    }
                    let entrada = entrada_de_usuario(v, &mut avisos);
                    politica.users.insert(chave, entrada);
                }
            }
            Some(_) => avisos.push("`access.users` nao e um mapa; ignorado".to_string()),
        }

        match obj.get("groups") {
            None => {}
            Some(Value::Object(m)) => {
                for (k, v) in m {
                    match k.as_str() {
                        "enabled" => match v.as_bool() {
                            Some(b) => politica.groups.enabled = politica.groups.enabled || b,
                            None => avisos.push(
                                "`access.groups.enabled` nao e booleano; ignorado".to_string(),
                            ),
                        },
                        "default" => {
                            politica.groups.default = alcance_de(
                                v,
                                "access.groups.default",
                                TetoDeNivel::Full,
                                &mut avisos,
                            );
                        }
                        jid => {
                            let alcance = alcance_de(
                                v,
                                "access.groups.<grupo>",
                                TetoDeNivel::Full,
                                &mut avisos,
                            );
                            politica
                                .groups
                                .por_grupo
                                .insert(jid.trim().to_string(), alcance);
                        }
                    }
                }
            }
            Some(_) => avisos.push("`access.groups` nao e um mapa; ignorado".to_string()),
        }

        politica.avisos = avisos;
        politica
    }
}

/// Resolve o principal de um turno. Pura; chamada **depois** de admitir, e
/// da a mesma resposta que o portao para quem nao entra (fail-closed nos
/// dois lados).
///
/// `pareado` e o que o [`super::PortaoDoCanal`] sabe deste remetente neste
/// processo. `chat` e o JID da conversa (o do grupo, quando `is_group`).
///
/// Ordem: bloqueio vence tudo; quem ninguem conhece so entra em `open`; em
/// grupo a politica e a do grupo (o dono no grupo e o grupo, ADR 0024/0025);
/// senao dono, usuario, pareado, desconhecido.
pub fn principal_do_turno(
    settings: &LinkedSettings,
    remetente: &str,
    chat: &str,
    is_group: bool,
    pareado: bool,
) -> Principal {
    let entrada = settings.entrada_de(remetente);
    if entrada.is_some_and(|e| e.bloqueado) {
        return Principal::Bloqueado;
    }
    let aberto = settings.access.admission == Admission::Open;
    if entrada.is_none() && !pareado && !aberto {
        return Principal::Estranho;
    }
    if is_group {
        let grupos = &settings.access.groups;
        let alcance = grupos
            .por_grupo
            .get(chat.trim())
            .copied()
            .unwrap_or(grupos.default);
        return Principal::Grupo(alcance);
    }
    match entrada {
        Some(e) if e.dono => Principal::Dono,
        Some(e) => Principal::Usuario(e.alcance),
        None if pareado => Principal::Pareado,
        None => Principal::Desconhecido(settings.access.default),
    }
}

/// O teto que vai no `ExecContext` (ADR 0025 §2). `None` para o dono e para
/// quem nao tem nivel declarado (`allow` legado, grupo sem politica): ai o
/// piso de modo decide sozinho, como sempre. Um nao-admitido recebe o teto de
/// `chat` (nenhuma ferramenta), para que um call-site que o deixe passar nao
/// abra nada.
pub fn teto_do_principal(principal: &Principal) -> Option<TetoDeCapacidades> {
    let alcance = match principal {
        Principal::Dono => return None,
        Principal::Bloqueado | Principal::Estranho => Alcance::CHAT,
        outro => outro.alcance()?,
    };
    if alcance == Alcance::COMPLETO {
        return None;
    }
    Some(TetoDeCapacidades {
        nome: format!("{} ({alcance})", principal.as_str()),
        politica: politica_do_nivel(alcance.nivel, alcance.write),
    })
}
