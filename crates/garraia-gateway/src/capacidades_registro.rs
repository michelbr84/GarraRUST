//! O registro de capacidades do runtime (#1381, #1387, #1416): **uma**
//! funcao pura que, dado o que o gateway sabe neste turno, diz para cada
//! capacidade em que estado ela esta — e por que — com o motivo legivel por
//! maquina e por humano.
//!
//! Antes, cada superficie montava a sua lista: o `garra_status` listava as
//! ferramentas do turno, o `/api/diagnostics` tinha `tools.bash` e
//! `mcp.servers`, e o modelo concluia "nao tenho MCP" quando o MCP existia e
//! estava escondido pela politica. Aqui as fontes que ja existem —
//! inventario do `AgentRuntime`, portao do turno (nome e classe, #1385),
//! disponibilidade da ferramenta (#1425), estado dos servidores MCP e a
//! exposicao do `bash` — entram numa visao so, e o `garra_status`, o
//! `/api/diagnostics` e o console leem **dela**. Nenhuma outra lista de
//! capacidades e montada a mao.
//!
//! Estados, do mais ao menos util:
//!
//! | estado | significa | o modelo deve dizer |
//! |---|---|---|
//! | `visible` | esta na lista do turno | "posso" |
//! | `denied` | existe e opera, a politica desta conversa nega | "existe, nao esta liberada aqui" |
//! | `unavailable` | registrada, falta contexto/integracao (sem raiz, canal desligado) | a remediacao |
//! | `unhealthy` | servidor MCP conhecido mas caido | "existe, esta fora do ar" |
//! | `not_configured` | o gateway sabe do recurso, nao esta configurado (`bash` sem sandbox) | "nao esta configurado" |
//!
//! Sem I/O, sem lock: quem chama ja colheu tudo. Nada aqui carrega segredo
//! nem caminho do host — os motivos vem de constantes ou do
//! `Disponibilidade` da propria ferramenta, que tem o mesmo contrato.

use garraia_agents::McpServerState;
use garraia_agents::mcp::McpServerStatus;
use garraia_agents::runtime::ToolInventoryEntry;
use garraia_agents::tools::Disponibilidade;
use serde::Serialize;

/// O estado de uma capacidade neste turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    Visible,
    Denied,
    Unavailable,
    Unhealthy,
    NotConfigured,
}

impl Estado {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
            Self::Unhealthy => "unhealthy",
            Self::NotConfigured => "not_configured",
        }
    }
}

/// Uma linha do registro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capacidade {
    pub name: String,
    /// `native` | `mcp` | `channel` (acao de canal) | `runtime` (sintetica:
    /// `bash` desligado, servidor MCP caido sem ferramentas).
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// As classes (#1385), quando a ferramenta existe no inventario.
    pub classes: Vec<&'static str>,
    pub state: Estado,
    /// Legivel por maquina: `ok` | `policy` | `not_configured` |
    /// `channel_offline` | `no_roots` | `disconnected` | `retrying` |
    /// `failed` | ...
    pub reason_code: &'static str,
    /// Legivel por humano, sem segredo nem caminho.
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// O que o gateway sabe neste turno. Tudo ja colhido por quem chama.
pub struct Entradas<'a> {
    /// `AgentRuntime::tool_inventory()`.
    pub inventario: &'a [ToolInventoryEntry],
    /// O portao do turno (nome E classe): `AgentRuntime::portao_permite`.
    pub permite: &'a dyn Fn(&str) -> bool,
    /// `AgentRuntime::disponibilidade_de`.
    pub disponibilidade: &'a dyn Fn(&str) -> Disponibilidade,
    /// `McpManager::server_statuses()`; vazio sem manager.
    pub mcp: &'a [McpServerStatus],
    /// `Some((motivo, remediacao))` quando o `bash` **nao** esta registrado
    /// (sandbox desligado, plataforma sem sandbox): vira uma linha
    /// `not_configured`. `None` quando esta registrado, ou quando quem chama
    /// nao sabe (nao inventa).
    pub bash_desligado: Option<(String, String)>,
    /// Turno restrito (#1347): nomes de servidor MCP caidos sao retidos —
    /// a linha sintetica do servidor sai como `mcp/*`.
    pub restrito: bool,
}

/// Nomes que o gateway considera acoes de canal (a origem vira `channel`).
const ACOES_DE_CANAL: &[&str] = &["telegram_send"];

fn origem_de(entrada: &ToolInventoryEntry) -> &'static str {
    if entrada.source == "mcp" {
        "mcp"
    } else if ACOES_DE_CANAL.contains(&entrada.name.as_str()) {
        "channel"
    } else {
        "native"
    }
}

fn estado_do_servidor<'a>(mcp: &'a [McpServerStatus], nome: &str) -> Option<&'a McpServerStatus> {
    mcp.iter().find(|s| s.name == nome)
}

/// O registro deste turno, na ordem: inventario (por nome), depois as
/// linhas sinteticas. Puro.
pub fn registro(e: &Entradas<'_>) -> Vec<Capacidade> {
    let mut linhas: Vec<Capacidade> = Vec::with_capacity(e.inventario.len() + 4);
    let mut entradas: Vec<&ToolInventoryEntry> = e.inventario.iter().collect();
    entradas.sort_by(|a, b| a.name.cmp(&b.name));
    for entrada in entradas {
        let source = origem_de(entrada);
        let servidor_caido = entrada
            .server
            .as_deref()
            .and_then(|s| estado_do_servidor(e.mcp, s))
            .filter(|s| s.state != McpServerState::Connected);
        let (state, reason_code, reason, remediation) = if let Some(s) = servidor_caido {
            (
                Estado::Unhealthy,
                s.state.as_str(),
                format!(
                    "o servidor MCP desta ferramenta esta {}; a ferramenta existe, mas nao responde agora.",
                    s.state.as_str()
                ),
                Some(
                    "Veja `mcp.servers` no `/api/diagnostics`; o operador reinicia o servidor pelo console (MCP Servers) ou com `garraia mcp restart <nome>`."
                        .to_string(),
                ),
            )
        } else {
            match (e.disponibilidade)(&entrada.name) {
                Disponibilidade::Indisponivel {
                    codigo,
                    motivo,
                    remediacao,
                } => (
                    if codigo == "not_configured" {
                        Estado::NotConfigured
                    } else {
                        Estado::Unavailable
                    },
                    codigo,
                    motivo,
                    remediacao,
                ),
                Disponibilidade::Disponivel => {
                    if (e.permite)(&entrada.name) {
                        (
                            Estado::Visible,
                            "ok",
                            "disponivel neste turno.".to_string(),
                            None,
                        )
                    } else {
                        (
                            Estado::Denied,
                            "policy",
                            "existe e esta operacional, mas a politica desta conversa (modo da sessao ou teto de quem fala) nao a libera."
                                .to_string(),
                            None,
                        )
                    }
                }
            }
        };
        linhas.push(Capacidade {
            name: entrada.name.clone(),
            source,
            server: entrada.server.clone(),
            classes: entrada.capacidades.clone(),
            state,
            reason_code,
            reason,
            remediation,
        });
    }

    // Servidor MCP conhecido, caido e SEM ferramenta no inventario: existe,
    // esta fora do ar — e nao "nao tem MCP".
    for s in e
        .mcp
        .iter()
        .filter(|s| s.state != McpServerState::Connected)
    {
        let ja_representado = e
            .inventario
            .iter()
            .any(|t| t.server.as_deref() == Some(s.name.as_str()));
        if ja_representado {
            continue;
        }
        let nome = if e.restrito {
            "mcp/*".to_string()
        } else {
            format!("{}/*", s.name)
        };
        linhas.push(Capacidade {
            name: nome,
            source: "runtime",
            server: (!e.restrito).then(|| s.name.clone()),
            classes: Vec::new(),
            state: Estado::Unhealthy,
            reason_code: s.state.as_str(),
            reason: format!(
                "um servidor MCP configurado esta {}: as ferramentas dele existem, mas nao estao carregadas agora.",
                s.state.as_str()
            ),
            remediation: Some(
                "Veja `mcp.servers` no `/api/diagnostics`; o operador reinicia o servidor pelo console (MCP Servers) ou com `garraia mcp restart <nome>`."
                    .to_string(),
            ),
        });
    }

    // `bash` fora do inventario com motivo conhecido: nao configurado, e nao
    // inexistente.
    if !e.inventario.iter().any(|t| t.name == "bash")
        && let Some((motivo, remediacao)) = &e.bash_desligado
    {
        linhas.push(Capacidade {
            name: "bash".to_string(),
            source: "runtime",
            server: None,
            classes: vec!["process.execute"],
            state: Estado::NotConfigured,
            reason_code: "not_configured",
            reason: motivo.clone(),
            remediation: Some(remediacao.clone()),
        });
    }
    linhas
}

/// Contagens por estado, para o diagnostico e o console.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Contagens {
    pub visible: usize,
    pub denied: usize,
    pub unavailable: usize,
    pub unhealthy: usize,
    pub not_configured: usize,
}

pub fn contagens(linhas: &[Capacidade]) -> Contagens {
    let mut c = Contagens::default();
    for l in linhas {
        match l.state {
            Estado::Visible => c.visible += 1,
            Estado::Denied => c.denied += 1,
            Estado::Unavailable => c.unavailable += 1,
            Estado::Unhealthy => c.unhealthy += 1,
            Estado::NotConfigured => c.not_configured += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrada(
        name: &str,
        source: &str,
        server: Option<&str>,
        classes: &[&'static str],
    ) -> ToolInventoryEntry {
        ToolInventoryEntry {
            name: name.to_string(),
            description: String::new(),
            source: source.to_string(),
            server: server.map(str::to_string),
            capacidades: classes.to_vec(),
        }
    }

    fn servidor(name: &str, state: McpServerState, tools: usize) -> McpServerStatus {
        McpServerStatus {
            name: name.to_string(),
            state,
            tool_count: tools,
            attempts: 0,
            max_restarts: 0,
            cause: None,
            last_error: None,
        }
    }

    fn por_nome<'a>(linhas: &'a [Capacidade], nome: &str) -> &'a Capacidade {
        linhas
            .iter()
            .find(|l| l.name == nome)
            .unwrap_or_else(|| panic!("{nome} ausente: {linhas:?}"))
    }

    /// Os cinco estados nascem das cinco fontes; nenhum e inventado.
    #[test]
    fn cada_estado_vem_da_sua_fonte() {
        let inventario = vec![
            entrada("file_read", "native", None, &["filesystem.read"]),
            entrada("file_write", "native", None, &["filesystem.write"]),
            entrada("telegram_send", "native", None, &["message.send"]),
            entrada(
                "filesystem__read_file",
                "mcp",
                Some("filesystem"),
                &["filesystem.read"],
            ),
            entrada("memoria__busca", "mcp", Some("memoria"), &["mcp.read"]),
        ];
        let permite = |n: &str| n != "file_write";
        let disponibilidade = |n: &str| {
            if n == "telegram_send" {
                Disponibilidade::indisponivel(
                    "channel_offline",
                    "o canal Telegram nao esta conectado agora.",
                    Some("veja garra_status".into()),
                )
            } else {
                Disponibilidade::Disponivel
            }
        };
        let mcp = vec![
            servidor("filesystem", McpServerState::Connected, 1),
            servidor("memoria", McpServerState::Retrying, 0),
            servidor("caido", McpServerState::Failed, 0),
        ];
        let linhas = registro(&Entradas {
            inventario: &inventario,
            permite: &permite,
            disponibilidade: &disponibilidade,
            mcp: &mcp,
            bash_desligado: Some(("sandbox desligado".into(), "ligue `agent.sandbox`".into())),
            restrito: false,
        });
        assert_eq!(por_nome(&linhas, "file_read").state, Estado::Visible);
        let negada = por_nome(&linhas, "file_write");
        assert_eq!(negada.state, Estado::Denied);
        assert_eq!(negada.reason_code, "policy");
        assert!(negada.reason.contains("existe"), "{}", negada.reason);
        let canal = por_nome(&linhas, "telegram_send");
        assert_eq!(canal.state, Estado::Unavailable);
        assert_eq!(canal.reason_code, "channel_offline");
        assert_eq!(canal.source, "channel");
        assert!(canal.remediation.is_some());
        assert_eq!(
            por_nome(&linhas, "filesystem__read_file").state,
            Estado::Visible
        );
        let doente = por_nome(&linhas, "memoria__busca");
        assert_eq!(doente.state, Estado::Unhealthy);
        assert_eq!(doente.reason_code, "retrying");
        let sem_tools = por_nome(&linhas, "caido/*");
        assert_eq!(sem_tools.state, Estado::Unhealthy);
        assert_eq!(sem_tools.reason_code, "failed");
        assert_eq!(sem_tools.source, "runtime");
        let bash = por_nome(&linhas, "bash");
        assert_eq!(bash.state, Estado::NotConfigured);
        assert_eq!(bash.classes, vec!["process.execute"]);
        let c = contagens(&linhas);
        assert_eq!(
            (
                c.visible,
                c.denied,
                c.unavailable,
                c.unhealthy,
                c.not_configured
            ),
            (2, 1, 1, 2, 1)
        );
        // Serializa com os nomes que o console e o modelo leem.
        let json = serde_json::to_value(&linhas).expect("json");
        assert_eq!(json[0]["state"], serde_json::json!("visible"));
        assert!(json.to_string().contains("\"not_configured\""));
    }

    /// `not_configured` da propria ferramenta (canal desligado na config) e
    /// distinto de `unavailable` (canal configurado e caido).
    #[test]
    fn nao_configurado_da_ferramenta_vira_not_configured() {
        let inventario = vec![entrada("telegram_send", "native", None, &["message.send"])];
        let permite = |_: &str| true;
        let disponibilidade = |_: &str| {
            Disponibilidade::indisponivel(
                "not_configured",
                "o canal Telegram nao esta configurado.",
                None,
            )
        };
        let linhas = registro(&Entradas {
            inventario: &inventario,
            permite: &permite,
            disponibilidade: &disponibilidade,
            mcp: &[],
            bash_desligado: None,
            restrito: false,
        });
        assert_eq!(linhas[0].state, Estado::NotConfigured);
        assert_eq!(linhas[0].reason_code, "not_configured");
        assert!(
            !linhas.iter().any(|l| l.name == "bash"),
            "sem motivo conhecido, bash nao e inventado"
        );
    }

    /// Turno restrito: o nome de um servidor MCP caido e retido (`mcp/*`),
    /// e nada mais muda — o modelo continua sabendo que existe e esta fora.
    #[test]
    fn turno_restrito_retem_o_nome_do_servidor_caido() {
        let permite = |_: &str| true;
        let disponibilidade = |_: &str| Disponibilidade::Disponivel;
        let mcp = vec![servidor(
            "interno-da-empresa",
            McpServerState::Disconnected,
            0,
        )];
        let linhas = registro(&Entradas {
            inventario: &[],
            permite: &permite,
            disponibilidade: &disponibilidade,
            mcp: &mcp,
            bash_desligado: None,
            restrito: true,
        });
        assert_eq!(linhas.len(), 1);
        assert_eq!(linhas[0].name, "mcp/*");
        assert!(linhas[0].server.is_none());
        assert_eq!(linhas[0].state, Estado::Unhealthy);
        assert!(!format!("{linhas:?}").contains("interno-da-empresa"));
    }
}
