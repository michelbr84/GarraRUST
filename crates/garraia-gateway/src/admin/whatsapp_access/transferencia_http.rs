//! `GET /admin/api/whatsapp/access/export` e `POST .../import` (#1435): a
//! politica de acesso exportada e importada **sem segredo**, no formato
//! versionado `garraia.access-policy`.
//!
//! Modulo filho de `whatsapp_access`: reusa os mesmos helpers (carga da
//! config, erros, teto do audit) e a mesma regra de permissao; so o motor de
//! transferencia (`politica::transferencia`) e novo.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use garraia_config::ChannelConfig;
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::middleware::{AuthenticatedAdmin, extract_ip};
use super::super::rbac::{Action, Resource, check_permission};
use super::super::shared::AdminState;
use super::{carregar, erro, erro_com_codigo, falha_de_config, proibido, teto_do_audit};
use crate::bootstrap::whatsapp_linked_politica::{auditoria, impacto, transferencia, visao};
use crate::bootstrap::{WHATSAPP_LINKED_CONFIG_KEY as CONFIG_KEY, whatsapp_linked_settings};

/// `?redact_pii=` de `GET /admin/api/whatsapp/access/export` (#1435).
///
/// Sobre HTTP o default e REDIGIDO: a API admin nunca revela identidade
/// (`…1234` em tudo), e o export nao pode ser a excecao por omissao. O export
/// cru (`redact_pii=false`, o unico reimportavel) exige `Channels/Update` — o
/// mesmo papel que poderia importar — e e auditado.
#[derive(Debug, Clone, Deserialize)]
pub struct ExportQuery {
    #[serde(default = "redigir_por_padrao")]
    pub redact_pii: bool,
}

impl Default for ExportQuery {
    fn default() -> Self {
        Self {
            redact_pii: redigir_por_padrao(),
        }
    }
}

fn redigir_por_padrao() -> bool {
    true
}

/// O corpo de `POST /admin/api/whatsapp/access/import` (#1435): o documento de
/// export mais as flags da importacao.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessImportRequest {
    /// O documento `garraia.access-policy` (o mesmo que o export devolve).
    pub document: Value,
    /// So calcula o impacto; nada e gravado nem auditado.
    #[serde(default)]
    pub dry_run: bool,
    /// Autoriza o alargamento (algum principal GANHA capacidade). Sem isto, um
    /// import que alarga e recusado (409) com o impacto no corpo.
    #[serde(default)]
    pub confirm_widening: bool,
}

/// `GET /admin/api/whatsapp/access/export[?redact_pii=false]` — a politica
/// de acesso **sem segredo**, no formato versionado `garraia.access-policy`
/// (#1435). Nunca inclui token, cofre nem sessao.
///
/// Redigido por padrao (`…1234`, Channels/Read, como o `GET /access`). O
/// export cru traz identidades completas e e o unico reimportavel: exige
/// `Channels/Update` e fica no audit, porque e o unico caminho HTTP que
/// devolve numero de telefone inteiro.
pub async fn admin_whatsapp_access_export(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Query(query): Query<ExportQuery>,
) -> impl IntoResponse {
    let acao = if query.redact_pii {
        Action::Read
    } else {
        Action::Update
    };
    if !check_permission(admin.role, Resource::Channels, acao) {
        return proibido();
    }
    match carregar() {
        Ok((_, config)) => {
            let settings = whatsapp_linked_settings(&config);
            if !query.redact_pii {
                let guard = state.store.lock().await;
                let _ = guard.append_audit(
                    Some(&admin.user_id),
                    Some(&admin.username),
                    "export_unredacted",
                    "whatsapp_access",
                    None,
                    None,
                    extract_ip(&headers, None).as_deref(),
                    "success",
                );
            }
            (
                StatusCode::OK,
                Json(transferencia::exportar(&settings, query.redact_pii)),
            )
        }
        Err(e) => falha_de_config(e),
    }
}

/// `POST /admin/api/whatsapp/access/import` — aplica um documento exportado,
/// substituindo a politica (#1435). Channels/Update. Valida formato e versao,
/// recusa export redigido, calcula o impacto (dry-run embutido), exige
/// `confirm_widening` para alargar e grava atomico (com backup) so quando algo
/// muda.
pub async fn admin_whatsapp_access_import(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(req): Json<AccessImportRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Update) {
        return proibido();
    }
    let importada = match transferencia::parse_importacao(&req.document) {
        Ok(p) => p,
        Err(e) => return erro_com_codigo(StatusCode::BAD_REQUEST, e.to_string(), e.codigo()),
    };
    let (loader, config) = match carregar() {
        Ok(v) => v,
        Err(e) => return falha_de_config(e),
    };
    let original = config.clone();
    let mut config = config;
    let secao = config
        .channels
        .entry(CONFIG_KEY.to_string())
        .or_insert_with(|| ChannelConfig {
            channel_type: CONFIG_KEY.to_string(),
            enabled: None,
            settings: Default::default(),
        });
    if let Err(e) = transferencia::aplicar_importada(secao, &importada) {
        return erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let perfil = state.app_state.config.execution.perfil();
    let antes = whatsapp_linked_settings(&original);
    let depois = whatsapp_linked_settings(&config);
    let mudou = antes != depois;
    let diferencas = impacto::diferencas(&antes, &depois, perfil);
    let alarga = transferencia::ha_alargamento(&diferencas);
    let impact: Vec<Value> = diferencas
        .iter()
        .map(|d| {
            json!({
                "principal": d.principal, "target": d.alvo,
                "gains": d.ganha, "loses": d.perde,
            })
        })
        .collect();

    // Alargar sem autorizacao e recusado — mas so quando de fato grava; o
    // dry-run sempre devolve o preview, para o operador ver o que confirmar.
    if alarga && !req.confirm_widening && !req.dry_run {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "this import widens access for some principal; resend with confirm_widening=true",
                "error_code": "widening_requires_confirmation",
                "changed": mudou,
                "widens": true,
                "impact": impact,
                "policy_preview": visao::documento(&config, perfil, false),
            })),
        );
    }

    let mut written = false;
    let mut audit = json!({ "written": false });
    let mut backup: Option<String> = None;
    if !req.dry_run && mudou {
        // Backup para rollback antes da escrita atomica (temp+fsync+rename do
        // `save`). Best-effort: a falta de backup nao impede o import, mas o
        // caminho (quando ha) volta no corpo.
        let origem = loader.config_dir().join("config.yml");
        let destino = loader.config_dir().join("config.yml.import-bak");
        if origem.exists() {
            // A config inteira (com os segredos dos outros canais) vai para o
            // backup: pelo mesmo caminho endurecido do `save` (0600 desde o
            // `open`, sem seguir symlink), nunca por `fs::copy`.
            let copia = std::fs::read(&origem)
                .map_err(|e| garraia_common::Error::Config(e.to_string()))
                .and_then(|bytes| garraia_config::write_secret_file(&destino, &bytes));
            match copia {
                Ok(()) => backup = Some(destino.display().to_string()),
                Err(e) => {
                    tracing::warn!(erro = %e, "admin: nao consegui gravar o backup antes do import da politica");
                }
            }
        }
        if let Err(e) = loader.ensure_dirs().and_then(|()| loader.save(&config)) {
            tracing::warn!(erro = %e, "admin: nao consegui gravar o config.yml (import da politica)");
            return erro(StatusCode::INTERNAL_SERVER_ERROR, "failed to write config");
        }
        written = true;
        let evento = auditoria::Evento::novo(
            "admin_api",
            &admin.username,
            "import",
            None,
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
                tracing::warn!(erro = %e, "admin: o import foi gravado, mas o audit da politica nao");
                json!({ "written": false, "error": e.to_string() })
            }
        };
        let guard = state.store.lock().await;
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "import",
            "whatsapp_access",
            None,
            None,
            extract_ip(&headers, None).as_deref(),
            "success",
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "dry_run": req.dry_run,
            "changed": mudou,
            "written": written,
            "widens": alarga,
            "impact": impact,
            "backup": backup,
            "audit": audit,
            "policy": visao::documento(&config, perfil, false),
            "hot_reload": state.app_state.has_config_watcher(),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::ExportQuery;

    /// #1435 (auditoria F1): sem `redact_pii` na query o export HTTP sai
    /// REDIGIDO — o export cru so com o pedido explicito (e `Channels/Update`).
    #[test]
    fn export_http_e_redigido_por_padrao() {
        let sem: ExportQuery = serde_json::from_str("{}").expect("query vazia");
        assert!(sem.redact_pii);
        assert!(ExportQuery::default().redact_pii);
        let cru: ExportQuery =
            serde_json::from_str(r#"{"redact_pii":false}"#).expect("query explicita");
        assert!(!cru.redact_pii);
    }
}
