//! O registry de dispositivos — descoberta dinâmica pelo agente.
//!
//! Adapters/skills registram dispositivos no boot (e podem re-registrar
//! quando a topologia muda); o agente consulta e enxerga o mundo físico.
//! O registry guarda `Arc<dyn Device>` — ele não conhece transporte, só
//! ids e capabilities.

use crate::device::{Device, DeviceSummary, resumo};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Os dispositivos conhecidos, por id.
///
/// `RwLock` e não mutex async: o registry é um mapa em memória, as leituras
/// (descoberta) são o caminho quente e nenhuma operação cruza `.await`
/// enquanto segura o lock.
#[derive(Default)]
pub struct DeviceRegistry {
    devices: RwLock<HashMap<String, Arc<dyn Device>>>,
}

impl DeviceRegistry {
    /// Registry vazio — o estado de todo gateway antes do primeiro adapter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra (ou substitui) um dispositivo.
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
    pub fn register(&self, device: Arc<dyn Device>) {
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
        let id = device.id().to_string();
        let mut guard = self.devices.write().unwrap();
        if guard.contains_key(&id) {
            return false;
        }
        guard.insert(id, device);
        true
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
}
