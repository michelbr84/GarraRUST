//! `/api/chatgpt/*` — o cartao "Continuar com ChatGPT" da pagina Providers
//! (Sign in with ChatGPT, provider `openai-codex`).
//!
//! O login roda **neste processo**: o gateway abre o callback loopback em
//! `127.0.0.1:1455` (ou uma porta livre) e devolve a URL de autorizacao, que
//! o painel abre numa aba. Por isso o login pelo painel so funciona com o
//! navegador na mesma maquina do gateway; num gateway remoto, entre pela CLI
//! na maquina local e copie as credenciais (guia `self-hosted-vms` da OpenAI).
//!
//! Nada aqui devolve token, codigo ou `id_token`: so a visao publica da conta
//! ([`ContaChatGpt`]) e a URL de autorizacao (que nao carrega segredo — o
//! `id_token_hint` so vai na reautorizacao e e o proprio dono quem abre).

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use futures::StreamExt;
use garraia_agents::openai_codex::{
    ArmazemDeCredenciais, ESFORCO_PADRAO, LoginEmAndamento, MODELO_PADRAO, OpcoesDeLogin,
    OpenAiCodexProvider, esforco_normalizado, login,
};
use garraia_agents::providers::{
    ChatMessage, ChatRole, LlmProvider, LlmRequest, MessagePart, StreamEvent,
};
use garraia_config::ConfigLoader;
use garraia_config::chatgpt::{AjusteDoChatGpt, aplicar, entrada_existente};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::SharedState;

/// O login em andamento (um por gateway) e o resultado do ultimo.
#[derive(Default)]
struct EstadoDoLogin {
    andamento: Option<LoginEmAndamento>,
    tornar_padrao: bool,
    resultado: Option<Value>,
}

static LOGIN: OnceLock<tokio::sync::Mutex<EstadoDoLogin>> = OnceLock::new();

fn login_global() -> &'static tokio::sync::Mutex<EstadoDoLogin> {
    LOGIN.get_or_init(Default::default)
}

fn armazem() -> ArmazemDeCredenciais {
    ArmazemDeCredenciais::new(ConfigLoader::openai_codex_credentials_dir())
}

fn erro(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"ok": false, "error": msg.into()})))
}

/// A entrada `openai-codex` do config.yml atual (chave, modelo, esforco).
fn entrada_da_config() -> Option<(String, String, String, bool)> {
    let config = ConfigLoader::new().ok()?.load().ok()?;
    let chave = entrada_existente(&config)?;
    let e = config.llm.get(&chave)?;
    let modelo = e.model.clone().unwrap_or_else(|| MODELO_PADRAO.into());
    let esforco = e
        .extra
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| ESFORCO_PADRAO.into());
    let padrao = config.agent.default_provider.as_deref() == Some(chave.as_str());
    Some((chave, modelo, esforco, padrao))
}

/// O navegador esta na mesma maquina? (o callback loopback so chega la).
fn acesso_local(headers: &HeaderMap) -> bool {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let nome = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(nome, "127.0.0.1" | "localhost" | "[::1]" | "")
}

/// Grava a entrada (e o padrao, se pedido) e registra/atualiza o provider
/// no runtime, sem reiniciar o gateway.
fn aplicar_e_registrar(
    state: &SharedState,
    ajuste: &AjusteDoChatGpt<'_>,
) -> anyhow::Result<(String, String, String)> {
    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;
    let mut config = loader.load().map_err(|e| {
        anyhow::anyhow!("o config.yml atual nao pode ser lido ({e}); corrija antes de gravar")
    })?;
    let chave = aplicar(&mut config, ajuste);
    loader.save(&config)?;
    let entrada = &config.llm[&chave];
    let modelo = entrada
        .model
        .clone()
        .unwrap_or_else(|| MODELO_PADRAO.into());
    let esforco = entrada
        .extra
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or(ESFORCO_PADRAO)
        .to_string();
    let cliente = garraia_agents::cliente_llm::cliente_do_llm(
        &garraia_agents::cliente_llm::cliente_compartilhado(state.config.timeouts.llm.default_secs),
        entrada.timeout_secs(),
    );
    let provider = OpenAiCodexProvider::new(
        ConfigLoader::openai_codex_credentials_dir(),
        Some(modelo.clone()),
        Some(esforco.clone()),
    )
    .with_client(cliente)
    .with_name(chave.clone());
    state.agents.upsert_provider(Arc::new(provider));
    if ajuste.tornar_padrao {
        state.agents.set_default_provider_id(&chave);
    }
    Ok((chave, modelo, esforco))
}

/// GET /api/chatgpt/status
pub async fn status(State(state): State<SharedState>, headers: HeaderMap) -> Json<Value> {
    let a = armazem();
    let mut guarda = login_global().lock().await;
    if guarda.andamento.as_ref().is_some_and(|l| l.terminou())
        && let Some(l) = guarda.andamento.take()
    {
        let tornar_padrao = guarda.tornar_padrao;
        guarda.resultado = Some(match l.concluir().await {
            Ok(r) => {
                let mut v =
                    json!({"estado": "ok", "conta": r.conta, "registro_novo": r.registro_novo});
                if r.conta.plano_habilitado {
                    // Cria a entrada com o modelo preferencial (sem trocar a
                    // escolha da pessoa se ja existir) e, se pedido, o padrao.
                    match aplicar_e_registrar(
                        &state,
                        &AjusteDoChatGpt {
                            tornar_padrao,
                            ..Default::default()
                        },
                    ) {
                        Ok((chave, modelo, esforco)) => {
                            v["config"] = json!({"entrada": chave, "modelo": modelo, "esforco": esforco, "padrao": tornar_padrao});
                        }
                        Err(e) => {
                            v["aviso"] =
                                json!(format!("login ok, mas a config nao foi gravada: {e}"))
                        }
                    }
                } else {
                    v["estado"] = json!("plano_negado");
                }
                v
            }
            Err(e) => json!({"estado": "erro", "mensagem": e.to_string()}),
        });
    }
    let login = if guarda.andamento.is_some() {
        json!({"estado": "aguardando"})
    } else {
        guarda
            .resultado
            .clone()
            .unwrap_or_else(|| json!({"estado": "inativo"}))
    };
    drop(guarda);

    let conta = a.conta_ativa();
    let estado_conta = match &conta {
        None => "desconectado",
        Some(c) if !c.conectado => "sessao_expirada",
        Some(c) if !c.plano_habilitado => "plano_pendente",
        Some(_) => "conectado",
    };
    let config = entrada_da_config();
    let registrado = config
        .as_ref()
        .is_some_and(|(k, ..)| state.agents.get_provider(k).is_some());
    let padrao_runtime = config
        .as_ref()
        .is_some_and(|(k, ..)| state.agents.default_provider_id().as_deref() == Some(k.as_str()));
    Json(json!({
        "estado": estado_conta,
        "conta": conta,
        "contas": a.contas(),
        "login": login,
        "config": config.map(|(entrada, modelo, esforco, padrao)| json!({
            "entrada": entrada, "modelo": modelo, "esforco": esforco, "padrao": padrao,
        })),
        "runtime": {"registrado": registrado, "padrao": padrao_runtime},
        "preferencial": {"modelo": MODELO_PADRAO, "esforco": ESFORCO_PADRAO},
        "acesso_local": acesso_local(&headers),
        "uso_url": garraia_agents::openai_codex::PAGINA_DE_USO,
    }))
}

#[derive(Deserialize, Default)]
pub struct PedidoDeLogin {
    #[serde(default)]
    pub nova_conta: bool,
    #[serde(default = "verdadeiro")]
    pub padrao: bool,
}

fn verdadeiro() -> bool {
    true
}

/// POST /api/chatgpt/login — abre o callback e devolve a URL de autorizacao.
pub async fn iniciar_login(
    headers: HeaderMap,
    body: Option<Json<PedidoDeLogin>>,
) -> (StatusCode, Json<Value>) {
    if !acesso_local(&headers) {
        return erro(
            StatusCode::CONFLICT,
            "o login pelo painel exige o navegador na mesma maquina do gateway (o callback e 127.0.0.1). \
             Num gateway remoto, rode `garra chatgpt login` no computador local e copie as credenciais.",
        );
    }
    let pedido = body.map(|Json(b)| b).unwrap_or_default();
    let mut guarda = login_global().lock().await;
    if let Some(anterior) = guarda.andamento.take() {
        anterior.cancelar();
    }
    match login::iniciar(
        &armazem(),
        OpcoesDeLogin {
            nova_conta: pedido.nova_conta,
            ..Default::default()
        },
    )
    .await
    {
        Ok(l) => {
            let url = l.url.clone();
            let registro_novo = l.registro_novo;
            guarda.andamento = Some(l);
            guarda.tornar_padrao = pedido.padrao;
            guarda.resultado = None;
            (
                StatusCode::OK,
                Json(json!({"ok": true, "url": url, "registro_novo": registro_novo})),
            )
        }
        Err(e) => erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// DELETE /api/chatgpt/login — cancela o login em andamento.
pub async fn cancelar_login() -> Json<Value> {
    let mut guarda = login_global().lock().await;
    if let Some(l) = guarda.andamento.take() {
        l.cancelar();
    }
    guarda.resultado = Some(json!({"estado": "inativo"}));
    Json(json!({"ok": true}))
}

/// POST /api/chatgpt/logout
pub async fn logout() -> (StatusCode, Json<Value>) {
    let a = armazem();
    if a.conta_ativa().is_none() {
        return (
            StatusCode::OK,
            Json(json!({"ok": true, "revogacao_confirmada": true})),
        );
    }
    match a.sair().await {
        Ok(confirmada) => (
            StatusCode::OK,
            Json(json!({"ok": true, "revogacao_confirmada": confirmada})),
        ),
        Err(e) => erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct PedidoDeConta {
    pub client_id: String,
}

/// POST /api/chatgpt/usar — troca a conta em uso.
pub async fn usar(Json(p): Json<PedidoDeConta>) -> (StatusCode, Json<Value>) {
    match armazem().ativar(p.client_id.trim()) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "conta": armazem().conta_ativa()})),
        ),
        Err(e) => erro(StatusCode::NOT_FOUND, e.to_string()),
    }
}

/// GET /api/chatgpt/modelos — catalogo da conta em uso.
pub async fn modelos() -> (StatusCode, Json<Value>) {
    let p = OpenAiCodexProvider::new(ConfigLoader::openai_codex_credentials_dir(), None, None);
    match p.listar_modelos().await {
        Ok(lista) => {
            let preferencial_disponivel = lista.iter().any(|m| m.slug == MODELO_PADRAO);
            (
                StatusCode::OK,
                Json(
                    json!({"ok": true, "modelos": lista, "preferencial": MODELO_PADRAO,
                    "preferencial_disponivel": preferencial_disponivel}),
                ),
            )
        }
        Err(e) => erro(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct PedidoDeConfig {
    #[serde(default)]
    pub modelo: Option<String>,
    #[serde(default)]
    pub esforco: Option<String>,
    #[serde(default)]
    pub padrao: bool,
}

/// POST /api/chatgpt/configurar — grava modelo/esforco (e o padrao) e
/// aplica no runtime na hora.
pub async fn configurar(
    State(state): State<SharedState>,
    Json(p): Json<PedidoDeConfig>,
) -> (StatusCode, Json<Value>) {
    if let Some(e) = p.esforco.as_deref()
        && esforco_normalizado(e).is_none()
    {
        return erro(
            StatusCode::BAD_REQUEST,
            format!("reasoning effort invalido: '{e}'"),
        );
    }
    let ajuste = AjusteDoChatGpt {
        modelo: p.modelo.as_deref(),
        esforco: p.esforco.as_deref(),
        tornar_padrao: p.padrao,
    };
    match aplicar_e_registrar(&state, &ajuste) {
        Ok((entrada, modelo, esforco)) => (
            StatusCode::OK,
            Json(
                json!({"ok": true, "entrada": entrada, "modelo": modelo, "esforco": esforco, "padrao": p.padrao}),
            ),
        ),
        Err(e) => erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct PedidoDeTeste {
    #[serde(default)]
    pub modelo: Option<String>,
}

/// POST /api/chatgpt/testar — inferencia curta e REAL (nao e mock) com a
/// conta em uso: prova login, plano, modelo, esforco e streaming.
pub async fn testar(body: Option<Json<PedidoDeTeste>>) -> (StatusCode, Json<Value>) {
    let pedido = body.map(|Json(b)| b).unwrap_or_default();
    let (modelo, esforco) = match entrada_da_config() {
        Some((_, m, e, _)) => (Some(m), Some(e)),
        None => (None, None),
    };
    let modelo = pedido.modelo.filter(|m| !m.trim().is_empty()).or(modelo);
    let p = OpenAiCodexProvider::new(
        ConfigLoader::openai_codex_credentials_dir(),
        modelo,
        esforco,
    );
    let req = LlmRequest {
        model: String::new(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("Responda exatamente: ok".into()),
        }],
        system: None,
        max_tokens: None,
        temperature: None,
        tools: Vec::new(),
    };
    // O que vai no corpo (sem segredo): prova de que o esforco e enviado.
    let corpo = p.montar_corpo(&req);
    let enviado = json!({"model": corpo["model"], "reasoning": corpo["reasoning"], "store": corpo["store"], "stream": corpo["stream"]});
    let inicio = Instant::now();
    let mut stream = match p.stream_complete(&req).await {
        Ok(s) => s,
        Err(e) => return erro(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let mut texto = String::new();
    let mut trechos = 0usize;
    let mut uso = None;
    let mut concluido = false;
    while let Some(ev) = stream.next().await {
        match ev {
            Ok(StreamEvent::TextDelta(t)) => {
                trechos += 1;
                texto.push_str(&t);
            }
            Ok(StreamEvent::MessageDelta { usage, .. }) => uso = usage,
            Ok(StreamEvent::MessageStop) => concluido = true,
            Ok(_) => {}
            Err(e) => return erro(StatusCode::BAD_GATEWAY, e.to_string()),
        }
    }
    if !concluido {
        return erro(
            StatusCode::BAD_GATEWAY,
            "o stream terminou sem response.completed",
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "texto": texto.trim(),
            "trechos_em_streaming": trechos,
            "ms": inicio.elapsed().as_millis() as u64,
            "modelo": p.configured_model(),
            "enviado": enviado,
            "uso": uso.map(|u| json!({"entrada": u.input_tokens, "saida": u.output_tokens})),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acesso_local_so_para_loopback() {
        let h = |v: &str| {
            let mut m = HeaderMap::new();
            m.insert("host", v.parse().expect("header"));
            m
        };
        assert!(acesso_local(&h("127.0.0.1:3888")));
        assert!(acesso_local(&h("localhost:3888")));
        assert!(!acesso_local(&h("192.168.0.10:3888")));
        assert!(!acesso_local(&h("garra.exemplo.com")));
    }
}
