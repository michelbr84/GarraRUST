//! `garra_capabilities` — consulta sob demanda ao catálogo versionado de
//! funcionalidades (#1670).
//!
//! O Garra precisa CONHECER e EXPLICAR as funcionalidades da versão que está
//! executando, inclusive depois de instalação limpa (sem clone do repositório).
//! A fonte única é [`crate::tools::catalog_tool`] + o manifesto JSON
//! `assets/catalogo-funcionalidades.json`, embutido no binário via
//! [`include_str!`] — nada é lido do disco em runtime, então instalador e
//! atualizador levam o catálogo junto "de graça".
//!
//! ## Formato do catálogo (decisão de design, #1670)
//!
//! - Arquivo JSON versionado em `assets/catalogo-funcionalidades.json`,
//!   embutido com [`include_str!`] (o build falha se o arquivo-fonte não
//!   existir — é o teste de "instalação limpa" pedido na issue).
//! - **Chaves estáveis em inglês** (machine-readable: `id`, `name`,
//!   `category`, `summary`, `prerequisites`, `limitations`, `availability`,
//!   `gate`, `tool`, `channel`, `operator_only`) e **valores em pt-BR**.
//! - `availability` é o vocabulário da issue: `disponivel` (funciona numa
//!   instalação limpa), `configuravel` (existe nesta versão, mas exige
//!   configuração — `prerequisites`), `opcional` (só existe atrás de feature
//!   de compilação ou interruptor de config — o campo `gate` nomeia o
//!   interruptor). Um recurso que exige configuração NUNCA aparece como
//!   `disponivel`; um recurso `opcional` SEMPRE carrega `gate` — os testes
//!   abaixo fecham as duas regras.
//! - `catalog_version` acompanha a versão do workspace (`CARGO_PKG_VERSION`,
//!   que os crates herdam via `version.workspace = true`); um teste confere a
//!   igualdade e um segundo confere contra o `Cargo.toml` raiz.
//! - O catálogo descreve o que ESTA versão entrega. Estado da instalação
//!   (ativo, configurado, offline, agora) vem do runtime — `garra_status` —
//!   e nunca deste arquivo.
//!
//! ## Consumo de contexto baixo (#1670)
//!
//! A tool NUNCA despeja o catálogo inteiro: sem filtro devolve só um resumo
//! (contagens por categoria + como consultar); com filtro (`name`/`category`)
//! devolve no máximo [`LIMITE_PADRAO`] itens por chamada ([`LIMITE_MAXIMO`]
//! no teto), com `total_matches`/`truncated` para o modelo paginar.
//!
//! ## Turno restrito (#1670, "permissões efetivas")
//!
//! Mesma decisão de operador do `garra_status`
//! ([`crate::channels_view::sessao_do_operador`] + o portão do turno): em
//! turno restrito os itens `operator_only` (superfícies de administração do
//! host) não são divulgados, e a resposta diz quantos foram retidos — nunca
//! "não existe".

use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::Result;
use serde::Deserialize;

use crate::state::AppState;

/// O manifesto versionado, embutido no binário. `include_str!` é compile-time:
/// se `assets/catalogo-funcionalidades.json` não existir, o build falha — e é
/// isso que garante que uma instalação limpa (só o binário) carrega o
/// catálogo.
pub const CATALOGO_FONTE: &str = include_str!("../../assets/catalogo-funcionalidades.json");

/// Itens por resposta quando o chamador não passa `limit`.
pub const LIMITE_PADRAO: usize = 10;

/// Teto de itens por resposta (o `limit` do chamador é clampado aqui).
pub const LIMITE_MAXIMO: usize = 50;

/// Teto de caracteres do resumo sem filtro (#1670, teste de consumo baixo).
/// O resumo é contagem + instrução de consulta, nunca detalhe de item.
pub const TETO_DO_RESUMO: usize = 2_000;

/// O que um recurso do catálogo entrega NESTA versão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    /// Funciona numa instalação limpa, sem configuração.
    Disponivel,
    /// Existe nesta versão, mas exige configuração (`prerequisites`).
    Configuravel,
    /// Só existe atrás de feature de compilação ou interruptor de config
    /// (o campo `gate` nomeia o interruptor).
    Opcional,
}

/// Uma entrada do catálogo. Chaves em inglês (estáveis, machine-readable),
/// valores em pt-BR — ver o módulo.
#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    /// Identificador estável (kebab-case).
    pub id: String,
    /// Nome legível.
    pub name: String,
    /// Uma das categorias declaradas em [`Catalogo::categories`].
    pub category: String,
    /// Descrição curta do que o recurso oferece.
    pub summary: String,
    /// O que precisa estar instalado/configurado ANTES de usar.
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// Limitações do recurso nesta versão.
    #[serde(default)]
    pub limitations: Vec<String>,
    /// Vocabulário da issue: `disponivel` | `configuravel` | `opcional`.
    pub availability: Availability,
    /// Marcador de feature/config para recurso opcional (ex.:
    /// `feature:tls`, `config:agent.sandbox.mode`). Presente IFF o recurso é
    /// opcional — os testes fecham as duas direções.
    #[serde(default)]
    pub gate: Option<String>,
    /// Nome da tool do runtime que materializa o recurso, quando aplicável.
    #[serde(default)]
    pub tool: Option<String>,
    /// Id do canal em [`crate::channels_view::KNOWN_CHANNELS`], quando o
    /// recurso é um canal de mensagens.
    #[serde(default)]
    pub channel: Option<String>,
    /// Superfície de operação/administração do host: em turno restrito o item
    /// não é divulgado.
    #[serde(default)]
    pub operator_only: bool,
}

/// O catálogo embutido: versão + itens.
#[derive(Debug, Deserialize)]
pub struct Catalogo {
    /// Identificador do esquema (`garra.funcionalidades/v1`).
    pub schema: String,
    /// Versão do catálogo — casa com a versão do binário.
    pub catalog_version: String,
    /// Idioma dos valores (pt-BR).
    pub language: String,
    /// Categorias válidas para o filtro `category`.
    pub categories: Vec<String>,
    /// Os recursos que ESTA versão entrega.
    pub items: Vec<Item>,
}

/// O catálogo parseado uma vez do [`CATALOGO_FONTE`].
pub fn catalogo() -> &'static Catalogo {
    static CATALOGO: OnceLock<Catalogo> = OnceLock::new();
    CATALOGO.get_or_init(|| {
        serde_json::from_str(CATALOGO_FONTE).expect(
            "assets/catalogo-funcionalidades.json inválido (veja os testes de catalog_tool)",
        )
    })
}

/// A versão do catálogo — a mesma do workspace (teste: `versao_do_catalogo`).
pub fn versao_do_catalogo() -> &'static str {
    &catalogo().catalog_version
}

/// Filtro de consulta ao catálogo.
#[derive(Debug, Default, Clone, Copy)]
pub struct Filtro<'a> {
    /// Substring (case-insensitive) do `id` ou do `name`.
    pub name: Option<&'a str>,
    /// Categoria exata (case-insensitive).
    pub category: Option<&'a str>,
    /// Itens por resposta (clampado em 1..=[`LIMITE_MAXIMO`]).
    pub limit: Option<usize>,
    /// Turno restrito: retém os itens `operator_only`.
    pub restrito: bool,
}

fn item_json(item: &Item) -> serde_json::Value {
    serde_json::json!({
        "id": item.id,
        "name": item.name,
        "category": item.category,
        "availability": match item.availability {
            Availability::Disponivel => "disponivel",
            Availability::Configuravel => "configuravel",
            Availability::Opcional => "opcional",
        },
        "summary": item.summary,
        "prerequisites": item.prerequisites,
        "limitations": item.limitations,
        "gate": item.gate,
        "tool": item.tool,
        "channel": item.channel,
    })
}

/// Resumo SEM filtro (#1670, consumo baixo): versão, contagens por categoria
/// e como consultar — nunca detalhe de item.
pub fn resumo() -> serde_json::Value {
    let cat = catalogo();
    let categories: Vec<serde_json::Value> = cat
        .categories
        .iter()
        .map(|c| {
            let n = cat.items.iter().filter(|i| i.category == *c).count();
            serde_json::json!({ "id": c, "count": n })
        })
        .collect();
    serde_json::json!({
        "catalog_version": cat.catalog_version,
        "schema": cat.schema,
        "total_items": cat.items.len(),
        "categories": categories,
        "usage": "this summary intentionally omits feature details to keep the context small: \
                  query with `name` (substring of the feature id/name) and/or `category` \
                  (one of the ids above); each call returns at most 10 items (`limit`, max 50). \
                  `garra_status` reports the LIVE state of this installation; this catalog \
                  reports what THIS version ships."
    })
}

/// Consulta filtrada: itens batidos, ordenados por `id`, cortados no limite.
pub fn consultar(f: &Filtro<'_>) -> serde_json::Value {
    let cat = catalogo();
    let limite = f.limit.unwrap_or(LIMITE_PADRAO).clamp(1, LIMITE_MAXIMO);
    let nome = f.name.map(str::to_lowercase);
    let mut retidos = 0usize;
    let mut batidos: Vec<&Item> = Vec::new();
    for item in &cat.items {
        if f.restrito && item.operator_only {
            retidos += 1;
            continue;
        }
        if let Some(c) = f.category
            && !item.category.eq_ignore_ascii_case(c)
        {
            continue;
        }
        if let Some(n) = &nome {
            let id = item.id.to_lowercase();
            let name = item.name.to_lowercase();
            if !id.contains(n.as_str()) && !name.contains(n.as_str()) {
                continue;
            }
        }
        batidos.push(item);
    }
    batidos.sort_by(|a, b| a.id.cmp(&b.id));
    let total = batidos.len();
    let itens: Vec<serde_json::Value> = batidos.iter().take(limite).map(|i| item_json(i)).collect();
    serde_json::json!({
        "catalog_version": cat.catalog_version,
        "query": { "name": f.name, "category": f.category },
        "total_matches": total,
        "returned": itens.len(),
        "truncated": total > itens.len(),
        // `null` quando nada foi retido — como `withheld_means` do
        // `garra_status`, a chave existe sempre.
        "withheld_operator_only": (retidos > 0).then_some(retidos),
        "items": itens,
    })
}

/// O bloco `catalog` do relatório do `garra_status` (função autocontida,
/// #1670): versão do catálogo embutido e como consultá-lo. Enlace pequeno de
/// propósito — o catálogo inteiro só vai para a conversa pela tool
/// `garra_capabilities`, com filtro e limite.
pub fn enlace_do_catalogo() -> serde_json::Value {
    let cat = catalogo();
    serde_json::json!({
        "catalog_version": cat.catalog_version,
        "total_items": cat.items.len(),
        "categories": cat.categories,
        "query_tool": "garra_capabilities",
        "means": "the feature catalog of THIS binary version is queried on demand with the \
                  `garra_capabilities` tool (filters: `name`, `category`) — it is never \
                  injected into the prompt. `garra_status` shows the LIVE state of this \
                  installation; the catalog shows what this version ships."
    })
}

/// A tool de consulta ao catálogo. Segura o `AppState` só para aplicar a
/// MESMA decisão de operador do `garra_status` (turno restrito retém
/// `operator_only`); o catálogo em si é estático e vem do binário.
pub struct GarraCapabilitiesTool {
    /// Pelo mesmo motivo do `GarraStatusTool`: `AppState` é dono do runtime,
    /// que é dono desta tool — um `Arc` de volta fecharia ciclo.
    state: Weak<AppState>,
}

impl GarraCapabilitiesTool {
    pub fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::downgrade(state),
        }
    }
}

#[async_trait]
impl Tool for GarraCapabilitiesTool {
    fn name(&self) -> &str {
        "garra_capabilities"
    }

    fn description(&self) -> &str {
        "Queries the versioned feature catalog embedded in THIS binary: what this Garra version \
         ships — each feature with a short description, prerequisites, limitations and \
         availability (`disponivel` = works in a clean install, `configuravel` = exists but \
         needs configuration, `opcional` = only behind a compile-time feature or config switch; \
         `gate` names the switch). Filter by `name` (substring of the feature id/name) and/or \
         `category` (chat, files, execution, web, channels, mcp, memory-scheduling, devices, \
         diagnostics-admin); `limit` caps returned items (default 10, max 50). Without filters \
         it returns ONLY a summary with per-category counts — never the whole catalog. The \
         catalog describes the VERSION; for the live state of this installation (channels up, \
         providers, tools allowed in this turn) use `garra_status` instead."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "substring of the feature id or name (case-insensitive)"
                },
                "category": {
                    "type": "string",
                    "description": "one of: chat, files, execution, web, channels, mcp, memory-scheduling, devices, diagnostics-admin"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "description": "max items in the response (default 10)"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let Some(state) = self.state.upgrade() else {
            return Ok(ToolOutput::error("gateway state is gone (shutting down?)"));
        };

        // Mesma decisão de operador do `garra_status` (#1670, "permissões
        // efetivas"): portão do turno OU sessão não provadamente do operador
        // → restrito (fail-closed).
        let restrito = garraia_agents::tools::turn_tools::turno_restrito().unwrap_or(false)
            || !crate::channels_view::sessao_do_operador(&state, &ctx.session_id).await;

        let name = match input.get("name") {
            None => None,
            Some(serde_json::Value::String(s)) => Some(s.as_str()),
            Some(_) => {
                return Ok(ToolOutput::error(
                    "argumento 'name' inválido — garra_capabilities requer string. \
                     Reenvie a chamada com o argumento correto.",
                ));
            }
        };
        let category = match input.get("category") {
            None => None,
            Some(serde_json::Value::String(s)) => Some(s.as_str()),
            Some(_) => {
                return Ok(ToolOutput::error(
                    "argumento 'category' inválido — garra_capabilities requer string. \
                     Reenvie a chamada com o argumento correto.",
                ));
            }
        };
        let limit = match input.get("limit") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Number(n)) => match n.as_u64() {
                Some(v) => Some(v as usize),
                None => {
                    return Ok(ToolOutput::error(
                        "argumento 'limit' inválido — garra_capabilities requer inteiro \
                         entre 1 e 50. Reenvie a chamada com o argumento correto.",
                    ));
                }
            },
            Some(_) => {
                return Ok(ToolOutput::error(
                    "argumento 'limit' inválido — garra_capabilities requer inteiro \
                     entre 1 e 50. Reenvie a chamada com o argumento correto.",
                ));
            }
        };

        // Sem filtro NUNCA sai o catálogo inteiro: o resumo é contagem +
        // instrução de consulta (#1670, consumo baixo).
        let resposta = if name.is_none() && category.is_none() {
            resumo()
        } else {
            consultar(&Filtro {
                name,
                category,
                limit,
                restrito,
            })
        };
        let text = serde_json::to_string_pretty(&resposta).unwrap_or_else(|_| resposta.to_string());
        Ok(ToolOutput::success(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;

    fn config_no(dir: &std::path::Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    /// Estado com `sessions.db`, como o boot monta quando há banco — é o que
    /// faz `sessao_do_operador` olhar a superfície gravada da sessão.
    fn state_com_banco(dir: &std::path::Path) -> Arc<AppState> {
        let store = garraia_db::SessionStore::open(&dir.join("sessions.db")).expect("store");
        let store = Arc::new(tokio::sync::Mutex::new(store));
        let mut st = AppState::with_config_dir(
            config_no(dir),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir,
        );
        st.set_session_store(Arc::clone(&store));
        st.set_chat_session_manager(Arc::new(garraia_db::ChatSessionManager::new(store)));
        Arc::new(st)
    }

    fn ctx_na_sessao(session_id: &str) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        }
    }

    async fn executa(st: &Arc<AppState>, session_id: &str, input: serde_json::Value) -> String {
        let tool = GarraCapabilitiesTool::new(st);
        let out = tool
            .execute(&ctx_na_sessao(session_id), input)
            .await
            .expect("executa");
        assert!(!out.is_error, "{}", out.content);
        out.content
    }

    // ─── (a) versão ────────────────────────────────────────────────────────

    /// O `catalog_version` casa com a versão do workspace: a do crate (herdada
    /// via `version.workspace = true`) e a do `Cargo.toml` raiz.
    #[test]
    fn versao_do_catalogo_casa_com_a_versao_do_workspace() {
        assert_eq!(versao_do_catalogo(), env!("CARGO_PKG_VERSION"));
        let raiz = include_str!("../../../../Cargo.toml");
        let secao = &raiz[raiz
            .find("[workspace.package]")
            .expect("Cargo.toml raiz tem [workspace.package]")..];
        let linha = secao
            .lines()
            .find(|l| l.trim_start().starts_with("version"))
            .expect("[workspace.package] declara version");
        let versao = linha
            .split('"')
            .nth(1)
            .expect("version = \"...\" no Cargo.toml raiz");
        assert_eq!(
            versao_do_catalogo(),
            versao,
            "catalog_version diverge da versão do workspace"
        );
    }

    // ─── (b) instalação limpa ──────────────────────────────────────────────

    /// O conteúdo vem do binário: [`CATALOGO_FONTE`] é `include_str!` (o build
    /// FALHA se `assets/catalogo-funcionalidades.json` não existir), e este
    /// teste prova que o embutido é o MESMO arquivo versionado no repo — sem
    /// nenhuma leitura de disco em runtime.
    #[test]
    fn conteudo_embutido_e_o_arquivo_fonte_do_build() {
        assert!(!CATALOGO_FONTE.is_empty(), "catálogo embutido vazio");
        let caminho = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/catalogo-funcionalidades.json"
        );
        let disco =
            std::fs::read_to_string(caminho).expect("arquivo-fonte do catálogo existe no build");
        assert_eq!(
            CATALOGO_FONTE, disco,
            "o embutido divergiu do arquivo-fonte versionado"
        );
        let fonte: Catalogo = serde_json::from_str(&disco).expect("fonte parseia");
        assert_eq!(catalogo().items.len(), fonte.items.len());
    }

    // ─── (c) opcionais: marcador de feature/config aparece e não mente ─────

    /// Todo recurso `opcional` carrega `gate` (o marcador APARECE); todo
    /// recurso com `gate` não é `disponivel` (não MENTE); e todo recurso que
    /// exige configuração nunca é `disponivel` — é `configuravel` ou
    /// `opcional` (critério de aceite da #1670).
    #[test]
    fn opcionais_marcados_e_nao_mentem() {
        for item in &catalogo().items {
            if item.availability == Availability::Opcional {
                assert!(
                    item.gate.as_deref().is_some_and(|g| !g.trim().is_empty()),
                    "opcional sem gate: {}",
                    item.id
                );
            }
            if item.gate.is_some() {
                assert_ne!(
                    item.availability,
                    Availability::Disponivel,
                    "com gate e marcado como disponivel: {}",
                    item.id
                );
            }
            if !item.prerequisites.is_empty() {
                assert_ne!(
                    item.availability,
                    Availability::Disponivel,
                    "exige configuração e aparece como disponivel: {}",
                    item.id
                );
            }
        }
        // Os três exemplos da issue estão presentes e marcados.
        for id in ["sandbox", "tls", "mcp-http"] {
            let item = catalogo()
                .items
                .iter()
                .find(|i| i.id == id)
                .unwrap_or_else(|| panic!("item {id} ausente do catálogo"));
            assert!(
                item.gate.as_deref().is_some_and(|g| !g.trim().is_empty()),
                "{id} sem marcador de feature/config"
            );
            assert!(!item.prerequisites.is_empty(), "{id} sem pré-requisito");
        }
    }

    /// Recurso que exige configuração aparece como `configuravel`, nunca
    /// `disponivel` (critério de aceite da #1670) — fechado por amostra nos
    /// recursos que dependem de credencial/integração.
    #[test]
    fn recurso_que_exige_config_nunca_aparece_como_disponivel() {
        for id in [
            "telegram_send",
            "whatsapp_linked",
            "bash",
            "memoria",
            "mcp-client",
        ] {
            let item = catalogo()
                .items
                .iter()
                .find(|i| i.id == id)
                .unwrap_or_else(|| panic!("item {id} ausente do catálogo"));
            assert_eq!(
                item.availability,
                Availability::Configuravel,
                "{id} deveria ser configuravel"
            );
            assert!(!item.prerequisites.is_empty(), "{id} sem pré-requisito");
        }
    }

    /// Consistência (critério de aceite da #1670): todo recurso citado existe
    /// — ids únicos, categoria declarada, `tool` registrável e canal
    /// conhecido do runtime.
    #[test]
    fn toda_referencia_do_catalogo_existe() {
        let cat = catalogo();
        let mut vistos: Vec<&str> = Vec::new();
        for item in &cat.items {
            assert!(
                !vistos.contains(&item.id.as_str()),
                "id repetido: {}",
                item.id
            );
            vistos.push(&item.id);
            assert!(
                cat.categories.contains(&item.category),
                "categoria não declarada em {}: {}",
                item.id,
                item.category
            );
            if let Some(tool) = &item.tool {
                let conhecida = tool == "garra_capabilities"
                    || garraia_agents::capacidades::NATIVAS
                        .iter()
                        .any(|(n, _)| n == tool);
                assert!(
                    conhecida,
                    "tool citada e inexistente em {}: {tool}",
                    item.id
                );
            }
            if let Some(canal) = &item.channel {
                assert!(
                    crate::channels_view::KNOWN_CHANNELS
                        .iter()
                        .any(|(id, ..)| id == canal),
                    "canal citado e inexistente em {}: {canal}",
                    item.id
                );
            }
        }
    }

    // ─── (d) integração: filtros por categoria/nome + garra_status ─────────

    /// A tool devolve filtro por categoria e por nome, com contagem e corte.
    #[tokio::test]
    async fn consulta_filtra_por_categoria_e_nome_e_limita() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com_banco(dir.path());
        let texto = executa(&st, "sessao-web", serde_json::json!({"category": "files"})).await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        assert_eq!(json["catalog_version"], versao_do_catalogo());
        let itens = json["items"].as_array().expect("items");
        assert!(!itens.is_empty(), "{texto}");
        assert!(itens.iter().all(|i| i["category"] == "files"), "{texto}");
        assert_eq!(json["returned"], itens.len());
        assert_eq!(json["total_matches"], itens.len());

        let texto = executa(&st, "sessao-web", serde_json::json!({"name": "file_read"})).await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        let itens = json["items"].as_array().expect("items");
        assert_eq!(itens.len(), 1, "{texto}");
        assert_eq!(itens[0]["id"], "file_read");
        assert_eq!(itens[0]["tool"], "file_read");

        // `limit` é respeitado e `truncated` sinaliza a paginação.
        let texto = executa(
            &st,
            "sessao-web",
            serde_json::json!({"category": "channels", "limit": 2}),
        )
        .await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        assert_eq!(json["returned"], 2, "{texto}");
        assert_eq!(json["truncated"], true, "{texto}");
        assert!(json["total_matches"].as_u64().unwrap_or(0) > 2, "{texto}");

        // Categoria desconhecida não vaza tudo: resposta vazia e explicada.
        let texto = executa(
            &st,
            "sessao-web",
            serde_json::json!({"category": "inventada"}),
        )
        .await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        assert_eq!(json["total_matches"], 0, "{texto}");
        assert_eq!(json["items"].as_array().expect("items").len(), 0);
    }

    /// O `garra_status` ganha a seção/enlace do catálogo no turno aberto
    /// (#1670): versão do catálogo + a tool de consulta.
    #[tokio::test]
    async fn garra_status_traz_a_secao_do_catalogo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com_banco(dir.path());
        st.hydrate_session_history("sessao-web", Some("web"), None)
            .await;
        let status =
            crate::tools::GarraStatusTool::new(&st, crate::push_channels::PushMounted::default());
        let out = status
            .execute(&ctx_na_sessao("sessao-web"), serde_json::json!({}))
            .await
            .expect("executa");
        assert!(!out.is_error, "{}", out.content);
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        let secao = &json["catalog"];
        assert_eq!(secao["catalog_version"], versao_do_catalogo());
        assert_eq!(secao["query_tool"], "garra_capabilities");
        assert_eq!(secao["total_items"], catalogo().items.len());
    }

    // ─── (e) consumo baixo ─────────────────────────────────────────────────

    /// Sem argumento a tool NÃO despeja o catálogo inteiro: resumo limitado
    /// em caracteres, sem campo de detalhe de item.
    #[tokio::test]
    async fn consulta_sem_argumento_nao_despeja_o_catalogo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com_banco(dir.path());
        let texto = executa(&st, "sessao-web", serde_json::json!({})).await;
        assert!(
            texto.len() < TETO_DO_RESUMO,
            "resumo sem filtro tem {} caracteres (teto {TETO_DO_RESUMO})",
            texto.len()
        );
        assert!(!texto.contains("prerequisites"), "{texto}");
        assert!(!texto.contains("limitations"), "{texto}");
        assert!(!texto.contains("\"items\""), "{texto}");
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        assert_eq!(json["catalog_version"], versao_do_catalogo());
        assert!(json["total_items"].as_u64().unwrap_or(0) > 0);
    }

    /// Com filtro a resposta também é cortada: o teto de itens vale sempre.
    #[tokio::test]
    async fn resposta_com_filtro_respeita_o_teto_de_itens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com_banco(dir.path());
        let texto = executa(
            &st,
            "sessao-web",
            serde_json::json!({"category": "channels", "limit": 999}),
        )
        .await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        let itens = json["items"].as_array().expect("items");
        assert!(itens.len() <= LIMITE_MAXIMO, "{} itens", itens.len());
    }

    // ─── permissões efetivas: remoto recebe só o subconjunto ───────────────

    /// Turno restrito (sessão remota) não recebe os itens `operator_only`, e a
    /// resposta diz quantos retidos — nunca "não existe".
    #[tokio::test]
    async fn turno_restrito_recebe_so_o_subconjunto_permitido() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com_banco(dir.path());
        st.hydrate_session_history("discord-123456789", Some("discord"), Some("alguem"))
            .await;
        let filtro = serde_json::json!({"category": "diagnostics-admin"});
        let texto = executa(&st, "discord-123456789", filtro.clone()).await;
        let json: serde_json::Value = serde_json::from_str(&texto).expect("json");
        let itens = json["items"].as_array().expect("items");
        assert!(!itens.is_empty(), "{texto}");
        for item in itens {
            assert_ne!(item["id"], "doctor-cli", "{texto}");
            assert_ne!(item["id"], "instalacao-atualizacao", "{texto}");
            assert_ne!(item["id"], "diagnostics-api", "{texto}");
            assert_ne!(item["id"], "tls", "{texto}");
        }
        assert!(
            json["withheld_operator_only"].as_u64().unwrap_or(0) > 0,
            "nada foi retido: {texto}"
        );

        // A mesma consulta numa sessão local (web) vê o conjunto inteiro.
        st.hydrate_session_history("sessao-web", Some("web"), None)
            .await;
        let texto_aberto = executa(&st, "sessao-web", filtro).await;
        let json_aberto: serde_json::Value = serde_json::from_str(&texto_aberto).expect("json");
        let ids: Vec<&str> = json_aberto["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|i| i["id"].as_str())
            .collect();
        assert!(ids.contains(&"doctor-cli"), "{texto_aberto}");
        assert!(ids.contains(&"instalacao-atualizacao"), "{texto_aberto}");
    }
}
