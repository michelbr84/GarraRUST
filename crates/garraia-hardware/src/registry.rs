//! O registry de dispositivos — descoberta dinâmica pelo agente.
//!
//! Adapters/skills registram dispositivos no boot (e podem re-registrar
//! quando a topologia muda); o agente consulta e enxerga o mundo físico.
//! O registry guarda `Arc<dyn Device>` — ele não conhece transporte, só
//! ids e capabilities.

use crate::Result;
use crate::capability::Capability;
use crate::device::{Device, DeviceSummary, resumo};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A composição do risco com o que o conteúdo empacotado declara (#1250).
///
/// A crate de hardware é a dona do [`RiskClass`], mas quem sabe o que um
/// skill declarou é o catálogo de skills (feature `skills`). O registry
/// recebe essa composição **injetada** em vez de conhecê-la: sem elevador,
/// o risco é o do adapter — que é o comportamento de sempre e, por isso
/// mesmo, a direção fail-closed. Um elevador que erga o risco a menos é
/// recusado por construção: quem implementa só recebe `capability_efetiva`,
/// que é `max(adapter, skill)`.
pub trait ElevadorDeRisco: Send + Sync {
    /// A capability como o gate deve vê-la (ver
    /// `CatalogoDeSkills::capability_efetiva`): leitura intacta, ação no
    /// máximo entre adapter e skill.
    fn capability_efetiva(&self, device_id: &str, cap: &Capability) -> Capability;
}

/// Os sinônimos ("luz da sala") que um skill declara para um dispositivo —
/// a fonte que a tool `device_list` consulta para mostrar os aliases.
///
/// Vive aqui, feature-free, pelo mesmo motivo do elevador: a camada de
/// devices não pode depender do catálogo de skills, e a camada de skills não
/// precisa estar ligada para a descoberta existir. Quem tem presets injeta
/// a fonte; quem não tem, não injeta nada — e a linha de aliases simplesmente
/// não aparece no `device_list`.
pub trait FonteDeSinonimos: Send + Sync {
    /// Os apelidos declarados para o id (já namespaceado, `ha:light.sala_teto`).
    /// Vazio = nenhum alias — uma linha vazia na descoberta seria ruído.
    fn sinonimos_de(&self, device_id: &str) -> Vec<String>;
}

/// Um [`Device`] com o risco elevado pelo catálogo — o decorador do #1250.
///
/// `read`/`execute` passam por dentro; só a **visão** muda: o que o agente
/// enxerga na descoberta é a capability efetiva, então o [`HardwareGate`]
/// decide pela escalada que o skill declarou, e não pelo teto do adapter.
struct DeviceElevado {
    inner: Arc<dyn Device>,
    elevador: Arc<dyn ElevadorDeRisco>,
}

#[async_trait]
impl Device for DeviceElevado {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        let id = self.inner.id().to_string();
        self.inner
            .capabilities()
            .into_iter()
            .map(|c| self.elevador.capability_efetiva(&id, &c))
            .collect()
    }

    async fn read(&self, capability: &str) -> Result<serde_json::Value> {
        self.inner.read(capability).await
    }

    async fn execute(
        &self,
        capability: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.inner.execute(capability, args).await
    }
}

/// Os dispositivos conhecidos, por id.
///
/// `RwLock` e não mutex async: o registry é um mapa em memória, as leituras
/// (descoberta) são o caminho quente e nenhuma operação cruza `.await`
/// enquanto segura o lock.
#[derive(Default)]
pub struct DeviceRegistry {
    devices: RwLock<HashMap<String, Arc<dyn Device>>>,
    elevador: RwLock<Option<Arc<dyn ElevadorDeRisco>>>,
}

impl DeviceRegistry {
    /// Registry vazio — o estado de todo gateway antes do primeiro adapter.
    pub fn new() -> Self {
        Self::default()
    }

    /// #1250: injeta a composição `max(adapter, skill)` na fronteira do
    /// registro. Chamada antes dos adapters subirem — a descoberta deles é
    /// assíncrona, e o elevador precisa já estar lá quando o primeiro
    /// dispositivo chegar.
    pub fn com_elevador(&self, elevador: Arc<dyn ElevadorDeRisco>) {
        *self.elevador.write().unwrap() = Some(elevador);
    }

    /// Registra (ou substitui) um dispositivo — a variante explícita de
    /// "sim, isto deve sobrescrever".
    ///
    /// Re-registrar o mesmo id **substitui** — é como um adapter que perdeu
    /// a conexão e reconecta atualiza o device sem que o registry guarde a
    /// versão velha em silêncio. A substituição é logada em `debug`, e não em
    /// `warn`, **de propósito**: os adapters MQTT e Home Assistant
    /// re-registram o catálogo inteiro a cada reconexão/reentrega do broker,
    /// que é o caminho normal deles, então `warn` aqui viraria dezenas de
    /// linhas por reconexão numa casa com dezenas de entidades — ruído que
    /// destrói o sinal do nível `warn` justamente para quem lê o log atrás de
    /// anomalia.
    ///
    /// Quem precisa gritar na colisão não é este método: é o chamador que
    /// sabe que uma substituição ali é anômala. O transporte serial é esse
    /// caso e não passa por aqui — ver o parágrafo seguinte.
    ///
    /// Transporte em que o **dispositivo** escolhe o próprio id (serial/USB,
    /// o módulo `adapter_serial`) não deve usar este método: uma placa hostil
    /// se anunciando com o id de um device legítimo sequestraria as leituras
    /// endereçadas a ele. Para esses, [`Self::register_if_absent`] — que
    /// recusa a substituição, e o `adotar_stream` loga a recusa em `warn`
    /// porque lá a colisão nunca é rotina.
    ///
    /// O que torna a substituição *rotineira* segura é o id já chegar aqui
    /// namespaceado por transporte — `mqtt:<id>`, `ha:<entity_id>`,
    /// `serial:<id>`, `gpio:<id>` (#1168). Com o namespace, "o mesmo id"
    /// só pode significar "o mesmo adapter, de novo": um dispositivo que
    /// anuncie um id no formato de outro transporte é registrado debaixo do
    /// prefixo do **seu** adapter e nunca alcança a chave alheia.
    pub fn register(&self, device: Arc<dyn Device>) {
        let device = self.elevar(device);
        let id = device.id().to_string();
        let anterior = self.devices.write().unwrap().insert(id.clone(), device);
        if anterior.is_some() {
            tracing::debug!(
                dispositivo = %id,
                "registry: id re-registrado — versão anterior substituída"
            );
        }
    }

    /// Registra **só se o id estiver livre**; devolve `false` sem tocar em
    /// nada quando já existe um dispositivo com esse id.
    ///
    /// A checagem e a inserção acontecem sob o mesmo `write()`: consultar
    /// [`Self::get`] antes de [`Self::register`] deixaria uma janela entre as
    /// duas travas, e "registrei porque estava vazio há um instante" não é
    /// fail-closed.
    #[must_use = "ignorar o `false` é aceitar a substituição que este método existe para impedir"]
    pub fn register_if_absent(&self, device: Arc<dyn Device>) -> bool {
        let device = self.elevar(device);
        let id = device.id().to_string();
        let mut guard = self.devices.write().unwrap();
        if guard.contains_key(&id) {
            return false;
        }
        guard.insert(id, device);
        true
    }

    /// #1250: sob o elevador injetado, o registro guarda a **visão efetiva**
    /// do dispositivo. Sem elevador, o device entra como veio.
    fn elevar(&self, device: Arc<dyn Device>) -> Arc<dyn Device> {
        match self.elevador.read().unwrap().clone() {
            Some(elevador) => Arc::new(DeviceElevado {
                inner: device,
                elevador,
            }),
            None => device,
        }
    }

    /// O dispositivo pelo id, se registrado.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Device>> {
        self.devices.read().unwrap().get(id).cloned()
    }

    /// A descoberta: todos os dispositivos com suas capabilities, ordenados
    /// por id (determinístico para o agente e para testes).
    pub fn list(&self) -> Vec<DeviceSummary> {
        let guard = self.devices.read().unwrap();
        let mut ids: Vec<&String> = guard.keys().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| resumo(guard[id].as_ref()))
            .collect()
    }

    /// Quantos dispositivos estão registrados.
    pub fn len(&self) -> usize {
        self.devices.read().unwrap().len()
    }

    /// `true` quando nenhum dispositivo está registrado — o estado de todo
    /// deploy até um adapter entrar.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::mock::MockDevice;
    use crate::risk::RiskClass;

    /// Sem adapter registrado, a descoberta é vazia — o estado de todo
    /// deploy que ainda não conectou hardware (#1125: "sem drivers no core").
    #[test]
    fn registry_vazio_lista_nada() {
        let reg = DeviceRegistry::new();
        assert!(reg.is_empty());
        assert!(reg.list().is_empty());
        assert!(reg.get("sensor").is_none());
    }

    #[test]
    fn descoberta_lista_dispositivos_com_capabilities() {
        let reg = DeviceRegistry::new();
        reg.register(Arc::new(MockDevice::sensor_temperatura()));
        reg.register(Arc::new(MockDevice::lampada_sala()));

        let lista = reg.list();
        assert_eq!(lista.len(), 2);
        let ids: Vec<&str> = lista.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["lampada-sala", "sensor-sala"],
            "ordem determinística por id"
        );

        let lampada = &lista[0];
        let nomes: Vec<&str> = lampada
            .capabilities
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(nomes, vec!["power", "brightness"]);
    }

    #[test]
    fn re_registrar_mesmo_id_substitui() {
        let reg = DeviceRegistry::new();
        reg.register(Arc::new(MockDevice::sensor_temperatura()));
        let v2 = MockDevice::sensor_temperatura().com_cap(Capability::leitura("humidity", None));
        reg.register(Arc::new(v2));

        assert_eq!(reg.len(), 1, "mesmo id não duplica");
        let lista = reg.list();
        let nomes: Vec<&str> = lista[0]
            .capabilities
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(nomes.contains(&"humidity"), "versão nova vence: {nomes:?}");
    }

    /// O oposto do teste acima, e a defesa que o transporte serial usa: um
    /// segundo dispositivo com o mesmo id é recusado, e quem estava lá fica.
    #[test]
    fn register_if_absent_nao_substitui_id_ocupado() {
        let reg = DeviceRegistry::new();
        assert!(
            reg.register_if_absent(Arc::new(MockDevice::sensor_temperatura())),
            "id livre é aceito"
        );

        let impostor =
            MockDevice::sensor_temperatura().com_cap(Capability::leitura("humidity", None));
        assert!(
            !reg.register_if_absent(Arc::new(impostor)),
            "id ocupado é recusado"
        );

        assert_eq!(reg.len(), 1);
        let nomes: Vec<String> = reg.list()[0]
            .capabilities
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert!(
            !nomes.iter().any(|n| n == "humidity"),
            "o registrado original sobrevive intacto: {nomes:?}"
        );
    }

    #[test]
    fn lookup_desconhecido_nao_panica() {
        let reg = DeviceRegistry::new();
        assert!(reg.get("nada").is_none());
    }

    /// #1250: sem elevador injetado, o risco é o do adapter — o boot de
    /// sempre. A lampada entra e sai com os R1 declarados.
    #[test]
    fn sem_elevador_risco_fica_o_do_adapter() {
        let reg = DeviceRegistry::new();
        reg.register(Arc::new(MockDevice::lampada_sala()));

        let lista = reg.list();
        let power = lista[0]
            .capabilities
            .iter()
            .find(|c| c.name == "power")
            .expect("power");
        assert_eq!(power.risk, RiskClass::R1);
    }

    /// A composição max(adapter, skill): o fake sobe só ações, leitura
    /// continua R0 — e o decorator só muda a **visão**: o que a descoberta
    /// enxerga é a capability efetiva, id intacto.
    #[test]
    fn elevador_injetado_sobe_risco_na_descoberta() {
        struct ElevadorFalso;

        impl ElevadorDeRisco for ElevadorFalso {
            fn capability_efetiva(&self, _device_id: &str, cap: &Capability) -> Capability {
                if cap.read_only {
                    return cap.clone();
                }
                Capability {
                    risk: RiskClass::R3,
                    ..cap.clone()
                }
            }
        }

        let reg = DeviceRegistry::new();
        reg.com_elevador(Arc::new(ElevadorFalso));
        let mock = MockDevice::lampada_sala().com_cap(Capability::leitura("humidity", None));
        reg.register(Arc::new(mock));

        let lista = reg.list();
        let power = lista[0]
            .capabilities
            .iter()
            .find(|c| c.name == "power")
            .expect("power");
        assert_eq!(power.risk, RiskClass::R3, "ação sobe para R3");
        assert_eq!(power.read_only, false);

        let humidity = lista[0]
            .capabilities
            .iter()
            .find(|c| c.name == "humidity")
            .expect("humidity");
        assert_eq!(humidity.risk, RiskClass::R0, "leitura continua R0");

        assert_eq!(lista[0].id, "lampada-sala");
    }

    /// A porta de escape do serial também passa pela elevação —
    /// `register_if_absent` não é atalho para registrar sem o teto do
    /// catálogo.
    #[test]
    fn register_if_absent_tambem_eleva() {
        struct ElevadorFalso;

        impl ElevadorDeRisco for ElevadorFalso {
            fn capability_efetiva(&self, _device_id: &str, cap: &Capability) -> Capability {
                Capability {
                    risk: RiskClass::R4,
                    ..cap.clone()
                }
            }
        }

        let reg = DeviceRegistry::new();
        reg.com_elevador(Arc::new(ElevadorFalso));
        assert!(reg.register_if_absent(Arc::new(MockDevice::lampada_sala())));

        let lista = reg.list();
        let power = lista[0]
            .capabilities
            .iter()
            .find(|c| c.name == "power")
            .expect("power");
        assert_eq!(power.risk, RiskClass::R4);
    }

    /// O que a `device_list` mostra vem da fonte injetada — sem fonte, a
    /// linha de aliases não existe (comportamento de sempre). Aqui a forma:
    /// quem injeta devolve os apelidos do id, e vazio para desconhecido.
    #[test]
    fn fonte_de_sinonimos_injetada_devolve_os_apelidos() {
        struct SinonimosFalsos;

        impl FonteDeSinonimos for SinonimosFalsos {
            fn sinonimos_de(&self, device_id: &str) -> Vec<String> {
                if device_id == "lampada-sala" {
                    vec!["luz da sala".into(), "living room light".into()]
                } else {
                    Vec::new()
                }
            }
        }

        let fonte = Arc::new(SinonimosFalsos);
        assert_eq!(
            fonte.sinonimos_de("lampada-sala"),
            vec!["luz da sala".to_string(), "living room light".to_string()]
        );
        assert!(fonte.sinonimos_de("desconhecido").is_empty());
    }
}
