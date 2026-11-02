//! O [`OpenAiCodexProvider`]: inferencia pelo plano do ChatGPT no endpoint
//! publico `POST https://api.openai.com/v1/responses`.
//!
//! O corpo segue `preview-limitations` ao pe da letra:
//! - sempre `store: false` e `stream: true`;
//! - historico inteiro em `input` (sem `previous_response_id`);
//! - instrucoes de sistema em `instructions` (itens `role: system` sao
//!   recusados pela API);
//! - ferramentas de funcao agrupadas num `namespace`;
//! - nenhum dos 15 campos proibidos (`temperature`, `max_output_tokens`,
//!   `metadata`, `user`...) e nenhum override de `service_tier`. Por isso o
//!   `max_tokens`/`temperature` do pedido do runtime sao ignorados aqui.
//!
//! `reasoning.effort` vai em todo pedido (padrao `high`).

use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};

use super::credenciais::ArmazemDeCredenciais;
use super::sse::{LeitorDeEventos, NAMESPACE, erro_estruturado};
use super::{ESFORCO_PADRAO, Endpoints, MODELO_PADRAO, PROVIDER_ID, esforco_normalizado};
use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent, Usage, erro_de_envio,
};

/// Um modelo do catalogo da conta (`GET /v1/models`, `visibility: "list"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModeloDisponivel {
    pub slug: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
struct CatalogoDeModelos {
    #[serde(default)]
    models: Vec<ItemDoCatalogo>,
}

#[derive(Deserialize)]
struct ItemDoCatalogo {
    slug: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
}

pub struct OpenAiCodexProvider {
    armazem: ArmazemDeCredenciais,
    cliente: reqwest::Client,
    modelo: String,
    esforco: String,
    nome: String,
}

impl OpenAiCodexProvider {
    /// `dir_credenciais` e `<config_dir>/credentials/openai-codex`. Modelo e
    /// esforco ausentes (ou invalidos) caem nos padroes `gpt-6.1-sol`/`high`.
    pub fn new(
        dir_credenciais: impl Into<PathBuf>,
        modelo: Option<String>,
        esforco: Option<String>,
    ) -> Self {
        let modelo = modelo
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| MODELO_PADRAO.to_string());
        let esforco = match esforco.as_deref().map(esforco_normalizado) {
            Some(Some(e)) => e,
            Some(None) => {
                tracing::warn!(valor = ?esforco, "openai-codex: reasoning_effort invalido; usando 'high'");
                ESFORCO_PADRAO.to_string()
            }
            None => ESFORCO_PADRAO.to_string(),
        };
        let cliente = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            armazem: ArmazemDeCredenciais::new(dir_credenciais),
            cliente,
            modelo,
            esforco,
            nome: PROVIDER_ID.to_string(),
        }
    }

    /// Troca o cliente HTTP da inferencia (timeouts da config).
    pub fn with_client(mut self, cliente: reqwest::Client) -> Self {
        self.cliente = cliente;
        self
    }

    /// Troca o `provider_id` (o nome da entrada `llm.<nome>`).
    pub fn with_name(mut self, nome: impl Into<String>) -> Self {
        self.nome = nome.into();
        self
    }

    pub fn with_endpoints(mut self, endpoints: Endpoints) -> Self {
        self.armazem = self.armazem.with_endpoints(endpoints);
        self
    }

    pub fn armazem(&self) -> &ArmazemDeCredenciais {
        &self.armazem
    }

    pub fn esforco(&self) -> &str {
        &self.esforco
    }

    fn url(&self, caminho: &str) -> String {
        format!(
            "{}/{caminho}",
            self.armazem.endpoints().api_base.trim_end_matches('/')
        )
    }

    /// O corpo do `POST /v1/responses` para um pedido do runtime.
    pub fn montar_corpo(&self, pedido: &LlmRequest) -> serde_json::Value {
        let modelo = if pedido.model.trim().is_empty() {
            self.modelo.as_str()
        } else {
            pedido.model.as_str()
        };
        let mut instrucoes: Vec<String> = pedido
            .system
            .iter()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .collect();
        let mut entrada: Vec<serde_json::Value> = Vec::new();
        for msg in &pedido.messages {
            itens_da_mensagem(msg, &mut instrucoes, &mut entrada);
        }
        let mut corpo = serde_json::json!({
            "model": modelo,
            "input": entrada,
            "store": false,
            "stream": true,
            "reasoning": {"effort": self.esforco},
        });
        if !instrucoes.is_empty() {
            corpo["instructions"] = serde_json::Value::String(instrucoes.join("\n\n"));
        }
        if !pedido.tools.is_empty() {
            let funcoes: Vec<serde_json::Value> = pedido
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    })
                })
                .collect();
            corpo["tools"] = serde_json::json!([{
                "type": "namespace",
                "name": NAMESPACE,
                "description": "Ferramentas locais do GarraIA (arquivos, shell, web, memoria e integracoes).",
                "tools": funcoes,
            }]);
            corpo["tool_choice"] = "auto".into();
            corpo["parallel_tool_calls"] = true.into();
        }
        corpo
    }

    async fn enviar(&self, token: &str, corpo: &serde_json::Value) -> Result<reqwest::Response> {
        self.cliente
            .post(self.url("responses"))
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .header(
                reqwest::header::USER_AGENT,
                concat!("garraia/", env!("CARGO_PKG_VERSION")),
            )
            .json(corpo)
            .send()
            .await
            .map_err(|e| erro_de_envio("openai-codex request failed", &e))
    }

    /// Abre o stream. Um 401 na admissao renova o token uma vez e repete.
    async fn abrir(&self, pedido: &LlmRequest) -> Result<reqwest::Response> {
        let corpo = self.montar_corpo(pedido);
        let token = self.armazem.access_token().await?;
        let mut resp = self.enviar(&token, &corpo).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let novo = self.armazem.renovar_apos_recusa(&token).await?;
            resp = self.enviar(&novo, &corpo).await?;
        }
        if resp.status().is_success() {
            return Ok(resp);
        }
        Err(erro_http(resp).await)
    }

    /// O catalogo de modelos da conta em uso, na ordem do servidor.
    pub async fn listar_modelos(&self) -> Result<Vec<ModeloDisponivel>> {
        let token = self.armazem.access_token().await?;
        let pedir = |t: String| {
            self.cliente
                .get(self.url("models"))
                .bearer_auth(t)
                .header(
                    reqwest::header::USER_AGENT,
                    concat!("garraia/", env!("CARGO_PKG_VERSION")),
                )
                .send()
        };
        let mut resp = pedir(token.clone())
            .await
            .map_err(|e| erro_de_envio("openai-codex models request failed", &e))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let novo = self.armazem.renovar_apos_recusa(&token).await?;
            resp = pedir(novo)
                .await
                .map_err(|e| erro_de_envio("openai-codex models request failed", &e))?;
        }
        if !resp.status().is_success() {
            return Err(erro_http(resp).await);
        }
        let catalogo: CatalogoDeModelos = resp.json().await.map_err(|e| {
            Error::Agent(format!("openai-codex: catalogo de modelos ilegivel: {e}"))
        })?;
        Ok(catalogo
            .models
            .into_iter()
            .filter(|m| m.visibility.as_deref().is_none_or(|v| v == "list"))
            .map(|m| ModeloDisponivel {
                slug: m.slug,
                display_name: m.display_name,
            })
            .collect())
    }
}

/// Converte uma mensagem do runtime em itens de `input` (e instrucoes).
fn itens_da_mensagem(
    msg: &ChatMessage,
    instrucoes: &mut Vec<String>,
    entrada: &mut Vec<serde_json::Value>,
) {
    let blocos: Vec<ContentBlock> = match &msg.content {
        MessagePart::Text(t) => vec![ContentBlock::Text { text: t.clone() }],
        MessagePart::Parts(p) => p.clone(),
    };
    if matches!(msg.role, ChatRole::System) {
        for b in blocos {
            if let ContentBlock::Text { text } = b
                && !text.trim().is_empty()
            {
                instrucoes.push(text);
            }
        }
        return;
    }
    let assistente = matches!(msg.role, ChatRole::Assistant);
    let mut conteudo: Vec<serde_json::Value> = Vec::new();
    let mut depois: Vec<serde_json::Value> = Vec::new();
    for b in blocos {
        match b {
            ContentBlock::Text { text } if !text.is_empty() => {
                let tipo = if assistente {
                    "output_text"
                } else {
                    "input_text"
                };
                conteudo.push(serde_json::json!({"type": tipo, "text": text}));
            }
            ContentBlock::Text { .. } => {}
            ContentBlock::Image { url } if !assistente => {
                conteudo.push(serde_json::json!({"type": "input_image", "image_url": url}));
            }
            ContentBlock::Image { .. } => {}
            ContentBlock::ToolUse { id, name, input } => depois.push(serde_json::json!({
                "type": "function_call",
                "call_id": id,
                "name": name,
                "namespace": NAMESPACE,
                "arguments": input.to_string(),
            })),
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => depois.push(serde_json::json!({
                "type": "function_call_output",
                "call_id": tool_use_id,
                "output": content,
            })),
        }
    }
    if !conteudo.is_empty() {
        let papel = if assistente { "assistant" } else { "user" };
        entrada.push(serde_json::json!({"type": "message", "role": papel, "content": conteudo}));
    }
    entrada.extend(depois);
}

async fn erro_http(resp: reqwest::Response) -> Error {
    let status = resp.status().as_u16();
    let corpo = resp.text().await.unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&corpo).unwrap_or_default();
    let erro = &v["error"];
    let campo = |c: &str| {
        erro.get(c)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let codigo = campo("code");
    let mensagem = match campo("message") {
        m if !m.is_empty() => m,
        _ => v
            .get("detail")
            .and_then(|d| d.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| crate::providers::corpo_de_erro_sanitizado(&corpo)),
    };
    tracing::warn!(status, codigo = %codigo, "openai-codex: Responses API recusou o pedido");
    erro_estruturado(Some(status), &codigo, &mensagem, &campo("param"))
}

#[async_trait]
impl LlmProvider for OpenAiCodexProvider {
    fn provider_id(&self) -> &str {
        &self.nome
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.modelo)
    }

    async fn complete(&self, pedido: &LlmRequest) -> Result<LlmResponse> {
        let modelo = if pedido.model.trim().is_empty() {
            self.modelo.clone()
        } else {
            pedido.model.clone()
        };
        let mut stream = self.stream_complete(pedido).await?;
        let mut texto = String::new();
        let mut ferramentas: Vec<(String, String, String)> = Vec::new();
        let mut aberta: Option<(String, String, String)> = None;
        let mut stop_reason = None;
        let mut usage: Option<Usage> = None;
        while let Some(ev) = stream.next().await {
            match ev? {
                StreamEvent::TextDelta(t) => texto.push_str(&t),
                StreamEvent::ToolUseStart { id, name, .. } => {
                    if let Some(f) = aberta.take() {
                        ferramentas.push(f);
                    }
                    aberta = Some((id, name, String::new()));
                }
                StreamEvent::InputJsonDelta(d) => {
                    if let Some((_, _, args)) = aberta.as_mut() {
                        args.push_str(&d);
                    }
                }
                StreamEvent::ContentBlockStop { .. } => {
                    if let Some(f) = aberta.take() {
                        ferramentas.push(f);
                    }
                }
                StreamEvent::MessageDelta {
                    stop_reason: s,
                    usage: u,
                } => {
                    stop_reason = s;
                    usage = u;
                }
                StreamEvent::MessageStop => break,
            }
        }
        if let Some(f) = aberta.take() {
            ferramentas.push(f);
        }
        let mut content = Vec::new();
        if !texto.is_empty() {
            content.push(ContentBlock::Text { text: texto });
        }
        for (id, name, args) in ferramentas {
            let input = serde_json::from_str(&args).unwrap_or_else(|_| serde_json::json!({}));
            content.push(ContentBlock::ToolUse { id, name, input });
        }
        Ok(LlmResponse {
            content,
            model: modelo,
            usage,
            stop_reason,
        })
    }

    async fn stream_complete(
        &self,
        pedido: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let resp = self.abrir(pedido).await?;
        let estado = (resp.bytes_stream(), LeitorDeEventos::default(), false);
        let fluxo =
            futures::stream::unfold(estado, |(mut bytes, mut leitor, mut fim)| async move {
                loop {
                    if let Some(ev) = leitor.proximo() {
                        return Some((Ok(ev), (bytes, leitor, fim)));
                    }
                    if fim || leitor.terminou() {
                        return None;
                    }
                    match bytes.next().await {
                        Some(Ok(chunk)) => {
                            if let Err(e) = leitor.alimentar(&chunk) {
                                return Some((Err(e), (bytes, leitor, true)));
                            }
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(erro_de_envio("openai-codex stream failed", &e)),
                                (bytes, leitor, true),
                            ));
                        }
                        None => {
                            fim = true;
                            if let Err(e) = leitor.finalizar() {
                                return Some((Err(e), (bytes, leitor, true)));
                            }
                        }
                    }
                }
            });
        Ok(Box::pin(fluxo))
    }

    async fn available_models(&self) -> Result<Vec<String>> {
        Ok(self
            .listar_modelos()
            .await?
            .into_iter()
            .map(|m| m.slug)
            .collect())
    }

    /// Sem rede: conectado com o escopo do plano e com token renovavel.
    async fn health_check(&self) -> Result<bool> {
        Ok(self
            .armazem
            .registro_ativo()
            .ok()
            .flatten()
            .is_some_and(|r| r.conectado() && r.plano_habilitado()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ESCOPO_DO_PLANO;
    use super::super::credenciais::RegistroDeCredencial;
    use super::*;
    use crate::providers::ToolDefinition;
    use std::sync::{Arc, Mutex};

    const CAMPOS_PROIBIDOS: [&str; 17] = [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
        "service_tier",
    ];

    fn pedido_com_ferramenta() -> LlmRequest {
        LlmRequest {
            model: String::new(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: MessagePart::Text("regra extra".into()),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text("leia a.txt".into()),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Parts(vec![
                        ContentBlock::Text {
                            text: "vou ler".into(),
                        },
                        ContentBlock::ToolUse {
                            id: "call_1".into(),
                            name: "file_read".into(),
                            input: serde_json::json!({"path": "a.txt"}),
                        },
                    ]),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Parts(vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".into(),
                        content: "ola".into(),
                    }]),
                },
            ],
            system: Some("Voce e o GarraIA.".into()),
            max_tokens: Some(4096),
            temperature: Some(0.2),
            tools: vec![ToolDefinition {
                name: "file_read".into(),
                description: "le arquivo".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }],
        }
    }

    #[test]
    fn corpo_segue_as_regras_do_plano() {
        let p = OpenAiCodexProvider::new("/tmp/nao-usado", None, None);
        let c = p.montar_corpo(&pedido_com_ferramenta());
        assert_eq!(c["model"], "gpt-6.1-sol");
        assert_eq!(c["store"], false);
        assert_eq!(c["stream"], true);
        assert_eq!(c["reasoning"]["effort"], "high");
        assert_eq!(c["instructions"], "Voce e o GarraIA.\n\nregra extra");
        for campo in CAMPOS_PROIBIDOS {
            assert!(c.get(campo).is_none(), "campo proibido no corpo: {campo}");
        }
        // Nenhum item de sistema no input.
        let input = c["input"].as_array().expect("input");
        assert!(
            input
                .iter()
                .all(|i| i.get("role").and_then(|r| r.as_str()) != Some("system"))
        );
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "call_1");
        assert_eq!(input[2]["arguments"], "{\"path\":\"a.txt\"}");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["output"], "ola");
        // Ferramentas agrupadas num namespace.
        assert_eq!(c["tools"][0]["type"], "namespace");
        assert_eq!(c["tools"][0]["tools"][0]["type"], "function");
        assert_eq!(c["tools"][0]["tools"][0]["name"], "file_read");
    }

    #[test]
    fn modelo_e_esforco_da_config_vencem_e_invalido_cai_no_padrao() {
        let p = OpenAiCodexProvider::new("/tmp/x", Some("gpt-6-luna".into()), Some("Max".into()));
        let mut req = pedido_com_ferramenta();
        req.tools.clear();
        let c = p.montar_corpo(&req);
        assert_eq!(c["model"], "gpt-6-luna");
        assert_eq!(c["reasoning"]["effort"], "max");
        assert!(c.get("tools").is_none() && c.get("tool_choice").is_none());
        let p = OpenAiCodexProvider::new("/tmp/x", None, Some("turbo".into()));
        assert_eq!(p.esforco(), "high");
        // O modelo do pedido vence o da config.
        req.model = "gpt-6-sol".into();
        assert_eq!(p.montar_corpo(&req)["model"], "gpt-6-sol");
    }

    /// API falsa: `/v1/responses` (SSE) e `/v1/models`, e `/token` para a
    /// renovacao depois de um 401. Guarda os cabecalhos e corpos recebidos.
    struct ApiFalsa {
        endpoints: Endpoints,
        pedidos: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
        _desligar: tokio::sync::oneshot::Sender<()>,
    }

    async fn api_falsa(recusar_primeiro: bool, sse: &'static str) -> ApiFalsa {
        use axum::http::{HeaderMap, StatusCode};
        use axum::response::IntoResponse;
        use axum::{Router, extract::State, routing::get, routing::post};
        let pedidos: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::default();
        let recusas = Arc::new(Mutex::new(recusar_primeiro));
        #[derive(Clone)]
        struct E {
            pedidos: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
            recusas: Arc<Mutex<bool>>,
            sse: &'static str,
        }
        let app = Router::new()
            .route(
                "/v1/responses",
                post(|State(s): State<E>, cab: HeaderMap, corpo: String| async move {
                    let auth = cab.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                    let json: serde_json::Value = serde_json::from_str(&corpo).unwrap_or_default();
                    s.pedidos.lock().expect("lock").push((auth, json));
                    let mut recusar = s.recusas.lock().expect("lock");
                    if *recusar {
                        *recusar = false;
                        return (StatusCode::UNAUTHORIZED, r#"{"detail":"token expired"}"#).into_response();
                    }
                    ([("content-type", "text/event-stream")], s.sse).into_response()
                }),
            )
            .route(
                "/v1/models",
                get(|| async {
                    axum::Json(serde_json::json!({"models": [
                        {"slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list"},
                        {"slug": "interno", "display_name": "Interno", "visibility": "hide"},
                        {"slug": "gpt-6-luna", "display_name": "GPT-6 Luna", "visibility": "list"}
                    ]}))
                }),
            )
            .route(
                "/token",
                post(|| async { axum::Json(serde_json::json!({"access_token": "AT-novo", "refresh_token": "RT-novo", "expires_in": 3600})) }),
            )
            .with_state(E { pedidos: pedidos.clone(), recusas, sse });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        ApiFalsa {
            endpoints: Endpoints {
                authorize: format!("{base}/authorize"),
                token: format!("{base}/token"),
                revoke: format!("{base}/revoke"),
                jwks: format!("{base}/jwks"),
                issuer: base.clone(),
                api_base: format!("{base}/v1"),
            },
            pedidos,
            _desligar: tx,
        }
    }

    fn conta_conectada(dir: &std::path::Path, endpoints: &Endpoints) -> ArmazemDeCredenciais {
        let a = ArmazemDeCredenciais::new(dir).with_endpoints(endpoints.clone());
        a.salvar_e_ativar(&RegistroDeCredencial {
            email: Some("ana@exemplo.com".into()),
            name: None,
            issuer: endpoints.issuer.clone(),
            subject: "user-1".into(),
            client_id: "oaiapp_1".into(),
            ext_agent_host_id: "urn:uuid:x".into(),
            id_token: None,
            access_token: Some("AT-velho".into()),
            refresh_token: Some("RT-velho".into()),
            token_type: "Bearer".into(),
            expires_in: 3600,
            scopes: vec![ESCOPO_DO_PLANO.into()],
            saved_at: super::super::credenciais::agora_iso(),
            earliest_refresh_at: None,
        })
        .expect("salva");
        a
    }

    const SSE_TEXTO: &str = "event: response.created\ndata: {\"type\":\"response.created\"}\n\n\
        data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello, \"}\n\n\
        data: {\"type\":\"response.output_text.delta\",\"delta\":\"world!\"}\n\n\
        data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n";

    #[tokio::test]
    async fn complete_por_stream_com_bearer_e_corpo_do_plano() {
        let api = api_falsa(false, SSE_TEXTO).await;
        let tmp = tempfile::tempdir().expect("tmp");
        conta_conectada(tmp.path(), &api.endpoints);
        let p =
            OpenAiCodexProvider::new(tmp.path(), None, None).with_endpoints(api.endpoints.clone());
        let mut req = pedido_com_ferramenta();
        req.tools.clear();
        let r = p.complete(&req).await.expect("completa");
        assert!(matches!(&r.content[0], ContentBlock::Text { text } if text == "Hello, world!"));
        assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(r.usage.map(|u| u.output_tokens), Some(2));
        let pedidos = api.pedidos.lock().expect("lock");
        assert_eq!(pedidos[0].0, "Bearer AT-velho");
        assert_eq!(pedidos[0].1["reasoning"]["effort"], "high");
        assert_eq!(pedidos[0].1["store"], false);
    }

    #[tokio::test]
    async fn um_401_renova_o_token_e_repete_uma_vez() {
        let api = api_falsa(true, SSE_TEXTO).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = conta_conectada(tmp.path(), &api.endpoints);
        let p =
            OpenAiCodexProvider::new(tmp.path(), None, None).with_endpoints(api.endpoints.clone());
        let mut req = pedido_com_ferramenta();
        req.tools.clear();
        p.complete(&req).await.expect("completa depois de renovar");
        let pedidos = api.pedidos.lock().expect("lock");
        assert_eq!(pedidos.len(), 2);
        assert_eq!(pedidos[1].0, "Bearer AT-novo");
        // O refresh token rotativo foi trocado junto.
        let reg = armazem.registro("oaiapp_1").expect("le").expect("existe");
        assert_eq!(reg.refresh_token.as_deref(), Some("RT-novo"));
    }

    #[tokio::test]
    async fn catalogo_so_lista_visibilidade_list_na_ordem_do_servidor() {
        let api = api_falsa(false, SSE_TEXTO).await;
        let tmp = tempfile::tempdir().expect("tmp");
        conta_conectada(tmp.path(), &api.endpoints);
        let p =
            OpenAiCodexProvider::new(tmp.path(), None, None).with_endpoints(api.endpoints.clone());
        let slugs = p.available_models().await.expect("modelos");
        assert_eq!(slugs, vec!["gpt-6.1-sol", "gpt-6-luna"]);
        assert!(p.health_check().await.expect("health"));
    }

    #[tokio::test]
    async fn sem_conta_o_erro_diz_como_entrar() {
        let tmp = tempfile::tempdir().expect("tmp");
        let p = OpenAiCodexProvider::new(tmp.path(), None, None);
        let e = p
            .complete(&pedido_com_ferramenta())
            .await
            .expect_err("sem conta");
        let msg = e.to_string();
        assert!(msg.contains("garra chatgpt login") && msg.contains("Continuar com ChatGPT"));
        // Classificado como credencial recusada: o runtime vai ao fallback sem repetir.
        assert!(msg.contains("status=401"));
        assert!(
            !msg.contains("3888"),
            "a mensagem nao pode citar uma porta fixa"
        );
        assert!(!p.health_check().await.expect("health"));
    }
}
