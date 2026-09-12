//! Validação truncada de `args` contra o JSON Schema leve da capability.
//!
//! O schema que uma [`crate::Device`] declara em `Capability::args_schema`
//! é um **subconjunto** de JSON Schema — só `type`/`properties`/`required`
//! são garantidos (ver ADR 0020 §capabilities). Este módulo é a única
//! implementação do validador: o adapter MQTT (#1126) e o Home Assistant
//! (#1127) validam contra o MESMO código, para dois adapters nunca
//! divergirem no que aceitam.

use crate::error::HardwareError;
use crate::Result;

/// Checa `args` contra `schema` — fail-closed em tudo que o validador não
/// entende: tipo de schema diferente de `object`, propriedade sem `type`,
/// tipo primitivo desconhecido → recusa. Chaves extras nos args passam
/// (o schema truncado não declara `additionalProperties`).
///
/// `schema: None` significa "o dispositivo declarou que não recebe
/// argumentos" — args precisam ser `null` ou objeto vazio.
pub fn validar_args(
    args: &serde_json::Value,
    schema: Option<&serde_json::Value>,
    dispositivo: &str,
    capability: &str,
) -> Result<()> {
    let recusa = |fonte: String| {
        Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte,
        })
    };
    let Some(schema) = schema else {
        // Sem schema, o dispositivo declarou que não recebe argumentos.
        return if args.as_object().is_some_and(|obj| obj.is_empty()) || args.is_null() {
            Ok(())
        } else {
            recusa(format!(
                "capability '{capability}' não declara argumentos; recebi {args}"
            ))
        };
    };
    let Some(obj_schema) = schema.as_object() else {
        return recusa(format!("args_schema não é um objeto JSON: {schema}"));
    };
    if let Some(tipo) = obj_schema.get("type")
        && tipo != "object"
    {
        return recusa(format!(
            "args_schema suporta apenas type=object neste slice; recebi {tipo}"
        ));
    }
    let Some(obj_args) = args.as_object() else {
        return recusa(format!(
            "argumentos precisam ser um objeto JSON; recebi {args}"
        ));
    };
    if let Some(obrigatorios) = obj_schema.get("required").and_then(serde_json::Value::as_array) {
        for nome in obrigatorios {
            let Some(nome) = nome.as_str() else {
                return recusa(format!(
                    "args_schema.required contém item não-texto: {nome}"
                ));
            };
            if !obj_args.contains_key(nome) {
                return recusa(format!("argumento obrigatório '{nome}' ausente"));
            }
        }
    }
    let Some(propriedades) = obj_schema.get("properties").and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    for (nome, spec) in propriedades {
        let Some(valor) = obj_args.get(nome) else {
            continue;
        };
        let Some(tipo) = spec.get("type").and_then(serde_json::Value::as_str) else {
            return recusa(format!(
                "argumento '{nome}': schema não declara o tipo — o validador truncado só conhece type/properties/required"
            ));
        };
        let ok = match tipo {
            "string" => valor.is_string(),
            "boolean" => valor.is_boolean(),
            "number" => valor.is_number(),
            "integer" => valor.as_i64().is_some(),
            "array" => valor.is_array(),
            "object" => valor.is_object(),
            "null" => valor.is_null(),
            _ => {
                return recusa(format!(
                    "argumento '{nome}': tipo '{tipo}' não suportado pelo validador (fail-closed)"
                ));
            }
        };
        if !ok {
            return recusa(format!(
                "argumento '{nome}': esperado {tipo}, recebi {valor}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Os quatro casos que o gate de execução confia: primitivos passam,
    /// erros típicos recusam, schema ausente recusa argumento, schema que
    /// o validador não entende recusa por inteiro (fail-closed).
    #[test]
    fn aceita_tipos_primitivos_e_recusa_os_erros_tipicos() {
        let schema = json!({
            "type": "object",
            "properties": {
                "on": { "type": "boolean" },
                "brightness": { "type": "integer" },
                "nome": { "type": "string" }
            },
            "required": ["on"]
        });

        assert!(validar_args(&json!({"on": true, "brightness": 200}), Some(&schema), "l", "p").is_ok());
        assert!(validar_args(&json!({"on": true, "nome": "x"}), Some(&schema), "l", "p").is_ok());
        // Faltando o obrigatório.
        assert!(validar_args(&json!({}), Some(&schema), "l", "p").is_err());
        // Tipo errado.
        assert!(validar_args(&json!({"on": "sim"}), Some(&schema), "l", "p").is_err());
        // Args não-objeto.
        assert!(validar_args(&json!([1]), Some(&schema), "l", "p").is_err());
        // Chave extra passa (schema truncado não declara additionalProperties).
        assert!(validar_args(&json!({"on": true, "extra": 1}), Some(&schema), "l", "p").is_ok());
    }

    #[test]
    fn sem_schema_so_aceita_vazio_ou_null() {
        assert!(validar_args(&json!({}), None, "l", "p").is_ok());
        assert!(validar_args(&serde_json::Value::Null, None, "l", "p").is_ok());
        assert!(validar_args(&json!({"on": true}), None, "l", "p").is_err());
    }

    #[test]
    fn fail_closed_em_schema_que_nao_entende() {
        // type != object.
        assert!(validar_args(&json!({}), Some(&json!({"type": "string"})), "l", "p").is_err());
        // Propriedade sem type.
        let schema = json!({"type": "object", "properties": {"x": {}}});
        assert!(validar_args(&json!({"x": 1}), Some(&schema), "l", "p").is_err());
        // Tipo desconhecido.
        let schema = json!({"type": "object", "properties": {"x": {"type": "word"}}});
        assert!(validar_args(&json!({"x": 1}), Some(&schema), "l", "p").is_err());
        // Schema não-objeto.
        assert!(validar_args(&json!({}), Some(&json!([1])), "l", "p").is_err());
    }

    /// O erro cita dispositivo e capability — é o texto que o modelo enxerga.
    #[test]
    fn erro_nomeia_dispositivo_e_capability() {
        let err = validar_args(&json!({"on": true}), None, "light.sala", "power")
            .expect_err("args sem schema recusa");
        let msg = err.to_string();
        assert!(msg.contains("light.sala"), "cita o dispositivo: {msg}");
        assert!(msg.contains("power"), "cita a capability: {msg}");
    }
}