//! O barramento de eventos de hardware — o fio que liga os adapters ao
//! motor de automações (#1128).
//!
//! Os adapters (MQTT #1126, Home Assistant #1127) publicam aqui cada mudança
//! de estado que veem; o motor assina. Um [`tokio::sync::broadcast`] bastão:
//! publicar nunca bloqueia o adapter, e assinante lento perde eventos antigos
//! (`RecvError::Lagged`) — automação não é log, é gatilho; o motor trata o
//! lag contando o que perdeu e seguindo.

use tokio::sync::broadcast;

/// O estado de um dispositivo no momento da observação — o que o hub falou,
/// sem inventar formato: no Home Assistant, `state` é a string do hub
/// (`"33.5"`) e `attributes` o objeto do hub; no MQTT, `state` é o payload
/// da mensagem (limitado) e `attributes` é nulo. `online = false` cobre
/// `unavailable`/`offline` — nesse caso `state` costuma ser `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct EstadoObservado {
    pub state: Option<String>,
    pub attributes: serde_json::Value,
    pub online: bool,
}

/// A mudança de estado que um adapter viu. `velho` é o estado anterior
/// quando o transporte o traz (Home Assistant traz; MQTT não — o payload
/// é stateless).
#[derive(Debug, Clone, PartialEq)]
pub struct StateChanged {
    /// O id do dispositivo no registry (para o HA, a `entity_id`).
    pub device_id: String,
    pub novo: EstadoObservado,
    pub velho: Option<EstadoObservado>,
    /// Momento da observação, em milissegundos desde a época (UTC).
    pub em_milis: u64,
}

/// O evento que cruza o barramento. Uma variante hoje; o enum existe para
/// que eventos futuros (gatilho binário, telemetria) entrem aditivamente.
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareEvent {
    StateChanged(StateChanged),
}

impl StateChanged {
    /// Carimba o momento da observação com o relógio do processo.
    pub fn agora(
        device_id: impl Into<String>,
        novo: EstadoObservado,
        velho: Option<EstadoObservado>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            novo,
            velho,
            em_milis: milis_agora(),
        }
    }
}

impl EstadoObservado {
    /// Um estado sem payload — fixture comum de adapter e teste.
    pub fn simples(online: bool) -> Self {
        Self {
            state: None,
            attributes: serde_json::Value::Null,
            online,
        }
    }

    /// Um estado com valor e atributos do hub.
    pub fn com(state: Option<String>, attributes: serde_json::Value, online: bool) -> Self {
        Self {
            state,
            attributes,
            online,
        }
    }
}

/// Milissegundos desde a época — o único relógio que o barramento precisa.
pub fn milis_agora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        // Relógio antes da época só em máquina com relógio quebrado; 0 é
        // honesto o suficiente para um carimbo de observação.
        .unwrap_or(0)
}

/// O barramento. `Arc<HardwareEventBus>` — os adapters seguram um clone
/// para publicar, o motor segura um para assinar.
#[derive(Debug, Clone)]
pub struct HardwareEventBus {
    tx: broadcast::Sender<HardwareEvent>,
}

impl HardwareEventBus {
    /// Barramento novo com fila de 1024 eventos — margem para uma casa
    /// barulhenta sem que o adapter sinta o motor.
    pub fn nova() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    /// Publica sem bloquear. Sem assinantes, o evento cai no chão — é o
    /// caso do hardware ligado sem motor de automações (o default).
    pub fn publicar(&self, evento: HardwareEvent) {
        let _ = self.tx.send(evento);
    }

    /// Assina o barramento a partir de agora (eventos anteriores não são
    /// replayados — broadcast, não fila persistente).
    pub fn subscrever(&self) -> broadcast::Receiver<HardwareEvent> {
        self.tx.subscribe()
    }

    /// Quantos consumidores há agora — diagnóstico de wiring (0 = ninguém
    /// escutando).
    pub fn receptores(&self) -> usize {
        self.tx.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn publica_e_entrega_para_assinante() {
        let bus = HardwareEventBus::nova();
        let mut rx = bus.subscrever();
        bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            "light.sala",
            EstadoObservado::com(Some("on".into()), json!({ "brightness": 200 }), true),
            None,
        )));
        let evento = rx.try_recv().expect("evento deveria chegar");
        let HardwareEvent::StateChanged(mudanca) = evento;
        assert_eq!(mudanca.device_id, "light.sala");
        assert_eq!(mudanca.novo.state.as_deref(), Some("on"));
        assert_eq!(mudanca.novo.attributes["brightness"], 200);
        assert!(mudanca.novo.online);
        assert!(mudanca.velho.is_none());
        assert!(mudanca.em_milis > 0);
    }

    #[test]
    fn publicar_sem_assinante_nao_panica() {
        let bus = HardwareEventBus::nova();
        assert_eq!(bus.receptores(), 0);
        bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            "sensor.x",
            EstadoObservado::simples(true),
            None,
        )));
        assert_eq!(bus.receptores(), 0);
    }

    #[test]
    fn assinante_lento_recebe_lagged_e_o_bus_segue() {
        let bus = HardwareEventBus::nova();
        let mut rx = bus.subscrever();
        for i in 0..1200 {
            bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
                format!("sensor.{i}"),
                EstadoObservado::simples(true),
                None,
            )));
        }
        // A fila é 1024: os primeiros já caíram — o consumidor enxerga o
        // Lagged e, depois dele, os eventos vivos.
        let mut lagged = false;
        let mut vivos = 0;
        loop {
            match rx.try_recv() {
                Ok(_) => vivos += 1,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    lagged = true;
                    assert!(n >= 100, "perdeu menos do que a fila comporta: {n}");
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        assert!(lagged, "o lag deveria ter sido reportado");
        assert!(vivos >= 1000 - 16, "eventos vivos somiram: {vivos}");
    }

    #[test]
    fn clones_do_bus_compartilham_o_canal() {
        let bus = HardwareEventBus::nova();
        let mut rx = bus.subscrever();
        let clone = bus.clone();
        clone.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            "lock.porta",
            EstadoObservado::simples(false),
            None,
        )));
        assert!(rx.try_recv().is_ok());
    }
}
