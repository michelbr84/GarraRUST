//! Adapter MQTT (#1126) — o primeiro transporte real do `garraia-hardware`.
//!
//! `rumqttc` (async puro, sem runtime C) atrás da feature `mqtt`. Este
//! módulo implementa a convenção de tópicos `garra/devices/...` e faz a
//! ponte entre o mundo MQTT e o [`crate::DeviceRegistry`]:
//!
//! | Tópico | Direção | Payload |
//! |---|---|---|
//! | `{p}/devices/{id}/capabilities` | device → gateway | [`DeviceManifest`] (retained) |
//! | `{p}/devices/{id}/status` | device → gateway | `"online"`/`"offline"` (LWT) |
//! | `{p}/devices/{id}/get/{cap}` | gateway → device | `{"request_id": ...}` |
//! | `{p}/devices/{id}/state/{cap}` | device → gateway | `{"request_id": ..., "value": ...}` |
//! | `{p}/devices/{id}/set/{cap}` | gateway → device | `{"request_id": ..., "args": ...}` |
//!
//! # Contratos
//!
//! - **Manifesto**: o dispositivo publica `{"id", "capabilities": [Capability]}` em
//!   `capabilities` **retained**, logo que conecta. Retained resolve a corrida
//!   clássica de descoberta — o manifesto sobrevive a reinícios do gateway e
//!   chega assinando. O risk class vem do **manifesto**, nunca inferido de
//!   payload em runtime; o parse valida a invariante leitura↔R0 na fronteira
//!   (`Capability::validar`) e recusa o dispositivo inteiro se violada.
//! - **Leitura**: publish em `get/{cap}` com `request_id` + espera da resposta
//!   correlacionada em `state/{cap}` até [`MqttAdapterConfig::timeout`]. Sem
//!   resposta → erro (nada de valor velho apresentado como atual).
//! - **Execução**: publish em `set/{cap}` com args mini-validados contra o
//!   subset truncado do `args_schema` (`type`/`properties`/`required`, tipos
//!   primitivos). O retorno `{"published": true}` é honesto: confirma que o
//!   **broker** aceitou a entrega, não que o dispositivo aplicou — para o
//!   estado aplicado, `read` na sequência. O gate (R0–R5) continua na camada
//!   de tools; o adapter não duplica a decisão.
//! - **Presença**: `status` (ou LWT do dispositivo no mesmo tópico) marca
//!   online/offline no [`DeviceStateStore`]. Recomendado retained, para o
//!   próximo subscriber já nascer sabendo.
//! - **Segurança**: a senha chega aqui já **resolvida do env** pelo chamador
//!   (`password_env` no config); nunca é serializada nem aparece no `Debug`
//!   (write-only, disciplina do settings registry).

use crate::Result;
use crate::capability::Capability;
use crate::device::Device;
use crate::error::HardwareError;
use crate::registry::DeviceRegistry;
use crate::state::DeviceStateStore;
use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, Publish, QoS};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

/// Prefixo de tópico padrão — a convenção `garra/devices/...` da issue.
pub const TOPIC_PREFIX_DEFAULT: &str = "garra";

/// Timeout padrão de leitura (correlação `get` → `state`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Capacidade do canal de requisições do cliente MQTT.
const CLIENT_CHANNEL_CAPACITY: usize = 64;

/// Mapa de leituras em voo: `request_id` → canal de resposta. Compartilhado
/// entre o `MqttDevice` (que registra) e o event loop do manager (que roteia).
type Pendentes = Mutex<HashMap<String, oneshot::Sender<Value>>>;

static SEQUENCIA: AtomicU64 = AtomicU64::new(0);

/// Novo `request_id` para correlação get/set → state. Contador do processo:
/// colisão só ocorre se duas leituras em voo partilharem id — impossível
/// dentro de um processo com contador monotônico.
fn novo_request_id() -> String {
    format!("req-{}", SEQUENCIA.fetch_add(1, Ordering::Relaxed))
}

// ─────────────────────────────────────────────────────────────────────────────
// Config do adapter — o que o gateway/CLI converte do `garraia-config`.
// ─────────────────────────────────────────────────────────────────────────────

/// Config de transporte do adapter MQTT.
///
/// `garraia-hardware` **não** depende de `garraia-config` — o gateway recebe
/// `hardware.mqtt` do config (`MqttConfig`), resolve `password_env` e monta
/// esta struct. O parse de `host:porta` vive aqui para ser testável no crate.
pub struct MqttAdapterConfig {
    /// Host ou IP do broker (sem porta).
    pub broker_host: String,
    /// Porta do broker (1–65535).
    pub broker_port: u16,
    /// Username MQTT, se o broker exigir.
    pub username: Option<String>,
    /// A senha JÁ RESOLVIDA do env — value-only, nunca logada (o `Debug`
    /// desta struct redige) e nunca serializada.
    pub password: Option<String>,
    /// Client id do gateway no broker.
    pub client_id: String,
    /// Prefixo de tópico (default [`TOPIC_PREFIX_DEFAULT`]).
    pub topic_prefix: String,
    /// Quanto tempo uma leitura espera a resposta correlacionada.
    pub timeout: Duration,
}

impl MqttAdapterConfig {
    /// Config a partir de `broker` na forma `host:porta`.
    ///
    /// Literais IPv6 (`[::1]:1883`) não são suportados neste slice — os
    /// brokers domésticos rodando no mesmo host usam `127.0.0.1`.
    pub fn new(broker: &str, username: Option<String>, password: Option<String>) -> Result<Self> {
        let mut partes = broker.rsplitn(2, ':');
        // rsplitn(2, ':') devolve [porta, host] — o host vem por último.
        let (port_str, host) = (
            partes.next().unwrap_or_default(),
            partes.next().unwrap_or_default(),
        );
        if host.is_empty() || port_str.is_empty() {
            return Err(HardwareError::Mqtt(format!(
                "broker '{broker}' malformado: esperado host:porta"
            )));
        }
        let broker_port: u16 = port_str
            .parse()
            .map_err(|_| HardwareError::Mqtt(format!("broker '{broker}': porta não é número")))?;
        if broker_port == 0 {
            return Err(HardwareError::Mqtt(format!(
                "broker '{broker}': porta 0 é inválida"
            )));
        }
        Ok(Self {
            broker_host: host.to_string(),
            broker_port,
            username,
            password,
            client_id: "garra-hardware".to_string(),
            topic_prefix: TOPIC_PREFIX_DEFAULT.to_string(),
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Seta o client id (único por conexão — dois clientes com o mesmo id
    /// se des conectam em loop no broker).
    #[must_use]
    pub fn com_client_id(mut self, client_id: String) -> Self {
        self.client_id = client_id;
        self
    }

    /// Seta o prefixo de tópico.
    #[must_use]
    pub fn com_prefixo(mut self, topic_prefix: String) -> Self {
        self.topic_prefix = topic_prefix;
        self
    }

    /// Seta o timeout de leitura.
    #[must_use]
    pub fn com_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for MqttAdapterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MqttAdapterConfig")
            .field("broker_host", &self.broker_host)
            .field("broker_port", &self.broker_port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("client_id", &self.client_id)
            .field("topic_prefix", &self.topic_prefix)
            .field("timeout", &self.timeout)
            .finish()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tópicos — a convenção é o contrato; funções nomeadas, ninguém remonta
// string de tópico à mão.
// ─────────────────────────────────────────────────────────────────────────────

fn topico_get(prefixo: &str, id: &str, cap: &str) -> String {
    format!("{prefixo}/devices/{id}/get/{cap}")
}

fn topico_set(prefixo: &str, id: &str, cap: &str) -> String {
    format!("{prefixo}/devices/{id}/set/{cap}")
}

/// Os três filtros que o manager assina. `+` cobre o id do dispositivo.
fn filtros_assinatura(prefixo: &str) -> [String; 3] {
    [
        format!("{prefixo}/devices/+/capabilities"),
        format!("{prefixo}/devices/+/status"),
        format!("{prefixo}/devices/+/state/+"),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Manifesto de descoberta — parse + validação na fronteira.
// ─────────────────────────────────────────────────────────────────────────────

/// O que um dispositivo publica (retained) em `{p}/devices/{id}/capabilities`.
///
/// O risk class de cada capability vem **aqui** — o manifesto do dispositivo
/// é a declaração de risco; payload em runtime nunca reclassifica (#1126,
/// §Segurança).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManifest {
    /// Id do dispositivo — tem que bater com o `{id}` do tópico.
    pub id: String,
    /// As capabilities expostas, cada uma com o risco declarado.
    pub capabilities: Vec<Capability>,
}

/// Parseia (e valida) um manifesto vindo do broker.
///
/// Falha fechada: qualquer violação — risco incoerente com read_only, nome
/// com caractere de tópico, id malformado — recusa o dispositivo inteiro.
fn parse_manifesto(payload: &[u8]) -> Result<DeviceManifest> {
    let manifesto: DeviceManifest = serde_json::from_slice(payload)
        .map_err(|e| HardwareError::Mqtt(format!("manifesto não é JSON válido: {e}")))?;
    if manifesto.id.is_empty() {
        return Err(HardwareError::Mqtt(
            "manifesto sem id de dispositivo".to_string(),
        ));
    }
    valida_segmento_topico(&manifesto.id)?;
    for cap in &manifesto.capabilities {
        if cap.name.is_empty() {
            return Err(HardwareError::Mqtt(format!(
                "manifesto do dispositivo '{}' tem capability sem nome",
                manifesto.id
            )));
        }
        valida_segmento_topico(&cap.name).map_err(|e| {
            HardwareError::Mqtt(format!(
                "capability '{}' do dispositivo '{}': {e}",
                cap.name, manifesto.id
            ))
        })?;
        cap.validar().map_err(|e| {
            HardwareError::Mqtt(format!("manifesto do dispositivo '{}': {e}", manifesto.id))
        })?;
    }
    Ok(manifesto)
}

/// Nada de `/`, `+` ou `#` em segmentos de tópico — id e capability viajam
/// dentro de tópicos, e um desses caracteres quebraria a convenção.
fn valida_segmento_topico(segmento: &str) -> Result<()> {
    if segmento.is_empty()
        || segmento.contains('/')
        || segmento.contains('+')
        || segmento.contains('#')
    {
        return Err(HardwareError::Mqtt(format!(
            "segmento '{segmento}' inválido: não pode ser vazio nem conter '/', '+' ou '#'"
        )));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// MqttDevice — o Device que fala MQTT.
// ─────────────────────────────────────────────────────────────────────────────

/// Um dispositivo descoberto via MQTT. Clona o cliente (barato — canal
/// interno) e publica `get`/`set`; a resposta do `get` chega pelo event loop
/// do manager, que roteia pelo `request_id`.
pub struct MqttDevice {
    id: String,
    caps: Vec<Capability>,
    client: AsyncClient,
    prefixo: String,
    pendentes: Arc<Pendentes>,
    timeout: Duration,
}

impl MqttDevice {
    fn new(
        id: String,
        caps: Vec<Capability>,
        client: AsyncClient,
        prefixo: String,
        pendentes: Arc<Pendentes>,
        timeout: Duration,
    ) -> Self {
        Self {
            id,
            caps,
            client,
            prefixo,
            pendentes,
            timeout,
        }
    }

    fn capability(&self, name: &str) -> Result<&Capability> {
        self.caps.iter().find(|c| c.name == name).ok_or_else(|| {
            HardwareError::CapabilityDesconhecida {
                dispositivo: self.id.clone(),
                capability: name.to_string(),
            }
        })
    }

    fn erro_adapter(&self, fonte: String) -> HardwareError {
        HardwareError::Adapter {
            dispositivo: self.id.clone(),
            fonte,
        }
    }
}

#[async_trait]
impl Device for MqttDevice {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.caps.clone()
    }

    async fn read(&self, capability: &str) -> Result<Value> {
        self.capability(capability)?;
        let rid = novo_request_id();
        let (tx, rx) = oneshot::channel();
        self.pendentes.lock().await.insert(rid.clone(), tx);

        let topico = topico_get(&self.prefixo, &self.id, capability);
        let publicar = self
            .client
            .publish(
                &topico,
                QoS::AtLeastOnce,
                false,
                json!({ "request_id": rid }).to_string(),
            )
            .await;
        if let Err(e) = publicar {
            self.pendentes.lock().await.remove(&rid);
            return Err(self.erro_adapter(format!("falha ao publicar '{topico}': {e}")));
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(valor)) => Ok(valor),
            Ok(Err(_)) => {
                Err(self
                    .erro_adapter("resposta descartada pelo manager (canal fechado)".to_string()))
            }
            Err(_) => {
                self.pendentes.lock().await.remove(&rid);
                Err(self.erro_adapter(format!(
                    "sem resposta em {:?} (correlação get→state em '{topico}')",
                    self.timeout
                )))
            }
        }
    }

    async fn execute(&self, capability: &str, args: Value) -> Result<Value> {
        let cap = self.capability(capability)?;
        crate::schema::validar_args(&args, cap.args_schema.as_ref(), &self.id, capability)?;
        let rid = novo_request_id();
        let topico = topico_set(&self.prefixo, &self.id, capability);
        self.client
            .publish(
                &topico,
                QoS::AtLeastOnce,
                false,
                json!({ "request_id": rid, "args": args }).to_string(),
            )
            .await
            .map_err(|e| self.erro_adapter(format!("falha ao publicar '{topico}': {e}")))?;
        // Honestidade do transporte: só o broker confirmou a entrega. O
        // estado aplicado é o que `read` devolver depois.
        Ok(json!({
            "device": self.id,
            "capability": capability,
            "published": true,
            "request_id": rid,
        }))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// O manager — event loop único para o gateway inteiro.
// ─────────────────────────────────────────────────────────────────────────────

/// Boot e event loop do adapter MQTT. Um [`MqttAdapterManager`] por
/// processo: uma conexão, três assinaturas, N dispositivos.
pub struct MqttAdapterManager {
    client: AsyncClient,
    tarefa: tokio::task::JoinHandle<()>,
}

impl MqttAdapterManager {
    /// Sobe a conexão e o event loop em uma task tokio.
    ///
    /// Chamar **dentro de um runtime tokio** (gateway e CLI já são).
    /// As assinaturas são (re)emitidas a cada `ConnAck` — reconnect do
    /// rumqttc não re-assina sozinho com `clean_session: true`.
    pub fn spawn(
        config: MqttAdapterConfig,
        registry: Arc<DeviceRegistry>,
        state: Option<Arc<DeviceStateStore>>,
    ) -> Result<Self> {
        let mut opts = MqttOptions::new(
            config.client_id.clone(),
            config.broker_host.clone(),
            config.broker_port,
        );
        if let Some(username) = &config.username {
            // Senha sem username não forma par de credenciais MQTT — entra
            // como anônimo. O `config check` avisa quando só há password_env.
            opts.set_credentials(username, config.password.clone().unwrap_or_default());
        }
        let (client, eventloop) = AsyncClient::new(opts, CLIENT_CHANNEL_CAPACITY);
        let pendentes: Arc<Pendentes> = Arc::default();
        let tarefa = tokio::spawn(rodar(
            config,
            eventloop,
            client.clone(),
            registry,
            state,
            pendentes,
        ));
        Ok(Self { client, tarefa })
    }

    /// O cliente MQTT — para publicações fora do ciclo device (automações,
    /// #1128) que queiram falar a mesma convenção de tópicos.
    pub fn client(&self) -> &AsyncClient {
        &self.client
    }

    /// Para o event loop (reload de config, shutdown de teste).
    pub async fn encerrar(self) {
        self.tarefa.abort();
        let _ = self.tarefa.await;
    }
}

async fn rodar(
    config: MqttAdapterConfig,
    mut eventloop: EventLoop,
    client: AsyncClient,
    registry: Arc<DeviceRegistry>,
    state: Option<Arc<DeviceStateStore>>,
    pendentes: Arc<Pendentes>,
) {
    let prefixo = config.topic_prefix.clone();
    let filtros = filtros_assinatura(&prefixo);
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                for filtro in &filtros {
                    if let Err(e) = client.subscribe(filtro.clone(), QoS::AtLeastOnce).await {
                        tracing::warn!("mqtt: falha ao assinar '{filtro}': {e}");
                    }
                }
            }
            Ok(Event::Incoming(Incoming::Publish(p))) => {
                processa_publish(
                    &prefixo,
                    &p,
                    &client,
                    &registry,
                    state.as_ref(),
                    &pendentes,
                    config.timeout,
                )
                .await;
            }
            Ok(_) => {}
            Err(e) => {
                // rumqttc reconecta no próximo poll (backoff próprio); as
                // assinaturas voltam no ConnAck — por isso o handler acima.
                tracing::warn!(error = %e, "mqtt: event loop com erro; reconectando");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn processa_publish(
    prefixo: &str,
    p: &Publish,
    client: &AsyncClient,
    registry: &DeviceRegistry,
    state: Option<&Arc<DeviceStateStore>>,
    pendentes: &Arc<Pendentes>,
    timeout: Duration,
) {
    let Some(resto) = p
        .topic
        .strip_prefix(&format!("{prefixo}/devices/"))
        .map(str::to_owned)
    else {
        return;
    };
    let segmentos: Vec<&str> = resto.split('/').collect();
    match segmentos.as_slice() {
        [id, "capabilities"] => {
            registrar_dispositivo(
                id,
                &p.payload,
                client,
                prefixo,
                registry,
                state,
                pendentes.clone(),
                timeout,
            )
            .await;
        }
        [id, "status"] => {
            if let Some(store) = state {
                aplicar_status(id, &p.payload, store).await;
            }
        }
        [id, "state", cap] => {
            // A resposta de leitura não depende de quem publicou — correlação
            // por request_id, não por tópico. O {id} serve só para o log.
            rotear_estado(id, cap, &p.payload, pendentes).await;
        }
        _ => {
            // `get`/`set` são outbound; tópicos fora do contrato, ignorados.
            tracing::debug!(topic = %p.topic, "mqtt: mensagem fora do contrato, ignorada");
        }
    }
}

async fn registrar_dispositivo(
    id_topico: &str,
    payload: &[u8],
    client: &AsyncClient,
    prefixo: &str,
    registry: &DeviceRegistry,
    state: Option<&Arc<DeviceStateStore>>,
    pendentes: Arc<Pendentes>,
    timeout: Duration,
) {
    let manifesto = match parse_manifesto(payload) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(topico = %id_topico, error = %e, "mqtt: manifesto inválido — dispositivo ignorado");
            return;
        }
    };
    if manifesto.id != id_topico {
        tracing::warn!(
            topico = %id_topico,
            id_payload = %manifesto.id,
            "mqtt: id do manifesto difere do tópico — dispositivo ignorado (fail-closed)"
        );
        return;
    }
    let device = MqttDevice::new(
        manifesto.id.clone(),
        manifesto.capabilities,
        client.clone(),
        prefixo.to_string(),
        pendentes,
        timeout,
    );
    registry.register(Arc::new(device));
    if let Some(store) = state
        && let Err(e) = store.marcar(id_topico, true).await
    {
        tracing::warn!(dispositivo = %id_topico, error = %e, "mqtt: falha ao marcar presença");
    }
    tracing::info!(dispositivo = %id_topico, "mqtt: dispositivo descoberto e registrado");
}

async fn aplicar_status(id: &str, payload: &[u8], store: &Arc<DeviceStateStore>) {
    let texto = String::from_utf8_lossy(payload).trim().to_ascii_lowercase();
    let online = match texto.as_str() {
        "online" => true,
        "offline" => false,
        _ => {
            tracing::warn!(dispositivo = %id, "mqtt: status desconhecido '{texto}' — ignorado");
            return;
        }
    };
    if let Err(e) = store.marcar(id, online).await {
        tracing::warn!(dispositivo = %id, error = %e, "mqtt: falha ao marcar presença");
    }
}

/// A resposta de leitura: `{"request_id": ..., "value": ...}` roteada para
/// o canal do `read` que está esperando. Sem pending para o id, é estado
/// não solicitado — log de debug e segue.
async fn rotear_estado(id: &str, cap: &str, payload: &[u8], pendentes: &Arc<Pendentes>) {
    #[derive(Deserialize)]
    struct RespostaEstado {
        request_id: String,
        #[serde(default)]
        value: Value,
    }
    let Ok(mensagem) = serde_json::from_slice::<RespostaEstado>(payload) else {
        tracing::debug!(dispositivo = %id, capability = %cap, "mqtt: state sem request_id legível — ignorado");
        return;
    };
    if let Some(tx) = pendentes.lock().await.remove(&mensagem.request_id) {
        let _ = tx.send(mensagem.value);
    } else {
        tracing::debug!(dispositivo = %id, capability = %cap, "mqtt: estado não solicitado — ignorado");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::RiskClass;
    use serde_json::json;

    // ─── Config ────────────────────────────────────────────────────────────

    #[test]
    fn config_parseia_host_e_porta() {
        let cfg = MqttAdapterConfig::new("127.0.0.1:1883", None, None).expect("válido");
        assert_eq!(cfg.broker_host, "127.0.0.1");
        assert_eq!(cfg.broker_port, 1883);
        assert_eq!(cfg.client_id, "garra-hardware");
        assert_eq!(cfg.topic_prefix, "garra");
        assert_eq!(cfg.timeout, DEFAULT_TIMEOUT);
        assert!(cfg.username.is_none());
    }

    #[test]
    fn config_recusa_broker_malformado() {
        for broker in ["127.0.0.1", "127.0.0.1:mqtt", "127.0.0.1:0", ":1883", ""] {
            assert!(
                MqttAdapterConfig::new(broker, None, None).is_err(),
                "broker '{broker}' deveria ser recusado"
            );
        }
    }

    #[test]
    fn config_debug_nunca_mostra_senha() {
        let cfg = MqttAdapterConfig::new(
            "127.0.0.1:1883",
            Some("meu-user".to_string()),
            Some("senha-super-secreta".to_string()),
        )
        .expect("válido");
        let debug = format!("{cfg:?}");
        assert!(debug.contains("<redacted>"), "senha redigida: {debug}");
        assert!(
            !debug.contains("senha-super-secreta"),
            "senha vazou: {debug}"
        );
        assert!(
            debug.contains("meu-user"),
            "username aparece (não é secret)"
        );
    }

    // ─── Tópicos ───────────────────────────────────────────────────────────

    /// Os tópicos inbound (manifesto/status/estado) são construídos só por
    /// quem publica — o gateway não publica neles. Os builders vivem nos
    /// testes; a convenção fica travada aqui.
    fn topico_manifesto(prefixo: &str, id: &str) -> String {
        format!("{prefixo}/devices/{id}/capabilities")
    }

    fn topico_status(prefixo: &str, id: &str) -> String {
        format!("{prefixo}/devices/{id}/status")
    }

    fn topico_estado(prefixo: &str, id: &str, cap: &str) -> String {
        format!("{prefixo}/devices/{id}/state/{cap}")
    }

    #[test]
    fn convencao_de_topicos_e_filtro() {
        assert_eq!(
            topico_manifesto("garra", "lampada"),
            "garra/devices/lampada/capabilities"
        );
        assert_eq!(
            topico_status("garra", "lampada"),
            "garra/devices/lampada/status"
        );
        assert_eq!(
            topico_get("garra", "lampada", "power"),
            "garra/devices/lampada/get/power"
        );
        assert_eq!(
            topico_set("garra", "lampada", "power"),
            "garra/devices/lampada/set/power"
        );
        assert_eq!(
            topico_estado("garra", "lampada", "power"),
            "garra/devices/lampada/state/power"
        );
        assert_eq!(
            filtros_assinatura("garra"),
            [
                "garra/devices/+/capabilities".to_string(),
                "garra/devices/+/status".to_string(),
                "garra/devices/+/state/+".to_string(),
            ]
        );
    }

    // ─── Manifesto ─────────────────────────────────────────────────────────

    fn manifesto_ok() -> Value {
        json!({
            "id": "sensor-1",
            "capabilities": [
                { "name": "temperature", "risk": "r0", "read_only": true },
                {
                    "name": "power",
                    "risk": "r1",
                    "read_only": false,
                    "args_schema": {
                        "type": "object",
                        "properties": { "on": { "type": "boolean" } },
                        "required": ["on"]
                    }
                }
            ]
        })
    }

    #[test]
    fn manifesto_parseia_risco_e_schema() {
        let m = parse_manifesto(manifesto_ok().to_string().as_bytes()).expect("válido");
        assert_eq!(m.id, "sensor-1");
        assert_eq!(m.capabilities.len(), 2);
        let power = &m.capabilities[1];
        assert_eq!(power.risk, RiskClass::R1);
        assert!(!power.read_only);
        assert!(power.args_schema.is_some());
    }

    #[test]
    fn manifesto_invalido_recusado_na_fronteira() {
        // read_only com R3 — viola a invariante leitura↔R0.
        let json = json!({
            "id": "s",
            "capabilities": [{ "name": "x", "risk": "r3", "read_only": true }]
        });
        let err = parse_manifesto(json.to_string().as_bytes()).expect_err("recusado");
        assert!(err.to_string().contains("R3"), "erro cita o risco: {err}");

        // risk fora da tabela não desserializa.
        let json = json!({
            "id": "s",
            "capabilities": [{ "name": "x", "risk": "r9", "read_only": false }]
        });
        assert!(parse_manifesto(json.to_string().as_bytes()).is_err());

        // nome com '/' quebraria a convenção de tópicos.
        let json = json!({
            "id": "s",
            "capabilities": [{ "name": "a/b", "risk": "r1", "read_only": false }]
        });
        let err = parse_manifesto(json.to_string().as_bytes()).expect_err("recusado");
        assert!(err.to_string().contains("a/b"), "erro cita o nome: {err}");

        // id com caractere de wildcard de tópico.
        let json = json!({
            "id": "sen+sor",
            "capabilities": [{ "name": "x", "risk": "r1", "read_only": false }]
        });
        assert!(parse_manifesto(json.to_string().as_bytes()).is_err());
    }

    // ─── Mini-validador ────────────────────────────────────────────────────

    #[test]
    fn validar_args_aceita_tipos_primitivos() {
        let schema = json!({
            "type": "object",
            "properties": {
                "on": { "type": "boolean" },
                "percent": { "type": "integer" },
                "tempo": { "type": "number" },
                "nome": { "type": "string" },
                "lista": { "type": "array" },
                "extra": { "type": "object" },
                "nada": { "type": "null" }
            },
            "required": ["on"]
        });
        let args = json!({
            "on": true, "percent": 50, "tempo": 1.5,
            "lista": [1, 2], "extra": {}, "nada": null
        });
        assert!(crate::schema::validar_args(&args, Some(&schema), "dev", "cap").is_ok());
    }

    #[test]
    fn validar_args_recusa_erros_tipicos() {
        let schema = json!({
            "type": "object",
            "properties": { "on": { "type": "boolean" } },
            "required": ["on"]
        });
        // obrigatório ausente
        let err = crate::schema::validar_args(&json!({}), Some(&schema), "dev", "power").expect_err("recusa");
        assert!(err.to_string().contains("obrigatório"), "{err}");
        // tipo errado
        let err = crate::schema::validar_args(&json!({ "on": "sim" }), Some(&schema), "dev", "power")
            .expect_err("recusa");
        assert!(err.to_string().contains("boolean"), "{err}");
        // 50.5 não é integer
        let schema_int =
            json!({ "type": "object", "properties": { "percent": { "type": "integer" } } });
        let err = crate::schema::validar_args(&json!({ "percent": 50.5 }), Some(&schema_int), "dev", "cap")
            .expect_err("recusa");
        assert!(err.to_string().contains("integer"), "{err}");
        // args não-objeto
        let err = crate::schema::validar_args(&json!(true), Some(&schema), "dev", "power").expect_err("recusa");
        assert!(err.to_string().contains("objeto"), "{err}");
    }

    #[test]
    fn validar_args_sem_schema_recusa_argumentos() {
        assert!(crate::schema::validar_args(&json!({}), None, "dev", "power").is_ok());
        assert!(crate::schema::validar_args(&json!(null), None, "dev", "power").is_ok());
        let err = crate::schema::validar_args(&json!({ "x": 1 }), None, "dev", "power").expect_err("recusa");
        assert!(err.to_string().contains("não declara argumentos"), "{err}");
    }

    #[test]
    fn validar_args_fail_closed_em_schema_que_nao_entende() {
        // tipo de propriedade desconhecido
        let schema = json!({ "type": "object", "properties": { "x": { "type": "bloco" } } });
        let err = crate::schema::validar_args(&json!({ "x": 1 }), Some(&schema), "dev", "cap")
            .expect_err("recusa");
        assert!(err.to_string().contains("fail-closed"), "{err}");
        // propriedade sem type declarado
        let schema = json!({ "type": "object", "properties": { "x": {} } });
        let err = crate::schema::validar_args(&json!({ "x": 1 }), Some(&schema), "dev", "cap")
            .expect_err("recusa");
        assert!(err.to_string().contains("não declara o tipo"), "{err}");
        // schema de nível de topo não-objeto
        let err = crate::schema::validar_args(&json!({}), Some(&json!([])), "dev", "cap").expect_err("recusa");
        assert!(err.to_string().contains("objeto JSON"), "{err}");
    }

    // ─── Integração (broker embutido rumqttd) ──────────────────────────────

    const P: &str = "garra";

    /// Escolhe uma porta livre (efêmera) e devolve para o broker do teste.
    fn porta_livre() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("bind efêmero")
            .local_addr()
            .expect("addr")
            .port()
    }

    /// rumqttd in-process, thread própria: `Broker::start` é blocking e
    /// spawna os servers em threads internas. RouterConfig precisa de
    /// valores reais (o Default tem zeros que o router recusa/panica).
    fn subir_broker(porta: u16) {
        let config = rumqttd::Config {
            id: 1,
            router: rumqttd::RouterConfig {
                max_connections: 32,
                max_outgoing_packet_count: 100_000,
                max_segment_size: 1024 * 1024,
                max_segment_count: 1000,
                ..rumqttd::RouterConfig::default()
            },
            v4: Some(
                std::iter::once((
                    "teste".to_string(),
                    rumqttd::ServerSettings {
                        name: "teste".to_string(),
                        listen: std::net::SocketAddr::from(([127, 0, 0, 1], porta)),
                        tls: None,
                        next_connection_delay_ms: 0,
                        connections: rumqttd::ConnectionSettings {
                            connection_timeout_ms: 10_000,
                            max_payload_size: 1024 * 1024,
                            max_inflight_count: 100,
                            auth: None,
                            external_auth: None,
                            dynamic_filters: false,
                        },
                    },
                ))
                .collect::<std::collections::HashMap<String, rumqttd::ServerSettings>>(),
            ),
            ..rumqttd::Config::default()
        };
        std::thread::spawn(move || {
            let mut broker = rumqttd::Broker::new(config);
            let _ = broker.start();
        });
    }

    /// Espera o broker aceitar TCP (o start() do rumqttd é assíncrono em
    /// relação ao chamador).
    async fn esperar_broker(porta: u16) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], porta)),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "broker não subiu na porta {porta}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn config_manager(porta: u16) -> MqttAdapterConfig {
        MqttAdapterConfig::new(&format!("127.0.0.1:{porta}"), None, None)
            .expect("config válida")
            .com_client_id(format!("garra-hw-{porta}"))
            .com_timeout(Duration::from_secs(10))
    }

    /// Client MQTT do "dispositivo" do teste.
    fn client_device(porta: u16, id: &str) -> (AsyncClient, EventLoop) {
        AsyncClient::new(
            rumqttc::MqttOptions::new(format!("dev-{id}-{porta}"), "127.0.0.1", porta),
            16,
        )
    }

    async fn esperar(mut cond: impl FnMut() -> bool, descricao: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(
                std::time::Instant::now() < deadline,
                "timeout esperando: {descricao}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Espera a presença do dispositivo chegar ao estado esperado no store.
    async fn esperar_presenca(
        state: &Arc<DeviceStateStore>,
        id: &str,
        esperado_online: bool,
        descricao: &str,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let bate = match state.estado(id).await {
                Ok(Some(p)) => p.online == esperado_online,
                _ => false,
            };
            if bate {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "timeout: {descricao}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// O ciclo completo da issue #1126 sobre um broker embutido: manifesto
    /// retained → descoberta; get→state com request_id → leitura; set com
    /// args validados → execução (o device recebe); manifesto com id
    /// divergente não registra; status "offline" marca presença.
    #[tokio::test]
    async fn ciclo_completo_descoberta_leitura_e_execucao() {
        let porta = porta_livre();
        subir_broker(porta);
        esperar_broker(porta).await;

        // O "dispositivo": assina get/set e responde get com estado fixo.
        let (device_client, mut device_ev) = client_device(porta, "sensor-1");
        // O responder consome um clone do client; o original publica mais
        // tarde (manifesto divergente, status offline).
        let device_responder = device_client.clone();
        device_client
            .subscribe(topico_get(P, "sensor-1", "+"), QoS::AtLeastOnce)
            .await
            .expect("assina get");
        device_client
            .subscribe(topico_set(P, "sensor-1", "+"), QoS::AtLeastOnce)
            .await
            .expect("assina set");
        device_client
            .publish(
                topico_manifesto(P, "sensor-1"),
                QoS::AtLeastOnce,
                true,
                manifesto_ok().to_string(),
            )
            .await
            .expect("publica manifesto retained");
        device_client
            .publish(
                topico_status(P, "sensor-1"),
                QoS::AtLeastOnce,
                true,
                "online",
            )
            .await
            .expect("publica status retained");

        let recebidos: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let task_recebidos = recebidos.clone();
        let device_task = device_responder;
        tokio::spawn(async move {
            loop {
                match device_ev.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        if !(p.topic.contains("/get/") || p.topic.contains("/set/")) {
                            continue;
                        }
                        let Ok(carga) = serde_json::from_slice::<Value>(&p.payload) else {
                            continue;
                        };
                        task_recebidos
                            .lock()
                            .await
                            .push((p.topic.clone(), carga.clone()));
                        if p.topic.contains("/get/") {
                            let cap = p.topic.rsplit('/').next().unwrap_or("");
                            let rid = carga["request_id"].as_str().unwrap_or("").to_string();
                            let _ = device_task
                                .publish(
                                    topico_estado(P, "sensor-1", cap),
                                    QoS::AtLeastOnce,
                                    false,
                                    json!({ "request_id": rid, "value": { "celsius": 23.0 } })
                                        .to_string(),
                                )
                                .await;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        let registry = Arc::new(DeviceRegistry::new());
        let state = Arc::new(DeviceStateStore::em_memoria().expect("store"));
        let manager =
            MqttAdapterManager::spawn(config_manager(porta), registry.clone(), Some(state.clone()))
                .expect("manager sobe");

        // 1. Descoberta: o manifesto retained chega na assinatura.
        esperar(
            || registry.get("sensor-1").is_some(),
            "dispositivo registrado",
        )
        .await;
        let dev = registry.get("sensor-1").expect("registrado");
        let caps = dev.capabilities();
        let nomes: Vec<&str> = caps.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            nomes,
            vec!["temperature", "power"],
            "capabilities do manifesto"
        );

        // Presença marcada online pela descoberta.
        esperar_presenca(&state, "sensor-1", true, "presença online pela descoberta").await;

        // 2. Leitura com correlação request_id.
        let valor = dev.read("temperature").await.expect("lê temperature");
        assert_eq!(valor, json!({ "celsius": 23.0 }));
        esperar(
            || {
                recebidos
                    .try_lock()
                    .map(|r| {
                        r.iter().any(|(t, c)| {
                            t.ends_with("/get/temperature") && c["request_id"].is_string()
                        })
                    })
                    .unwrap_or(false)
            },
            "get chegou no dispositivo",
        )
        .await;

        // 3. Execução: args válidos são publicados com request_id.
        let resultado = dev
            .execute("power", json!({ "on": true }))
            .await
            .expect("executa");
        assert_eq!(resultado["published"], json!(true));
        assert_eq!(resultado["device"], json!("sensor-1"));
        esperar(
            || {
                recebidos
                    .try_lock()
                    .map(|r| {
                        r.iter().any(|(t, c)| {
                            t.ends_with("/set/power") && c["args"] == json!({ "on": true })
                        })
                    })
                    .unwrap_or(false)
            },
            "set chegou no dispositivo",
        )
        .await;

        // 4. Args fora do schema são recusados antes de virar publish.
        let err = dev
            .execute("power", json!({ "on": "sim" }))
            .await
            .expect_err("recusa tipo errado");
        assert!(err.to_string().contains("boolean"), "{err}");
        let err = dev
            .execute("power", json!({}))
            .await
            .expect_err("recusa obrigatório ausente");
        assert!(err.to_string().contains("obrigatório"), "{err}");
        assert!(dev.read("nada").await.is_err(), "capability desconhecida");

        // 5. Manifesto com id divergente do tópico: fail-closed, não registra.
        let ghost = json!({
            "id": "outro-id",
            "capabilities": [{ "name": "x", "risk": "r1", "read_only": false }]
        });
        device_client
            .publish(
                topico_manifesto(P, "ghost"),
                QoS::AtLeastOnce,
                true,
                ghost.to_string(),
            )
            .await
            .expect("publica manifesto divergente");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            registry.get("ghost").is_none(),
            "id divergente não registra (fail-closed)"
        );

        // 6. Status "offline" marca presença.
        device_client
            .publish(
                topico_status(P, "sensor-1"),
                QoS::AtLeastOnce,
                false,
                "offline",
            )
            .await
            .expect("publica offline");
        esperar_presenca(&state, "sensor-1", false, "presença offline pelo status").await;

        manager.encerrar().await;
    }

    /// O aceite "LWT/availability": o dispositivo desconecta, o broker
    /// publica a will em `status`, o manager marca offline no store.
    #[tokio::test]
    async fn lwt_do_dispositivo_marca_offline() {
        let porta = porta_livre();
        subir_broker(porta);
        esperar_broker(porta).await;

        let mut opts = rumqttc::MqttOptions::new(format!("dev-lwt-{porta}"), "127.0.0.1", porta);
        opts.set_last_will(rumqttc::LastWill {
            topic: topico_status(P, "sensor-2"),
            message: "offline".into(),
            qos: QoS::AtLeastOnce,
            retain: false,
        });
        let (device_client, mut device_ev) = AsyncClient::new(opts, 16);
        // AsyncClient só funciona com o event loop sendo drenado — a task
        // abaixo é quem mantém a conexão viva; abortá-la derruba o socket.
        let task_device = tokio::spawn(async move { while device_ev.poll().await.is_ok() {} });
        let manifesto = json!({
            "id": "sensor-2",
            "capabilities": [{ "name": "temperature", "risk": "r0", "read_only": true }]
        });
        device_client
            .publish(
                topico_manifesto(P, "sensor-2"),
                QoS::AtLeastOnce,
                true,
                manifesto.to_string(),
            )
            .await
            .expect("publica manifesto");

        let registry = Arc::new(DeviceRegistry::new());
        let state = Arc::new(DeviceStateStore::em_memoria().expect("store"));
        let manager =
            MqttAdapterManager::spawn(config_manager(porta), registry.clone(), Some(state.clone()))
                .expect("manager sobe");
        esperar(
            || registry.get("sensor-2").is_some(),
            "dispositivo registrado",
        )
        .await;
        esperar_presenca(&state, "sensor-2", true, "presença online pela descoberta").await;

        // Mata o dispositivo: a task do event loop é abortada, o socket fecha e
        // o broker publica a will.
        task_device.abort();
        let _ = task_device.await;
        esperar_presenca(&state, "sensor-2", false, "presença offline após LWT").await;

        manager.encerrar().await;
    }
}
