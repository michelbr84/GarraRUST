//! #2027: `AppState::with_config_dir` resolve as referencias `vault:` do
//! `mcp.json` pelo cofre do config dir que recebeu, e nao pelo cofre do config
//! dir padrao.
//!
//! O `mcp.json` ja vinha do diretorio passado, mas o cofre vinha sempre de
//! `<config_dir padrao>/credentials/vault.json`. Um teste que montasse o estado
//! sobre um diretorio descartavel, com a passphrase no ambiente, decifrava os
//! segredos do cofre real de quem roda `cargo test`.
//!
//! Um teste so neste binario, porque ele escreve no ambiente do processo.

use std::sync::Arc;

use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::state::AppState;
use garraia_security::CredentialVault;

const PASSPHRASE: &str = "passphrase-de-teste-2027";
const CHAVE: &str = "mcp.cofre.TOKEN";

/// Um config dir com um cofre que guarda `valor` em [`CHAVE`] e um `mcp.json`
/// cujo servidor `cofre` le essa chave por referencia.
fn config_dir_com_cofre(valor: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let cofre = dir.path().join("credentials").join("vault.json");
    std::fs::create_dir_all(cofre.parent().expect("credentials/")).expect("criar credentials/");
    let mut vault = CredentialVault::create(&cofre, PASSPHRASE).expect("criar o cofre");
    vault.set(CHAVE, valor);
    vault.save().expect("gravar o cofre");

    let mcp = serde_json::json!({
        "mcpServers": {
            "cofre": { "command": "true", "env": { "TOKEN": format!("vault:{CHAVE}") } }
        }
    });
    std::fs::write(
        dir.path().join("mcp.json"),
        serde_json::to_vec(&mcp).expect("serializar o mcp.json"),
    )
    .expect("gravar o mcp.json");
    dir
}

#[tokio::test]
async fn as_referencias_vault_saem_do_cofre_do_config_dir_passado() {
    let passado = config_dir_com_cofre("segredo-do-dir-passado");
    let padrao = config_dir_com_cofre("segredo-do-config-dir-padrao");

    // SAFETY: `set_var` so e sound sem outra thread lendo ou escrevendo o
    // environ ao mesmo tempo. Este e o unico teste do binario e o runtime do
    // `#[tokio::test]` e de uma thread so, entao ninguem mais toca o environ.
    unsafe {
        std::env::set_var("GARRAIA_VAULT_PASSPHRASE", PASSPHRASE);
        std::env::set_var("GARRAIA_CONFIG_DIR", padrao.path());
    }

    let mut config = AppConfig::default();
    config.data_dir = Some(passado.path().join("data"));
    let state = AppState::with_config_dir(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
        passado.path(),
    );

    let snapshot = state.mcp_registry.config_snapshot().await;
    let token = snapshot
        .mcp_servers
        .get("cofre")
        .and_then(|servidor| servidor.env.get("TOKEN"))
        .cloned();
    assert_eq!(
        token.as_deref(),
        Some("segredo-do-dir-passado"),
        "o `vault:` do mcp.json tem de ser resolvido pelo cofre do config dir passado ao \
         with_config_dir, nao pelo do config dir padrao (GARRAIA_CONFIG_DIR)"
    );
}
