//! A tabela fechada de periféricos de placa — `digital_read`, `analog_read`,
//! `digital_write` e `pwm` (#1130).
//!
//! Os dois adapters da terceira onda expõem o **mesmo** conjunto de
//! capabilities: o serial (Arduino/ESP32, [`crate::adapter_serial`]) e o GPIO
//! direto (Raspberry Pi, [`crate::adapter_gpio`]). Um Arduino piscando um LED
//! no pino 13 e um Pi piscando o mesmo LED no BCM 13 são, para o agente, a
//! mesma capability com o mesmo risco — e é isso que este módulo garante.
//!
//! | Capability | Risco | Leitura? | Efeito |
//! |---|---|---|---|
//! | `digital_read` | R0 | sim | lê os pinos de entrada declarados |
//! | `analog_read` | R0 | sim | lê os canais analógicos declarados |
//! | `digital_write` | R2 | não | muda o nível de um pino de saída |
//! | `pwm` | R2 | não | muda o duty cycle de um pino de saída |
//!
//! # Por que R2 e não R1
//!
//! Um `digital_write` num pino de placa é *pequena mudança física*: aciona
//! relé, motor, válvula, trava. O adapter não sabe — e não pode saber — o que
//! está do outro lado do fio. R1 ("ação reversível", como acender uma lâmpada
//! que o hub já classificou) pressupõe um hub que conhece o domínio do
//! dispositivo; aqui não há hub. R2 é o teto declarado do slice e o piso de
//! todo `write`: entra na policy **com rate limit**, nunca em `Auto`.
//!
//! # Por que a tabela é fechada (e o risco não vem do dispositivo)
//!
//! O adapter MQTT (#1126) aceita o risk class do manifesto porque o broker
//! tem ACL: quem publica em `garra/devices/+/capabilities` passou por
//! autenticação do broker. Uma placa USB **não tem essa barreira** — qualquer
//! pessoa com acesso físico à máquina pluga um dispositivo que se anuncia
//! como quiser. Se o risco viesse do manifesto, esse dispositivo declararia
//! `{"name": "digital_write", "risk": "r0", "read_only": true}` e o
//! [`crate::HardwareGate`] o aprovaria em `Auto` — escalada de privilégio por
//! cabo USB.
//!
//! Então, aqui, o manifesto declara apenas **quais** capabilities da tabela a
//! placa expõe; o risco vem do código, revisado neste PR. Nome fora da tabela
//! não vira capability e o dispositivo inteiro é recusado — a mesma regra
//! fail-closed do domínio desconhecido no adapter Home Assistant (#1127).

use crate::Result;
use crate::capability::Capability;
use crate::error::HardwareError;
use crate::risk::RiskClass;
use serde_json::{Value, json};

/// Lê os pinos digitais de entrada declarados (R0).
pub const DIGITAL_READ: &str = "digital_read";
/// Lê os canais analógicos declarados (R0).
pub const ANALOG_READ: &str = "analog_read";
/// Muda o nível de um pino de saída (R2).
pub const DIGITAL_WRITE: &str = "digital_write";
/// Muda o duty cycle de um pino de saída (R2).
pub const PWM: &str = "pwm";

/// Os nomes da tabela, na ordem canônica de exibição.
pub const NOMES: [&str; 4] = [DIGITAL_READ, ANALOG_READ, DIGITAL_WRITE, PWM];

/// Duty cycle máximo do `pwm` — a escala de 8 bits do `analogWrite` do
/// Arduino, que o adapter GPIO normaliza para a fração que o rppal espera.
pub const PWM_DUTY_MAX: i64 = 255;

/// Maior número de pino aceito. Cobre com folga o BCM do Raspberry Pi
/// (0–27) e a numeração de placas Arduino/ESP32 usuais, e é o que impede um
/// argumento absurdo (`pin: 99999`) de chegar ao firmware.
pub const PINO_MAX: i64 = 63;

/// A [`Capability`] da tabela para este nome, ou `None` se o nome não está
/// na tabela (fail-closed: o chamador recusa o dispositivo).
pub fn capability(nome: &str) -> Option<Capability> {
    let pino = || {
        json!({
            "type": "object",
            "properties": { "pin": { "type": "integer" } },
            "required": ["pin"]
        })
    };
    match nome {
        DIGITAL_READ | ANALOG_READ => Some(Capability::leitura(nome, None)),
        DIGITAL_WRITE => {
            let mut schema = pino();
            schema["properties"]["value"] = json!({ "type": "integer" });
            schema["required"] = json!(["pin", "value"]);
            // `acao` só falha com risco R0, e R2 é constante aqui — mas o
            // `.ok()` mantém a promessa de "nenhum unwrap em produção".
            Capability::acao(nome, RiskClass::R2, Some(schema)).ok()
        }
        PWM => {
            let mut schema = pino();
            schema["properties"]["duty"] = json!({ "type": "integer" });
            schema["required"] = json!(["pin", "duty"]);
            Capability::acao(nome, RiskClass::R2, Some(schema)).ok()
        }
        _ => None,
    }
}

/// A tabela inteira — o que uma placa com as quatro funções expõe.
pub fn capabilities_padrao() -> Vec<Capability> {
    NOMES.iter().filter_map(|nome| capability(nome)).collect()
}

/// Resolve uma lista de nomes declarados numa lista de capabilities da
/// tabela.
///
/// Fail-closed em três frentes: lista vazia (um dispositivo sem capability
/// não é um dispositivo), nome fora da tabela e nome repetido.
pub fn resolver(nomes: &[String], dispositivo: &str) -> Result<Vec<Capability>> {
    if nomes.is_empty() {
        return Err(HardwareError::CapabilityInvalida {
            nome: "<vazio>".to_string(),
            classe: "-".to_string(),
            motivo: format!("dispositivo '{dispositivo}' não declarou nenhuma capability"),
        });
    }
    let mut caps: Vec<Capability> = Vec::with_capacity(nomes.len());
    for nome in nomes {
        if caps.iter().any(|c| &c.name == nome) {
            return Err(HardwareError::CapabilityInvalida {
                nome: nome.clone(),
                classe: "-".to_string(),
                motivo: format!("declarada duas vezes por '{dispositivo}'"),
            });
        }
        let cap = capability(nome).ok_or_else(|| HardwareError::CapabilityInvalida {
            nome: nome.clone(),
            classe: "-".to_string(),
            motivo: format!(
                "fora da tabela de periféricos ({}) — dispositivo '{dispositivo}' recusado \
                 (fail-closed: risco vem do código, não do dispositivo)",
                NOMES.join(", ")
            ),
        })?;
        // Cinto e suspensório: a tabela é constante e correta, mas a
        // invariante leitura↔R0 é checada na fronteira de qualquer jeito.
        cap.validar()?;
        caps.push(cap);
    }
    Ok(caps)
}

/// Extrai e valida o argumento `pin`, na faixa `0..=PINO_MAX`.
pub fn extrair_pino(args: &Value, dispositivo: &str) -> Result<u8> {
    let bruto = args
        .get("pin")
        .and_then(Value::as_i64)
        .ok_or_else(|| HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: "argumento 'pin' ausente ou não é inteiro".to_string(),
        })?;
    if !(0..=PINO_MAX).contains(&bruto) {
        return Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: format!("pino {bruto} fora da faixa 0..={PINO_MAX}"),
        });
    }
    Ok(bruto as u8)
}

/// Extrai e valida o argumento `duty` do `pwm`, na faixa `0..=PWM_DUTY_MAX`.
pub fn extrair_duty(args: &Value, dispositivo: &str) -> Result<u8> {
    let bruto = args
        .get("duty")
        .and_then(Value::as_i64)
        .ok_or_else(|| HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: "argumento 'duty' ausente ou não é inteiro".to_string(),
        })?;
    if !(0..=PWM_DUTY_MAX).contains(&bruto) {
        return Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: format!("duty {bruto} fora da faixa 0..={PWM_DUTY_MAX}"),
        });
    }
    Ok(bruto as u8)
}

/// Extrai e valida o argumento `value` do `digital_write` — só 0 ou 1.
pub fn extrair_nivel(args: &Value, dispositivo: &str) -> Result<bool> {
    let bruto =
        args.get("value")
            .and_then(Value::as_i64)
            .ok_or_else(|| HardwareError::Adapter {
                dispositivo: dispositivo.to_string(),
                fonte: "argumento 'value' ausente ou não é inteiro".to_string(),
            })?;
    match bruto {
        0 => Ok(false),
        1 => Ok(true),
        outro => Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: format!("value {outro} inválido: digital_write aceita apenas 0 ou 1"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tabela é o contrato de risco do slice: leitura R0, escrita R2.
    /// Se este teste mudar, mudou a superfície física que o agente alcança
    /// sem confirmação — e isso é decisão de PR, não de refactor.
    #[test]
    fn tabela_fixa_leitura_r0_e_escrita_r2() {
        let caps = capabilities_padrao();
        assert_eq!(caps.len(), 4, "a tabela tem exatamente quatro entradas");

        for cap in &caps {
            cap.validar().expect("invariante leitura↔R0");
            match cap.name.as_str() {
                DIGITAL_READ | ANALOG_READ => {
                    assert_eq!(cap.risk, RiskClass::R0, "{} é leitura", cap.name);
                    assert!(cap.read_only, "{} é read_only", cap.name);
                    assert!(
                        cap.args_schema.is_none(),
                        "{} não recebe argumentos (o trait Device::read não os carrega)",
                        cap.name
                    );
                }
                DIGITAL_WRITE | PWM => {
                    assert_eq!(cap.risk, RiskClass::R2, "{} é escrita física", cap.name);
                    assert!(!cap.read_only, "{} não é read_only", cap.name);
                    assert!(cap.args_schema.is_some(), "{} recebe pino", cap.name);
                }
                outro => panic!("nome fora da tabela: {outro}"),
            }
        }
    }

    /// Nome fora da tabela nunca vira capability — é o que impede uma placa
    /// USB de se anunciar como `door_unlock`.
    #[test]
    fn nome_fora_da_tabela_nao_existe() {
        assert!(capability("door_unlock").is_none());
        assert!(capability("digital_writ").is_none());
        assert!(capability("").is_none());
        assert!(capability("DIGITAL_WRITE").is_none(), "case-sensitive");
    }

    #[test]
    fn resolver_recusa_vazio_desconhecido_e_repetido() {
        let err = resolver(&[], "placa").expect_err("lista vazia recusada");
        assert!(err.to_string().contains("nenhuma capability"), "{err}");

        let err = resolver(&["door_unlock".to_string()], "placa").expect_err("recusado");
        assert!(err.to_string().contains("fora da tabela"), "{err}");
        assert!(
            err.to_string().contains("placa"),
            "cita o dispositivo: {err}"
        );

        let err = resolver(
            &[DIGITAL_READ.to_string(), DIGITAL_READ.to_string()],
            "placa",
        )
        .expect_err("repetido recusado");
        assert!(err.to_string().contains("duas vezes"), "{err}");
    }

    #[test]
    fn resolver_aceita_subconjunto_na_ordem_declarada() {
        let caps = resolver(&[PWM.to_string(), DIGITAL_READ.to_string()], "placa").expect("válido");
        let nomes: Vec<&str> = caps.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(nomes, vec![PWM, DIGITAL_READ], "ordem do declarante");
    }

    /// Os argumentos são a fronteira entre o modelo e o fio: faixa fechada,
    /// tipo fechado, nada de `pin: 99999` nem `duty: -1`.
    #[test]
    fn argumentos_fora_de_faixa_sao_recusados() {
        assert_eq!(extrair_pino(&json!({"pin": 13}), "d").expect("ok"), 13);
        assert_eq!(extrair_pino(&json!({"pin": 0}), "d").expect("ok"), 0);
        assert!(extrair_pino(&json!({"pin": -1}), "d").is_err());
        assert!(extrair_pino(&json!({"pin": PINO_MAX + 1}), "d").is_err());
        assert!(extrair_pino(&json!({"pin": "13"}), "d").is_err());
        assert!(extrair_pino(&json!({}), "d").is_err());

        assert_eq!(extrair_duty(&json!({"duty": 255}), "d").expect("ok"), 255);
        assert!(extrair_duty(&json!({"duty": 256}), "d").is_err());
        assert!(extrair_duty(&json!({"duty": -1}), "d").is_err());

        assert!(!extrair_nivel(&json!({"value": 0}), "d").expect("ok"));
        assert!(extrair_nivel(&json!({"value": 1}), "d").expect("ok"));
        assert!(
            extrair_nivel(&json!({"value": 2}), "d").is_err(),
            "só 0 ou 1"
        );
        assert!(
            extrair_nivel(&json!({"value": true}), "d").is_err(),
            "booleano não é o contrato do fio"
        );
    }

    /// O erro aponta o dispositivo — é o texto que o modelo lê para decidir
    /// o próximo passo.
    #[test]
    fn erro_de_argumento_nomeia_o_dispositivo() {
        let err = extrair_pino(&json!({"pin": 900}), "arduino-bancada").expect_err("fora da faixa");
        assert!(err.to_string().contains("arduino-bancada"), "{err}");
    }

    /// O schema da tabela tem que passar pelo validador do crate — dois
    /// caminhos que precisam concordar (a capability declara, o
    /// `schema::validar_args` cobra).
    #[test]
    fn schema_da_tabela_casa_com_o_validador() {
        let escrita = capability(DIGITAL_WRITE).expect("existe");
        let schema = escrita.args_schema.as_ref();
        assert!(
            crate::schema::validar_args(
                &json!({"pin": 13, "value": 1}),
                schema,
                "d",
                DIGITAL_WRITE
            )
            .is_ok()
        );
        // Faltando o obrigatório.
        assert!(
            crate::schema::validar_args(&json!({"pin": 13}), schema, "d", DIGITAL_WRITE).is_err()
        );
        // Tipo errado.
        assert!(
            crate::schema::validar_args(
                &json!({"pin": "13", "value": 1}),
                schema,
                "d",
                DIGITAL_WRITE
            )
            .is_err()
        );

        let pwm = capability(PWM).expect("existe");
        let schema = pwm.args_schema.as_ref();
        assert!(
            crate::schema::validar_args(&json!({"pin": 5, "duty": 128}), schema, "d", PWM).is_ok()
        );
        assert!(crate::schema::validar_args(&json!({"pin": 5}), schema, "d", PWM).is_err());
    }
}
