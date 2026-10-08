//! `garraia doctor mcp` — health-check real dos servidores MCP (#1595).
//!
//! O `garraia doctor` padrão confere instalação; o `doctor mcp` confere
//! **subida**: para cada servidor stdio configurado ele chama o MESMO
//! [`garraia_gateway::bootstrap::build_mcp_tools`] do boot do gateway — o
//! mesmo merge de `mcp.json`/`config.yml`, a mesma resolução de `vault:`, o
//! mesmo timeout, a mesma recuperação de cache npx (#1346) — conta as tools
//! que o handshake devolveu, e desligama os filhos antes de sair. Uma falha
//! de subida (cache corrompido, binário ausente, handshake morto) que no
//! boot vira warning de log aqui vira **exit 2**, útil em CI/scripts.
//!
//! Fora do spawn (caro, toca rede num cache npx frio) o `doctor` padrão
//! ganha um resumo CONFIG do plano MCP — quantos servidores declarados e se
//! a entrada `filesystem` está com a versão fixada (#1346) — puro, sem
//! filho, sem rede.
//!
//! Exit codes (mesma semântica do `doctor` raiz):
//!   - `0`  — todos os servidores stdio declarados subiram com tools.
//!   - `2`  — algum servidor não subiu (ou `--strict` com versão não fixada).
//!   - `65` — `EX_DATAERR`: o arquivo de config existe mas não parseia.

use std::collections::HashMap;

use anyhow::Result;
use garraia_config::{AppConfig, ConfigLoader, McpServerConfig};
use garraia_gateway::bootstrap::build_mcp_tools;
use garraia_gateway::mcp::persistence::{McpPersistenceService, VersaoDoFilesystem};
use serde::Serialize;

/// Ordem determinística: `config.mcp` e o `mcp.json` são mapas, e a saída
/// (humana e JSON) tem de ser afirmável em teste.
fn servidores_ordenados(
    mapa: &HashMap<String, McpServerConfig>,
) -> Vec<(String, &McpServerConfig)> {
    let mut v: Vec<(String, &McpServerConfig)> = mapa.iter().map(|(k, v)| (k.clone(), v)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// O que o spawn de um servidor devolveu.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StatusServidor {
    /// Handshake MCP completo e `n` tools registradas.
    Ok { tools: usize },
    /// O spawn/handshake falhou — o gateway também não conseguiria subi-lo.
    NaoSubiu { erro: String },
    /// `enabled: false` na config — decidido pelo operador, não é falha.
    Desabilitado,
    /// Entrada HTTP: não passa por spawn stdio; o cliente mcp-http (feature)
    /// é quem fala com ela, e o health-check de rede dela é outro assunto.
    HttpNaoVerificado,
    /// stdio sem `command` — o loader até deixa entrar (#1274), mas o boot
    /// recusa antes do spawn; aqui o mesmo recusa, explícito.
    SemComando,
}

/// Um servidor do relatório: nome + o que o check observou.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServidorReport {
    pub name: String,
    pub transport: String,
    #[serde(flatten)]
    pub status: StatusServidor,
}

/// Classifica um servidor declarado a partir do que o `build_mcp_tools`
/// observou — pura, testável sem spawn.
///
/// `conectados` vem do `manager.list_servers()` (nome, tools, vivo);
/// `falhas` vem do terceiro retorno do `build_mcp_tools`. Um servidor que
/// está nas duas (falhou no boot e ficou pendente) conta como falha.
pub fn classifica_servidor(
    nome: &str,
    cfg: &McpServerConfig,
    conectados: &[(String, usize, bool)],
    falhas: &[(String, String)],
) -> StatusServidor {
    if cfg.enabled == Some(false) {
        return StatusServidor::Desabilitado;
    }
    if cfg.transport != "stdio" {
        return StatusServidor::HttpNaoVerificado;
    }
    if cfg.command.trim().is_empty() {
        return StatusServidor::SemComando;
    }
    if let Some((_, erro)) = falhas.iter().find(|(n, _)| n == nome) {
        return StatusServidor::NaoSubiu { erro: erro.clone() };
    }
    match conectados.iter().find(|(n, _, _)| n == nome) {
        Some((_, tools, true)) => StatusServidor::Ok { tools: *tools },
        // Conhecido mas morto: o transport caiu depois do handshake.
        Some((_, _, false)) => StatusServidor::NaoSubiu {
            erro: "conectou no boot mas o transport morreu antes do check".to_string(),
        },
        None => StatusServidor::NaoSubiu {
            erro: "não apareceu nem como conectado nem como falha do boot".to_string(),
        },
    }
}

/// Exit code do check de spawn: falha de subida é sempre vermelha; versão
/// não fixada do `filesystem` só reprova sob `--strict` (a mesma semântica
/// dos warnings do `doctor` raiz).
pub fn exit_code(servidores: &[ServidorReport], versao_warn: bool, strict: bool) -> i32 {
    let falhou = servidores
        .iter()
        .any(|s| matches!(s.status, StatusServidor::NaoSubiu { .. }));
    if falhou {
        return 2;
    }
    if versao_warn && strict {
        return 2;
    }
    0
}

/// Uma linha humana por servidor — pura, para o teste não depender de spawn.
pub fn linha_humana(s: &ServidorReport) -> String {
    match &s.status {
        StatusServidor::Ok { tools } => format!("• {} — ok ({tools} tools)", s.name),
        StatusServidor::NaoSubiu { erro } => format!("• {} — NÃO SUBIU: {erro}", s.name),
        StatusServidor::Desabilitado => format!("• {} — desabilitado", s.name),
        StatusServidor::HttpNaoVerificado => format!(
            "• {} — http (subida não verificada por este check; cliente mcp-http)",
            s.name
        ),
        StatusServidor::SemComando => format!("• {} — stdio sem `command` (o boot recusa)", s.name),
    }
}

/// Classificação da versão do `filesystem` para o relatório, pura.
///
/// `Fixada` é o estado testado (#1346); `SemVersao` é o warning (cada cache
/// frio do npx baixa o build mais novo do registry); `ForaDoNpx` é escolha
/// do operador (binário local) e `Ausente` é o default de instalação limpa.
pub fn versao_filesystem_report(v: &VersaoDoFilesystem) -> (String, bool) {
    match v {
        VersaoDoFilesystem::Ausente => (
            "nenhuma entrada `filesystem` (o gateway provisiona no primeiro boot)".to_string(),
            false,
        ),
        VersaoDoFilesystem::ForaDoNpx => (
            "filesystem não roda via npx — versão é do operador".to_string(),
            false,
        ),
        VersaoDoFilesystem::Fixada(versao) => (
            format!("@modelcontextprotocol/server-filesystem@{versao} (fixada)"),
            false,
        ),
        VersaoDoFilesystem::SemVersao { .. } => (
            "filesystem sem versão fixada — cada cache frio do npx baixa o build mais novo"
                .to_string(),
            true,
        ),
    }
}

/// Ponto de entrada do `garraia doctor mcp`. Devolve o exit code.
///
/// O check de spawn é opt-in por área justamente porque toca processo e,
/// num cache npx frio, rede — o `doctor` raiz continua rápido e barato.
pub fn run(json: bool, strict: bool) -> Result<i32> {
    let loader = ConfigLoader::new()?;
    let config = match loader.load() {
        Ok(c) => c,
        Err(e) => {
            let erro = crate::config_cmd::truncate_error(format!("{e}"));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "exit_code": 65,
                        "error": erro,
                    }))?
                );
            } else {
                println!("🩺 GarraIA doctor mcp — config inválida (exit 65)");
                println!("        {erro}");
            }
            return Ok(65);
        }
    };

    let mcp_json = McpPersistenceService::new(loader.config_dir().join("mcp.json"))
        .load()
        .unwrap_or_default();
    let versao =
        garraia_gateway::mcp::persistence::versao_do_filesystem_efetivo(&config.mcp, &mcp_json);
    let (versao_texto, versao_warn) = versao_filesystem_report(&versao);

    // Nenhum servidor declarado em NENHUMA das duas fontes: instalação limpa
    // antes do primeiro boot do gateway — não há o que spawnar, e isso não é
    // falha. O mesmo merge do boot decide "nenhum".
    if config.mcp.is_empty() && mcp_json.mcp_servers.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "exit_code": 0,
                    "report": {
                        "servers": [],
                        "filesystem_version": versao_texto,
                        "note": "nenhum servidor MCP configurado — o gateway provisiona o `filesystem` no primeiro boot",
                    },
                }))?
            );
        } else {
            println!("🩺 GarraIA doctor mcp v{}", env!("CARGO_PKG_VERSION"));
            println!("        nenhum servidor MCP configurado — o gateway provisiona o");
            println!("        `filesystem` no primeiro boot (`garraia start`).");
        }
        return Ok(0);
    }

    // O spawn é o MESMO código do boot: mesma função, mesma config, mesmo
    // merge. Se o doctor subir, o gateway sobe; se não, a falha reportada é
    // a falha real (vault:, cache npx, binário ausente, handshake).
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (manager, tools, falhas) = runtime.block_on(build_mcp_tools(&config));
    let conectados = runtime.block_on(manager.list_servers());

    let declarados = merged_declarados(&loader, &config);
    let servidores: Vec<ServidorReport> = servidores_ordenados(&declarados)
        .into_iter()
        .map(|(name, cfg)| ServidorReport {
            transport: cfg.transport.clone(),
            status: classifica_servidor(&name, cfg, &conectados, &falhas),
            name,
        })
        .collect();

    // Filhos MCP são processos nossos: matar antes de sair para o doctor não
    // deixar servidor zumbi no chão do CI.
    runtime.block_on(manager.disconnect_all());
    drop(tools);
    drop(runtime);

    let code = exit_code(&servidores, versao_warn, strict);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": code == 0,
                "exit_code": code,
                "report": {
                    "servers": servidores,
                    "filesystem_version": versao_texto,
                    "filesystem_version_warning": versao_warn,
                },
            }))?
        );
    } else {
        println!("🩺 GarraIA doctor mcp v{}", env!("CARGO_PKG_VERSION"));
        println!("        config: {}", loader.config_dir().display());
        println!();
        for s in &servidores {
            println!("        {}", linha_humana(s));
        }
        println!();
        println!("        filesystem: {versao_texto}");
        if versao_warn && code == 0 {
            println!(
                "        aviso: sob `--strict` a versão não fixada reprova o check \
                 (troque os args do `filesystem` pela versão testada e reinicie)."
            );
        }
    }

    Ok(code)
}

/// O merge DECLARADO (config.yml + mcp.json) na mesma precedência do boot —
/// `config.yml` vence — sem spawnar nada. Usado para os servidores que o
/// `build_mcp_tools` nem tentou (desabilitados) e para o resumo.
pub fn merged_declarados(
    loader: &ConfigLoader,
    config: &AppConfig,
) -> HashMap<String, McpServerConfig> {
    let mut merged = loader.load_mcp_json();
    for (name, cfg) in &config.mcp {
        merged.insert(name.clone(), cfg.clone());
    }
    merged
}

/// Resumo CONFIG do plano MCP para o `doctor` raiz — puro, sem spawn: quantos
/// servidores declarados (nas duas fontes, precedência do boot) e o status
/// da versão do `filesystem`. O health-check de subida é `doctor mcp`.
pub fn resumo_config(loader: &ConfigLoader, config: &AppConfig) -> ResumoMcp {
    let mcp_json = McpPersistenceService::new(loader.config_dir().join("mcp.json"))
        .load()
        .unwrap_or_default();
    let merged = merged_declarados(loader, config);
    let (versao_texto, versao_warn) = versao_filesystem_report(
        &garraia_gateway::mcp::persistence::versao_do_filesystem_efetivo(&config.mcp, &mcp_json),
    );
    ResumoMcp {
        servidores: merged.len(),
        filesystem_version: versao_texto,
        filesystem_version_warning: versao_warn,
    }
}

/// O que o `doctor` raiz imprime em `[5/5] MCP` — sem spawn, sem rede.
#[derive(Debug, Clone, Serialize)]
pub struct ResumoMcp {
    pub servidores: usize,
    pub filesystem_version: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub filesystem_version_warning: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_stdio() -> McpServerConfig {
        McpServerConfig {
            command: "python3".into(),
            args: vec!["server.py".into()],
            env: HashMap::new(),
            transport: "stdio".into(),
            url: None,
            enabled: None,
            timeout: None,
            allowed_tools: vec![],
            memory_limit_mb: None,
            max_restarts: None,
            restart_delay_secs: None,
            inherit_env: false,
        }
    }

    #[test]
    fn servidor_conectado_e_ok_com_a_contagem_de_tools() {
        let conectados = vec![("fs".to_string(), 12, true)];
        let s = classifica_servidor("fs", &cfg_stdio(), &conectados, &[]);
        assert_eq!(s, StatusServidor::Ok { tools: 12 });
    }

    #[test]
    fn falha_do_boot_e_nao_subiu_com_o_erro_original() {
        let falhas = vec![("fs".to_string(), "npx cache corrupt".to_string())];
        let s = classifica_servidor("fs", &cfg_stdio(), &[], &falhas);
        assert_eq!(
            s,
            StatusServidor::NaoSubiu {
                erro: "npx cache corrupt".to_string()
            }
        );
    }

    #[test]
    fn desabilitado_e_decisao_do_operador_nao_falha() {
        let mut cfg = cfg_stdio();
        cfg.enabled = Some(false);
        assert_eq!(
            classifica_servidor("fs", &cfg, &[], &[]),
            StatusServidor::Desabilitado
        );
    }

    #[test]
    fn http_nao_passa_por_spawn_e_stdio_sem_comando_e_recusa_explicita() {
        let mut http = cfg_stdio();
        http.transport = "http".into();
        http.command = String::new();
        assert_eq!(
            classifica_servidor("x", &http, &[], &[]),
            StatusServidor::HttpNaoVerificado
        );

        let mut sem_cmd = cfg_stdio();
        sem_cmd.command = "  ".into();
        assert_eq!(
            classifica_servidor("x", &sem_cmd, &[], &[]),
            StatusServidor::SemComando
        );
    }

    #[test]
    fn conectado_mas_morto_e_falha_nao_sucesso_silencioso() {
        let conectados = vec![("fs".to_string(), 3, false)];
        let s = classifica_servidor("fs", &cfg_stdio(), &conectados, &[]);
        assert!(matches!(s, StatusServidor::NaoSubiu { .. }), "{s:?}");
    }

    #[test]
    fn exit_code_falha_de_subida_e_sempre_vermelho() {
        let ok = ServidorReport {
            name: "a".into(),
            transport: "stdio".into(),
            status: StatusServidor::Ok { tools: 1 },
        };
        let morto = ServidorReport {
            name: "b".into(),
            transport: "stdio".into(),
            status: StatusServidor::NaoSubiu { erro: "x".into() },
        };
        assert_eq!(exit_code(&[ok.clone()], false, false), 0);
        assert_eq!(exit_code(&[ok, morto], false, false), 2);
    }

    #[test]
    fn versao_nao_fixada_so_reprova_sob_strict() {
        let ok = ServidorReport {
            name: "a".into(),
            transport: "stdio".into(),
            status: StatusServidor::Ok { tools: 1 },
        };
        assert_eq!(exit_code(&[ok.clone()], true, false), 0);
        assert_eq!(exit_code(&[ok], true, true), 2);
    }

    #[test]
    fn versao_filesystem_fixada_nao_warna_e_sem_versao_warna() {
        let (t, warn) = versao_filesystem_report(&VersaoDoFilesystem::Fixada("1.2.3".into()));
        assert!(!warn, "{t}");
        assert!(t.contains("@1.2.3"), "{t}");

        let (t, warn) = versao_filesystem_report(&VersaoDoFilesystem::SemVersao {
            args_sugeridos: vec![],
        });
        assert!(warn, "{t}");

        let (_, warn) = versao_filesystem_report(&VersaoDoFilesystem::Ausente);
        assert!(!warn);
    }

    #[test]
    fn linha_humana_nomeia_cada_estado() {
        let casos = [
            (
                ServidorReport {
                    name: "fs".into(),
                    transport: "stdio".into(),
                    status: StatusServidor::Ok { tools: 12 },
                },
                "ok (12 tools)",
            ),
            (
                ServidorReport {
                    name: "fs".into(),
                    transport: "stdio".into(),
                    status: StatusServidor::NaoSubiu {
                        erro: "morto".into(),
                    },
                },
                "NÃO SUBIU: morto",
            ),
            (
                ServidorReport {
                    name: "fs".into(),
                    transport: "stdio".into(),
                    status: StatusServidor::Desabilitado,
                },
                "desabilitado",
            ),
        ];
        for (s, esperado) in casos {
            assert!(linha_humana(&s).contains(esperado), "{s:?}");
        }
    }
}
