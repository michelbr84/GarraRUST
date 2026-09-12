//! Testes de integração do adapter Home Assistant (#1127) contra um hub
//! mockado em axum — sem docker, na linha do rumqttd dos testes MQTT.
//!
//! O mock fala o contrato real do HA:
//!
//! - `GET /api/states` — descoberta;
//! - `GET /api/states/{entity_id}` — leitura;
//! - `POST /api/services/{domain}/{service}` — execução (o corpo é capturado
//!   para o teste afirmar o payload exato que sairia para o hub);
//! - `GET /api/websocket` — o protocolo de autenticação do HA
//!   (`auth_required` → `auth` → `auth_ok` → `subscribe_events`), com um
//!   sinal para o teste injetar eventos `state_changed`.
//!
//! Cada teste sobe o servidor em `127.0.0.1:0`, monta a config pelo mesmo
//! caminho de produção (`HaAdapterConfig::new` — vetting de verdade, o que
//! também garante que o guard deixa loopback com `AllowPrivate`) e espera
//! condições com timeout — nada de sleeps fixos.

#![cfg(feature = "home-assistant")]

use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use garraia_hardware::{
    DeviceRegistry, DeviceStateStore, HaAdapterConfig, HaAdapterManager, RiskClass,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// O hub mockado.
// ─────────────────────────────────────────────────────────────────────────────

/// Uma chamada de serviço capturada: (domínio, serviço, corpo).
#[derive(Debug, Clone)]
struct Chamada {
    dominio: String,
    servico: String,
    corpo: Value,
}

/// Estado compartilhado do mock.
#[derive(Clone)]
struct MockState {
    entidades: Arc<tokio::sync::Mutex<Vec<Value>>>,
    chamadas: Arc<tokio::sync::Mutex<Vec<Chamada>>>,
    /// Mensagens recebidas no WebSocket, já parseadas (auth, subscribe…).
    ws_msgs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    /// Token que o mock exige no handshake — configurar outro testa o
    /// caminho `auth_invalid`.
    token_esperado: Arc<tokio::sync::Mutex<String>>,
    /// Evento `state_changed` a injetar quando o teste sinaliza.
    evento: Arc<tokio::sync::Mutex<Value>>,
    /// O teste avisa "injete o evento agora".
    sinal: Arc<tokio::sync::Notify>,
}

impl MockState {
    fn novo() -> Self {
        Self {
            entidades: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            chamadas: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            ws_msgs: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            token_esperado: Arc::new(tokio::sync::Mutex::new("token-do-ha".to_string())),
            evento: Arc::new(tokio::sync::Mutex::new(json!(null))),
            sinal: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

async fn lista_estados(State(st): State<MockState>) -> Json<Vec<Value>> {
    Json(st.entidades.lock().await.clone())
}

async fn estado_de(
    Path(entity_id): Path<String>,
    State(st): State<MockState>,
) -> Result<Json<Value>, StatusCode> {
    let entidades = st.entidades.lock().await;
    entidades
        .iter()
        .find(|e| e.get("entity_id").and_then(Value::as_str) == Some(entity_id.as_str()))
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn servico(
    Path((dominio, servico)): Path<(String, String)>,
    State(st): State<MockState>,
    Json(corpo): Json<Value>,
) -> Json<Value> {
    st.chamadas.lock().await.push(Chamada {
        dominio,
        servico,
        corpo,
    });
    // O HA devolve um array vazio no 200.
    Json(json!([]))
}

/// O WebSocket `/api/websocket` com o protocolo de auth do HA.
async fn websocket(upg: WebSocketUpgrade, State(st): State<MockState>) -> axum::response::Response {
    upg.on_upgrade(move |socket| protocolo_ha(socket, st))
}

async fn protocolo_ha(mut ws: WebSocket, st: MockState) {
    // 1. auth_required.
    if ws
        .send(WsMsg::Text(
            json!({"type": "auth_required"}).to_string().into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    // 2. recebe o auth — registra e valida o token.
    let Some(Ok(WsMsg::Text(t))) = ws.recv().await else {
        return;
    };
    let auth: Value = serde_json::from_str(t.as_str()).unwrap_or(json!(null));
    st.ws_msgs.lock().await.push(auth.clone());
    if auth.get("access_token").and_then(Value::as_str)
        != Some(st.token_esperado.lock().await.as_str())
    {
        let _ = ws
            .send(WsMsg::Text(
                json!({"type": "auth_invalid"}).to_string().into(),
            ))
            .await;
        return;
    }
    // 3. auth_ok.
    if ws
        .send(WsMsg::Text(json!({"type": "auth_ok"}).to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    // 4. recebe o subscribe — registra.
    let Some(Ok(WsMsg::Text(t))) = ws.recv().await else {
        return;
    };
    st.ws_msgs
        .lock()
        .await
        .push(serde_json::from_str(t.as_str()).unwrap_or(json!(null)));

    // 5. injeta os eventos que o teste pedir, até a conexão morrer.
    loop {
        st.sinal.notified().await;
        let evento = st.evento.lock().await.clone();
        if ws
            .send(WsMsg::Text(evento.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }
}

/// Um hub no ar, com URL pronta para a config.
struct MockHub {
    url: String,
    state: MockState,
}

async fn subir_hub(st: MockState) -> MockHub {
    let app = Router::new()
        .route("/api/states", get(lista_estados))
        .route("/api/states/{entity_id}", get(estado_de))
        .route("/api/services/{dominio}/{servico}", post(servico))
        .route("/api/websocket", get(websocket))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockHub {
        url: format!("http://{addr}"),
        state: st,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers.
// ─────────────────────────────────────────────────────────────────────────────

/// Espera até `condicao` devolver `Some` (ou estourar o prazo) — nada de
/// sleeps fixos: cada teste define a condição que interessa. Closure async
/// (edition 2024) porque as condições reais (`store.estado`, ws_msgs) são
/// async.
async fn espera<T>(prazo: Duration, mut condicao: impl AsyncFnMut() -> Option<T>) -> Option<T> {
    let fim = tokio::time::Instant::now() + prazo;
    loop {
        if let Some(v) = condicao().await {
            return Some(v);
        }
        if tokio::time::Instant::now() >= fim {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

const PRAZO: Duration = Duration::from_secs(5);

/// As entidades de uma casa pequena, com uma armadilha de cada tipo:
/// domínio fora da lista, entity_id sem ponto, objeto com espaço.
fn casa_pequena() -> Vec<Value> {
    vec![
        json!({"entity_id": "light.sala", "state": "on", "attributes": {"brightness_pct": 80}}),
        json!({"entity_id": "sensor.temp", "state": "23.5", "attributes": {"unit": "°C"}}),
        json!({"entity_id": "binary_sensor.porta", "state": "off", "attributes": {}}),
        json!({"entity_id": "lock.porta", "state": "locked", "attributes": {}}),
        json!({"entity_id": "cover.janela", "state": "unavailable", "attributes": {}}),
        json!({"entity_id": "switch.tomada", "state": "on", "attributes": {}}),
        json!({"entity_id": "climate.ac", "state": "cool", "attributes": {}}),
        json!({"entity_id": "vacuum.robo", "state": "cleaning", "attributes": {}}),
        json!({"entity_id": "sem_ponto", "state": "?", "attributes": {}}),
        json!({"entity_id": "light.Casa limpa", "state": "on", "attributes": {}}),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Os testes.
// ─────────────────────────────────────────────────────────────────────────────

/// Descoberta: só os domínios da lista viram dispositivos, cada um com o
/// risco do domínio; `unavailable` nasce offline; presença inicial vai para
/// o store.
#[tokio::test]
async fn descoberta_registra_por_dominio_com_risco() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();

    let registry = Arc::new(DeviceRegistry::new());
    let store = Arc::new(DeviceStateStore::em_memoria().expect("store"));
    let config = HaAdapterConfig::new(&hub.url, "token-do-ha".to_string()).expect("config vetada");
    let manager = HaAdapterManager::spawn(config, registry.clone(), Some(store.clone()), None);

    let registrado = espera(PRAZO, || async { (registry.len() == 7).then_some(()) }).await;
    assert_eq!(registrado, Some(()), "7 entidades viram dispositivos");

    let ids: Vec<String> = registry.list().iter().map(|d| d.id.clone()).collect();
    assert_eq!(
        ids,
        [
            "binary_sensor.porta",
            "climate.ac",
            "cover.janela",
            "light.sala",
            "lock.porta",
            "sensor.temp",
            "switch.tomada"
        ],
        "vacuum/sem_ponto/'Casa limpa' ficam fora"
    );

    // Risco por domínio, na capability certa.
    let light = registry.get("light.sala").expect("light registrado");
    let caps = light.capabilities();
    let power = caps.iter().find(|c| c.name == "power").expect("power");
    assert_eq!(power.risk, RiskClass::R1);
    assert!(!power.read_only);
    assert!(caps.iter().all(|c| c.risk <= RiskClass::R1));

    let lock = registry.get("lock.porta").expect("lock registrado");
    assert!(
        lock.capabilities()
            .iter()
            .filter(|c| !c.read_only)
            .all(|c| c.risk == RiskClass::R3)
    );

    let cover = registry.get("cover.janela").expect("cover registrado");
    assert!(
        cover
            .capabilities()
            .iter()
            .filter(|c| !c.read_only)
            .all(|c| c.risk == RiskClass::R2)
    );
    assert!(
        cover
            .capabilities()
            .iter()
            .all(|c| c.read_only || c.risk >= RiskClass::R2)
    );

    let sensor = registry.get("sensor.temp").expect("sensor registrado");
    assert!(
        sensor
            .capabilities()
            .iter()
            .all(|c| c.risk == RiskClass::R0 && c.read_only)
    );

    // Presença inicial: o estado real da descoberta.
    let online = store
        .estado("light.sala")
        .await
        .expect("lê")
        .expect("marcado");
    assert!(online.online);
    let offline = store
        .estado("cover.janela")
        .await
        .expect("lê")
        .expect("marcado");
    assert!(!offline.online, "unavailable nasce offline");

    manager.encerrar().await;
}

/// `read` fala `GET /api/states/{entity_id}` e devolve o payload do hub.
/// Capability que não existe recusa sem sair do processo.
#[tokio::test]
async fn read_busca_estado_no_hub() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();

    let registry = Arc::new(DeviceRegistry::new());
    let config = HaAdapterConfig::new(&hub.url, "token-do-ha".to_string()).expect("config");
    let manager = HaAdapterManager::spawn(config, registry.clone(), None, None);

    let sensor = espera(PRAZO, || async { registry.get("sensor.temp") })
        .await
        .expect("sensor registrado");

    let estado = sensor.read("state").await.expect("lê estado");
    assert_eq!(estado["entity_id"], json!("sensor.temp"));
    assert_eq!(estado["state"], json!("23.5"));

    let err = sensor
        .read("temperature")
        .await
        .expect_err("capability inexistente");
    assert!(
        err.to_string().contains("temperature"),
        "erro cita a capability: {err}"
    );

    // Executar uma leitura também recusa — leitura não vira serviço.
    let err = sensor
        .execute("state", json!({}))
        .await
        .expect_err("leitura não executa");
    assert!(
        err.to_string().contains("não mapeia"),
        "sensor não tem serviço: {err}"
    );

    manager.encerrar().await;
}

/// `execute` mapeia (domínio, capability, args) → serviço e payload exatos.
#[tokio::test]
async fn execute_mapeia_servico_e_payload() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();

    let registry = Arc::new(DeviceRegistry::new());
    let config = HaAdapterConfig::new(&hub.url, "token-do-ha".to_string()).expect("config");
    let manager = HaAdapterManager::spawn(config, registry.clone(), None, None);

    let light = espera(PRAZO, || async { registry.get("light.sala") })
        .await
        .expect("light registrado");

    let r = light
        .execute("power", json!({"on": true}))
        .await
        .expect("liga");
    assert_eq!(r["enviado"], json!(true));
    assert_eq!(r["servico"], json!("light/turn_on"));

    let r = light
        .execute("power", json!({"on": false}))
        .await
        .expect("desliga");
    assert_eq!(r["servico"], json!("light/turn_off"));

    let r = light
        .execute("brightness", json!({"brightness_pct": 50}))
        .await
        .expect("brilho");
    assert_eq!(r["servico"], json!("light/turn_on"));

    let cover = espera(PRAZO, || async { registry.get("cover.janela") })
        .await
        .expect("cover registrado");
    cover.execute("open", json!({})).await.expect("abre");
    let r = cover
        .execute("set_position", json!({"position": 80}))
        .await
        .expect("posição");
    assert_eq!(r["servico"], json!("cover/set_cover_position"));

    let lock = espera(PRAZO, || async { registry.get("lock.porta") })
        .await
        .expect("lock registrado");
    let r = lock.execute("unlock", json!({})).await.expect("destranca");
    assert_eq!(r["servico"], json!("lock/unlock"));

    let clima = espera(PRAZO, || async { registry.get("climate.ac") })
        .await
        .expect("climate registrado");
    clima
        .execute("temperature", json!({"temperature": 22.5}))
        .await
        .expect("temperatura");

    manager.encerrar().await;

    // O que exatamente saiu para o hub?
    let chamadas = hub.state.chamadas.lock().await;
    assert_eq!(chamadas.len(), 7);
    let caminho = |i: usize| format!("{}/{}", chamadas[i].dominio, chamadas[i].servico);
    assert_eq!(caminho(0), "light/turn_on");
    assert_eq!(chamadas[0].corpo, json!({"entity_id": "light.sala"}));
    assert_eq!(caminho(1), "light/turn_off");
    assert_eq!(caminho(2), "light/turn_on");
    assert_eq!(
        chamadas[2].corpo,
        json!({"entity_id": "light.sala", "brightness_pct": 50})
    );
    assert_eq!(caminho(3), "cover/open_cover");
    assert_eq!(chamadas[3].corpo, json!({"entity_id": "cover.janela"}));
    assert_eq!(caminho(4), "cover/set_cover_position");
    assert_eq!(
        chamadas[4].corpo,
        json!({"entity_id": "cover.janela", "position": 80})
    );
    assert_eq!(caminho(5), "lock/unlock");
    assert_eq!(chamadas[5].corpo, json!({"entity_id": "lock.porta"}));
    assert_eq!(caminho(6), "climate/set_temperature");
    assert_eq!(
        chamadas[6].corpo,
        json!({"entity_id": "climate.ac", "temperature": 22.5})
    );
}

/// Args inválidos são recusados pelo mesmo `validar_args` do MQTT — e nada
/// sai para o hub.
#[tokio::test]
async fn execute_recusa_args_invalidos_antes_do_hub() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();

    let registry = Arc::new(DeviceRegistry::new());
    let config = HaAdapterConfig::new(&hub.url, "token-do-ha".to_string()).expect("config");
    let manager = HaAdapterManager::spawn(config, registry.clone(), None, None);

    let light = espera(PRAZO, || async { registry.get("light.sala") })
        .await
        .expect("light registrado");

    // Schema exige "on" boolean — faltando, errado no tipo, capability
    // desconhecida: tudo erro, e o mock não vê NENHUM POST.
    assert!(light.execute("power", json!({})).await.is_err());
    assert!(light.execute("power", json!({"on": "sim"})).await.is_err());
    assert!(light.execute("brightness", json!({})).await.is_err());
    assert!(
        light
            .execute("brightness", json!({"brightness_pct": 12.5}))
            .await
            .is_err(),
        "brightness_pct é integer, não number fracionário"
    );
    assert!(light.execute("nao_existe", json!({})).await.is_err());

    manager.encerrar().await;
    assert!(
        hub.state.chamadas.lock().await.is_empty(),
        "nenhum POST deveria ter saído"
    );
}

/// O WebSocket autentica, assina `state_changed` e marca presença — o fluxo
/// inteiro do protocolo HA contra um servidor real.
#[tokio::test]
async fn ws_marca_presenca_por_evento() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();

    let registry = Arc::new(DeviceRegistry::new());
    let store = Arc::new(DeviceStateStore::em_memoria().expect("store"));
    let config = HaAdapterConfig::new(&hub.url, "token-do-ha".to_string()).expect("config");
    let manager = HaAdapterManager::spawn(config, registry.clone(), Some(store.clone()), None);

    // Presença inicial da descoberta (cover.janela nasce offline).
    espera(PRAZO, || async {
        store
            .estado("cover.janela")
            .await
            .ok()
            .flatten()
            .map(|_| ())
    })
    .await
    .expect("descoberta marcou presença");

    // O adapter assinou (auth + subscribe registrados no mock)?
    espera(PRAZO, || async {
        (hub.state.ws_msgs.lock().await.len() >= 2).then_some(())
    })
    .await
    .expect("handshake completou");

    let ws_msgs = hub.state.ws_msgs.lock().await;
    assert_eq!(ws_msgs[0]["type"], json!("auth"));
    assert_eq!(ws_msgs[0]["access_token"], json!("token-do-ha"));
    assert_eq!(ws_msgs[1]["type"], json!("subscribe_events"));
    assert_eq!(ws_msgs[1]["event_type"], json!("state_changed"));
    drop(ws_msgs);

    // Evento de volta online — cover.janela nasceu offline (unavailable).
    *hub.state.evento.lock().await = json!({
        "id": 1, "type": "event",
        "event": {"event_type": "state_changed", "data": {
            "entity_id": "cover.janela",
            "new_state": {"state": "open", "attributes": {}}
        }}
    });
    hub.state.sinal.notify_one();
    espera(PRAZO, || async {
        store
            .estado("cover.janela")
            .await
            .ok()
            .flatten()
            .filter(|p| p.online)
            .map(|_| ())
    })
    .await
    .expect("evento on marcou online");

    // E de volta offline (unavailable).
    *hub.state.evento.lock().await = json!({
        "id": 2, "type": "event",
        "event": {"event_type": "state_changed", "data": {
            "entity_id": "cover.janela", "new_state": {"state": "unavailable"}
        }}
    });
    hub.state.sinal.notify_one();
    espera(PRAZO, || async {
        store
            .estado("cover.janela")
            .await
            .ok()
            .flatten()
            .filter(|p| !p.online)
            .map(|_| ())
    })
    .await
    .expect("evento unavailable marcou offline");

    manager.encerrar().await;
}

/// Token errado: o mock responde `auth_invalid` e o adapter nunca chega a
/// assinar — sem desistir, sem crash.
#[tokio::test]
async fn ws_token_errado_nao_assina() {
    let hub = subir_hub(MockState::novo()).await;
    *hub.state.entidades.lock().await = casa_pequena();
    // O hub espera outro token.
    *hub.state.token_esperado.lock().await = "token-certo".to_string();

    let registry = Arc::new(DeviceRegistry::new());
    let config = HaAdapterConfig::new(&hub.url, "token-errado".to_string()).expect("config");
    let manager = HaAdapterManager::spawn(config, registry.clone(), None, None);

    // A tentativa de auth chega…
    espera(PRAZO, || async {
        (!hub.state.ws_msgs.lock().await.is_empty()).then_some(())
    })
    .await
    .expect("auth enviado");

    // …e o subscribe nunca vem.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        hub.state.ws_msgs.lock().await.len(),
        1,
        "auth_invalid deve interromper antes do subscribe"
    );

    manager.encerrar().await;
}

/// Hub fora do ar: a descoberta falha, o registry segue vazio e ninguém
/// panica — fail-soft, o loop segue tentando.
#[tokio::test]
async fn hub_inacessivel_registry_fica_vazio() {
    // Uma porta com nada escutando.
    let fantasma = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let porta = fantasma.local_addr().expect("addr").port();
    drop(fantasma);

    let registry = Arc::new(DeviceRegistry::new());
    let config = HaAdapterConfig::new(&format!("http://127.0.0.1:{porta}"), "t".to_string())
        .expect("config vetada (loopback)");
    let manager = HaAdapterManager::spawn(config, registry.clone(), None, None);

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        registry.is_empty(),
        "sem hub, nenhuma entidade pode ter entrado"
    );

    manager.encerrar().await;
}
