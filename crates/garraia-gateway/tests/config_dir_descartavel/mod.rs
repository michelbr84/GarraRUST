//! #2021 e #2027: config dir descartavel para os testes que montam o gateway.
//!
//! `GatewayServer::run()` abre o `admin.db`, resolve o `master.key` e grava o
//! `sessions.db` e o `mcp.json` sob o `<config_dir>`. O `AppState::new` le o
//! `mcp.json` e o `allowlist.json`, provisiona um `mcp.json` quando ele nao
//! existe e liga o cofre ao registry do MCP. Sem `GARRAIA_CONFIG_DIR` esse
//! diretorio e o de quem roda `cargo test` (`~/.config/garraia`), e uma rodada
//! de teste trabalha sobre os dados reais do operador.
//!
//! Um diretorio por processo, nao por teste: os testes de um mesmo arquivo
//! rodam em paralelo com servidores que seguem de pe ate o fim do binario. Um
//! diretorio por teste seria trocado ou apagado debaixo do servidor de outro
//! teste.
//!
//! Cada binario usa so parte deste modulo, dai o `allow(dead_code)`.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, OnceLock};

use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::state::AppState;

/// O config dir descartavel do processo. Vive ate o fim do binario.
///
/// Um `static` nao roda `Drop`, entao o diretorio fica no temp do sistema
/// depois do binario (prefixo `garraia-teste-config-`). E o preco de nao
/// apagar o config dir debaixo de um servidor ainda de pe.
pub fn caminho() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        tempfile::Builder::new()
            .prefix("garraia-teste-config-")
            .tempdir()
            .expect("criar o config dir descartavel")
    })
    .path()
}

/// O `AppState::new` dos testes: o mesmo estado, montado sobre o config dir
/// descartavel em vez do de quem roda `cargo test`.
///
/// O `data_dir`, de onde saem as raizes do MCP `filesystem` e o
/// `data/workspace/`, vai para dentro do mesmo diretorio quando a config nao
/// traz um. Nao mexe no ambiente do processo.
pub fn app_state(
    mut config: AppConfig,
    agents: Arc<AgentRuntime>,
    channels: ChannelRegistry,
) -> AppState {
    if config.data_dir.is_none() {
        config.data_dir = Some(caminho().join("data"));
    }
    AppState::with_config_dir(config, agents, channels, caminho())
}

/// Aponta `GARRAIA_CONFIG_DIR` para o [`caminho`] do processo. Para quem sobe
/// o `GatewayServer`, que so le o config dir do ambiente. Chame antes de
/// construir o servidor.
pub fn isolar() {
    static FEITO: OnceLock<()> = OnceLock::new();
    FEITO.get_or_init(|| {
        let dir = caminho();
        // SAFETY: `set_var` so e sound sem nenhuma outra thread lendo ou
        // escrevendo o environ no mesmo instante, e isso nao da para provar
        // num binario de teste: outro teste em paralelo pode estar num
        // `getenv` (tokio, reqwest) agora. O risco fica pequeno porque a
        // escrita e unica (`get_or_init`) e vem antes de qualquer teste deste
        // arquivo subir o gateway. E o mesmo compromisso do harness em
        // `tests/common`, e nao ha saida sem env: o `GatewayServer` so le o
        // config dir de `GARRAIA_CONFIG_DIR`.
        unsafe {
            std::env::set_var("GARRAIA_CONFIG_DIR", dir);
        }
    });
}
