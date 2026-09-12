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

    /// Registra (ou substitui) um dispositivo — a variante explícita de
    /// "sim, isto deve sobrescrever".
    ///
    /// Re-registrar o mesmo id **substitui** — é como um adapter que perdeu
    /// a conexão e reconecta atualiza o device sem que o registry guarde a
    /// versão velha em silêncio (reconexão MQTT com manifesto retained,
    /// reentrega de estado do Home Assistant). Só é seguro quando o id é
    /// namespaceado por transporte (`mqtt:<id>`, `ha:<entity_id>`,
    /// `serial:<id>`) — assim "o mesmo id" só pode significar "o mesmo
    /// adapter, de novo". Um adapter que registra um id pela primeira vez
    /// e não quer herdar silenciosamente a identidade de outro adapter
    /// (ou de uma reentrega inesperada) deve preferir
    /// [`Self::register_if_absent`] (#1168).
    pub fn register(&self, device: Arc<dyn Device>) {
        let id = device.id().to_string();
        self.devices.write().unwrap().insert(id, device);
    }

    /// Registra o dispositivo só se o id ainda não existir — falha fechada.
    ///
    /// Ao contrário de [`Self::register`], nunca substitui: se o id já
    /// estiver ocupado — por outro adapter ou por uma reentrega que não
    /// deveria contar como a mesma identidade — a tentativa é recusada em
    /// vez de sobrescrever em silêncio o dispositivo existente (#1168: um
    /// dispositivo MQTT mal configurado ou malicioso não deve poder assumir
    /// a identidade de registro de um dispositivo já adotado por outro
    /// transporte). Devolve `true` quando o registro aconteceu, `false`
    /// quando o id já estava ocupado — o chamador decide se isso é erro ou
    /// só um aviso de log.
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

    #[test]
    fn lookup_desconhecido_nao_panica() {
        let reg = DeviceRegistry::new();
        assert!(reg.get("nada").is_none());
    }

    /// `register_if_absent` registra normalmente quando o id está livre.
    #[test]
    fn register_if_absent_registra_id_livre() {
        let reg = DeviceRegistry::new();
        let ok = reg.register_if_absent(Arc::new(MockDevice::sensor_temperatura()));
        assert!(ok, "id livre deve registrar");
        assert_eq!(reg.len(), 1);
    }

    /// `register_if_absent` recusa colisão em vez de substituir em silêncio
    /// (#1168) — o defeito que motivou a namespaceação de id por adapter.
    #[test]
    fn register_if_absent_recusa_colisao_fail_closed() {
        let reg = DeviceRegistry::new();
        assert!(reg.register_if_absent(Arc::new(MockDevice::sensor_temperatura())));

        let impostor =
            MockDevice::sensor_temperatura().com_cap(Capability::leitura("humidity", None));
        let ok = reg.register_if_absent(Arc::new(impostor));
        assert!(!ok, "id já ocupado deve ser recusado");

        // O dispositivo original continua intacto — não foi substituído.
        assert_eq!(reg.len(), 1);
        let lista = reg.list();
        let nomes: Vec<&str> = lista[0]
            .capabilities
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(
            !nomes.contains(&"humidity"),
            "o impostor não deveria ter substituído o original: {nomes:?}"
        );
    }
}
