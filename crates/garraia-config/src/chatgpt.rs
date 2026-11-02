//! A entrada `llm.<nome>` do provider `openai-codex` (Sign in with ChatGPT),
//! gravada pela CLI (`garra chatgpt login --padrao`, `garra chatgpt
//! configurar`) e pelo painel da porta 3888 — uma regra so para os dois.
//!
//! Mesma postura do `config set-model`: troca so a entrada do ChatGPT e, se
//! pedido, o `agent.default_provider`. Os outros providers ficam intactos; o
//! padrao anterior vira o primeiro do fallback em vez de sumir. Nunca grava
//! chave: a credencial do ChatGPT e o login OAuth em
//! `<config_dir>/credentials/openai-codex`.

use crate::model::{AppConfig, LlmProviderConfig};

/// Tipo do provider na config.
pub const TIPO_OPENAI_CODEX: &str = "openai-codex";
/// Nome da entrada criada quando nenhuma do tipo existe.
pub const ENTRADA_PADRAO: &str = "chatgpt";
/// Modelo e esforco preferenciais da entrada nova.
pub const MODELO_PREFERENCIAL: &str = "gpt-6.1-sol";
pub const ESFORCO_PREFERENCIAL: &str = "high";

/// O que gravar.
#[derive(Debug, Clone, Default)]
pub struct AjusteDoChatGpt<'a> {
    /// Modelo (`None` mantem o atual; entrada nova usa o preferencial).
    pub modelo: Option<&'a str>,
    /// `reasoning.effort` (`None` mantem o atual; entrada nova usa `high`).
    pub esforco: Option<&'a str>,
    /// Tornar esta entrada o provider padrao do agente.
    pub tornar_padrao: bool,
}

/// A chave da entrada `openai-codex` existente (a primeira em ordem
/// alfabetica, para ser deterministico), ou `None`.
pub fn entrada_existente(config: &AppConfig) -> Option<String> {
    let mut chaves: Vec<&String> = config
        .llm
        .iter()
        .filter(|(_, c)| c.provider == TIPO_OPENAI_CODEX)
        .map(|(k, _)| k)
        .collect();
    chaves.sort();
    chaves.first().map(|k| (*k).clone())
}

/// Aplica o ajuste em `config` e devolve a chave da entrada.
pub fn aplicar(config: &mut AppConfig, ajuste: &AjusteDoChatGpt<'_>) -> String {
    let chave = entrada_existente(config).unwrap_or_else(|| ENTRADA_PADRAO.to_string());
    let entrada = config
        .llm
        .entry(chave.clone())
        .or_insert_with(|| LlmProviderConfig {
            provider: TIPO_OPENAI_CODEX.to_string(),
            model: Some(MODELO_PREFERENCIAL.to_string()),
            api_key: None,
            base_url: None,
            extra: Default::default(),
        });
    if let Some(m) = ajuste.modelo.map(str::trim).filter(|m| !m.is_empty()) {
        entrada.model = Some(m.to_string());
    }
    if entrada.model.as_deref().is_none_or(|m| m.trim().is_empty()) {
        entrada.model = Some(MODELO_PREFERENCIAL.to_string());
    }
    let esforco = ajuste
        .esforco
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .or_else(|| {
            entrada
                .extra
                .get("reasoning_effort")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| ESFORCO_PREFERENCIAL.to_string());
    entrada.extra.insert(
        "reasoning_effort".into(),
        serde_json::Value::String(esforco),
    );
    // Credencial e endpoint nunca vem da config para este tipo.
    entrada.api_key = None;
    entrada.base_url = None;

    if ajuste.tornar_padrao {
        let anterior = config.agent.default_provider.replace(chave.clone());
        if let Some(prev) = anterior
            && prev != chave
            && config.llm.contains_key(&prev)
            && !config.agent.fallback_providers.contains(&prev)
        {
            config.agent.fallback_providers.insert(0, prev);
        }
        config.agent.fallback_providers.retain(|f| f != &chave);
    }
    chave
}

#[cfg(test)]
mod tests {
    use super::*;

    fn com_openrouter() -> AppConfig {
        let mut c = AppConfig::default();
        c.llm.insert(
            "openrouter".into(),
            LlmProviderConfig {
                provider: "openrouter".into(),
                model: Some("anthropic/claude-haiku-5.5".into()),
                api_key: None,
                base_url: Some("https://openrouter.ai/api/v1".into()),
                extra: Default::default(),
            },
        );
        c.agent.default_provider = Some("openrouter".into());
        c
    }

    #[test]
    fn entrada_nova_tem_o_modelo_e_o_esforco_preferenciais() {
        let mut c = com_openrouter();
        let chave = aplicar(
            &mut c,
            &AjusteDoChatGpt {
                tornar_padrao: true,
                ..Default::default()
            },
        );
        assert_eq!(chave, "chatgpt");
        let e = &c.llm["chatgpt"];
        assert_eq!(e.provider, "openai-codex");
        assert_eq!(e.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(e.extra["reasoning_effort"], "high");
        assert!(e.api_key.is_none() && e.base_url.is_none());
        assert_eq!(c.agent.default_provider.as_deref(), Some("chatgpt"));
        // O padrao anterior vira o primeiro fallback; o resto fica intacto.
        assert_eq!(c.agent.fallback_providers, vec!["openrouter".to_string()]);
        assert_eq!(
            c.llm["openrouter"].model.as_deref(),
            Some("anthropic/claude-haiku-5.5")
        );
    }

    #[test]
    fn ajuste_muda_so_o_pedido_e_reusa_a_entrada_existente() {
        let mut c = com_openrouter();
        aplicar(&mut c, &AjusteDoChatGpt::default());
        assert_eq!(
            c.agent.default_provider.as_deref(),
            Some("openrouter"),
            "sem tornar_padrao nao mexe"
        );
        let chave = aplicar(
            &mut c,
            &AjusteDoChatGpt {
                esforco: Some("XHigh"),
                ..Default::default()
            },
        );
        assert_eq!(chave, "chatgpt");
        assert_eq!(c.llm["chatgpt"].model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(c.llm["chatgpt"].extra["reasoning_effort"], "xhigh");
        aplicar(
            &mut c,
            &AjusteDoChatGpt {
                modelo: Some("gpt-6-luna"),
                ..Default::default()
            },
        );
        assert_eq!(c.llm["chatgpt"].model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(
            c.llm["chatgpt"].extra["reasoning_effort"], "xhigh",
            "esforco mantido"
        );
        assert_eq!(c.llm.len(), 2);
    }
}
