//! #1712 — estado **declarado** (nunca verificado) das integrações opcionais.
//!
//! O `cli_inspect` responde sobre o processo da CLI. Gateway e servidores MCP
//! são processos SEPARADOS: a CLI não fala com eles, não os sobe e não tem
//! canal de saúde. Até esta fatia o relatório devolvia `unknown` para os dois —
//! honesto, mas indistingue **"não existe nada configurado"** de **"existe
//! configuração, mas a saúde não é observável daqui"**, que é justamente a
//! diferença que os critérios da issue pedem.
//!
//! A única fonte autorizada dentro do processo é a configuração que ESTE
//! processo carregou: `AppConfig` (`config.yml`) e o `mcp.json` ao lado dele —
//! a mesma visão mesclada que o boot do gateway conecta
//! (`ConfigLoader::merged_mcp_config`). Ela é *declarada*, nunca *verificada*:
//! o relatório diz isso com o campo `live_verification`, para o modelo não
//! transformar configuração em afirmação de saúde.
//!
//! Nenhum valor de segredo sai daqui: [`garraia_config::McpServerConfig`]
//! carrega `env`, `args` e `url`, que podem conter credencial. Só nome,
//! transporte e habilitado são projetados; o nome ainda passa por sanitização
//! no consumidor ([`crate::cli_inspect_tool`]).

use garraia_config::{AppConfig, ConfigLoader};

/// Quantos servidores MCP declarados o relatório do `cli_inspect` lista.
///
/// O limite existe pelo mesmo motivo da paginação de ferramentas: o resultado
/// vai para o LLM e um `mcp.json` grande não pode inflar o turno.
pub const MAX_SERVIDORES: usize = 16;

/// Transporte declarado de um servidor MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransporteDeclarado {
    /// Entrada com `command` (o boot sobe um filho).
    Stdio,
    /// Entrada com `url` (o cliente HTTP fala com ela).
    Http,
}

impl TransporteDeclarado {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

/// Um servidor MCP **declarado** na configuração, sem nenhum campo sensível.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServidorDeclarado {
    /// Nome da chave no mapa de configuração — não é credencial, mas passa por
    /// [`crate::cli_inspect_tool::safe_tool_name`] antes de ir ao modelo.
    pub nome: String,
    pub transporte: TransporteDeclarado,
    /// Campo `enabled` ausente vale `true` — o mesmo default do loader e do
    /// boot; declarar `false` aqui seria inventar estado.
    pub habilitado: bool,
}

/// Configuração efetiva do processo, projetada para o relatório.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EstadoDeclarado {
    /// Listen declarado do gateway (`gateway.host`:`gateway.port`).
    pub gateway_listen: String,
    pub mcp_servers: Vec<ServidorDeclarado>,
    /// `true` quando a visão mesclada incluiu o `mcp.json`; `false` quando o
    /// loader não pôde ser construído e o estado é só o `config.yml`.
    pub mcp_json_incluido: bool,
}

impl EstadoDeclarado {
    /// Quantos servidores declarados estão habilitados.
    pub fn habilitados(&self) -> usize {
        self.mcp_servers.iter().filter(|s| s.habilitado).count()
    }
}

fn listen_declarado(config: &AppConfig) -> String {
    format!("{}:{}", config.gateway.host, config.gateway.port)
}

fn servidores_de_config(config: &AppConfig) -> Vec<ServidorDeclarado> {
    config
        .mcp
        .iter()
        .map(|(nome, cfg)| ServidorDeclarado {
            nome: nome.clone(),
            transporte: transporte_de(cfg),
            habilitado: cfg.enabled.unwrap_or(true),
        })
        .collect()
}

fn transporte_de(cfg: &garraia_config::McpServerConfig) -> TransporteDeclarado {
    // #1274: uma entrada HTTP não tem `command` e é legítima; `url` não-vazia é
    // o que distingue as duas formas no próprio loader.
    match cfg.url.as_deref() {
        Some(url) if !url.trim().is_empty() => TransporteDeclarado::Http,
        _ => TransporteDeclarado::Stdio,
    }
}

fn ordenar(mut servidores: Vec<ServidorDeclarado>) -> Vec<ServidorDeclarado> {
    // `config.yml` e `mcp.json` são mapas: a saída tem de ser afirmável.
    servidores.sort_by(|a, b| a.nome.cmp(&b.nome));
    servidores
}

/// Coleta a partir de um loader EXPLÍCITO — a forma testável do coletor.
pub fn coletar(config: &AppConfig, loader: &ConfigLoader) -> EstadoDeclarado {
    // A visão mesclada (config.yml + mcp.json) é a mesma que o boot do gateway
    // conecta; o `config.yml` vence no conflito, dentro do próprio loader.
    let mesclado: Vec<ServidorDeclarado> = loader
        .merged_mcp_config(config)
        .iter()
        .map(|(nome, cfg)| ServidorDeclarado {
            nome: nome.clone(),
            transporte: transporte_de(cfg),
            habilitado: cfg.enabled.unwrap_or(true),
        })
        .collect();
    EstadoDeclarado {
        gateway_listen: listen_declarado(config),
        mcp_servers: ordenar(mesclado),
        mcp_json_incluido: true,
    }
}

/// Coleta a partir da configuração que o processo já carregou.
///
/// Não faz rede, não sobe nada, não escreve: lê o `mcp.json` ao lado do
/// `config.yml` (leitura somente) e desiste para a visão `config.yml`-only
/// quando o loader não existe — declarando isso em `mcp_json_incluido`.
pub fn coletar_do_processo(config: &AppConfig) -> EstadoDeclarado {
    match ConfigLoader::new() {
        Ok(loader) => coletar(config, &loader),
        Err(_) => EstadoDeclarado {
            gateway_listen: listen_declarado(config),
            mcp_servers: ordenar(servidores_de_config(config)),
            mcp_json_incluido: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn config_com_mcp(entradas: &[(&str, garraia_config::McpServerConfig)]) -> AppConfig {
        let mut config = AppConfig::default();
        let mut mcp = HashMap::new();
        for (nome, cfg) in entradas {
            mcp.insert((*nome).to_string(), cfg.clone());
        }
        config.mcp = mcp;
        config
    }

    fn servidor_stdio(habilitado: Option<bool>) -> garraia_config::McpServerConfig {
        // `McpServerConfig` não implementa `Default` (os campos de tuning têm
        // default de SERDE, não de tipo) — a entrada de teste nasce da mesma
        // forma que o operador escreve no `mcp.json`.
        serde_json::from_value(serde_json::json!({
            "command": "npx",
            "enabled": habilitado,
        }))
        .expect("entrada stdio valida")
    }

    #[test]
    fn sem_configuracao_nenhuma_declara_lista_vazia_e_listen_padrao() {
        let dir = tempfile::tempdir().unwrap();
        let estado = coletar(&AppConfig::default(), &ConfigLoader::with_dir(dir.path()));

        assert!(estado.mcp_servers.is_empty());
        assert_eq!(estado.habilitados(), 0);
        assert!(estado.mcp_json_incluido);
        assert_eq!(estado.gateway_listen, "127.0.0.1:3888");
    }

    #[test]
    fn mcp_json_entra_na_visao_com_transporte_e_enabled_sem_valores() {
        let dir = tempfile::tempdir().unwrap();
        // Sentinela de segredo no `env` e um `url` com token: nada disso pode
        // virar campo do estado — a projeção só tem nome/transporte/enabled.
        let sentinela = ["sk", "-live-", "SENTINELA"].concat();
        fs::write(
            dir.path().join("mcp.json"),
            format!(
                r#"{{"mcpServers": {{
                    "filesystem": {{"command": "npx", "args": ["-y", "server-fs"], "env": {{"API_KEY": "{sentinela}"}}}},
                    "remoto": {{"url": "https://mcp.example/sse?token={sentinela}", "enabled": false}}
                }}}}"#
            ),
        )
        .unwrap();

        let estado = coletar(&AppConfig::default(), &ConfigLoader::with_dir(dir.path()));

        assert_eq!(estado.mcp_servers.len(), 2);
        assert_eq!(
            estado.mcp_servers[0],
            ServidorDeclarado {
                nome: "filesystem".to_string(),
                transporte: TransporteDeclarado::Stdio,
                habilitado: true,
            }
        );
        assert_eq!(
            estado.mcp_servers[1],
            ServidorDeclarado {
                nome: "remoto".to_string(),
                transporte: TransporteDeclarado::Http,
                habilitado: false,
            }
        );
        assert_eq!(estado.habilitados(), 1);
        let serializado = format!("{estado:?}");
        assert!(!serializado.contains(&sentinela), "{serializado}");
    }

    #[test]
    fn entrada_do_config_yml_vence_o_mcp_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("mcp.json"),
            r#"{"mcpServers": {"comum": {"command": "npx"}}}"#,
        )
        .unwrap();
        let config = config_com_mcp(&[("comum", servidor_stdio(Some(false)))]);

        let estado = coletar(&config, &ConfigLoader::with_dir(dir.path()));

        assert_eq!(estado.mcp_servers.len(), 1);
        assert!(!estado.mcp_servers[0].habilitado);
    }

    #[test]
    fn servidores_declarados_saem_em_ordem_estavel() {
        let config = config_com_mcp(&[
            ("zeta", servidor_stdio(None)),
            ("alfa", servidor_stdio(None)),
            ("meio", servidor_stdio(None)),
        ]);
        let dir = tempfile::tempdir().unwrap();

        let estado = coletar(&config, &ConfigLoader::with_dir(dir.path()));

        let nomes: Vec<&str> = estado.mcp_servers.iter().map(|s| s.nome.as_str()).collect();
        assert_eq!(nomes, ["alfa", "meio", "zeta"]);
    }
}
