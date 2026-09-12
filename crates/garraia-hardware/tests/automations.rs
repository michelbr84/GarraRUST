//! Testes de integração do motor de automações (#1128) contra o
//! [`MockDevice`] e o barramento de eventos de verdade — o mesmo caminho de
//! produção: spec TOML carregada por `carregar_dir`, evento entra, regra
//! casa, condição avalia, ação passa pelo gate sem canal de confirmação e o
//! device executa. A auditoria é o store em memória, lido de volta pelo
//! teste.

#![cfg(feature = "automations")]

use garraia_hardware::automations::{
    ConditionSpec, EngineConfig, Execucao, ResultadoExecucao, TetoRisco,
};
use garraia_hardware::{
    AutomationEngine, AutomationSpec, AutomationStore, Capability, DeviceRegistry, EstadoObservado,
    HardwareEvent, HardwareEventBus, MockDevice, RiskClass, StateChanged, carregar_automacoes,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------- helpers

/// Espera a primeira execução auditada da automação — o motor é assíncrono,
/// então nada de sleeps fixos na direção "o motor vai agir".
async fn espera_historico(store: &AutomationStore, nome: &str) -> Execucao {
    let fim = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(h) = store.historico(Some(nome), 10).await
            && let Some(e) = h.into_iter().next()
        {
            return e;
        }
        assert!(
            tokio::time::Instant::now() < fim,
            "execução de '{nome}' não chegou na auditoria"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Espera uma execução com o resultado exato — para os casos em que a
/// execução ruim também é audita (ex.: rate limit depois de uma executada).
async fn espera_resultado(
    store: &AutomationStore,
    nome: &str,
    alvo: ResultadoExecucao,
) -> Execucao {
    let fim = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(h) = store.historico(Some(nome), 10).await
            && let Some(e) = h.into_iter().find(|e| e.resultado == alvo)
        {
            return e;
        }
        assert!(
            tokio::time::Instant::now() < fim,
            "resultado {:?} de '{nome}' não chegou na auditoria",
            alvo
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Espera o mock acumular `n` execuções — prova que a ação chegou ao
/// dispositivo, não só que a auditoria disse que sim.
async fn espera_execs(mock: &MockDevice, n: usize) {
    let fim = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if mock.executed().len() >= n {
            return;
        }
        assert!(
            tokio::time::Instant::now() < fim,
            "mock não executou {n} vez(es)"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Um ventilador R1 com `power` — o caso do issue: power on/off pelo teto.
fn fan_garagem() -> Arc<MockDevice> {
    let power = Capability::acao(
        "power",
        RiskClass::R1,
        Some(json!({
            "type": "object",
            "properties": { "on": { "type": "boolean" } },
            "required": ["on"],
        })),
    )
    .expect("power é ação válida");
    Arc::new(MockDevice::new("fan.garagem", vec![power]))
}

/// Um cover R2 — o teto máximo do que uma automação pode pedir.
fn cover_garagem() -> Arc<MockDevice> {
    let abrir = Capability::acao("open_cover", RiskClass::R2, None).expect("R2 válido");
    Arc::new(MockDevice::new("cover.garagem", vec![abrir]))
}

/// Carrega specs do texto TOML pelo caminho de produção (`carregar_dir`).
fn specs_de_toml(rotulo: &str, texto: &str) -> Vec<AutomationSpec> {
    let dir = std::env::temp_dir().join(format!(
        "garraia-auto-int-{}-{}",
        rotulo,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("cria dir tmp");
    std::fs::write(dir.join("regra.toml"), texto).expect("escreve toml");
    let specs = carregar_automacoes(&dir).expect("specs válidas");
    std::fs::remove_dir_all(&dir).ok();
    specs
}

/// Monta o motor com as regras e os devices no registry. Devolve o bus para
/// o teste publicar eventos e o store para afirmar a auditoria.
async fn sobe_motor(
    specs: Vec<AutomationSpec>,
    teto: TetoRisco,
    devices: Vec<Arc<MockDevice>>,
) -> (
    AutomationEngine,
    Arc<HardwareEventBus>,
    Arc<AutomationStore>,
) {
    let bus = Arc::new(HardwareEventBus::nova());
    let registry = Arc::new(DeviceRegistry::new());
    for device in devices {
        registry.register(device);
    }
    let store = Arc::new(AutomationStore::em_memoria().expect("store em memória"));
    let engine = AutomationEngine::spawn(
        EngineConfig { specs, teto },
        bus.clone(),
        registry.clone(),
        store.clone(),
    )
    .expect("motor sobe");
    // O motor assina o barramento dentro do task dele — esperar a
    // assinatura antes de publicar, ou o primeiro evento cai no chão
    // (zero receptores é a corrida spawn-vs-publish do teste).
    let fim = tokio::time::Instant::now() + Duration::from_secs(2);
    while bus.receptores() == 0 {
        assert!(
            tokio::time::Instant::now() < fim,
            "motor não assinou o barramento"
        );
        tokio::task::yield_now().await;
    }
    (engine, bus, store)
}

fn evento_temperatura(valor: &str) -> HardwareEvent {
    HardwareEvent::StateChanged(StateChanged::agora(
        "sensor.garagem_temperatura",
        EstadoObservado::com(Some(valor.into()), json!({ "unit": "C" }), true),
        None,
    ))
}

const SPEC_VENTILADOR: &str = r#"
[[automation]]
name = "ventilador-garagem-quente"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.garagem_temperatura"
[[automation.condition]]
expr = "to.state > 32"
[[automation.action]]
type = "device_execute"
device = "fan.garagem"
capability = "power"
args = { on = true }
"#;

const SPEC_COVER: &str = r#"
[[automation]]
name = "abrir-garagem"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.portao"
[[automation.action]]
type = "device_execute"
device = "cover.garagem"
capability = "open_cover"
"#;

// ------------------------------------------------------------------ casos

/// O caminho feliz completo: evento → regra → condição verdadeira → gate
/// R1 sob teto R1 → device executa → auditoria `executada`.
#[tokio::test]
async fn dispara_com_condicao_verdadeira_e_executa_a_acao() {
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("ventilador", SPEC_VENTILADOR),
        TetoRisco::R1,
        vec![fan.clone()],
    )
    .await;
    bus.publicar(evento_temperatura("33.5"));

    let execucao = espera_historico(&store, "ventilador-garagem-quente").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::Executada);
    assert_eq!(execucao.gatilho["entity_id"], "sensor.garagem_temperatura");
    assert_eq!(execucao.detalhe[0]["resultado"], "executada");
    assert_eq!(execucao.detalhe[0]["resposta"]["ok"], true);

    espera_execs(&fan, 1).await;
    assert_eq!(
        fan.executed(),
        vec![("power".to_string(), json!({ "on": true }))]
    );
    engine.encerrar();
}

/// Condição falsa não é falha — é a regra decidindo não agir, com a
/// auditoria dizendo o porquê.
#[tokio::test]
async fn condicao_falsa_nao_executa_e_fica_auditorada() {
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("falsa", SPEC_VENTILADOR),
        TetoRisco::R1,
        vec![fan.clone()],
    )
    .await;
    bus.publicar(evento_temperatura("28.0"));

    let execucao = espera_historico(&store, "ventilador-garagem-quente").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::CondicaoFalsa);
    assert_eq!(execucao.detalhe[0]["avaliacao"], false);
    assert!(fan.executed().is_empty(), "condição falsa não executa");
    engine.encerrar();
}

/// Caminho que não existe no contexto é erro de condição — fail-closed, e
/// nunca um `false` silencioso (o autor da regra tem que saber que a
/// expressão não avaliou).
#[tokio::test]
async fn condicao_com_caminho_inexistente_vira_condicao_erro() {
    let mut specs = specs_de_toml("caminho", SPEC_VENTILADOR);
    specs[0].condition = vec![ConditionSpec {
        expr: "to.new_state.state > 32".into(), // forma literal da issue não resolve no ctx
    }];
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(specs, TetoRisco::R1, vec![fan.clone()]).await;
    bus.publicar(evento_temperatura("33.5"));

    let execucao = espera_historico(&store, "ventilador-garagem-quente").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::CondicaoErro);
    assert!(
        execucao.detalhe[0].get("erro").is_some(),
        "erro nomeado no detalhe"
    );
    assert!(fan.executed().is_empty());
    engine.encerrar();
}

/// Teto R0 nega a ação R1 — e a negativa fica na auditoria com classe e
/// teto, não é um silêncio.
#[tokio::test]
async fn teto_r0_bloqueia_acao_r1_com_auditoria() {
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("teto-r0", SPEC_VENTILADOR),
        TetoRisco::R0,
        vec![fan.clone()],
    )
    .await;
    bus.publicar(evento_temperatura("33.5"));

    let execucao = espera_historico(&store, "ventilador-garagem-quente").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::BloqueadaRisco);
    assert_eq!(execucao.detalhe[0]["classe"], "R1");
    assert_eq!(execucao.detalhe[0]["teto"], "r0");
    assert!(fan.executed().is_empty(), "ação bloqueada não executa");
    engine.encerrar();
}

/// Teto R1 permite R1 mas nega R2 — o teto máximo é R2, e R1 fica aquém.
#[tokio::test]
async fn teto_r1_bloqueia_acao_r2() {
    let cover = cover_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("teto-r1", SPEC_COVER),
        TetoRisco::R1,
        vec![cover.clone()],
    )
    .await;
    bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
        "sensor.portao",
        EstadoObservado::com(Some("cheguei".into()), json!({}), true),
        None,
    )));

    let execucao = espera_historico(&store, "abrir-garagem").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::BloqueadaRisco);
    assert_eq!(execucao.detalhe[0]["classe"], "R2");
    assert_eq!(execucao.detalhe[0]["teto"], "r1");
    assert!(cover.executed().is_empty());
    engine.encerrar();
}

/// Teto R2 permite o cover — e o rate limit da regra rege acima do risco:
/// segunda execução dentro da hora é rejeitada mesmo sendo permitida.
#[tokio::test]
async fn teto_r2_permite_r2_e_rate_limit_ainda_rege() {
    let spec = {
        let mut s = specs_de_toml("rate", SPEC_COVER);
        s[0].rate_limit = Some(garraia_hardware::automations::RateLimitSpec { max_per_hour: 1 });
        s
    };
    let cover = cover_garagem();
    let (engine, bus, store) = sobe_motor(spec, TetoRisco::R2, vec![cover.clone()]).await;
    let evento = |v: &str| {
        HardwareEvent::StateChanged(StateChanged::agora(
            "sensor.portao",
            EstadoObservado::com(Some(v.into()), json!({}), true),
            None,
        ))
    };

    bus.publicar(evento("cheguei"));
    espera_execs(&cover, 1).await;
    let primeira = espera_resultado(&store, "abrir-garagem", ResultadoExecucao::Executada).await;
    assert_eq!(primeira.detalhe[0]["resposta"]["ok"], true);

    // Segundo evento dentro da hora: rate limit rejeita.
    bus.publicar(evento("sai"));
    let limitada = espera_resultado(&store, "abrir-garagem", ResultadoExecucao::RateLimited).await;
    assert_eq!(limitada.detalhe["max_per_hour"], 1);
    assert_eq!(cover.executed().len(), 1, "rate limit rege sobre o risco");
    engine.encerrar();
}

/// Dispositivo ausente no registry vira erro auditado — o motor não panica
/// e o resto do pipeline continua saudável.
#[tokio::test]
async fn dispositivo_ausente_vira_erro_auditorado() {
    let spec_texto = SPEC_VENTILADOR.replace("fan.garagem", "fan.inexistente");
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("fantasma", &spec_texto),
        TetoRisco::R1,
        vec![fan_garagem()],
    )
    .await;
    bus.publicar(evento_temperatura("33.5"));

    let execucao = espera_historico(&store, "ventilador-garagem-quente").await;
    assert_eq!(execucao.resultado, ResultadoExecucao::Erro);
    assert_eq!(execucao.detalhe[0]["motivo"], "dispositivo não registrado");
    engine.encerrar();
}

/// Regra desabilitada não arma — nem executa, nem audita.
#[tokio::test]
async fn regra_desabilitada_nao_arma() {
    let mut specs = specs_de_toml("desligada", SPEC_VENTILADOR);
    specs[0].enabled = false;
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(specs, TetoRisco::R1, vec![fan.clone()]).await;
    bus.publicar(evento_temperatura("33.5"));

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(fan.executed().is_empty());
    assert!(
        store
            .historico(Some("ventilador-garagem-quente"), 10)
            .await
            .unwrap()
            .is_empty()
    );
    engine.encerrar();
}

/// Rajada de eventos dentro da janela de debounce vira uma execução só —
/// o sensor barulhento não liga o ventilador três vezes.
#[tokio::test]
async fn debounce_absorve_rajada_de_eventos() {
    let mut specs = specs_de_toml("debounce", SPEC_VENTILADOR);
    specs[0].debounce_secs = Some(2);
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(specs, TetoRisco::R1, vec![fan.clone()]).await;

    for _ in 0..3 {
        bus.publicar(evento_temperatura("33.5"));
    }
    espera_execs(&fan, 1).await;
    // O pipeline único terminou: uma execução auditada, e nenhuma outra
    // vem — o resto da rajada morreu no debounce.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fan.executed().len(), 1, "debounce colapsa a rajada");
    assert_eq!(
        store
            .historico(Some("ventilador-garagem-quente"), 10)
            .await
            .unwrap()
            .len(),
        1
    );
    engine.encerrar();
}

/// Duas ações rodam em sequência e o detalhe registra as duas.
#[tokio::test]
async fn multi_acao_executa_em_sequencia_com_detalhe() {
    let spec_texto = r#"
[[automation]]
name = "desligar-tudo"
[[automation.trigger]]
type = "state_changed"
entity = "sensor.garagem_temperatura"
[[automation.action]]
type = "device_execute"
device = "fan.garagem"
capability = "power"
args = { on = true }
[[automation.action]]
type = "device_execute"
device = "lampada.sala"
capability = "power"
args = { on = false }
"#;
    let lampada = Arc::new(MockDevice::new(
        "lampada.sala",
        vec![
            Capability::acao(
                "power",
                RiskClass::R1,
                Some(json!({
                    "type": "object",
                    "properties": { "on": { "type": "boolean" } },
                    "required": ["on"],
                })),
            )
            .expect("power é R1"),
        ],
    ));
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("multi", spec_texto),
        TetoRisco::R1,
        vec![fan.clone(), lampada.clone()],
    )
    .await;
    bus.publicar(evento_temperatura("33.5"));

    espera_execs(&lampada, 1).await;
    let execucao = espera_resultado(&store, "desligar-tudo", ResultadoExecucao::Executada).await;
    assert_eq!(execucao.detalhe.as_array().map(|a| a.len()), Some(2));
    assert_eq!(fan.executed().len(), 1);
    assert_eq!(
        fan.executed()[0],
        ("power".to_string(), json!({ "on": true }))
    );
    assert_eq!(lampada.executed().len(), 1);
    assert_eq!(
        lampada.executed()[0],
        ("power".to_string(), json!({ "on": false }))
    );
    engine.encerrar();
}

/// Regra com gatilho cron ignora eventos de estado — cada gatilho só casa
/// com o seu próprio fluxo (o disparo do cron em si é unit-tested em
/// `cron.rs`; o tick de 30s não é viável num teste rápido).
#[tokio::test]
async fn regra_cron_nao_casa_evento_de_estado() {
    let spec_texto = r#"
[[automation]]
name = "jantar-19h"
[[automation.trigger]]
type = "cron"
expr = "0 19 * * *"
[[automation.action]]
type = "device_execute"
device = "fan.garagem"
capability = "power"
args = { on = true }
"#;
    let fan = fan_garagem();
    let (engine, bus, store) = sobe_motor(
        specs_de_toml("cron", spec_texto),
        TetoRisco::R1,
        vec![fan.clone()],
    )
    .await;
    bus.publicar(evento_temperatura("33.5"));

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(fan.executed().is_empty());
    assert!(
        store
            .historico(Some("jantar-19h"), 10)
            .await
            .unwrap()
            .is_empty()
    );
    engine.encerrar();
}
