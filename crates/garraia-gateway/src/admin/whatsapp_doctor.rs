//! `GET /admin/api/whatsapp/doctor?lang=pt|en` (#1420): o "Test WhatsApp" do
//! Web Console, pelo MESMO motor do `garraia doctor whatsapp`.
//!
//! O que muda em relacao a CLI e so a **colheita** — e ela e mais completa
//! aqui, porque quem responde e o proprio gateway:
//!
//! - **vinculo:** `LinkedPaths` + `DiskFacts` + `classify` com a view REAL da
//!   ponte (`state.whatsapp_linked.bridge()`), e nao `BridgeView::Unknown`;
//! - **chave da sessao:** a mesma resolucao do boot (`SessionKey::resolve`
//!   com a passphrase do cofre do ambiente do gateway), em `spawn_blocking`
//!   porque PBKDF2 sao 600k iteracoes, e so quando ha sessao para abrir;
//! - **config:** `fatos_da_config` sobre a config VIVA (`current_config`);
//! - **gateway:** este processo — pid, bind, e as linhas do `/api/diagnostics`
//!   por chamada de funcao (`diagnostics_handler::relatorio`), sem HTTP e sem
//!   credencial para recusar.
//!
//! A classificacao, o agregado e o shape da resposta sao os da CLI
//! (`bootstrap::whatsapp_linked_doctor`): `{ status, version, lang, checks }`,
//! com `checks` identico ao `report.checks` do `--json`. Permissao:
//! `Channels/Read` (viewer le). Nada de segredo sai: a tabela so emite
//! contagens, origens e nomes — e um teste alimenta chave de gateway e de
//! provider e varre o corpo.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_channels::whatsapp_linked::health::{DiskFacts, LinkHealth, classify};
use garraia_channels::whatsapp_linked::{KeyOrigin, SessionKey, SessionStore};
use serde::Deserialize;
use serde_json::{Value, json};

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;
use crate::bootstrap::LinkedPaths;
use crate::bootstrap::whatsapp_linked_doctor as doctor;
use crate::bootstrap::whatsapp_linked_doctor::{Chave, Fatos, Gateway, Lang, LinhaViva, Sessao};
use crate::state::SharedState;

/// `?lang=pt|en`. Ausente ou desconhecido e pt-BR, o default do projeto.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DoctorQuery {
    pub lang: Option<String>,
}

/// `GET /admin/api/whatsapp/doctor` — o relatorio do doctor, em processo.
pub async fn admin_whatsapp_doctor(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Query(query): Query<DoctorQuery>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient permissions" })),
        );
    }
    let lang = query.lang.as_deref().map(Lang::parse).unwrap_or_default();
    let fatos = colher(&state.app_state).await;
    let bin = garraia_common::executavel::nome();
    let linhas = doctor::classificar(&fatos, lang, &bin);
    (StatusCode::OK, Json(relatorio(&linhas, lang)))
}

/// O corpo: o MESMO shape do `report` do `garraia doctor whatsapp --json`,
/// mais o idioma em que as frases sairam.
fn relatorio(linhas: &[doctor::Linha], lang: Lang) -> Value {
    json!({
        "status": doctor::agregado(linhas),
        "version": env!("CARGO_PKG_VERSION"),
        "lang": lang.as_str(),
        "checks": linhas,
    })
}

/// Colhe os [`Fatos`] de dentro do gateway.
pub(crate) async fn colher(app: &SharedState) -> Fatos {
    // A config VIVA: a mesma que o turno le para admitir (#1345) e a que o
    // `/api/diagnostics` usa para o portao.
    let config = app.current_config();
    let (sessao, bridge_dir) = match LinkedPaths::from_config(&config) {
        Ok(paths) => {
            let facts = DiskFacts::read(&paths.store, &paths.bridge_dir);
            // O gateway SABE a view da ponte: e ele quem a supervisiona.
            let saude = classify(&facts, app.whatsapp_linked.bridge());
            // Derivar a chave custa PBKDF2 (600k iteracoes): so quando ha
            // sessao para abrir, e fora do executor async.
            let chave = if saude == LinkHealth::NotLinked {
                None
            } else {
                Some(chave_da_sessao(paths.store.clone()).await)
            };
            (Sessao::Lida { saude, chave }, paths.bridge_dir)
        }
        // Inalcancavel com `DEFAULT_ACCOUNT` (ver `LinkedPaths::from_config`);
        // se um dia acontecer, a linha diz que o diretorio nao abriu — como
        // a CLI. `SessionError` carrega caminho e categoria, nunca conteudo.
        Err(e) => (
            Sessao::Indisponivel(e.to_string()),
            config.resolved_data_dir().join("whatsapp").join("bridge"),
        ),
    };

    // O que so o gateway sabe (ponte, provider registrado): as linhas do
    // proprio `/api/diagnostics`, por chamada de funcao.
    let ao_vivo = crate::diagnostics_handler::relatorio(app)
        .await
        .checks
        .iter()
        .map(|c| LinhaViva {
            id: c.id.to_string(),
            status: c.status.as_str().to_string(),
            detail: c.detail.clone(),
            next_step: c.next_step.clone(),
        })
        .collect();

    Fatos {
        sessao,
        bridge_dir,
        config: Some(doctor::fatos_da_config(&config)),
        gateway: Gateway {
            pid: Some(std::process::id()),
            ouvindo: true,
            host: config.gateway.host.clone(),
            porta: config.gateway.port,
            ao_vivo,
            recusou_credencial: false,
        },
    }
}

/// A chave da sessao: de onde veio e se abre o blob — a MESMA resolucao do
/// boot (`spawn_whatsapp_linked`), com a passphrase do ambiente do gateway.
///
/// Numa rota de leitura nada pode ser materializado: `SessionKey::resolve`
/// cria `session.key` quando nao ha passphrase nem arquivo, entao esse caso
/// e respondido sem chamar `resolve` — e a resposta e a mesma que a CLI da
/// depois de criar o arquivo (uma chave aleatoria nova nunca abre um blob
/// antigo): nao e do cofre e nao abre. Qualquer falha e "nao abre", nunca
/// panico, e o valor da chave nao entra em log nem em resposta.
async fn chave_da_sessao(store: SessionStore) -> Chave {
    let nao_abre = Chave {
        do_cofre: false,
        legivel: false,
    };
    tokio::task::spawn_blocking(move || {
        let passphrase = garraia_security::vault_passphrase_from_env();
        let tem_passphrase = passphrase.as_deref().is_some_and(|p| !p.is_empty());
        if !tem_passphrase && !store.dir().join("session.key").is_file() {
            return nao_abre;
        }
        match SessionKey::resolve(store.dir(), passphrase.as_deref()) {
            Ok(key) => Chave {
                do_cofre: key.origin() == KeyOrigin::VaultPassphrase,
                legivel: store.load(&key).is_ok(),
            },
            Err(_) => nao_abre,
        }
    })
    .await
    .unwrap_or(nao_abre)
}
