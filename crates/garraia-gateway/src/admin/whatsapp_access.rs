//! `GET|POST /admin/api/whatsapp/access` e `GET /admin/api/whatsapp/access/audit`
//! (#1402, #1412, #1413, #1414; ADR 0025 §4).
//!
//! O segundo consumidor do caminho unico da politica (a CLI e o primeiro,
//! o Web Console e o terceiro e fala com estas rotas): le pelo
//! `visao::documento`, muda pelo `mutacao::aplicar`, audita pelo
//! `auditoria` com `origem: admin_api` e o username do admin como ator.
//! Nada aqui reimplementa regra.
//!
//! Permissoes (RBAC existente): leitura com `Resource::Channels` +
//! `Action::Read` (viewer le), mutacao com `Action::Update` (viewer recebe
//! 403). O cookie do `/admin` e o CSRF vem dos layers de `auth_routes`.
//!
//! A API **nunca** revela identidade (nao existe `reveal` aqui): `…1234` em
//! tudo, como no audit. A fonte de verdade e o `config.yml` (lido e gravado
//! pelo `ConfigLoader`, como a CLI faz): o gateway com `ConfigWatcher` rele
//! na mensagem seguinte; sem ele, `hot_reload: false` diz que e no restart.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use garraia_agents::modes::Nivel;
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader};
use serde::Deserialize;
use serde_json::{Value, json};

use super::middleware::{AuthenticatedAdmin, extract_ip};
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;
use crate::bootstrap::whatsapp_linked_numero as numero;
use crate::bootstrap::whatsapp_linked_politica::mutacao::{
    self, Mutacao, MutacaoInvalida, mascarar,
};
use crate::bootstrap::whatsapp_linked_politica::presets::{NOMES as NOMES_DE_PRESET, Preset};
use crate::bootstrap::whatsapp_linked_politica::{Admission, Alcance, auditoria, impacto, visao};
use crate::bootstrap::{WHATSAPP_LINKED_CONFIG_KEY as CONFIG_KEY, whatsapp_linked_settings};

mod transferencia_http;
pub use transferencia_http::{
    AccessImportRequest, ExportQuery, admin_whatsapp_access_export, admin_whatsapp_access_import,
};

/// O corpo de `POST /admin/api/whatsapp/access`.
///
/// `action`: `open` | `restricted` | `default` | `level` | `write` | `preset`
/// | `block` | `unblock` | `groups` | `group-default` | `group` | `reset` —
/// os mesmos nomes do audit e da CLI. Os outros campos sao os que a acao usa.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessMutationRequest {
    pub action: String,
    /// Numero (com ou sem `+`) ou JID `@lid`: `level`, `write`, `block`,
    /// `unblock`, `owner`, `unowner`, `remove`.
    #[serde(default)]
    pub identity: Option<String>,
    /// Em vez de `identity`: os quatro ultimos digitos (`1234` ou `…1234`)
    /// de uma identidade **declarada**. E o que o Web Console manda — ele so
    /// conhece `…1234`. Zero candidatas = 400; mais de uma = 409.
    #[serde(default)]
    pub identity_last4: Option<String>,
    /// JID de grupo (`<digitos>@g.us`): `group`.
    #[serde(default)]
    pub jid: Option<String>,
    /// `chat` | `read` | `full`: `default`, `level`, `group-default`, `group`.
    #[serde(default)]
    pub level: Option<String>,
    /// `write`: o valor; `default`/`group-default`/`group`: o `write` do alcance.
    #[serde(default)]
    pub write: Option<bool>,
    /// `chat_only` | `read` | `developer` | `full_pod`: `preset` e, no lugar
    /// de `level`/`write`, `default`, `group-default` e `group` (#1434). O
    /// preset grava `level` + `write` juntos; nada de novo fica no config.
    /// Em `default` ele vale como qualquer alcance — `developer`/`full_pod`
    /// sao `full` e continuam recusados com 400 (#1390). Vindo junto com
    /// `level`/`write`, o preset vence (ele ja traz os dois).
    #[serde(default)]
    pub preset: Option<String>,
    /// `groups`: ligar ou desligar.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// So calcula: nada e gravado nem auditado (#1413).
    #[serde(default)]
    pub dry_run: bool,
    /// #1433: a confirmacao explicita de uma ELEVACAO sensivel. Quando a
    /// mutacao real (nao `dry_run`) concederia a algum principal uma classe
    /// mutante que ele nao tinha (`filesystem.write`, `process.execute`,
    /// `message.send`, `device.execute`, `memory.write`, `mcp.write`), o
    /// gateway recusa com 409 `elevation_confirmation_required` a menos que
    /// este campo seja `true`. E enforcado no servidor, nao so no dialogo do
    /// console: um POST direto sem passar pela confirmacao tambem e recusado.
    /// Para mudancas que so tiram, ou que so mexem em leitura, e ignorado.
    #[serde(default)]
    pub confirm_elevation: bool,
}

/// `?limit=N` de `GET /admin/api/whatsapp/access/audit`.
#[derive(Debug, Clone, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<usize>,
}

const ACOES: &str = "open | restricted | default | level | write | preset | block | unblock | groups | group-default | group | reset";

/// Resolve `identity_last4` entre as identidades **declaradas** na politica
/// (`allow`, `owners`, `access.users`): exatamente uma, ou erro (400 para
/// nenhuma, 409 para mais de uma). O `…1234` nunca vira identidade por
/// adivinhacao.
pub fn identidade_por_last4(
    settings: &crate::bootstrap::WhatsAppLinkedSettings,
    last4: &str,
) -> Result<String, (StatusCode, String)> {
    let alvo = last4.trim().trim_start_matches('…');
    let alvo = if alvo.is_empty() {
        alvo
    } else {
        alvo.trim_start_matches("...")
    };
    if alvo.len() != 4 || !alvo.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "`identity_last4` sao quatro digitos".to_string(),
        ));
    }
    let mut candidatas: Vec<String> = settings
        .allow
        .iter()
        .chain(settings.owners.iter())
        .cloned()
        .chain(settings.access.users.keys().cloned())
        .filter(|id| mascarar(id) == format!("…{alvo}"))
        .collect();
    // A mesma pessoa em varias grafias e UMA candidata.
    candidatas.sort_by_key(|id| crate::bootstrap::whatsapp_linked_chave_do_portao(id));
    candidatas.dedup_by_key(|id| crate::bootstrap::whatsapp_linked_chave_do_portao(id));
    match candidatas.len() {
        0 => Err((
            StatusCode::BAD_REQUEST,
            "nenhuma identidade declarada termina assim".to_string(),
        )),
        1 => Ok(candidatas.remove(0)),
        n => Err((
            StatusCode::CONFLICT,
            format!("{n} identidades declaradas terminam assim; use `identity`"),
        )),
    }
}

/// O que a API muda, traduzido para o motor. Puro; `Err` e o texto do 400.
/// `identidade_resolvida` e o que [`identidade_por_last4`] achou quando o
/// pedido veio por `identity_last4`.
/// Por que um pedido nao vira `Mutacao`: `codigo` estavel para o cliente
/// (`error_code`), `texto` para o humano. `From<String>` mantem os erros
/// genericos com `codigo: invalid_request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedidoInvalido {
    pub codigo: &'static str,
    pub texto: String,
}

impl From<String> for PedidoInvalido {
    fn from(texto: String) -> Self {
        Self {
            codigo: "invalid_request",
            texto,
        }
    }
}

pub fn mutacao_do_pedido(
    req: &AccessMutationRequest,
    identidade_resolvida: Option<&str>,
) -> Result<Mutacao, PedidoInvalido> {
    let nivel = || -> Result<Nivel, String> {
        let s = req
            .level
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "`level` e obrigatorio: chat | read | full".to_string())?;
        Nivel::parse(s)
            .ok_or_else(|| format!("`level` desconhecido: `{s}` (vale chat | read | full)"))
    };
    // #1434: fail-closed como `level` — ausente, vazio ou desconhecido nao
    // vira mutacao nenhuma.
    let preset = || -> Result<Preset, String> {
        let s = req
            .preset
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("`preset` e obrigatorio: {NOMES_DE_PRESET}"))?;
        Preset::parse(s)
            .ok_or_else(|| format!("`preset` desconhecido: `{s}` (vale {NOMES_DE_PRESET})"))
    };
    // #1434: `default`, `group-default` e `group` aceitam `level` + `write`
    // OU `preset` — o preset vence e ja traz o `write` canonico. Nenhuma
    // validacao nova: o alcance resultante passa pelas MESMAS guardas de
    // `mutacao::aplicar` (e por isso `developer`/`full_pod` continuam
    // recusados em `access.default`, que nunca pode ser `full`).
    let alcance = || -> Result<Alcance, String> {
        if req.preset.is_some() {
            return Ok(preset()?.alcance());
        }
        Ok(Alcance {
            nivel: nivel()?,
            write: req.write.unwrap_or(false),
        })
    };
    let identidade = || -> Result<String, PedidoInvalido> {
        // #1403: `identity` crua (o que o console manda) passa pela MESMA
        // validacao da CLI — `+` e codigo do pais obrigatorios, ou
        // `<digitos>@lid`. Sem o `+` nao ha como saber se o codigo do pais
        // veio, e um numero gravado sem ele nunca casa com o remetente.
        if let Some(raw) = req
            .identity
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return numero::normalizar_numero(raw).map_err(|e| PedidoInvalido {
                codigo: e.codigo(),
                texto: format!("`identity` invalida: {}", e.descricao()),
            });
        }
        // `identity_last4` ja foi resolvida contra a config: identidade
        // declarada, na forma gravada — nao passa de novo pelo `+`.
        if let Some(id) = identidade_resolvida {
            return Ok(id.to_string());
        }
        Err(PedidoInvalido {
            codigo: "identity_required",
            texto:
                "`identity` e obrigatoria: numero com `+` e codigo do pais, ou JID `<digitos>@lid`"
                    .to_string(),
        })
    };
    let jid = || -> Result<String, String> {
        let j = req
            .jid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "`jid` e obrigatorio: `<digitos>@g.us`".to_string())?;
        let ok = j
            .strip_suffix("@g.us")
            .is_some_and(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_digit() || c == '-'));
        if ok {
            Ok(j.to_string())
        } else {
            Err("`jid` invalido: e `<digitos>@g.us`".to_string())
        }
    };
    let obrigatorio = |campo: &str, v: Option<bool>| -> Result<bool, String> {
        v.ok_or_else(|| format!("`{campo}` (true | false) e obrigatorio"))
    };
    match req.action.trim() {
        "open" => Ok(Mutacao::Admissao(Admission::Open)),
        "restricted" => Ok(Mutacao::Admissao(Admission::Restricted)),
        "default" => Ok(Mutacao::DefaultDesconhecido(alcance()?)),
        "level" => Ok(Mutacao::Nivel {
            identidade: identidade()?,
            nivel: nivel()?,
        }),
        "write" => Ok(Mutacao::Write {
            identidade: identidade()?,
            on: obrigatorio("write", req.write)?,
        }),
        "preset" => Ok(Mutacao::Preset {
            identidade: identidade()?,
            preset: preset()?,
        }),
        "block" => Ok(Mutacao::Bloquear(identidade()?)),
        "unblock" => Ok(Mutacao::Desbloquear(identidade()?)),
        "owner" => Ok(Mutacao::Papel {
            identidade: identidade()?,
            dono: true,
        }),
        "unowner" => Ok(Mutacao::Papel {
            identidade: identidade()?,
            dono: false,
        }),
        "remove" => Ok(Mutacao::Remover(identidade()?)),
        "groups" => Ok(Mutacao::Grupos(obrigatorio("enabled", req.enabled)?)),
        "group-default" => Ok(Mutacao::DefaultDeGrupo(alcance()?)),
        "group" => Ok(Mutacao::Grupo {
            jid: jid()?,
            alcance: alcance()?,
        }),
        "reset" => Ok(Mutacao::Reset),
        outra => Err(format!("`action` desconhecida: `{outra}` (vale {ACOES})").into()),
    }
}

fn carregar() -> Result<(ConfigLoader, AppConfig), String> {
    let loader = ConfigLoader::new().map_err(|e| e.to_string())?;
    let config = loader.load_sem_env().map_err(|e| e.to_string())?;
    Ok((loader, config))
}

fn erro(status: StatusCode, texto: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": texto.into() })))
}

/// Erro com `error_code` estavel (#1403): o console mostra o texto, o script
/// decide pelo codigo.
fn erro_com_codigo(
    status: StatusCode,
    texto: impl Into<String>,
    codigo: &'static str,
) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({ "error": texto.into(), "error_code": codigo })),
    )
}

fn proibido() -> (StatusCode, Json<Value>) {
    erro(StatusCode::FORBIDDEN, "insufficient permissions")
}

fn falha_de_config(e: String) -> (StatusCode, Json<Value>) {
    // O erro do loader vai para o log; o corpo diz so que a config nao pode
    // ser lida — o caminho do arquivo nao e assunto da rota.
    tracing::warn!(erro = %e, "admin: nao consegui ler o config.yml para a politica do WhatsApp");
    erro(StatusCode::INTERNAL_SERVER_ERROR, "failed to read config")
}

/// `channels.whatsapp_linked.audit_max_bytes`, ou o default do motor.
fn teto_do_audit(config: &AppConfig) -> u64 {
    config
        .channels
        .get(CONFIG_KEY)
        .and_then(|s| s.settings.get("audit_max_bytes"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(auditoria::MAX_BYTES_DEFAULT)
}

fn documento(state: &AdminState, config: &AppConfig) -> Value {
    // O perfil de execucao e o do PROCESSO (env vence o arquivo, ADR 0024):
    // e o que decide o piso do dono no gateway que esta rodando.
    let perfil = state.app_state.config.execution.perfil();
    json!({
        "policy": visao::documento(config, perfil, false),
        "hot_reload": state.app_state.has_config_watcher(),
        // #1422: o que o portao recusou desde o boot — por motivo, com o
        // final apenas. Rota autenticada (Channels/Read).
        "rejections": state.app_state.whatsapp_linked.rejeicoes(),
    })
}

/// `GET /admin/api/whatsapp/access` — a politica efetiva (o mesmo documento
/// de `garraia whatsapp access --json`), mais `hot_reload`.
pub async fn admin_whatsapp_access(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Read) {
        return proibido();
    }
    match carregar() {
        Ok((_, config)) => (StatusCode::OK, Json(documento(&state, &config))),
        Err(e) => falha_de_config(e),
    }
}

/// `POST /admin/api/whatsapp/access` — uma mutacao (ou o seu `dry_run`).
pub async fn admin_whatsapp_access_mutate(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(req): Json<AccessMutationRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Update) {
        return proibido();
    }
    let (loader, config) = match carregar() {
        Ok(v) => v,
        Err(e) => return falha_de_config(e),
    };
    let original = config.clone();
    let resolvida = match (&req.identity, &req.identity_last4) {
        (None, Some(last4)) => {
            match identidade_por_last4(&whatsapp_linked_settings(&original), last4) {
                Ok(id) => Some(id),
                Err((status, texto)) => return erro(status, texto),
            }
        }
        _ => None,
    };
    let mutacao = match mutacao_do_pedido(&req, resolvida.as_deref()) {
        Ok(m) => m,
        Err(e) => return erro_com_codigo(StatusCode::BAD_REQUEST, e.texto, e.codigo),
    };
    let mut config = config;
    let secao = config
        .channels
        .entry(CONFIG_KEY.to_string())
        .or_insert_with(|| ChannelConfig {
            channel_type: CONFIG_KEY.to_string(),
            enabled: None,
            settings: Default::default(),
        });
    let aplicada = match mutacao::aplicar(secao, &mutacao) {
        Ok(a) => a,
        Err(e @ (MutacaoInvalida::SecaoDeOutroCanal(_) | MutacaoInvalida::NaoEMapa(_))) => {
            return erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
        Err(e) => return erro(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let perfil = state.app_state.config.execution.perfil();
    let antes = whatsapp_linked_settings(&original);
    let depois = whatsapp_linked_settings(&config);
    let impact: Vec<Value> = impacto::diferencas(&antes, &depois, perfil)
        .into_iter()
        .map(|d| {
            json!({
                "principal": d.principal, "target": d.alvo,
                "gains": d.ganha, "loses": d.perde,
            })
        })
        .collect();

    // #1433: a elevacao sensivel (classes mutantes recem-concedidas), pelo
    // mesmo motor do impacto. O `dry_run` so a MOSTRA; a mutacao real a
    // EXIGE confirmada (409 fail-closed) antes de conceder.
    let elevacoes = impacto::elevacoes_sensiveis(&antes, &depois, perfil);
    let elevation: Vec<Value> = elevacoes
        .iter()
        .map(|e| {
            json!({
                "principal": e.principal, "target": e.alvo, "classes": e.classes,
            })
        })
        .collect();
    if !req.dry_run && aplicada.mudou && !elevacoes.is_empty() && !req.confirm_elevation {
        // Nada foi gravado nem auditado: a recusa vem antes de qualquer
        // escrita. O corpo traz o preview (mudancas, impacto, elevacao) para o
        // console mostrar e reenviar com `confirm_elevation: true`.
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "this change elevates a principal into a sensitive capability; confirm it explicitly",
                "error_code": "elevation_confirmation_required",
                "dry_run": false,
                "changed": aplicada.mudou,
                "written": false,
                "changes": aplicada.mudancas,
                "impact": impact,
                "elevation": elevation,
                "policy": visao::documento(&original, perfil, false),
                "hot_reload": state.app_state.has_config_watcher(),
            })),
        );
    }

    let mut written = false;
    let mut audit = json!({ "written": false });
    if !req.dry_run && aplicada.mudou {
        if let Err(e) = loader.ensure_dirs().and_then(|()| loader.save(&config)) {
            tracing::warn!(erro = %e, "admin: nao consegui gravar o config.yml (politica do WhatsApp)");
            return erro(StatusCode::INTERNAL_SERVER_ERROR, "failed to write config");
        }
        written = true;
        let evento = auditoria::Evento::novo(
            "admin_api",
            &admin.username,
            mutacao.acao(),
            mutacao.alvo(),
            &antes,
            &depois,
        );
        audit = match auditoria::registrar(
            &config.resolved_data_dir(),
            &evento,
            teto_do_audit(&config),
        ) {
            Ok(_) => json!({ "written": true }),
            Err(e) => {
                tracing::warn!(erro = %e, "admin: a mudanca foi gravada, mas o audit da politica nao");
                json!({ "written": false, "error": e.to_string() })
            }
        };
        // A trilha do proprio admin tambem, com o alvo mascarado.
        let alvo = mutacao.alvo().map(mascarar);
        let guard = state.store.lock().await;
        // (action, resource_type), como `("login", "auth")` e `("save",
        // "config")` nos outros handlers — estava invertido.
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            mutacao.acao(),
            "whatsapp_access",
            alvo.as_deref(),
            None,
            extract_ip(&headers, None).as_deref(),
            "success",
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "dry_run": req.dry_run,
            "changed": aplicada.mudou,
            "written": written,
            "changes": aplicada.mudancas,
            "impact": impact,
            "elevation": elevation,
            "audit": audit,
            "policy": visao::documento(&config, perfil, false),
            "hot_reload": state.app_state.has_config_watcher(),
        })),
    )
}

/// `GET /admin/api/whatsapp/access/audit?limit=N` — a trilha, mais recente
/// primeiro.
pub async fn admin_whatsapp_access_audit(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Query(query): Query<AuditQuery>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Read) {
        return proibido();
    }
    let _ = &state;
    let (_, config) = match carregar() {
        Ok(v) => v,
        Err(e) => return falha_de_config(e),
    };
    let limite = query.limit.unwrap_or(20).clamp(1, 500);
    let eventos = match auditoria::ler(&config.resolved_data_dir(), limite) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(erro = %e, "admin: nao consegui ler o audit da politica do WhatsApp");
            return erro(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read audit log",
            );
        }
    };
    let events: Vec<Value> = eventos
        .into_iter()
        .map(|e| {
            json!({
                "ts": e.ts, "source": e.origem, "actor": e.ator, "action": e.acao,
                "target": e.alvo, "before": e.antes, "after": e.depois,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(json!({ "events": events, "limit": limite })),
    )
}

/// `POST /admin/api/whatsapp/access/rejections/reset` — zera as mensagens
/// recusadas (#1422). Channels/Update; auditado no proprio admin.
pub async fn admin_whatsapp_access_rejections_reset(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Update) {
        return proibido();
    }
    let apagadas = state.app_state.whatsapp_linked.zerar_rejeicoes();
    {
        let guard = state.store.lock().await;
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "rejections_reset",
            "whatsapp_access",
            None,
            Some(&format!("cleared={apagadas}")),
            extract_ip(&headers, None).as_deref(),
            "success",
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "cleared": apagadas,
            "rejections": state.app_state.whatsapp_linked.rejeicoes(),
        })),
    )
}
