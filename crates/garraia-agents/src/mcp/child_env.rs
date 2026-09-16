//! #1075 (continuação): o ambiente de um processo filho MCP.
//!
//! Até esta correção `McpManager::connect` montava o `Command` sem
//! `env_clear()`, então todo servidor MCP de transporte stdio recebia o
//! ambiente INTEIRO do gateway — JWT secret, chaves de provider, passphrase
//! do cofre, tudo que o `dotenvy` tivesse carregado. O mapa `env` da config
//! era aplicado POR CIMA dessa herança, nunca no lugar dela.
//!
//! Este módulo é a política, separada do `manager.rs` para que ela caiba
//! numa tela e para que os testes de tabela não dependam do ciclo de vida de
//! conexão. A *aplicação* (`env_clear` + `env`) fica no caller, porque o tipo
//! de `Command` varia por plataforma.

use std::collections::HashMap;

use garraia_common::safety_gate::{env_key_eq, is_mcp_child_env_allowed};

/// Pares (chave, valor) do ambiente do gateway, em `String`.
///
/// `std::env::vars()` entra em panic diante de uma variável que não é UTF-8
/// válido, e uma variável hostil no ambiente do host não pode derrubar o
/// gateway — daí `vars_os()` com descarte silencioso do que não converte.
/// O que não converte também não entra no filho, o que é o lado seguro.
pub(super) fn parent_env_pairs() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
}

/// Monta o ambiente completo de um filho MCP.
///
/// Ordem, e a ordem importa: primeiro a allowlist aplicada ao ambiente do
/// pai, depois o mapa `env` daquele servidor — o que o operador declarou
/// explicitamente sempre vence o que veio por herança.
///
/// Com `inherit = true` a allowlist é ignorada e o ambiente inteiro do
/// gateway passa: é a válvula de escape `inherit_env`, para o servidor
/// legado que dependia do comportamento antigo.
///
/// A deduplicação usa [`env_key_eq`], não `==`: no Windows o bloco de
/// ambiente é case-insensitive, então um `Path` herdado do gateway e um
/// `PATH` declarado no `env` do servidor são a MESMA variável. Comparar com
/// `==` produziria duas entradas e deixaria o sistema decidir qual vence —
/// isto é, o operador declararia `PATH` e poderia continuar recebendo o do
/// gateway.
///
/// Pura de propósito — o ambiente do pai entra por parâmetro — para que a
/// tabela de decisão inteira seja testável sem mexer no ambiente do processo
/// de teste, que é compartilhado entre testes paralelos.
pub(super) fn build_child_env<I>(
    parent: I,
    explicit: &HashMap<String, String>,
    inherit: bool,
) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut env: Vec<(String, String)> = parent
        .into_iter()
        .filter(|(key, _)| inherit || is_mcp_child_env_allowed(key))
        .collect();

    for (key, value) in explicit {
        match env
            .iter_mut()
            .find(|(existing, _)| env_key_eq(existing, key))
        {
            Some(slot) => {
                // Sobrescreve o valor E o nome: no Windows o operador que
                // declara `PATH` deve ver `PATH`, não o `Path` herdado.
                slot.0 = key.clone();
                slot.1 = value.clone();
            }
            None => env.push((key.clone(), value.clone())),
        }
    }

    env
}

#[cfg(test)]
mod tests {
    use super::build_child_env;
    use std::collections::HashMap;

    fn pai() -> Vec<(String, String)> {
        [
            ("PATH", "/usr/bin"),
            ("HOME", "/home/garra"),
            ("GARRAIA_JWT_SECRET", "nao-pode-vazar"),
            ("ANTHROPIC_API_KEY", "sk-ant-nao-pode-vazar"),
            ("GarraIA_VAULT_PASSPHRASE", "nao-pode-vazar"),
            ("DATABASE_URL", "postgres://user:senha@host/db"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn valor<'a>(env: &'a [(String, String)], chave: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| k == chave)
            .map(|(_, v)| v.as_str())
    }

    /// O defeito do #1075 (continuação): um servidor MCP de terceiro recebia
    /// JWT secret, chave de provider e passphrase do cofre só por ser
    /// spawnado. Falha antes da correção.
    #[test]
    fn filho_mcp_nao_recebe_segredo_do_gateway_por_padrao() {
        let env = build_child_env(pai(), &HashMap::new(), false);

        assert_eq!(valor(&env, "PATH"), Some("/usr/bin"));
        assert_eq!(valor(&env, "HOME"), Some("/home/garra"));
        for segredo in [
            "GARRAIA_JWT_SECRET",
            "ANTHROPIC_API_KEY",
            "GarraIA_VAULT_PASSPHRASE",
            "DATABASE_URL",
        ] {
            assert!(
                valor(&env, segredo).is_none(),
                "'{segredo}' chegou ao filho MCP"
            );
        }
    }

    /// O mapa `env` do servidor é onde o operador coloca de propósito o que
    /// aquele servidor precisa — e ele passa mesmo não estando na allowlist.
    #[test]
    fn mapa_explicito_do_servidor_chega_ao_filho() {
        let mut explicito = HashMap::new();
        explicito.insert("GITHUB_TOKEN".to_string(), "ghp_do_operador".to_string());

        let env = build_child_env(pai(), &explicito, false);

        assert_eq!(valor(&env, "GITHUB_TOKEN"), Some("ghp_do_operador"));
        assert!(valor(&env, "GARRAIA_JWT_SECRET").is_none());
    }

    /// Ordem: allowlist primeiro, mapa explícito por cima. Sem duplicata.
    #[test]
    fn mapa_explicito_sobrescreve_a_heranca() {
        let mut explicito = HashMap::new();
        explicito.insert("PATH".to_string(), "/opt/mcp/bin".to_string());

        let env = build_child_env(pai(), &explicito, false);

        assert_eq!(valor(&env, "PATH"), Some("/opt/mcp/bin"));
        assert_eq!(
            env.iter().filter(|(k, _)| k == "PATH").count(),
            1,
            "PATH não pode aparecer duas vezes"
        );
    }

    /// No Windows `Path` e `PATH` são a MESMA variável de ambiente. Com
    /// dedup por `==` o filho recebia as duas entradas e quem vencia era
    /// decisão do sistema — ou seja, o operador declarava `PATH` no `env` do
    /// servidor e podia continuar rodando com o `Path` herdado do gateway.
    #[cfg(windows)]
    #[test]
    fn dedup_no_windows_e_case_insensitive() {
        let pai = vec![("Path".to_string(), "C:\\herdado".to_string())];
        let mut explicito = HashMap::new();
        explicito.insert("PATH".to_string(), "C:\\do-operador".to_string());

        let env = build_child_env(pai, &explicito, false);

        assert_eq!(env.len(), 1, "Path e PATH são a mesma variável no Windows");
        assert_eq!(env[0].0, "PATH", "o nome declarado pelo operador vence");
        assert_eq!(env[0].1, "C:\\do-operador");
    }

    /// No Unix a comparação é exata: `Path` e `PATH` são variáveis distintas
    /// e ambas devem sobreviver — colapsá-las descartaria uma delas.
    #[cfg(not(windows))]
    #[test]
    fn dedup_no_unix_e_exato() {
        let mut explicito = HashMap::new();
        explicito.insert("PATH".to_string(), "/opt/mcp/bin".to_string());

        // `Path` não está na allowlist Unix, então nem chega aqui vindo do
        // pai; o que se testa é que o dedup não o confunde com `PATH`.
        let env = build_child_env(
            vec![("PATH".to_string(), "/usr/bin".to_string())],
            &explicito,
            false,
        );

        assert_eq!(env, vec![("PATH".to_string(), "/opt/mcp/bin".to_string())]);
    }

    /// A válvula de escape faz exatamente o que promete — e por isso existe
    /// o `warn!` no connect e o default `false`.
    #[test]
    fn inherit_env_devolve_o_ambiente_inteiro() {
        let env = build_child_env(pai(), &HashMap::new(), true);

        assert_eq!(valor(&env, "GARRAIA_JWT_SECRET"), Some("nao-pode-vazar"));
        assert_eq!(env.len(), pai().len());
    }

    /// Mesmo com `inherit_env`, o mapa explícito continua vencendo.
    #[test]
    fn inherit_env_ainda_deixa_o_mapa_explicito_vencer() {
        let mut explicito = HashMap::new();
        explicito.insert("PATH".to_string(), "/opt/mcp/bin".to_string());

        let env = build_child_env(pai(), &explicito, true);

        assert_eq!(valor(&env, "PATH"), Some("/opt/mcp/bin"));
        assert_eq!(
            valor(&env, "ANTHROPIC_API_KEY"),
            Some("sk-ant-nao-pode-vazar")
        );
    }

    /// Ambiente vazio não inventa variável nenhuma.
    #[test]
    fn ambiente_vazio_produz_filho_vazio() {
        assert!(build_child_env(Vec::new(), &HashMap::new(), false).is_empty());
        assert!(build_child_env(Vec::new(), &HashMap::new(), true).is_empty());
    }
}
