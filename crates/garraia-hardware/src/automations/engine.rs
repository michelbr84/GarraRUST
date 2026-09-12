//! O motor de automações — assina o barramento, casa gatilhos, avalia
//! condições, respeita debounce/rate limit e executa ações pela **mesma
//! policy do runtime** (nada de bypass).
//!
//! O [`HardwareGate`] aqui é sempre o `sem_canal_confirmacao()`: automação
//! roda desacompanhada, então R3/R4/R5 são negadas por natureza (approval
//! que ninguém pode dar não é approval). O que o operador controla é o
//! [`TetoRisco`] no config — o teto do que uma automação pode pedir:
//!
//! - R0 (leitura): sempre pode;
//! - R1 (`PolicyGated`): executada com teto R1 ou R2;
//! - R2 (`PolicyRateLimited`): executada só com teto R2 — e **toda**
//!   execução respeita o rate limit da regra, seja qual for o risco;
//! - R3/R4/R5: nunca (`HumanConfirmation`/`ExplicitApproval`/`Denied`
//!   sem canal humano viram execução bloqueada, com auditoria).
//!
//! Ordem do pipeline por evento: debounce (no loop, atômico) → condições
//! (AND, no task) → rate limit (atômico, pós-condições — protege o efeito,
//! não a avaliação) → delay → ações em sequência → auditoria.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use tokio::sync::{Mutex, broadcast};
use tokio::time;

use crate::error::HardwareError;
use crate::events::{HardwareEvent, HardwareEventBus, StateChanged};
use crate::gate::HardwareGate;
use crate::registry::DeviceRegistry;
use crate::risk::ExecutionDecision;
use crate::schema::validar_args;

use super::cron;
use super::expr::Expr;
use super::spec::{ActionSpec, AutomationSpec, TriggerSpec};
use super::store::{AutomationStore, Execucao, ResultadoExecucao};

/// O teto de risco que o operador declarou para as automações no config.
///
/// É a policy da automação — o análogo dos modos do runtime para quem não
/// está no chat. R3 e acima não existem aqui: não há quem confirme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TetoRisco {
    /// Só leitura (R0).
    R0,
    /// Leitura + ações domésticas básicas (R1: power/brightness/temperature).
    R1,
    /// Até R2 (covers, scenes) — o teto máximo; R3 e acima nunca.
    R2,
}

impl TetoRisco {
    pub fn de_texto(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "r0" => TetoRisco::R0,
            "r1" => TetoRisco::R1,
            "r2" => TetoRisco::R2,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TetoRisco::R0 => "r0",
            TetoRisco::R1 => "r1",
            TetoRisco::R2 => "r2",
        }
    }
}

/// A configuração do motor: as specs carregadas e o teto de risco.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub specs: Vec<AutomationSpec>,
    pub teto: TetoRisco,
}

/// A condição compilada — o fonte para a auditoria, a árvore para avaliar.
struct Condicao {
    fonte: String,
    expr: Expr,
}

/// A regra compilada — condições parseadas na carga, não a cada evento.
struct Regra {
    spec: AutomationSpec,
    condicoes: Vec<Condicao>,
    /// Expressões cron, na ordem em que aparecem nos gatilhos — alinhado
    /// com `EstadoRuntime::proximos_cron`.
    crons: Vec<String>,
}

impl Regra {
    fn compilar(spec: AutomationSpec) -> Result<Self, HardwareError> {
        let mut condicoes = Vec::new();
        for c in &spec.condition {
            let expr = Expr::parse(&c.expr).map_err(|e| {
                HardwareError::Automations(format!(
                    "condição inválida na automação '{}': {}",
                    spec.name, e
                ))
            })?;
            condicoes.push(Condicao {
                fonte: c.expr.clone(),
                expr,
            });
        }
        let crons = spec
            .trigger
            .iter()
            .filter_map(|t| match t {
                TriggerSpec::Cron { expr } => Some(expr.clone()),
                _ => None,
            })
            .collect();
        Ok(Self {
            spec,
            condicoes,
            crons,
        })
    }
}

/// O estado de runtime de uma automação — vivo só em memória: reinício do
/// gateway zera debounce/janela de rate (o pior caso é um disparo extra no
/// boot, nunca um bypass).
#[derive(Default)]
struct EstadoRuntime {
    ultima_execucao: Option<Instant>,
    janela_rate: Vec<Instant>,
    /// Próxima ocorrência de cada gatilho cron (alinhado a `Regra::crons`).
    proximos_cron: Vec<Option<DateTime<Local>>>,
}

/// O que o motor compartilha com as tasks de execução.
struct Compartilhado {
    regras: Vec<Regra>,
    estados: Mutex<HashMap<String, EstadoRuntime>>,
    registry: Arc<DeviceRegistry>,
    store: Arc<AutomationStore>,
    gate: HardwareGate,
    teto: TetoRisco,
}

/// O motor. Um task em loop (`select!` sobre o barramento e o tick de cron);
/// `encerrar` aborta.
pub struct AutomationEngine {
    tarefa: tokio::task::JoinHandle<()>,
}

impl AutomationEngine {
    /// Compila as specs e sobe o motor. Condição que não parses é erro de
    /// spawn — regra quebrada não sobe silenciosa.
    pub fn spawn(
        config: EngineConfig,
        bus: Arc<HardwareEventBus>,
        registry: Arc<DeviceRegistry>,
        store: Arc<AutomationStore>,
    ) -> Result<Self, HardwareError> {
        let mut regras = Vec::new();
        for spec in config.specs {
            if !spec.enabled {
                continue; // desabilitada não arma — segue registrada no store
            }
            regras.push(Regra::compilar(spec)?);
        }
        let mut estados = HashMap::new();
        for regra in &regras {
            estados.insert(
                regra.spec.name.clone(),
                EstadoRuntime {
                    proximos_cron: vec![None; regra.crons.len()],
                    ..Default::default()
                },
            );
        }
        let compartilhado = Arc::new(Compartilhado {
            regras,
            estados: Mutex::new(estados),
            registry,
            store,
            gate: HardwareGate::sem_canal_confirmacao(),
            teto: config.teto,
        });
        let tarefa = tokio::spawn(rodar(bus, compartilhado));
        Ok(Self { tarefa })
    }

    /// Para o motor (aborta o task — o barramento segue vivo para os
    /// próximos).
    pub fn encerrar(self) {
        self.tarefa.abort();
    }
}

/// A cadência do tick de cron: o croner é consultado a cada 30s — sobra
/// no máximo 30s de atraso em um gatilho que fala em minutos.
const TICK_CRON: Duration = Duration::from_secs(30);

async fn rodar(bus: Arc<HardwareEventBus>, sh: Arc<Compartilhado>) {
    let mut rx = bus.subscrever();
    let mut tick = time::interval(TICK_CRON);
    // O primeiro tick do `interval` dispara imediato — não é "todo mundo
    // vencido de uma vez", é só o construtor.
    tick.tick().await;
    loop {
        tokio::select! {
            evento = rx.recv() => match evento {
                Ok(ev) => processar_evento(&sh, ev).await,
                Err(broadcast::error::RecvError::Lagged(perdidos)) => {
                    tracing::warn!(perdidos, "automations: assinante lento, eventos perdidos");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("automations: barramento fechado, motor encerrado");
                    return;
                }
            },
            _ = tick.tick() => disparar_crons(&sh).await,
        }
    }
}

async fn processar_evento(sh: &Arc<Compartilhado>, evento: HardwareEvent) {
    let HardwareEvent::StateChanged(mudanca) = evento;
    for regra in &sh.regras {
        if !regra_casa_evento(regra, &mudanca) {
            continue;
        }
        // Debounce é reserva atômica no loop: dois eventos em rajada não
        // correm dois pipelines da mesma regra dentro da janela.
        let agora = Instant::now();
        let janela = Duration::from_secs(regra.spec.debounce_secs.unwrap_or(0));
        {
            let mut estados = sh.estados.lock().await;
            let estado = estados.entry(regra.spec.name.clone()).or_default();
            if let Some(ultima) = estado.ultima_execucao
                && agora.duration_since(ultima) < janela
            {
                tracing::debug!(
                    automacao = %regra.spec.name,
                    entidade = %mudanca.device_id,
                    "automations: debounce absorveu o evento"
                );
                continue;
            }
            estado.ultima_execucao = Some(agora);
        }
        let sh_task = sh.clone();
        let mudanca = mudanca.clone();
        let nome = regra.spec.name.clone();
        tokio::spawn(async move {
            executar_por_evento(sh_task, &nome, mudanca).await;
        });
    }
}

fn regra_casa_evento(regra: &Regra, mudanca: &StateChanged) -> bool {
    regra.spec.trigger.iter().any(|t| match t {
        TriggerSpec::StateChanged { entity } => entity == &mudanca.device_id,
        _ => false,
    })
}

/// O pipeline pós-debounce para um gatilho de evento.
async fn executar_por_evento(sh: Arc<Compartilhado>, nome: &str, mudanca: StateChanged) {
    let ctx = contexto_do_evento(&mudanca);
    let gatilho = serde_json::json!({
        "entity_id": mudanca.device_id,
        "state": mudanca.novo.state,
        "online": mudanca.novo.online,
    });
    executar_pipeline(sh, nome, &ctx, &gatilho).await;
}

/// O pipeline compartilhado entre gatilho de evento e gatilho de cron:
/// condições → rate limit → delay → ações → auditoria. A regra é procurada
/// aqui dentro: o borrow vive enquanto o `Arc` é dono local, sem mover.
async fn executar_pipeline(
    sh: Arc<Compartilhado>,
    nome: &str,
    ctx: &serde_json::Value,
    gatilho: &serde_json::Value,
) {
    let Some(regra) = sh.regras.iter().find(|r| r.spec.name == nome) else {
        return; // regra sumiu? não deve acontecer — o pool é fixo
    };
    let inicio = Instant::now();

    // 1. Condições — todas devem avaliar `true`. Erro de avaliação não é
    //    `false`: condição quebrada fica registrada como tal, e a ação não
    //    roda.
    for condicao in &regra.condicoes {
        let veredito = match condicao.expr.avaliar(ctx) {
            Ok(serde_json::Value::Bool(true)) => None,
            Ok(serde_json::Value::Bool(false)) => {
                Some(serde_json::json!([{ "expr": condicao.fonte, "avaliacao": false }]))
            }
            Ok(outro) => Some(serde_json::json!([{
                "expr": condicao.fonte,
                "erro": format!("resultado não booleano: {outro}"),
            }])),
            Err(erro) => Some(serde_json::json!([{
                "expr": condicao.fonte,
                "erro": erro.to_string(),
            }])),
        };
        if let Some(detalhe) = veredito {
            let resultado = if detalhe[0].get("erro").is_some() {
                ResultadoExecucao::CondicaoErro
            } else {
                ResultadoExecucao::CondicaoFalsa
            };
            tracing::info!(
                automacao = %regra.spec.name,
                resultado = resultado.as_str(),
                "automations: condição não passou — ação não roda"
            );
            auditar(
                &sh,
                regra,
                gatilho,
                resultado,
                inicio.elapsed().as_millis() as u64,
                detalhe,
            )
            .await;
            return;
        }
    }

    // 2. Rate limit — reserva atômica: ou a execução entra na janela agora,
    //    ou é rejeitada com auditoria. Janela deslizante de 1 hora.
    if let Some(limite) = regra.spec.rate_limit {
        let agora = Instant::now();
        let janela = Duration::from_secs(3600);
        {
            let mut estados = sh.estados.lock().await;
            let estado = estados.entry(regra.spec.name.clone()).or_default();
            estado
                .janela_rate
                .retain(|t| agora.duration_since(*t) < janela);
            if estado.janela_rate.len() >= limite.max_per_hour as usize {
                tracing::warn!(
                    automacao = %regra.spec.name,
                    limite = limite.max_per_hour,
                    "automations: rate limit atingido — execução rejeitada"
                );
                auditar(
                    &sh,
                    regra,
                    gatilho,
                    ResultadoExecucao::RateLimited,
                    inicio.elapsed().as_millis() as u64,
                    serde_json::json!({ "max_per_hour": limite.max_per_hour }),
                )
                .await;
                return;
            }
            estado.janela_rate.push(agora);
        }
    }

    // 3. Delay — a espera fixa entre condição satisfeita e ação.
    if let Some(secs) = regra.spec.delay_secs {
        time::sleep(Duration::from_secs(secs)).await;
    }

    // 4. Ações, em sequência. Cada uma avalia o próprio resultado; a soma
    //    vira o resultado geral da execução.
    let mut detalhe = Vec::new();
    let mut executadas = 0usize;
    let mut bloqueadas = 0usize;
    let mut erros = 0usize;
    for acao in &regra.spec.action {
        let resultado = executar_acao(&sh, acao).await;
        match resultado.get("resultado").and_then(|r| r.as_str()) {
            Some("executada") => executadas += 1,
            Some("bloqueada") => bloqueadas += 1,
            _ => erros += 1,
        }
        detalhe.push(resultado);
    }
    let resultado_geral = if bloqueadas > 0 && executadas == 0 {
        ResultadoExecucao::BloqueadaRisco
    } else if erros > 0 {
        ResultadoExecucao::Erro
    } else {
        ResultadoExecucao::Executada
    };
    tracing::info!(
        automacao = %regra.spec.name,
        resultado = resultado_geral.as_str(),
        acoes = detalhe.len(),
        "automations: execução concluída"
    );
    auditar(
        &sh,
        regra,
        gatilho,
        resultado_geral,
        inicio.elapsed().as_millis() as u64,
        serde_json::Value::Array(detalhe),
    )
    .await;
}

/// Executa uma ação `device_execute` — registry → gate → validar_args →
/// device. Falha em qualquer degrau não panica: vira resultado JSON na
/// auditoria.
async fn executar_acao(sh: &Compartilhado, acao: &ActionSpec) -> serde_json::Value {
    let ActionSpec::DeviceExecute {
        device,
        capability,
        args,
    } = acao;
    let Some(dispositivo) = sh.registry.get(device) else {
        tracing::warn!(dispositivo = %device, "automations: dispositivo não registrado");
        return serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "erro", "motivo": "dispositivo não registrado",
        });
    };
    let Some(cap) = dispositivo
        .capabilities()
        .iter()
        .find(|c| c.name == *capability)
        .cloned()
    else {
        tracing::warn!(dispositivo = %device, capability = %capability, "automations: capability desconhecida");
        return serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "erro", "motivo": "capability desconhecida",
        });
    };
    let decisao = sh.gate.decide_class(cap.risk, &cap.name);
    let permitida = match decisao {
        ExecutionDecision::Auto => true,
        ExecutionDecision::PolicyGated => sh.teto >= TetoRisco::R1,
        ExecutionDecision::PolicyRateLimited => sh.teto >= TetoRisco::R2,
        // Automação roda desacompanhada — sem canal de confirmação, o gate
        // já nega R3/R4/R5; a linha é defesa em profundidade.
        ExecutionDecision::HumanConfirmation
        | ExecutionDecision::ExplicitApproval
        | ExecutionDecision::Denied => false,
    };
    if !permitida {
        tracing::warn!(
            dispositivo = %device, capability = %cap.name,
            classe = cap.risk.as_str(),
            teto = sh.teto.as_str(),
            "automations: ação bloqueada pelo modelo de risco"
        );
        return serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "bloqueada", "classe": cap.risk.as_str(), "teto": sh.teto.as_str(),
        });
    }
    if let Err(erro) = validar_args(args, cap.args_schema.as_ref(), device, capability) {
        return serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "erro", "motivo": erro.to_string(),
        });
    }
    match dispositivo.execute(capability, args.clone()).await {
        Ok(resposta) => serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "executada", "resposta": resposta,
        }),
        Err(erro) => serde_json::json!({
            "acao": "device_execute", "device": device, "capability": capability,
            "resultado": "erro", "motivo": erro.to_string(),
        }),
    }
}

/// O contexto das condições, na forma do docs do módulo `expr`.
fn contexto_do_evento(mudanca: &StateChanged) -> serde_json::Value {
    let mut ctx = serde_json::json!({
        "to": estado_json(&mudanca.novo),
        "entity_id": mudanca.device_id,
        "domain": dominio_de(&mudanca.device_id),
    });
    if let Some(velho) = &mudanca.velho {
        ctx["from"] = estado_json(velho);
    }
    ctx
}

fn estado_json(estado: &crate::events::EstadoObservado) -> serde_json::Value {
    serde_json::json!({
        "state": estado.state,
        "attributes": estado.attributes,
        "online": estado.online,
    })
}

/// A parte antes do ponto da `entity_id` — sensor, light, cover…
fn dominio_de(entity_id: &str) -> String {
    entity_id.split('.').next().unwrap_or("").to_string()
}

/// Os gatilhos cron devidos: a cada tick, cada regra com cron verifica a
/// próxima ocorrência; vencida, dispara o pipeline com contexto de cron e
/// recomputa.
async fn disparar_crons(sh: &Arc<Compartilhado>) {
    let agora = Local::now();
    let mut disparos: Vec<(String, String)> = Vec::new(); // (regra, expr)
    {
        let mut estados = sh.estados.lock().await;
        for regra in &sh.regras {
            if regra.crons.is_empty() {
                continue;
            }
            let estado = estados.entry(regra.spec.name.clone()).or_default();
            if estado.proximos_cron.len() != regra.crons.len() {
                estado.proximos_cron = vec![None; regra.crons.len()];
            }
            for (i, expr) in regra.crons.iter().enumerate() {
                if estado.proximos_cron[i].is_none() {
                    estado.proximos_cron[i] = match cron::proxima_ocorrencia(expr, agora) {
                        Ok(proximo) => Some(proximo),
                        Err(erro) => {
                            tracing::warn!(
                                automacao = %regra.spec.name,
                                cron = %expr,
                                "automations: cron sem ocorrência futura ({erro})"
                            );
                            None
                        }
                    };
                }
                let devida = estado.proximos_cron[i]
                    .map(|proximo| proximo <= agora)
                    .unwrap_or(false);
                if devida {
                    estado.proximos_cron[i] = cron::proxima_ocorrencia(expr, agora).ok();
                    disparos.push((regra.spec.name.clone(), expr.clone()));
                }
            }
        }
    }
    for (nome, expr) in disparos {
        let sh_task = sh.clone();
        let gatilho = serde_json::json!({ "cron": expr });
        tokio::spawn(async move {
            let ctx = serde_json::json!({ "cron": expr });
            executar_pipeline(sh_task, &nome, &ctx, &gatilho).await;
        });
    }
}

async fn auditar(
    sh: &Arc<Compartilhado>,
    regra: &Regra,
    gatilho: &serde_json::Value,
    resultado: ResultadoExecucao,
    duracao_ms: u64,
    detalhe: serde_json::Value,
) {
    let execucao = Execucao {
        automacao: regra.spec.name.clone(),
        disparada_em: crate::events::milis_agora(),
        resultado,
        gatilho: gatilho.clone(),
        detalhe,
        duracao_ms,
    };
    if let Err(erro) = sh.store.registrar(&execucao).await {
        tracing::warn!(
            automacao = %regra.spec.name,
            error = %erro,
            "automations: falha ao registrar execução na auditoria"
        );
    }
}
