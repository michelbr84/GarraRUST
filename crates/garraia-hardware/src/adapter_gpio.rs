//! Adapter GPIO (#1130) — os pinos do Raspberry Pi, direto, sem placa no
//! meio.
//!
//! Atrás da feature `hardware-gpio`. Onde o adapter serial
//! ([`crate::adapter_serial`]) fala com um microcontrolador pelo cabo, aqui o
//! processo do Garra **é** o firmware: `rppal` mapeia `/dev/gpiomem` e o
//! [`crate::Device`] lê e escreve os pinos do próprio host.
//!
//! As capabilities são as mesmas do adapter serial, da mesma tabela fechada
//! de [`crate::perifericos`] — `digital_read` (R0), `digital_write` (R2),
//! `pwm` (R2). Um LED no BCM 13 é a mesma capability com o mesmo risco, quer
//! ele esteja pendurado num Arduino, quer nos pinos do Pi.
//!
//! `analog_read` **não** aparece aqui: o Raspberry Pi não tem ADC embutido.
//! Declarar a capability e falhar em runtime seria mentir para o modelo sobre
//! o que a placa sabe fazer.
//!
//! # O plano de pinos é a política
//!
//! O operador declara quais pinos são entrada e quais são saída
//! ([`GpioAdapterConfig`]). Tudo que não foi declarado é **negado** —
//! `digital_write` num pino fora da lista de saídas é erro, não uma escrita
//! silenciosa. Isso importa porque um Pi tem 28 pinos BCM e alguns deles
//! estão fisicamente ligados a coisas que ninguém quer que o agente toque
//! (I²C do relógio, SPI de um display, a fonte de um relé). A lista é o
//! contrato; [`PlanoPinos`] é quem o cobra, e é puro — testável sem hardware.
//!
//! # Testes: o que dá e o que não dá para automatizar
//!
//! Este módulo **não tem teste de integração com GPIO real**, e isso é
//! deliberado — a própria issue #1130 prevê "GPIO só com testes
//! manuais/golden no CI ARM". O CI do repo roda em x86_64, onde
//! `rppal::gpio::Gpio::new()` falha por não achar um Raspberry Pi. Fingir
//! cobertura com um mock de `rppal` testaria o mock, não o adapter.
//!
//! O que **é** testado aqui, sem hardware: a validação da config, o plano de
//! pinos (allowlist, faixa BCM, pino declarado nas duas listas), a tabela de
//! capabilities e o fato de que num host que não é Pi o registro falha com um
//! [`HardwareError::Gpio`] limpo em vez de panicar.
//!
//! Validação manual num Pi (o "golden" da issue), para quem for reproduzir:
//!
//! ```text
//! # no Raspberry Pi, com o usuário no grupo gpio:
//! sudo usermod -aG gpio "$USER"   # e reabrir a sessão
//! ls -l /dev/gpiomem              # deve ser root:gpio, crw-rw----
//! cargo test -p garraia-hardware --features hardware-gpio -- --ignored
//! ```
//!
//! # Permissões
//!
//! `rppal` usa `/dev/gpiomem`, que no Raspberry Pi OS pertence ao grupo
//! `gpio`. Sem estar no grupo, `Gpio::new()` devolve `PermissionDenied` — o
//! erro que este adapter propaga com o texto do remédio, em vez de exigir
//! root.

use crate::Result;
use crate::capability::Capability;
use crate::device::Device;
use crate::error::HardwareError;
use crate::events::{EstadoObservado, HardwareEvent, HardwareEventBus, StateChanged};
use crate::perifericos;
use crate::registry::DeviceRegistry;
use crate::schema::validar_args;
use crate::state::DeviceStateStore;
use async_trait::async_trait;
use rppal::gpio::{Gpio, InputPin, OutputPin};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Maior pino BCM exposto no header de 40 vias do Raspberry Pi.
pub const BCM_MAX: u8 = 27;

/// Frequência padrão do PWM por software, em Hz. 1 kHz é o valor usual para
/// LED e para a maioria dos drivers de motor simples.
pub const PWM_FREQ_PADRAO: f64 = 1000.0;

/// Faixa aceita para a frequência de PWM configurável.
pub const PWM_FREQ_MIN: f64 = 1.0;
/// Teto do PWM por software do rppal — acima disso o jitter de escalonamento
/// do Linux domina e o sinal deixa de ser o que foi pedido.
pub const PWM_FREQ_MAX: f64 = 8000.0;

// ─────────────────────────────────────────────────────────────────────────────
// Plano de pinos — puro, testável sem Pi.
// ─────────────────────────────────────────────────────────────────────────────

/// Quais pinos o agente pode ler e quais pode escrever.
///
/// Separado do [`GpioDevice`] de propósito: é a parte que carrega a política
/// e é a parte que dá para testar em qualquer máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanoPinos {
    entradas: Vec<u8>,
    saidas: Vec<u8>,
}

impl PlanoPinos {
    /// Monta o plano, recusando o que não faz sentido físico.
    ///
    /// Um pino declarado como entrada **e** saída é o erro clássico: o
    /// adapter teria dois handles do rppal para o mesmo pino, com modos
    /// conflitantes, e quem ganharia dependeria da ordem de inicialização.
    pub fn novo(mut entradas: Vec<u8>, mut saidas: Vec<u8>) -> Result<Self> {
        entradas.sort_unstable();
        entradas.dedup();
        saidas.sort_unstable();
        saidas.dedup();

        if entradas.is_empty() && saidas.is_empty() {
            return Err(HardwareError::Gpio(
                "nenhum pino declarado: um dispositivo GPIO sem pino não expõe nada".to_string(),
            ));
        }
        for pino in entradas.iter().chain(saidas.iter()) {
            if *pino > BCM_MAX {
                return Err(HardwareError::Gpio(format!(
                    "pino BCM {pino} fora da faixa 0..={BCM_MAX}"
                )));
            }
        }
        if let Some(conflito) = entradas.iter().find(|p| saidas.contains(p)) {
            return Err(HardwareError::Gpio(format!(
                "pino BCM {conflito} declarado como entrada e saída ao mesmo tempo"
            )));
        }
        Ok(Self { entradas, saidas })
    }

    /// Os pinos de entrada, ordenados.
    pub fn entradas(&self) -> &[u8] {
        &self.entradas
    }

    /// Os pinos de saída, ordenados.
    pub fn saidas(&self) -> &[u8] {
        &self.saidas
    }

    /// Autoriza (ou não) escrever num pino.
    pub fn checar_saida(&self, pino: u8, dispositivo: &str) -> Result<()> {
        if self.saidas.contains(&pino) {
            return Ok(());
        }
        Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: format!(
                "pino BCM {pino} não está declarado como saída (saídas: {:?}) — escrita negada",
                self.saidas
            ),
        })
    }

    /// As capabilities que este plano justifica: `digital_read` só se há
    /// entrada, `digital_write`/`pwm` só se há saída.
    pub fn capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();
        if !self.entradas.is_empty() {
            caps.extend(perifericos::capability(perifericos::DIGITAL_READ));
        }
        if !self.saidas.is_empty() {
            caps.extend(perifericos::capability(perifericos::DIGITAL_WRITE));
            caps.extend(perifericos::capability(perifericos::PWM));
        }
        caps
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config.
// ─────────────────────────────────────────────────────────────────────────────

/// Config do adapter GPIO.
#[derive(Debug, Clone)]
pub struct GpioAdapterConfig {
    /// Id do dispositivo no registry.
    pub id: String,
    /// Plano de pinos (a política de acesso).
    pub plano: PlanoPinos,
    /// Frequência do PWM por software, em Hz.
    pub pwm_freq_hz: f64,
}

impl GpioAdapterConfig {
    /// Config validada.
    pub fn nova(id: impl Into<String>, entradas: Vec<u8>, saidas: Vec<u8>) -> Result<Self> {
        let id = id.into();
        if id.is_empty() || id.len() > 64 {
            return Err(HardwareError::Gpio(format!(
                "id '{id}' inválido: esperado 1..=64 caracteres"
            )));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(HardwareError::Gpio(format!(
                "id '{id}' inválido: só ASCII alfanumérico, '-', '_' e '.'"
            )));
        }
        Ok(Self {
            id,
            plano: PlanoPinos::novo(entradas, saidas)?,
            pwm_freq_hz: PWM_FREQ_PADRAO,
        })
    }

    /// Seta a frequência do PWM, dentro da faixa aceita.
    pub fn com_pwm_freq(mut self, hz: f64) -> Result<Self> {
        if !hz.is_finite() || !(PWM_FREQ_MIN..=PWM_FREQ_MAX).contains(&hz) {
            return Err(HardwareError::Gpio(format!(
                "frequência de PWM {hz} fora da faixa {PWM_FREQ_MIN}..={PWM_FREQ_MAX} Hz"
            )));
        }
        self.pwm_freq_hz = hz;
        Ok(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpioDevice.
// ─────────────────────────────────────────────────────────────────────────────

/// Os pinos do host como um [`Device`].
///
/// Os handles do rppal ficam sob `std::sync::Mutex` (não o do tokio): as
/// operações são escrita em memória mapeada, na casa dos microssegundos, e
/// nenhuma delas cruza um `.await` — segurar o lock por esse tempo é mais
/// barato que a alternativa assíncrona, e `spawn_blocking` seria custo puro.
pub struct GpioDevice {
    id: String,
    caps: Vec<Capability>,
    plano: PlanoPinos,
    pwm_freq_hz: f64,
    entradas: Mutex<BTreeMap<u8, InputPin>>,
    saidas: Mutex<BTreeMap<u8, OutputPin>>,
}

impl GpioDevice {
    /// Abre `/dev/gpiomem` e reserva os pinos do plano.
    ///
    /// Falha limpa (sem panic) quando o host não é um Raspberry Pi ou quando
    /// falta permissão — o texto do erro carrega o remédio.
    pub fn novo(config: &GpioAdapterConfig) -> Result<Self> {
        let gpio = Gpio::new().map_err(|e| {
            HardwareError::Gpio(format!(
                "não foi possível abrir o GPIO ({e}). Num Raspberry Pi, confira se \
                 /dev/gpiomem existe e se o usuário está no grupo 'gpio' \
                 (sudo usermod -aG gpio \"$USER\", e reabrir a sessão); em outro \
                 hardware, este adapter não se aplica"
            ))
        })?;

        let mut entradas = BTreeMap::new();
        for pino in config.plano.entradas() {
            let handle = gpio
                .get(*pino)
                .map_err(|e| HardwareError::Gpio(format!("pino BCM {pino} indisponível: {e}")))?;
            entradas.insert(*pino, handle.into_input());
        }
        let mut saidas = BTreeMap::new();
        for pino in config.plano.saidas() {
            let handle = gpio
                .get(*pino)
                .map_err(|e| HardwareError::Gpio(format!("pino BCM {pino} indisponível: {e}")))?;
            // `into_output_low` e não `into_output`: o estado inicial de uma
            // saída precisa ser conhecido. Subir o processo e deixar o nível
            // "o que estava lá" é como um relé liga sozinho no boot.
            saidas.insert(*pino, handle.into_output_low());
        }

        Ok(Self {
            id: config.id.clone(),
            caps: config.plano.capabilities(),
            plano: config.plano.clone(),
            pwm_freq_hz: config.pwm_freq_hz,
            entradas: Mutex::new(entradas),
            saidas: Mutex::new(saidas),
        })
    }

    fn capability(&self, nome: &str) -> Result<&Capability> {
        self.caps.iter().find(|c| c.name == nome).ok_or_else(|| {
            HardwareError::CapabilityDesconhecida {
                dispositivo: self.id.clone(),
                capability: nome.to_string(),
            }
        })
    }

    fn erro(&self, fonte: String) -> HardwareError {
        HardwareError::Adapter {
            dispositivo: self.id.clone(),
            fonte,
        }
    }

    /// Mutex envenenado por um panic em outra thread. Sem `unwrap`: o erro
    /// sobe como falha do dispositivo, e o agente lê o motivo.
    fn envenenado(&self, qual: &str) -> HardwareError {
        self.erro(format!(
            "estado dos pinos de {qual} inconsistente (lock envenenado por panic anterior)"
        ))
    }
}

#[async_trait]
impl Device for GpioDevice {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.caps.clone()
    }

    async fn read(&self, capability: &str) -> Result<Value> {
        self.capability(capability)?;
        if capability != perifericos::DIGITAL_READ {
            return Err(self.erro(format!("capability '{capability}' não é leitura")));
        }
        let entradas = self
            .entradas
            .lock()
            .map_err(|_| self.envenenado("entrada"))?;
        let mut pinos = serde_json::Map::new();
        for (pino, handle) in entradas.iter() {
            pinos.insert(pino.to_string(), json!(u8::from(handle.is_high())));
        }
        Ok(json!({ "pins": pinos }))
    }

    async fn execute(&self, capability: &str, args: Value) -> Result<Value> {
        let cap = self.capability(capability)?;
        if cap.read_only {
            return Err(self.erro(format!(
                "capability '{capability}' é read_only (R0): use read, não execute"
            )));
        }
        validar_args(&args, cap.args_schema.as_ref(), &self.id, capability)?;
        let pino = perifericos::extrair_pino(&args, &self.id)?;
        // A allowlist do operador vem antes de qualquer toque no hardware.
        self.plano.checar_saida(pino, &self.id)?;

        let mut saidas = self.saidas.lock().map_err(|_| self.envenenado("saída"))?;
        let handle = saidas
            .get_mut(&pino)
            .ok_or_else(|| self.erro(format!("pino BCM {pino} não foi reservado no boot")))?;

        match capability {
            perifericos::DIGITAL_WRITE => {
                let alto = perifericos::extrair_nivel(&args, &self.id)?;
                // Um pino em PWM ignora set_high/set_low até o PWM parar.
                handle
                    .clear_pwm()
                    .map_err(|e| self.erro(format!("pino BCM {pino}: parar PWM falhou: {e}")))?;
                if alto {
                    handle.set_high();
                } else {
                    handle.set_low();
                }
                Ok(json!({
                    "device": self.id,
                    "capability": capability,
                    "pin": pino,
                    "value": u8::from(alto),
                }))
            }
            perifericos::PWM => {
                let duty = perifericos::extrair_duty(&args, &self.id)?;
                // A tabela fala na escala 0..=255 do `analogWrite`; o rppal
                // quer a fração. A conversão vive aqui, uma vez.
                let fracao = f64::from(duty) / perifericos::PWM_DUTY_MAX as f64;
                handle
                    .set_pwm_frequency(self.pwm_freq_hz, fracao)
                    .map_err(|e| self.erro(format!("pino BCM {pino}: PWM falhou: {e}")))?;
                Ok(json!({
                    "device": self.id,
                    "capability": capability,
                    "pin": pino,
                    "duty": duty,
                    "freq_hz": self.pwm_freq_hz,
                }))
            }
            outro => Err(self.erro(format!("capability '{outro}' não é executável aqui"))),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Registro.
// ─────────────────────────────────────────────────────────────────────────────

/// Abre os pinos e registra o dispositivo.
///
/// Não há manager com task de fundo aqui: GPIO local não tem conexão para
/// manter nem evento para escutar. O que existe é o registro, e ele acontece
/// uma vez no boot.
///
/// Num host que não é Raspberry Pi isto devolve [`HardwareError::Gpio`] e
/// **nada é registrado** — é o caminho que o gateway percorre em x86, e por
/// isso o chamador trata o erro como "adapter não se aplica", não como falha
/// de boot.
pub async fn registrar(
    config: &GpioAdapterConfig,
    registry: Arc<DeviceRegistry>,
    state: Option<Arc<DeviceStateStore>>,
    bus: Option<Arc<HardwareEventBus>>,
) -> Result<Arc<GpioDevice>> {
    let device = Arc::new(GpioDevice::novo(config)?);
    registry.register(device.clone());
    if let Some(store) = state
        && let Err(e) = store.marcar(&config.id, true).await
    {
        tracing::warn!(dispositivo = %config.id, error = %e, "gpio: falha ao marcar presença");
    }
    if let Some(bus) = bus {
        bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            config.id.clone(),
            EstadoObservado::com(Some("online".into()), Value::Null, true),
            None,
        )));
    }
    tracing::info!(
        dispositivo = %config.id,
        entradas = ?config.plano.entradas(),
        saidas = ?config.plano.saidas(),
        "gpio: dispositivo registrado"
    );
    Ok(device)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Plano de pinos (puro — roda em qualquer máquina) ──────────────────

    #[test]
    fn plano_ordena_e_deduplica() {
        let plano = PlanoPinos::novo(vec![17, 4, 17], vec![27, 27, 13]).expect("válido");
        assert_eq!(plano.entradas(), &[4, 17]);
        assert_eq!(plano.saidas(), &[13, 27]);
    }

    #[test]
    fn plano_recusa_vazio_fora_de_faixa_e_conflito() {
        let err = PlanoPinos::novo(vec![], vec![]).expect_err("vazio recusado");
        assert!(err.to_string().contains("nenhum pino"), "{err}");

        let err = PlanoPinos::novo(vec![], vec![BCM_MAX + 1]).expect_err("fora de faixa");
        assert!(err.to_string().contains("fora da faixa"), "{err}");

        let err = PlanoPinos::novo(vec![13], vec![13]).expect_err("conflito");
        assert!(
            err.to_string().contains("entrada e saída"),
            "o pino não pode ser os dois: {err}"
        );
    }

    /// A allowlist é a política: escrever num pino não declarado é erro, e o
    /// erro diz quais pinos eram permitidos.
    #[test]
    fn escrita_fora_da_allowlist_e_negada() {
        let plano = PlanoPinos::novo(vec![17], vec![13]).expect("válido");
        plano.checar_saida(13, "pi").expect("13 é saída declarada");

        let err = plano
            .checar_saida(17, "pi")
            .expect_err("entrada não é saída");
        assert!(err.to_string().contains("escrita negada"), "{err}");
        let err = plano.checar_saida(5, "pi").expect_err("não declarado");
        assert!(err.to_string().contains("[13]"), "cita as saídas: {err}");
        assert!(err.to_string().contains("pi"), "cita o dispositivo: {err}");
    }

    /// As capabilities acompanham o plano: sem saída declarada, o agente nem
    /// enxerga `digital_write` — o que ele não vê, ele não tenta.
    #[test]
    fn capabilities_seguem_o_plano() {
        let so_entrada = PlanoPinos::novo(vec![17], vec![]).expect("válido");
        let nomes: Vec<String> = so_entrada
            .capabilities()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(nomes, vec![perifericos::DIGITAL_READ.to_string()]);

        let so_saida = PlanoPinos::novo(vec![], vec![13]).expect("válido");
        let nomes: Vec<String> = so_saida
            .capabilities()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(
            nomes,
            vec![
                perifericos::DIGITAL_WRITE.to_string(),
                perifericos::PWM.to_string()
            ]
        );

        // E o risco é o da tabela, não do adapter.
        let ambos = PlanoPinos::novo(vec![17], vec![13]).expect("válido");
        for cap in ambos.capabilities() {
            cap.validar().expect("invariante leitura↔R0");
            match cap.name.as_str() {
                perifericos::DIGITAL_READ => assert_eq!(cap.risk, crate::risk::RiskClass::R0),
                _ => assert_eq!(cap.risk, crate::risk::RiskClass::R2),
            }
        }
        // O Pi não tem ADC: analog_read nunca é declarado.
        assert!(
            !ambos
                .capabilities()
                .iter()
                .any(|c| c.name == perifericos::ANALOG_READ),
            "o Raspberry Pi não tem conversor analógico embutido"
        );
    }

    // ─── Config ────────────────────────────────────────────────────────────

    #[test]
    fn config_valida_id_e_frequencia() {
        let cfg = GpioAdapterConfig::nova("pi-bancada", vec![17], vec![13]).expect("válida");
        assert_eq!(cfg.pwm_freq_hz, PWM_FREQ_PADRAO);

        assert!(GpioAdapterConfig::nova("", vec![17], vec![]).is_err());
        assert!(GpioAdapterConfig::nova("pi bancada", vec![17], vec![]).is_err());

        let cfg = GpioAdapterConfig::nova("pi", vec![], vec![13]).expect("válida");
        assert!(cfg.clone().com_pwm_freq(0.0).is_err());
        assert!(cfg.clone().com_pwm_freq(f64::NAN).is_err());
        assert!(cfg.clone().com_pwm_freq(PWM_FREQ_MAX + 1.0).is_err());
        assert_eq!(
            cfg.com_pwm_freq(500.0)
                .expect("500 Hz é válido")
                .pwm_freq_hz,
            500.0
        );
    }

    // ─── Host sem Pi ───────────────────────────────────────────────────────

    /// O caminho que o CI x86 percorre de verdade: `Gpio::new()` falha e o
    /// adapter devolve erro **limpo**, com o remédio no texto, sem panicar e
    /// sem registrar dispositivo fantasma.
    ///
    /// O teste aceita os dois desfechos de propósito — num Raspberry Pi real
    /// o registro tem que funcionar, e aí ele vira cobertura de verdade.
    #[tokio::test]
    async fn em_host_sem_gpio_falha_limpo_e_nao_registra() {
        let cfg = GpioAdapterConfig::nova("pi-teste", vec![17], vec![13]).expect("config válida");
        let registry = Arc::new(DeviceRegistry::new());
        match registrar(&cfg, registry.clone(), None, None).await {
            Ok(_) => {
                // Rodando num Pi: o dispositivo tem que estar no registry.
                assert!(
                    registry.get("pi-teste").is_some(),
                    "registro bem-sucedido precisa aparecer na descoberta"
                );
            }
            Err(e) => {
                assert!(
                    matches!(e, HardwareError::Gpio(_)),
                    "host sem GPIO devolve HardwareError::Gpio, não outro erro: {e}"
                );
                assert!(
                    e.to_string().contains("gpiomem"),
                    "o erro carrega o remédio (grupo gpio / /dev/gpiomem): {e}"
                );
                assert!(
                    registry.is_empty(),
                    "falha de abertura não pode deixar dispositivo fantasma no registry"
                );
            }
        }
    }
}
