//! Adapter Home Assistant (#1127) — REST + WebSocket atrás da feature
//! `home-assistant`.
//!
//! O Home Assistant já agregou o zilhão de integrações domésticas (Zigbee,
//! Z-Wave, Wi-Fi) — em vez de duplicar drivers, este adapter fala a API
//! oficial e expõe as entidades como [`crate::Device`] no registry:
//!
//! | Chamada | Direção | Uso |
//! |---|---|---|
//! | `GET /api/states` | REST | descoberta — entidades por domínio |
//! | `GET /api/states/{entity_id}` | REST | [`crate::Device::read`] |
//! | `POST /api/services/{domain}/{service}` | REST | [`crate::Device::execute`] |
//! | `ws(s)://…/api/websocket` | WebSocket | eventos `state_changed` → presença |
//!
//! # Contratos
//!
//! - **Risco por domínio, nunca por payload**: sensor/binary_sensor → R0
//!   (leitura), light/switch/climate → R1, cover → R2, lock → R3. Domínio
//!   desconhecido **não vira dispositivo** (fail-closed — o teto R2 do
//!   slice é da fundação; nada aqui sobe risco sem avaliação própria).
//! - **SSRF (regra 14)**: a URL base vem de config, então **toda** chamada
//!   sai por `garraia_common::ssrf` — `vet_url` com
//!   `UrlPolicy::http_public(..).with_ip_scope(IpScope::AllowPrivate)` (o HA
//!   é alvo legítimo da LAN, e o guard ainda bloqueia link-local, CGNAT,
//!   multicast e unspecified) e `pinned_client` com os IPs vetados. O
//!   WebSocket conecta no **mesmo IP pinado** (TcpStream manual + handshake
//!   `client_async_tls_with_config`) — sem re-resolução de DNS, sem janela
//!   TOCTOU. `read_capped` limita os corpos.
//! - **Token write-only**: config carrega o **nome** da env (`token_env`);
//!   o chamador resolve o valor e passa aqui. O `Debug` redige; o token
//!   nunca é logado nem serializado — os warns citam só o nome da env.
//! - **Validação de args**: mesma [`crate::schema::validar_args`] do adapter
//!   MQTT — um validador, dois transports, zero divergência.
//! - **Presença**: cada `state_changed` marca o dispositivo no
//!   [`DeviceStateStore`] (`state == "unavailable"` → offline); a descoberta
//!   marca quem respondeu. Desconexão do WebSocket → reconexão em loop
//!   (mesmo padrão do canal Slack), presença segue no retorno.
//!
//! # O que este slice NÃO faz
//!
//! Sem reload de config (desconecta só no shutdown — trabalho da #1128), sem
//! automações (motor da #1128) e sem registrar domínios além da lista
//! fechada abaixo — vacuum, media_player etc. continuam invisíveis até
//! alguém avaliar o risco deles num PR.

use crate::Result;
use crate::capability::Capability;
use crate::device::Device;
use crate::error::HardwareError;
use crate::registry::DeviceRegistry;
use crate::risk::RiskClass;
use crate::schema::validar_args;
use crate::state::DeviceStateStore;
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use garraia_common::ssrf::{IpScope, UrlPolicy, pinned_client, read_capped};
use serde_json::{Value, json};
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Timeout HTTP do cliente pinado (REST).
pub const HA_TIMEOUT: Duration = Duration::from_secs(10);

/// User-Agent das chamadas REST.
const USER_AGENT: &str = "garraia-hardware/home-assistant";

/// Teto dos corpos REST — `/api/states` de uma casa grande fica em ~1 MB;
/// 16 MiB é folga honesta, e `read_capped` corta o resto.
const BODY_CAP: usize = 16 * 1024 * 1024;

/// Intervalo entre tentativas de reconexão do WebSocket.
const INTERVALO_RECONEXAO: Duration = Duration::from_secs(5);

/// Ping do cliente no WebSocket — mantém NAT/TCP vivo e dá write real para
/// detectar conexão morta cedo (o HA responde pong por baixo, control frame
/// não sobe pro loop de eventos). O `WebSocketConfig` do tungstenite 0.29
/// não expõe `ping_interval`, então o ping é nosso, no `select!`.
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Silêncio máximo de eventos antes de reconectar, como backstop: casa quieta
/// não gera `state_changed` por minutos, e a morte silenciosa do TCP (hub
/// caiu sem FIN) não é detectada por leitura. Uma reconexão a cada
/// [`KEEPALIVE`] numa casa quieta é barata — re-descoberta + re-subscribe.
const KEEPALIVE: Duration = Duration::from_secs(600);

// ─────────────────────────────────────────────────────────────────────────────
// Config do adapter — vetting + cliente pinado na fronteira.
// ─────────────────────────────────────────────────────────────────────────────

/// Config de transporte do adapter Home Assistant, **já vetada**.
///
/// `garraia-hardware` **não** depende de `garraia-config` — o gateway recebe
/// `hardware.home_assistant` do config, resolve `token_env` e monta esta
/// struct. O vetting (`UrlPolicy` + resolução pinada) acontece no `new`: a
/// config que entra aqui já passou pela barreira.
pub struct HaAdapterConfig {
    /// URL base vetada (`http://homeassistant.local:8123`), com `/` no fim
    /// do path — `Url::join` troca o último segmento de um path sem barra
    /// (`http://hub:8123/ha` + `api/states` → `http://hub:8123/api/states`),
    /// então a normalização acontece aqui, uma vez, na fronteira.
    pub base: url::Url,
    /// Host vetado — os requests REST vão por `http`, pinado nestes IPs.
    host: String,
    /// IPs já resolvidos e validados no escopo. O WebSocket conecta num
    /// deles, sem re-resolver DNS (anti-rebinding no path WS também).
    addrs: Vec<SocketAddr>,
    /// Long-lived access token, **já resolvido do env** pelo chamador.
    /// Nunca logado nem serializado — o `Debug` redige.
    pub token: String,
    /// Cliente REST pinado nos IPs vetados, redirects off.
    pub http: reqwest::Client,
}

impl HaAdapterConfig {
    /// Veta a URL base (regra 14) e constrói o cliente pinado.
    ///
    /// `IpScope::AllowPrivate` porque o HA é um hub local — LAN e loopback
    /// são alvos legítimos; link-local (`169.254.169.254`), CGNAT, multicast
    /// e unspecified continuam bloqueados pelo guard.
    pub fn new(url_base: &str, token: String) -> Result<Self> {
        let policy =
            UrlPolicy::http_public(HA_TIMEOUT, USER_AGENT).with_ip_scope(IpScope::AllowPrivate);
        let vetted = garraia_common::ssrf::vet_url(url_base, &policy)
            .map_err(|e| HardwareError::HomeAssistant(format!("url base vetada: {e}")))?;
        let http = pinned_client(&vetted, &policy)
            .map_err(|e| HardwareError::HomeAssistant(format!("cliente HTTP pinado: {e}")))?;
        let mut base = vetted.url;
        if !base.path().ends_with('/') {
            let com_barra = format!("{}/", base.path());
            base.set_path(&com_barra);
        }
        Ok(Self {
            base,
            host: vetted.host,
            addrs: vetted.addrs,
            token,
            http,
        })
    }
}

impl fmt::Debug for HaAdapterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HaAdapterConfig")
            .field("base", &self.base.as_str())
            .field("host", &self.host)
            .field("addrs", &self.addrs)
            .field("token", &"<redacted>")
            .finish()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Domínio → capability / capability → serviço — o mapa é o contrato.
// ─────────────────────────────────────────────────────────────────────────────

/// As capabilities de um domínio, com o risco avaliado por domínio (#1127):
/// sensor/binary_sensor R0 · light/switch/climate R1 · cover R2 · lock R3.
/// Domínio fora da lista → `None` (fail-closed).
fn capabilities_de(dominio: &str) -> Option<Vec<Capability>> {
    let estado = || Capability::leitura("state", None);
    let power = || {
        Capability::acao(
            "power",
            RiskClass::R1,
            Some(json!({
                "type": "object",
                "properties": {"on": {"type": "boolean"}},
                "required": ["on"]
            })),
        )
        .ok()
    };
    let caps = match dominio {
        "sensor" | "binary_sensor" => vec![estado()],
        "light" => vec![
            estado(),
            power()?,
            Capability::acao(
                "brightness",
                RiskClass::R1,
                Some(json!({
                    "type": "object",
                    "properties": {"brightness_pct": {"type": "integer"}},
                    "required": ["brightness_pct"]
                })),
            )
            .ok()?,
        ],
        "switch" => vec![estado(), power()?],
        "climate" => vec![
            estado(),
            power()?,
            Capability::acao(
                "temperature",
                RiskClass::R1,
                Some(json!({
                    "type": "object",
                    "properties": {"temperature": {"type": "number"}},
                    "required": ["temperature"]
                })),
            )
            .ok()?,
        ],
        "cover" => vec![
            estado(),
            Capability::acao("open", RiskClass::R2, None).ok()?,
            Capability::acao("close", RiskClass::R2, None).ok()?,
            Capability::acao(
                "set_position",
                RiskClass::R2,
                Some(json!({
                    "type": "object",
                    "properties": {"position": {"type": "integer"}},
                    "required": ["position"]
                })),
            )
            .ok()?,
        ],
        "lock" => vec![
            estado(),
            Capability::acao("lock", RiskClass::R3, None).ok()?,
            Capability::acao("unlock", RiskClass::R3, None).ok()?,
        ],
        _ => return None,
    };
    Some(caps)
}

/// Mapeia (domínio, capability, args) → (serviço HA, payload REST).
///
/// O `entity_id` entra em todo payload — é o alvo do serviço. `None` para
/// (domínio, capability) fora da tabela: o `execute` recusa antes de falar
/// com o hub.
fn servico_de(
    dominio: &str,
    capability: &str,
    args: &Value,
    entity_id: &str,
) -> Option<(String, Value)> {
    let payload = json!({ "entity_id": entity_id });
    match (dominio, capability) {
        ("light" | "switch" | "climate", "power") => {
            let on = args.get("on")?.as_bool()?;
            let servico = if on { "turn_on" } else { "turn_off" };
            Some((format!("{dominio}/{servico}"), payload))
        }
        ("light", "brightness") => {
            let pct = args.get("brightness_pct")?.as_i64()?;
            Some((
                "light/turn_on".to_string(),
                com_campo(payload, "brightness_pct", json!(pct)),
            ))
        }
        ("climate", "temperature") => {
            let t = args.get("temperature")?.as_f64()?;
            Some((
                "climate/set_temperature".to_string(),
                com_campo(payload, "temperature", json!(t)),
            ))
        }
        ("cover", "open") => Some(("cover/open_cover".to_string(), payload)),
        ("cover", "close") => Some(("cover/close_cover".to_string(), payload)),
        ("cover", "set_position") => {
            let p = args.get("position")?.as_i64()?;
            Some((
                "cover/set_cover_position".to_string(),
                com_campo(payload, "position", json!(p)),
            ))
        }
        ("lock", "lock") => Some(("lock/lock".to_string(), payload)),
        ("lock", "unlock") => Some(("lock/unlock".to_string(), payload)),
        _ => None,
    }
}

/// Insere `valor` num payload JSON — helper para o caso raro de payload com
/// campo extra (o `json!` macro com `entity_id` fixo + campo variável fica
/// verboso; assim a tabela acima continua legível).
fn com_campo(mut payload: Value, chave: &str, valor: Value) -> Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(chave.to_string(), valor);
    }
    payload
}

// ─────────────────────────────────────────────────────────────────────────────
// Descoberta — parse das entidades da API.
// ─────────────────────────────────────────────────────────────────────────────

/// Uma entidade do HA que passou na fronteira de descoberta.
struct Entidade {
    entity_id: String,
    dominio: String,
    caps: Vec<Capability>,
    /// `state != "unavailable"` na descoberta — presença inicial honesta.
    online: bool,
}

/// Parseia a resposta de `GET /api/states` e recusa (não panica) o que não
/// entende: entity_id malformado, domínio fora da lista, capability que
/// viola a invariante leitura↔R0. Entidades recusadas simplesmente não
/// entram no registry.
fn parse_entidades(body: &[u8]) -> Result<Vec<Entidade>> {
    let arr: Vec<Value> = serde_json::from_slice(body)
        .map_err(|e| HardwareError::HomeAssistant(format!("/api/states não é JSON: {e}")))?;
    let mut out = Vec::new();
    for v in arr {
        let Some(id) = v.get("entity_id").and_then(Value::as_str) else {
            continue;
        };
        let Some((dominio, objeto)) = id.split_once('.') else {
            continue;
        };
        let Some(caps) = capabilities_de(dominio) else {
            continue;
        };
        // O `object_id` de um entity_id HA é `[a-z0-9_]+` — nada de espaço,
        // barra ou caractere que brigue com o path REST
        // (`GET /api/states/{id}`) ou com logs.
        if !valida_objeto(objeto) {
            continue;
        }
        for cap in &caps {
            if let Err(e) = cap.validar() {
                tracing::warn!(entity = id, "entidade descartada: capability inválida: {e}");
                continue;
            }
        }
        let online = v.get("state").and_then(Value::as_str) != Some("unavailable");
        out.push(Entidade {
            entity_id: id.to_string(),
            dominio: dominio.to_string(),
            caps,
            online,
        });
    }
    Ok(out)
}

/// O `object_id` de um entity_id HA é `[a-z0-9_]+` — nada de espaço, barra
/// ou caractere que briguem com o path REST (`GET /api/states/{id}`) ou com
/// logs.
fn valida_objeto(objeto: &str) -> bool {
    !objeto.is_empty()
        && objeto
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ─────────────────────────────────────────────────────────────────────────────
// HaDevice — o Device que fala REST com o hub.
// ─────────────────────────────────────────────────────────────────────────────

/// Um dispositivo exposto pelo Home Assistant. O id é o `entity_id`
/// (`"light.sala"`) — estável, único por hub, e é o que o agente vê na
/// `device_list`.
pub struct HaDevice {
    entity_id: String,
    dominio: String,
    caps: Vec<Capability>,
    config: Arc<HaAdapterConfig>,
}

#[async_trait]
impl Device for HaDevice {
    fn id(&self) -> &str {
        &self.entity_id
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.caps.clone()
    }

    /// `GET /api/states/{entity_id}` — devolve o objeto inteiro do HA
    /// (`state`, `attributes`, `last_changed`): o payload do hub, sem
    /// inventar um formato próprio em cima.
    async fn read(&self, capability: &str) -> Result<Value> {
        if capability != "state" {
            return Err(HardwareError::CapabilityDesconhecida {
                dispositivo: self.entity_id.clone(),
                capability: capability.to_string(),
            });
        }
        let path = format!("api/states/{}", self.entity_id);
        let url = self
            .config
            .base
            .join(&path)
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("url '{path}' malformada: {e}"),
            })?;
        let resp = self
            .config
            .http
            .get(url)
            .bearer_auth(&self.config.token)
            .send()
            .await
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("GET {path}: {e}"),
            })?;
        let status = resp.status();
        let bytes = read_capped(resp, BODY_CAP)
            .await
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("GET {path}: {e}"),
            })?;
        if !status.is_success() {
            return Err(HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("GET {path}: HTTP {status}{}", corpo_no_erro(&bytes)),
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| HardwareError::Adapter {
            dispositivo: self.entity_id.clone(),
            fonte: format!("GET {path}: corpo não é JSON: {e}"),
        })
    }

    /// `POST /api/services/{domain}/{service}` com o payload mapeado, args
    /// validados pelo mesmo [`crate::schema::validar_args`] do MQTT.
    ///
    /// O retorno `{"enviado": true}` é honesto como o `{"published": true}`
    /// do MQTT: confirma que o **hub** aceitou o comando, não que o
    /// dispositivo aplicou — para o estado aplicado, `read` na sequência.
    async fn execute(&self, capability: &str, args: Value) -> Result<Value> {
        let cap = self.caps.iter().find(|c| c.name == capability).ok_or(
            HardwareError::CapabilityDesconhecida {
                dispositivo: self.entity_id.clone(),
                capability: capability.to_string(),
            },
        )?;
        validar_args(&args, cap.args_schema.as_ref(), &self.entity_id, capability)?;

        let Some((servico, payload)) =
            servico_de(&self.dominio, capability, &args, &self.entity_id)
        else {
            return Err(HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("capability '{capability}' não mapeia para um serviço do HA"),
            });
        };

        let path = format!("api/services/{servico}");
        let url = self
            .config
            .base
            .join(&path)
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("url '{path}' malformada: {e}"),
            })?;
        let resp = self
            .config
            .http
            .post(url)
            .bearer_auth(&self.config.token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("POST {path}: {e}"),
            })?;
        let status = resp.status();
        let bytes = read_capped(resp, BODY_CAP)
            .await
            .map_err(|e| HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("POST {path}: {e}"),
            })?;
        if !status.is_success() {
            return Err(HardwareError::Adapter {
                dispositivo: self.entity_id.clone(),
                fonte: format!("POST {path}: HTTP {status}{}", corpo_no_erro(&bytes)),
            });
        }
        Ok(json!({"enviado": true, "entity_id": self.entity_id, "servico": servico}))
    }
}

/// Primeiros 200 bytes do corpo de erro — contexto para o modelo decidir o
/// próximo passo, sem estourar log com um HTML gigante de 404.
fn corpo_no_erro(bytes: &[u8]) -> String {
    let corpo = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]);
    if corpo.trim().is_empty() {
        String::new()
    } else {
        format!(" — {}", corpo.trim())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// O manager — descoberta, registro e o loop de WebSocket.
// ─────────────────────────────────────────────────────────────────────────────

/// Boot e loop de eventos do adapter Home Assistant. Um [`HaAdapterManager`]
/// por processo: uma conexão REST (descoberta) + um WebSocket de eventos.
/// Boot e loop de eventos do adapter Home Assistant. Um [`HaAdapterManager`]
/// por processo: uma conexão REST (descoberta) + um WebSocket de eventos.
pub struct HaAdapterManager {
    tarefa: tokio::task::JoinHandle<()>,
}

impl HaAdapterManager {
    /// Sobe a descoberta e o loop de eventos em uma task tokio.
    ///
    /// Chamar **dentro de um runtime tokio** (gateway e CLI já são). A
    /// descoberta é o primeiro ato da task: falha → registry vazio e o loop
    /// de eventos continua tentando (o hub pode só ter demorado a responder;
    /// um warn por rodada, sem retry agressivo).
    pub fn spawn(
        config: HaAdapterConfig,
        registry: Arc<DeviceRegistry>,
        state: Option<Arc<DeviceStateStore>>,
    ) -> Self {
        let tarefa = tokio::spawn(rodar(Arc::new(config), registry, state));
        Self { tarefa }
    }

    /// Para o loop de eventos (reload de config, shutdown de teste).
    pub async fn encerrar(self) {
        self.tarefa.abort();
        let _ = self.tarefa.await;
    }
}

async fn rodar(
    config: Arc<HaAdapterConfig>,
    registry: Arc<DeviceRegistry>,
    state: Option<Arc<DeviceStateStore>>,
) {
    // Cada rodada = descoberta + eventos. A descoberta é refeita a cada
    // reconexão de propósito: o hub pode não estar no ar no boot (a task
    // nasce antes do HA responder), e entidades novas aparecem com o uso.
    // Re-registrar o mesmo id substitui — o registry nunca guarda versão
    // velha. (Dispositivos removidos do HA continuam listados até o restart
    // — o registry não tem remoção; limite consciente do slice.)
    loop {
        match descobrir(&config).await {
            Ok(entidades) => {
                for e in entidades {
                    if let Some(store) = &state
                        && let Err(err) = store.marcar(&e.entity_id, e.online).await
                    {
                        tracing::warn!(entity = %e.entity_id, "presença inicial: {err}");
                    }
                    registry.register(Arc::new(HaDevice {
                        entity_id: e.entity_id.clone(),
                        dominio: e.dominio,
                        caps: e.caps,
                        config: config.clone(),
                    }));
                }
            }
            Err(e) => {
                tracing::warn!(
                    "hardware.home_assistant: descoberta falhou ({e}); registry segue \
                     como está — tentando de novo no próximo ciclo"
                );
            }
        }

        // Loop de eventos: conecta, autentica, assina `state_changed`; caiu,
        // espera e reconecta (mesmo padrão do canal Slack). Presença é o
        // consumo do slice — a #1128 liga as automações neste fluxo depois.
        match conectar_ws(&config).await {
            Ok(mut ws) => match autenticar_e_assinar(&mut ws, &config.token).await {
                Ok(()) => consumir_eventos(ws, state.as_ref()).await,
                Err(e) => tracing::warn!("hardware.home_assistant: handshake falhou: {e}"),
            },
            Err(e) => tracing::warn!("hardware.home_assistant: WebSocket indisponível: {e}"),
        }
        tokio::time::sleep(INTERVALO_RECONEXAO).await;
    }
}

/// `GET /api/states` — a lista de entidades do hub, bruta.
async fn descobrir(config: &HaAdapterConfig) -> Result<Vec<Entidade>> {
    let body = rest_get(config, "api/states").await?;
    parse_entidades(&body)
}

/// GET REST por `pinned_client` + `read_capped` (regra 14: teto de corpo e
/// redirects desligados são do guard, não da chamada).
async fn rest_get(config: &HaAdapterConfig, path: &str) -> Result<Vec<u8>> {
    let url = config.base.join(path).map_err(|e| {
        HardwareError::HomeAssistant(format!("path '{path}' não junta à base: {e}"))
    })?;
    let resp = config
        .http
        .get(url)
        .bearer_auth(&config.token)
        .send()
        .await
        .map_err(|e| HardwareError::HomeAssistant(format!("GET {path}: {e}")))?;
    let status = resp.status();
    let body = read_capped(resp, BODY_CAP)
        .await
        .map_err(|e| HardwareError::HomeAssistant(format!("GET {path}: {e}")))?;
    if !status.is_success() {
        return Err(HardwareError::HomeAssistant(format!(
            "GET {path}: HTTP {status}{}",
            corpo_no_erro(&body)
        )));
    }
    Ok(body)
}

// ─────────────────────────────────────────────────────────────────────────────
// WebSocket — autenticação e eventos `state_changed`.
// ─────────────────────────────────────────────────────────────────────────────

/// Conecta no **IP pinado** da config (sem re-resolver DNS) e faz o
/// handshake WebSocket. `client_async_tls_with_config` com connector default
/// cobre ws (plain) e wss (rustls + webpki-roots) sobre o mesmo TcpStream.
async fn conectar_ws(
    config: &HaAdapterConfig,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let ws_url = url_websocket(&config.base)?;
    let mut ultimo: Option<std::io::Error> = None;
    for addr in &config.addrs {
        match TcpStream::connect(addr).await {
            Ok(s) => {
                return tokio_tungstenite::client_async_tls_with_config(
                    ws_url.as_str().to_string(),
                    s,
                    None,
                    None,
                )
                .await
                .map(|(ws, _)| ws)
                .map_err(|e| HardwareError::HomeAssistant(format!("handshake WebSocket: {e}")));
            }
            Err(e) => ultimo = Some(e),
        }
    }
    Err(HardwareError::HomeAssistant(format!(
        "sem conexão ao hub {}: {:?}",
        config.host, ultimo
    )))
}

/// Deriva a URL do WebSocket da base: `http` → `ws`, `https` → `wss`,
/// path `/api/websocket`.
fn url_websocket(base: &url::Url) -> Result<url::Url> {
    let mut ws = base.clone();
    let novo = match ws.scheme() {
        "http" => "ws",
        "https" => "wss",
        outro => {
            return Err(HardwareError::HomeAssistant(format!(
                "esquema '{outro}' não vira WebSocket (use http/https na base)"
            )));
        }
    };
    ws.set_scheme(novo)
        .map_err(|_| HardwareError::HomeAssistant("esquema não trocável".to_string()))?;
    ws.set_path("/api/websocket");
    ws.set_query(None);
    ws.set_fragment(None);
    Ok(ws)
}

/// Handshake do protocolo HA: `auth_required` → `auth` (token) →
/// `auth_ok` → subscribe em `state_changed`. O token viaja aqui uma vez —
/// é o único lugar fora do header Bearer que o toca, e nunca vira log.
async fn autenticar_e_assinar(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    token: &str,
) -> Result<()> {
    exigir_msg(&proxima_msg(ws).await?, "auth_required")?;
    ws.send(Message::text(
        json!({"type": "auth", "access_token": token}).to_string(),
    ))
    .await
    .map_err(|e| HardwareError::HomeAssistant(format!("envio de auth: {e}")))?;
    exigir_msg(&proxima_msg(ws).await?, "auth_ok")?;
    ws.send(Message::text(
        json!({"type": "subscribe_events", "event_type": "state_changed", "id": 1}).to_string(),
    ))
    .await
    .map_err(|e| HardwareError::HomeAssistant(format!("envio de subscribe: {e}")))?;
    Ok(())
}

/// Próxima mensagem de texto; binário/control é ignorado (o tungstenite
/// responde ping por baixo), stream fechada é erro.
async fn proxima_msg(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> Result<String> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return Ok(t.to_string()),
            Some(Ok(_)) => continue,
            Some(Err(e)) => {
                return Err(HardwareError::HomeAssistant(format!(
                    "WebSocket durante handshake: {e}"
                )));
            }
            None => {
                return Err(HardwareError::HomeAssistant(
                    "WebSocket fechou no meio do handshake".to_string(),
                ));
            }
        }
    }
}

/// Confirma que a mensagem é `{"type": <esperado>}` — qualquer outra coisa
/// (inclusive `auth_invalid`) recusa com o que chegou, para o warn dizer o
/// que o hub mandou.
fn exigir_msg(msg: &str, esperado: &str) -> Result<()> {
    let v: Value = serde_json::from_str(msg)
        .map_err(|e| HardwareError::HomeAssistant(format!("handshake: não é JSON: {e}")))?;
    if v.get("type").and_then(Value::as_str) == Some(esperado) {
        Ok(())
    } else {
        Err(HardwareError::HomeAssistant(format!(
            "handshake: esperava '{esperado}', recebi: {msg}"
        )))
    }
}

/// Consome eventos até a conexão morrer (erro, fechamento, ping que não
/// sai, ou silêncio > [`KEEPALIVE`]). O stream é dividido (`split()`) porque
/// o `select!` precisa ler eventos **e** pingar — duas futures, cada uma
/// com seu lado. Cada `state_changed` marca presença do `entity_id`:
/// `new_state.state == "unavailable"` (ou `new_state: null` — estado
/// removido) → offline; qualquer estado real → online.
async fn consumir_eventos(
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    state: Option<&Arc<DeviceStateStore>>,
) {
    let (mut tx, mut rx) = ws.split();
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            // Backstop, não falha: casa quieta não gera evento por minutos —
            // debug, não warn, senão o log vira sinal falso.
            _ = tokio::time::sleep(KEEPALIVE) => {
                tracing::debug!(
                    "hardware.home_assistant: sem eventos por {:?}; reconectando",
                    KEEPALIVE
                );
                break;
            }
            // O ping é nosso write: erro aqui = conexão morta. O pong do hub
            // volta como control frame e não sobe pro loop.
            _ = ping.tick() => {
                if tx.send(Message::Ping(Vec::new().into())).await.is_err() {
                    tracing::warn!(
                        "hardware.home_assistant: ping sem resposta; reconectando"
                    );
                    break;
                }
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if let Some((entity_id, online)) = entidade_do_evento(t.as_str())
                        && let Some(store) = state
                        && let Err(err) = store.marcar(&entity_id, online).await
                    {
                        tracing::warn!(entity = %entity_id, "presença: {err}");
                    }
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    tracing::warn!("hardware.home_assistant: evento com erro: {e}");
                    break;
                }
                None => {
                    tracing::warn!("hardware.home_assistant: WebSocket fechado; reconectando");
                    break;
                }
            },
        }
    }
}

/// Extrai `(entity_id, online)` de um evento `state_changed`:
/// `{"id":1,"type":"event","event":{"event_type":"state_changed","data":
/// {"entity_id":"light.sala","new_state":{"state":"on",…}}}}`.
fn entidade_do_evento(msg: &str) -> Option<(String, bool)> {
    let v: Value = serde_json::from_str(msg).ok()?;
    let evento = v.get("event")?;
    if evento.get("event_type")?.as_str()? != "state_changed" {
        return None;
    }
    let dados = evento.get("data")?;
    let entity_id = dados.get("entity_id")?.as_str()?.to_string();
    let online = match dados.get("new_state") {
        Some(Value::Null) | None => false,
        Some(novo) => novo.get("state").and_then(Value::as_str) != Some("unavailable"),
    };
    Some((entity_id, online))
}

// ─────────────────────────────────────────────────────────────────────────────
// Testes de unidade — o que não precisa de hub: tabelas de risco, mapeamento
// de serviço, parse de entidades, vetting e derivação de URL.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// O risco por domínio é o contrato da issue: sensor/binary_sensor R0
    /// (leitura), light/switch/climate R1, cover R2, lock R3 — e nada
    /// além disso vira capability (sem R4/R5 neste slice).
    #[test]
    fn mapeia_riscos_por_dominio() {
        let caps = capabilities_de("light").expect("light suportado");
        let nomes: Vec<(&str, RiskClass)> =
            caps.iter().map(|c| (c.name.as_str(), c.risk)).collect();
        assert_eq!(
            nomes,
            vec![
                ("state", RiskClass::R0),
                ("power", RiskClass::R1),
                ("brightness", RiskClass::R1)
            ]
        );
        assert!(caps.iter().all(|c| c.validar().is_ok()));

        let sensor = capabilities_de("sensor").unwrap();
        assert_eq!(sensor.len(), 1);
        assert!(sensor[0].read_only && sensor[0].risk == RiskClass::R0);

        // cover: `state` é leitura R0; open/close/set_position são R2.
        let cover = capabilities_de("cover").unwrap();
        let acoes_cover: Vec<RiskClass> = cover
            .iter()
            .filter(|c| !c.read_only)
            .map(|c| c.risk)
            .collect();
        assert_eq!(
            acoes_cover,
            vec![RiskClass::R2, RiskClass::R2, RiskClass::R2]
        );

        // lock: `state` é leitura R0; lock/unlock são R3.
        let lock = capabilities_de("lock").unwrap();
        let acoes_lock: Vec<RiskClass> = lock
            .iter()
            .filter(|c| !c.read_only)
            .map(|c| c.risk)
            .collect();
        assert_eq!(acoes_lock, vec![RiskClass::R3, RiskClass::R3]);
    }

    /// Domínio fora da lista não vira dispositivo — fail-closed.
    #[test]
    fn dominio_desconhecido_e_recusado() {
        assert!(capabilities_de("vacuum").is_none());
        assert!(capabilities_de("media_player").is_none());
        assert!(capabilities_de("").is_none());
    }

    /// O vetting da config: link-local (metadata de cloud) é bloqueado
    /// mesmo com `AllowPrivate`; loopback e LAN passam (alvo legítimo).
    #[test]
    fn vetting_bloqueia_link_local_e_aceita_loopback() {
        let token = "t".to_string();
        let res = HaAdapterConfig::new("http://169.254.169.254:8123/", token.clone());
        assert!(res.is_err(), "link-local deve ser bloqueado");

        let cfg = HaAdapterConfig::new("http://127.0.0.1:8123", token).expect("loopback ok");
        assert_eq!(cfg.host, "127.0.0.1");
        // Base sem `/` no fim é normalizada — senão o `Url::join` de
        // `api/states` comeria o último segmento de um path tipo `/ha`.
        assert!(cfg.base.path().ends_with('/'));
        let j = cfg.base.join("api/states").unwrap();
        assert_eq!(j.as_str(), "http://127.0.0.1:8123/api/states");
    }

    /// `http` vira `ws` e `https` vira `wss`, path fixo `/api/websocket`.
    #[test]
    fn url_websocket_deriva_da_base() {
        let base: url::Url = "http://127.0.0.1:8123/".parse().unwrap();
        let ws = url_websocket(&base).unwrap();
        assert_eq!(ws.as_str(), "ws://127.0.0.1:8123/api/websocket");

        let base_https: url::Url = "https://ha.exemplo.io/".parse().unwrap();
        let wss = url_websocket(&base_https).unwrap();
        assert_eq!(wss.as_str(), "wss://ha.exemplo.io/api/websocket");
    }

    /// Esquema não-http na base não deriva WebSocket.
    #[test]
    fn url_websocket_recusa_esquema_estranho() {
        let base: url::Url = "ftp://127.0.0.1/".parse().unwrap();
        assert!(url_websocket(&base).is_err());
    }

    /// A tabela serviço: power → turn_on/turn_off, brightness → turn_on com
    /// brightness_pct, lock/unlock → lock/lock e lock/unlock.
    #[test]
    fn servico_mapeia_os_comandos_do_agente() {
        let entity = "light.sala";
        let (s, p) = servico_de("light", "power", &json!({"on": true}), entity).unwrap();
        assert_eq!(s, "light/turn_on");
        assert_eq!(p, json!({"entity_id": entity}));

        let (s, p) = servico_de("light", "power", &json!({"on": false}), entity).unwrap();
        assert_eq!(s, "light/turn_off");
        assert_eq!(p, json!({"entity_id": entity}));

        let (s, p) = servico_de(
            "light",
            "brightness",
            &json!({"brightness_pct": 50}),
            entity,
        )
        .unwrap();
        assert_eq!(s, "light/turn_on");
        assert_eq!(p, json!({"entity_id": entity, "brightness_pct": 50}));

        let (s, p) = servico_de("lock", "unlock", &json!({}), "lock.porta").unwrap();
        assert_eq!(s, "lock/unlock");
        assert_eq!(p, json!({"entity_id": "lock.porta"}));

        let (s, p) = servico_de(
            "cover",
            "set_position",
            &json!({"position": 80}),
            "cover.janela",
        )
        .unwrap();
        assert_eq!(s, "cover/set_cover_position");
        assert_eq!(p, json!({"entity_id": "cover.janela", "position": 80}));

        // (domínio, capability) fora da tabela → None.
        assert!(servico_de("vacuum", "power", &json!({"on": true}), "x").is_none());
        assert!(servico_de("light", "star_trek_mode", &json!({}), entity).is_none());
    }

    /// Parse de `/api/states`: só os domínios da lista passam, entity_id
    /// malformado cai fora, `unavailable` nasce offline.
    #[test]
    fn parse_entidades_filtra_e_classifica() {
        let body = serde_json::to_vec(&json!([
            {"entity_id": "light.sala", "state": "on", "attributes": {}},
            {"entity_id": "sensor.temp", "state": "23.5", "attributes": {}},
            {"entity_id": "binary_sensor.porta", "state": "off", "attributes": {}},
            {"entity_id": "lock.porta", "state": "locked", "attributes": {}},
            {"entity_id": "cover.janela", "state": "unavailable", "attributes": {}},
            {"entity_id": "switch.tomada", "state": "on", "attributes": {}},
            {"entity_id": "climate.ac", "state": "cool", "attributes": {}},
            {"entity_id": "vacuum.robo", "state": "cleaning", "attributes": {}},
            {"entity_id": "sem_ponto", "state": "?", "attributes": {}},
            {"entity_id": "light.Casa limpa", "state": "on", "attributes": {}},
            {"state": "on", "attributes": {}}
        ]))
        .unwrap();

        let entidades = parse_entidades(&body).unwrap();
        let ids: Vec<&str> = entidades.iter().map(|e| e.entity_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "light.sala",
                "sensor.temp",
                "binary_sensor.porta",
                "lock.porta",
                "cover.janela",
                "switch.tomada",
                "climate.ac"
            ],
            "vacuum/sem_ponto/objeto com espaço/sem entity_id caem fora"
        );

        let cover = entidades
            .iter()
            .find(|e| e.entity_id == "cover.janela")
            .unwrap();
        assert!(!cover.online, "unavailable nasce offline");

        let sensor = entidades
            .iter()
            .find(|e| e.entity_id == "sensor.temp")
            .unwrap();
        assert!(sensor.online);
        assert!(sensor.caps.iter().all(|c| c.risk == RiskClass::R0));
    }

    /// O evento `state_changed` do HA rende (entity_id, online) — e eventos
    /// de outro tipo não.
    #[test]
    fn evento_state_changed_rende_presenca() {
        let evento = json!({
            "id": 1, "type": "event",
            "event": {
                "event_type": "state_changed",
                "data": {
                    "entity_id": "light.sala",
                    "old_state": {"state": "off"},
                    "new_state": {"state": "on", "attributes": {}}
                }
            }
        })
        .to_string();
        assert_eq!(
            entidade_do_evento(&evento),
            Some(("light.sala".to_string(), true))
        );

        let offline = json!({
            "id": 1, "type": "event",
            "event": {"event_type": "state_changed", "data": {
                "entity_id": "light.sala", "new_state": {"state": "unavailable"}
            }}
        })
        .to_string();
        assert_eq!(
            entidade_do_evento(&offline),
            Some(("light.sala".to_string(), false))
        );

        // Estado removido (new_state: null) → offline.
        let removido = json!({
            "id": 1, "type": "event",
            "event": {"event_type": "state_changed", "data": {"entity_id": "light.sala", "new_state": null}}
        })
        .to_string();
        assert_eq!(
            entidade_do_evento(&removido),
            Some(("light.sala".to_string(), false))
        );

        // Outro tipo de evento → None.
        let outro =
            json!({"id": 2, "type": "event", "event": {"event_type": "call_service"}}).to_string();
        assert_eq!(entidade_do_evento(&outro), None);
    }

    /// Debug redige o token — o hub do HA é admin total da casa.
    #[test]
    fn debug_redige_o_token() {
        let cfg =
            HaAdapterConfig::new("http://127.0.0.1:8123/", "tokensecreto".to_string()).unwrap();
        let s = format!("{cfg:?}");
        assert!(!s.contains("tokensecreto"), "token vazou no Debug: {s}");
        assert!(s.contains("<redacted>"));
    }
}
