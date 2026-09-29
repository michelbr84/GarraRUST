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
    /// #1416: o que a SESSAO tem de contexto para as ferramentas que
    /// dependem dele. `None` em cada campo = quem chama nao sabe (nao
    /// inventa): a linha fica como o portao e a disponibilidade disserem.
    pub contexto: ContextoDaSessao,
}

/// O contexto de sessao que decide se uma ferramenta REGISTRADA e
/// PERMITIDA tem onde agir (#1416, #1381): raiz para as file tools,
/// repositorio para o `repo_search`. E o "missing context" da #1387 — um
/// estado distinto de negada (politica) e de nao configurada (instalacao):
/// a remediacao e do usuario da conversa (`/project <nome>`), nao do
/// operador.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextoDaSessao {
    /// `Some(false)`: nenhuma raiz efetiva — sem `working_dir` na sessao e
    /// sem raiz de config/workspace padrao. Ver [`contexto_de_arquivos`].
    pub tem_raiz: Option<bool>,
    /// `Some(false)`: sem `working_dir` e o diretorio do processo nao e um
    /// repositorio — o `repo_search` recusaria de imediato.
    pub tem_repositorio: Option<bool>,
}

impl ContextoDaSessao {
    /// Nada se sabe: o registro nao muda nenhuma linha por contexto.
    pub const DESCONHECIDO: ContextoDaSessao = ContextoDaSessao {
        tem_raiz: None,
        tem_repositorio: None,
    };
}

/// Ferramentas nativas que nao fazem nada sem raiz (o mesmo `FileJail`).
pub const PRECISAM_DE_RAIZ: &[&str] = &["file_read", "file_write", "list_dir"];
/// Ferramentas nativas que nao fazem nada sem repositorio.
pub const PRECISAM_DE_REPOSITORIO: &[&str] = &["repo_search"];

/// Codigo de motivo de uma file tool sem raiz nenhuma.
pub const NO_ROOTS: &str = "no_roots";
/// Codigo de motivo do `repo_search` sem repositorio.
pub const NO_REPOSITORY: &str = "no_repository";

/// O contexto de arquivos de UMA sessao, a partir do que o boot resolveu
/// para as raizes (`raizes_das_file_tools(config).fonte`) e do
/// `working_dir` da sessao. Puro.
///
/// - `Declaradas` (`agent.file_roots`) e `WorkspacePadrao` (`<data_dir>/
///   workspace/<sessao>`): toda sessao tem raiz, com ou sem projeto.
/// - `SomenteSessao`: so a sessao com `working_dir` (projeto selecionado)
///   tem raiz; as outras recebem `NO_ROOTS_MESSAGE` em toda chamada.
///
/// `cwd_em_repositorio` e consultado so quando nao ha `working_dir` — e o
/// `stat` que o `repo_search` faria; o chamador passa a pergunta, nao o
/// resultado, para o caminho comum (sessao com projeto) nao tocar o disco.
pub fn contexto_de_arquivos(
    fonte: crate::bootstrap::FonteDasRaizesDasFileTools,
    working_dir: Option<&std::path::Path>,
    cwd_em_repositorio: impl FnOnce() -> bool,
) -> ContextoDaSessao {
    use crate::bootstrap::FonteDasRaizesDasFileTools as Fonte;
    let tem_working_dir = working_dir.is_some();
    ContextoDaSessao {
        tem_raiz: Some(tem_working_dir || !matches!(fonte, Fonte::SomenteSessao)),
        tem_repositorio: Some(tem_working_dir || cwd_em_repositorio()),
    }
}

/// A linha de "falta contexto" (#1416) para uma ferramenta permitida e
/// disponivel, ou `None` quando o contexto basta (ou nao se sabe).
fn falta_de_contexto(
    nome: &str,
    contexto: ContextoDaSessao,
) -> Option<(&'static str, String, Option<String>)> {
    if PRECISAM_DE_RAIZ.contains(&nome) && contexto.tem_raiz == Some(false) {
        return Some((
            NO_ROOTS,
            "registrada e permitida, mas esta sessao nao tem raiz nenhuma para as file tools (sem projeto selecionado, sem `agent.file_roots` e sem workspace padrao): toda chamada seria recusada."
                .to_string(),
            Some(
                "Selecione um projeto nesta conversa com `/project <nome>` (ou o operador declara `agent.file_roots`)."
                    .to_string(),
            ),
        ));
    }
    if PRECISAM_DE_REPOSITORIO.contains(&nome) && contexto.tem_repositorio == Some(false) {
        return Some((
            NO_REPOSITORY,
            "registrada e permitida, mas esta sessao nao tem repositorio para buscar (sem projeto selecionado, e o diretorio do processo nao e um repositorio): a busca seria recusada de imediato."
                .to_string(),
            Some("Selecione um projeto nesta conversa com `/project <nome>`.".to_string()),
        ));
    }
    None
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
                    "Veja `mcp.servers` no `/api/diagnostics`; o operador reinicia o servidor pelo console (MCP Servers) ou com `POST /admin/api/mcp/<nome>/restart`."
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
                        // #1416: permitida e operacional, mas SEM o contexto
                        // de que depende (raiz, repositorio) — e "falta
                        // contexto", nao "negada": a remediacao e do usuario
                        // da conversa, e o modelo tem de dizer isso.
                        match falta_de_contexto(&entrada.name, e.contexto) {
                            Some((codigo, motivo, remediacao)) => {
                                (Estado::Unavailable, codigo, motivo, remediacao)
                            }
                            None => (
                                Estado::Visible,
                                "ok",
                                "disponivel neste turno.".to_string(),
                                None,
                            ),
                        }
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
                "Veja `mcp.servers` no `/api/diagnostics`; o operador reinicia o servidor pelo console (MCP Servers) ou com `POST /admin/api/mcp/<nome>/restart`."
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
            contexto: ContextoDaSessao::DESCONHECIDO,
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
            contexto: ContextoDaSessao::DESCONHECIDO,
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
            contexto: ContextoDaSessao::DESCONHECIDO,
        });
        assert_eq!(linhas.len(), 1);
        assert_eq!(linhas[0].name, "mcp/*");
        assert!(linhas[0].server.is_none());
        assert_eq!(linhas[0].state, Estado::Unhealthy);
        assert!(!format!("{linhas:?}").contains("interno-da-empresa"));
    }

    // -----------------------------------------------------------------------
    // #1416 / #1381: falta de contexto (raiz, repositorio) e um estado proprio
    // -----------------------------------------------------------------------

    fn entradas_com_contexto<'a>(
        inventario: &'a [ToolInventoryEntry],
        permite: &'a dyn Fn(&str) -> bool,
        disponivel: &'a dyn Fn(&str) -> Disponibilidade,
        contexto: ContextoDaSessao,
    ) -> Entradas<'a> {
        Entradas {
            inventario,
            permite,
            disponibilidade: disponivel,
            mcp: &[],
            bash_desligado: None,
            restrito: false,
            contexto,
        }
    }

    fn inventario_de_arquivos() -> Vec<ToolInventoryEntry> {
        vec![
            entrada("file_read", "native", None, &["filesystem.read"]),
            entrada("file_write", "native", None, &["filesystem.write"]),
            entrada("list_dir", "native", None, &["filesystem.read"]),
            entrada("repo_search", "native", None, &["filesystem.read"]),
            entrada("web_fetch", "native", None, &["network.read"]),
        ]
    }

    fn linha<'a>(linhas: &'a [Capacidade], nome: &str) -> &'a Capacidade {
        linhas
            .iter()
            .find(|c| c.name == nome)
            .unwrap_or_else(|| panic!("{nome} ausente: {linhas:?}"))
    }

    #[test]
    fn sem_raiz_e_sem_repositorio_as_ferramentas_de_arquivo_dizem_falta_de_contexto() {
        let inv = inventario_de_arquivos();
        let tudo = |_: &str| true;
        let disponivel = |_: &str| Disponibilidade::Disponivel;
        let linhas = registro(&entradas_com_contexto(
            &inv,
            &tudo,
            &disponivel,
            ContextoDaSessao {
                tem_raiz: Some(false),
                tem_repositorio: Some(false),
            },
        ));
        for nome in PRECISAM_DE_RAIZ {
            let l = linha(&linhas, nome);
            assert_eq!(l.state, Estado::Unavailable, "{nome}: {l:?}");
            assert_eq!(l.reason_code, NO_ROOTS, "{nome}");
            assert!(
                l.remediation
                    .as_deref()
                    .is_some_and(|r| r.contains("/project")),
                "{nome}: a remediacao e do usuario da conversa: {l:?}"
            );
            assert!(!l.reason.contains('/'), "sem caminho: {}", l.reason);
        }
        let rs = linha(&linhas, "repo_search");
        assert_eq!(rs.state, Estado::Unavailable);
        assert_eq!(rs.reason_code, NO_REPOSITORY);
        assert!(
            rs.remediation
                .as_deref()
                .is_some_and(|r| r.contains("/project"))
        );
        // Quem nao depende de raiz nao muda.
        assert_eq!(linha(&linhas, "web_fetch").state, Estado::Visible);
        let c = contagens(&linhas);
        assert_eq!((c.unavailable, c.visible), (4, 1));
    }

    #[test]
    fn a_politica_negada_vence_a_falta_de_contexto() {
        let inv = inventario_de_arquivos();
        let so_leitura = |n: &str| n != "file_write";
        let disponivel = |_: &str| Disponibilidade::Disponivel;
        let linhas = registro(&entradas_com_contexto(
            &inv,
            &so_leitura,
            &disponivel,
            ContextoDaSessao {
                tem_raiz: Some(false),
                tem_repositorio: Some(true),
            },
        ));
        // Negada pela politica: e isso que o modelo tem de dizer, e nao
        // "selecione um projeto" — selecionar nao liberaria a escrita.
        let fw = linha(&linhas, "file_write");
        assert_eq!(fw.state, Estado::Denied);
        assert_eq!(fw.reason_code, "policy");
        assert_eq!(linha(&linhas, "file_read").reason_code, NO_ROOTS);
        assert_eq!(linha(&linhas, "repo_search").state, Estado::Visible);
    }

    #[test]
    fn contexto_desconhecido_ou_presente_nao_muda_linha_nenhuma() {
        let inv = inventario_de_arquivos();
        let tudo = |_: &str| true;
        let disponivel = |_: &str| Disponibilidade::Disponivel;
        for contexto in [
            ContextoDaSessao::DESCONHECIDO,
            ContextoDaSessao {
                tem_raiz: Some(true),
                tem_repositorio: Some(true),
            },
        ] {
            let linhas = registro(&entradas_com_contexto(&inv, &tudo, &disponivel, contexto));
            assert!(
                linhas.iter().all(|l| l.state == Estado::Visible),
                "{contexto:?}: {linhas:?}"
            );
        }
    }

    #[test]
    fn contexto_de_arquivos_segue_a_fonte_das_raizes_e_o_working_dir() {
        use crate::bootstrap::FonteDasRaizesDasFileTools as Fonte;
        use std::cell::Cell;
        let wd = std::path::Path::new("/tmp/projeto");
        for (fonte, working_dir, raiz) in [
            (Fonte::Declaradas, None, true),
            (Fonte::WorkspacePadrao, None, true),
            (Fonte::SomenteSessao, None, false),
            (Fonte::SomenteSessao, Some(wd), true),
            (Fonte::Declaradas, Some(wd), true),
        ] {
            let c = contexto_de_arquivos(fonte, working_dir, || false);
            assert_eq!(c.tem_raiz, Some(raiz), "{fonte:?} {working_dir:?}");
        }
        // Repositorio: com `working_dir` nem pergunta ao CWD; sem, pergunta.
        let perguntou = Cell::new(false);
        let c = contexto_de_arquivos(Fonte::WorkspacePadrao, Some(wd), || {
            perguntou.set(true);
            false
        });
        assert_eq!(c.tem_repositorio, Some(true));
        assert!(!perguntou.get(), "com working_dir o CWD nao e consultado");
        let c = contexto_de_arquivos(Fonte::WorkspacePadrao, None, || {
            perguntou.set(true);
            false
        });
        assert_eq!(c.tem_repositorio, Some(false));
        assert!(perguntou.get());
        let c = contexto_de_arquivos(Fonte::WorkspacePadrao, None, || true);
        assert_eq!(c.tem_repositorio, Some(true));
    }
}
