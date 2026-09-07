use garraia_config::{AppConfig, NamedAgentConfig};

/// Resolve which named agent config to use for a given request.
///
/// Priority: explicit agent_id > channel setting > "default" agent > legacy config.
/// Resolve um agente pedido **pelo nome**, sem nenhum fallback (#965).
///
/// O [`resolve`] existe para escolher um agente quando ninguém escolheu: ele
/// tenta o `agent_id`, depois o canal, depois o `"default"`. Isso é o certo
/// para quem só quer *um* agente — e é errado para quem pediu **aquele**.
///
/// Num pedido A2A com `target: "hera"`, cair no `"default"` porque "hera" não
/// existe devolve 200 com a resposta de outro agente, e o chamador não tem
/// como saber. Aqui a ausência é ausência, e quem chama decide o que fazer com
/// ela — no A2A, um 400.
pub fn resolve_exact<'a>(config: &'a AppConfig, agent_id: &str) -> Option<&'a NamedAgentConfig> {
    config.agents.get(agent_id)
}

pub fn resolve<'a>(
    config: &'a AppConfig,
    agent_id: Option<&str>,
    channel_id: Option<&str>,
) -> Option<&'a NamedAgentConfig> {
    // 1. Explicit agent_id
    if let Some(id) = agent_id
        && let Some(agent) = config.agents.get(id)
    {
        return Some(agent);
    }

    // 2. Channel setting: look for `agent_id` in channel config's settings
    if let Some(ch_id) = channel_id
        && let Some(ch) = config.channels.get(ch_id)
        && let Some(serde_json::Value::String(agent_name)) = ch.settings.get("agent_id")
        && let Some(agent) = config.agents.get(agent_name.as_str())
    {
        return Some(agent);
    }

    // 3. "default" named agent
    if let Some(agent) = config.agents.get("default") {
        return Some(agent);
    }

    // 4. No named agent found — caller should fall back to legacy `agent:` config
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::AppConfig;

    #[test]
    fn resolve_explicit_agent_id() {
        let mut config = AppConfig::default();
        config.agents.insert(
            "helper".to_string(),
            NamedAgentConfig {
                provider: Some("claude".to_string()),
                model: None,
                system_prompt: Some("I help.".to_string()),
                max_tokens: None,
                max_context_tokens: None,
                tools: vec![],
            },
        );
        let result = resolve(&config, Some("helper"), None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().system_prompt.as_deref(), Some("I help."));
    }

    #[test]
    fn resolve_falls_back_to_default() {
        let mut config = AppConfig::default();
        config.agents.insert(
            "default".to_string(),
            NamedAgentConfig {
                provider: None,
                model: None,
                system_prompt: Some("Default agent.".to_string()),
                max_tokens: None,
                max_context_tokens: None,
                tools: vec![],
            },
        );
        let result = resolve(&config, None, None);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().system_prompt.as_deref(),
            Some("Default agent.")
        );
    }

    #[test]
    fn resolve_returns_none_when_no_agents() {
        let config = AppConfig::default();
        assert!(resolve(&config, None, None).is_none());
    }

    fn com_hera_e_default() -> AppConfig {
        let mut config = AppConfig::default();
        for (nome, prompt) in [("hera", "Eu sou a Hera."), ("default", "Eu sou a Garra.")] {
            config.agents.insert(
                nome.to_string(),
                NamedAgentConfig {
                    provider: None,
                    model: None,
                    system_prompt: Some(prompt.to_string()),
                    max_tokens: None,
                    max_context_tokens: None,
                    tools: vec![],
                },
            );
        }
        config
    }

    /// Nome que existe resolve para ele mesmo.
    #[test]
    fn resolve_exact_acha_o_agente_pedido() {
        let config = com_hera_e_default();
        let r = resolve_exact(&config, "hera").expect("hera existe");
        assert_eq!(r.system_prompt.as_deref(), Some("Eu sou a Hera."));
    }

    /// **O teste que importa (#965): nome desconhecido nao vira o default.**
    ///
    /// O `resolve` normal devolve o `"default"` nesse caso — e correto para
    /// ele, que existe para escolher quando ninguem escolheu. Quem pediu
    /// **aquele** agente precisa da ausencia, senao recebe a resposta de outro
    /// com 200 e sem sinal nenhum.
    #[test]
    fn resolve_exact_nao_cai_no_default() {
        let config = com_hera_e_default();

        // O contraste e o ponto: os dois na mesma entrada, resultados opostos.
        let com_fallback = resolve(&config, Some("nao-existe"), None);
        assert_eq!(
            com_fallback.and_then(|a| a.system_prompt.as_deref()),
            Some("Eu sou a Garra."),
            "o `resolve` cai no default — e por isso que o A2A nao pode usa-lo"
        );

        assert!(
            resolve_exact(&config, "nao-existe").is_none(),
            "o `resolve_exact` tem de devolver ausencia"
        );
    }

    /// Sem nenhum agente configurado, qualquer nome e desconhecido.
    #[test]
    fn resolve_exact_sem_agentes_e_sempre_ausente() {
        let config = AppConfig::default();
        assert!(resolve_exact(&config, "default").is_none());
        assert!(resolve_exact(&config, "hera").is_none());
    }
}
