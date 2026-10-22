//! `garra_diagnostics` — diagnostico nativo e seguro da instalacao do Garra
//! (issue #1669).
//!
//! O Garra precisa inspecionar a PROPRIA instalacao por interfaces nativas e
//! seguras — sem terminal geral e sem leitura arbitrária de arquivos do
//! usuario. Esta tool e somente leitura, sem shell e sem rede: cada verificacao
//! sai de funcoes Rust do proprio workspace (`channels_view`, `bootstrap`,
//! `capacidades_registro`, `garraia_config::check`, `logs_handler`).
//!
//! ## Formato de saida
//!
//! `{"verificacoes": [{id, estado, evidencia, detalhe}]}`, com `estado`:
//!
//! | estado | significa |
//! |---|---|
//! | `ok` | funcionando |
//! | `degradado` | configurado, mas caido ou com ressalva (canal fora do ar, cofre trancado) |
//! | `erro` | falha dura (config invalida, dependencia ausente, leitura que falhou) |
//! | `nao_configurado` | opcional que esta instalacao nunca configurou |
//! | `bloqueado` | negado por politica (sessao nao-operadora, jail de arquivo) |
//! | `nao_suportado` | nao existe nesta plataforma (iMessage fora de macOS) |
//!
//! Os seis estados sao casos REAIS e distintos: MCP ausente e `nao_configurado`
//! (ninguem pediu), canal configurado mas caido e `degradado` (alguem pediu e
//! nao funciona), consulta negada e `bloqueado`, e recurso inexistente na
//! plataforma e `nao_suportado`.
//!
//! ## Controle de acesso (fail-closed)
//!
//! So a sessao do OPERADOR ve o relatorio — a mesma decisao do `garra_status`:
//! bit `turno_restrito` ([`garraia_agents::tools::turn_tools::turno_restrito`])
//! OU [`crate::channels_view::sessao_do_operador`]. Fora de um turno despachado
//! nao ha evidencia de quem fala, e a resposta e o MINIMO absoluto: uma unica
//! entrada `acesso`/`bloqueado`, sem versao, sem nomes de canal, sem nada da
//! instalacao. Sessao remota (WhatsApp, Telegram, mobile, A2A), sessao
//! desconhecida e porta do operador aberta caem no mesmo caminho.
//!
//! ## O que nunca sai daqui
//!
//! Valores de credencial (so a ORIGEM — vault/config.yml/env), mensagens
//! privadas e identificadores de pessoa: todo texto passa pelo redator do
//! `garraia_security` (`redact_secrets` + `mascarar_numeros_longos`) antes de
//! entrar no relatorio, inclusive os valores de `evidencia`. Os logs sao uma
//! amostra LIMITADA das ultimas [`ULTIMAS_LINHAS_DE_LOG`] linhas do
//! `garraia.log` do proprio produto, e a leitura so acontece quando o arquivo
//! resolve DENTRO do diretorio de configuracao do produto (file jail,
//! fail-closed em qualquer duvida).

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::Result;
use serde::Serialize;

use crate::push_channels::PushMounted;
use crate::state::AppState;

/// Quantas linhas do fim do `garraia.log` o relatorio amostra. Limitado de
/// proposito: o log cresce sem rotacao e o relatorio viaja dentro de um turno.
pub(crate) const ULTIMAS_LINHAS_DE_LOG: usize = 40;

/// Estado de uma verificacao do diagnostico.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoDiag {
    /// Funcionando.
    Ok,
    /// Falha dura: configurado/assumido e quebrado, ou leitura que falhou.
    Erro,
    /// Opcional que esta instalacao nunca configurou. Nao e defeito.
    NaoConfigurado,
    /// Negado por politica (sessao nao-operadora, jail de arquivo).
    Bloqueado,
    /// Configurado, mas caido ou com ressalva que o operador precisa ver.
    Degradado,
    /// Recurso inexistente nesta plataforma (iMessage fora de macOS).
    NaoSuportado,
}

/// Uma entrada do relatorio: `{id, estado, evidencia, detalhe}`.
#[derive(Debug, Clone, Serialize)]
pub struct Verificacao {
    /// Id estavel ("versao.instalada", "canais.telegram", "mcp.<nome>", ...).
    pub id: String,
    pub estado: EstadoDiag,
    /// Evidencia estruturada (objeto JSON), ja sanitizada.
    pub evidencia: serde_json::Value,
    /// Frase curta para o operador, ja sanitizada.
    pub detalhe: String,
}

impl Verificacao {
    /// Monta a entrada sanitizando TODO texto que sai — os valores de
    /// `evidencia` e o `detalhe` passam pelo redator do `garraia_security`
    /// antes de entrar no relatorio. Caminho unico: nenhuma entrada pode
    /// esquecer a sanitizacao.
    fn nova(
        id: impl Into<String>,
        estado: EstadoDiag,
        evidencia: serde_json::Value,
        detalhe: impl AsRef<str>,
    ) -> Self {
        Self {
            id: id.into(),
            estado,
            evidencia: sanear_valor(evidencia),
            detalhe: sanear(detalhe.as_ref()),
        }
    }
}

/// Redator de texto: segredos reconheciveis viram `[REDACTED]` e sequencias
/// longas de digitos (telefone, id de chat, LID) sao mascaradas deixando os
/// 4 ultimos — a mesma convencao `phone_last4` dos logs.
fn sanear(texto: &str) -> String {
    garraia_security::mascarar_numeros_longos(&garraia_security::redact_secrets(texto)).into_owned()
}

/// [`sanear`] aplicado recursivamente aos valores (nunca as chaves) de um
/// `serde_json::Value`. Numeros e bools passam intocados — so texto carrega
/// segredo.
fn sanear_valor(valor: serde_json::Value) -> serde_json::Value {
    match valor {
        serde_json::Value::String(s) => serde_json::Value::String(sanear(&s)),
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(sanear_valor).collect())
        }
        serde_json::Value::Object(o) => {
            serde_json::Value::Object(o.into_iter().map(|(k, v)| (k, sanear_valor(v))).collect())
        }
        outro => outro,
    }
}

/// A nota de orientacao sobre esta tool, pronta para quem quiser publicar
/// (um `garra_status`, um console, um help de comando): aponta a tool certa em
/// vez de duplicar o diagnostico. Vive AQUI, uma fonte so. Ela ainda NAO vai
/// no JSON do `garra_status`: la o conjunto de chaves e fixado pelo teste
/// `saida_e_secret_free_e_tem_so_os_campos_previstos`, e editar esse teste e
/// proibido pelo contrato de interface compartilhada — adicionar a chave
/// `diagnostics` la e o follow-up de uma linha + uma linha no teste.
pub fn nota_diagnostics() -> &'static str {
    "Need a full read-only diagnostic of THIS installation (version, gateway, every channel, \
     MCP servers, capabilities and tools, validated effective config, credential PRESENCE AND \
     ORIGIN — never values —, last sanitized log lines, bridge/database components)? Call the \
     `garra_diagnostics` tool. It is operator-only and fail-closed: a session that is not the \
     operator's gets a refusal and nothing else."
}

pub struct GarraDiagnosticsTool {
    /// Weak pelo mesmo motivo do `GarraStatusTool`: o `AppState` e dono do
    /// runtime, o runtime e dono da tool, e um handle forte de volta fecharia
    /// um ciclo de `Arc` que vaza o estado inteiro do gateway.
    state: Weak<AppState>,
    /// So as contagens dos canais push (nunca o `PushChannelStates`), pelo
    /// mesmo motivo do `Weak` acima.
    push: PushMounted,
    /// Diretorio de configuracao do produto, capturado no registro da tool:
    /// e a unica raiz de onde o diagnostico le arquivo (o `garraia.log`).
    config_dir: PathBuf,
}

impl GarraDiagnosticsTool {
    pub fn new(state: &Arc<AppState>, push: PushMounted, config_dir: PathBuf) -> Self {
        Self {
            state: Arc::downgrade(state),
            push,
            config_dir,
        }
    }
}

#[async_trait]
impl Tool for GarraDiagnosticsTool {
    fn name(&self) -> &str {
        "garra_diagnostics"
    }

    fn description(&self) -> &str {
        "Runs a read-only, structured diagnostic of THIS Garra installation by native interfaces \
         (no shell, no arbitrary file reads, no network): installed version, gateway state, every \
         channel with its state, MCP servers, available tools and capabilities, the validated \
         effective config, credential PRESENCE AND ORIGIN (e.g. \"provider X: key from the \
         credential vault\" — never a value), the last sanitized log lines, and relevant \
         components (WhatsApp Node bridge, database reachability). Output is JSON: \
         `{\"verificacoes\": [{id, estado, evidencia, detalhe}]}` with `estado` in \
         {ok, erro, nao_configurado, bloqueado, degradado, nao_suportado}. Operator-only and \
         fail-closed: any session that is not provably the operator's gets a single refusal entry \
         and nothing else. Takes no input. Use it when the user asks how this Garra is installed, \
         configured or healthy — instead of guessing or saying you cannot inspect yourself."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, _input: serde_json::Value) -> Result<ToolOutput> {
        let Some(state) = self.state.upgrade() else {
            return Ok(ToolOutput::error("gateway state is gone (shutting down?)"));
        };

        // Fail-closed: fora de um turno despachado nao ha evidencia de quem
        // fala (`None` conta como restrito, ao contrario do `garra_status` —
        // aqui o relatorio e todo do operador). E o mesmo portao do
        // `garra_status`: bit do turno OU sessao que nao e do operador.
        let restrito = garraia_agents::tools::turn_tools::turno_restrito().unwrap_or(true);
        let operador = crate::channels_view::sessao_do_operador(&state, &ctx.session_id).await;
        if restrito || !operador {
            return Ok(ToolOutput::success(serializar(&[acesso_negado(
                restrito, operador,
            )])));
        }

        let verificacoes = relatorio_de_diagnostico(&state, self.push, &self.config_dir).await;
        Ok(ToolOutput::success(serializar(&verificacoes)))
    }
}

/// A recusa: o MINIMO absoluto. Uma entrada so, sem versao, sem nomes de
/// canal, sem nada da instalacao — fail-closed tambem no conteudo.
fn acesso_negado(restrito: bool, operador: bool) -> Verificacao {
    let motivo = if !operador {
        "sessao_nao_operadora"
    } else if restrito {
        "turno_restrito_ou_sem_evidencia_de_turno"
    } else {
        "nao_prova_operador"
    };
    Verificacao::nova(
        "acesso",
        EstadoDiag::Bloqueado,
        serde_json::json!({ "motivo": motivo }),
        "Diagnostico negado por politica: esta conversa nao e do operador autenticado. Nada da \
         instalacao e divulgado aqui (fail-closed). O operador consulta o mesmo diagnostico numa \
         superficie local (web chat, CLI ou desktop).",
    )
}

/// O relatorio inteiro, na ordem dos itens obrigatorios da #1669.
async fn relatorio_de_diagnostico(
    state: &AppState,
    push: PushMounted,
    config_dir: &Path,
) -> Vec<Verificacao> {
    let mut v = Vec::new();
    v.push(verificacao_versao(state.boot_time.elapsed().as_secs()));
    v.push(verificacao_gateway(
        &state.config.gateway.host,
        state.config.gateway.port,
        state.config.gateway.tls_cert_path.is_some(),
    ));
    v.extend(verificacoes_dos_canais(state, push).await);
    v.extend(verificacoes_mcp(state).await);
    v.push(verificacao_ferramentas(state));
    v.push(verificacao_capacidades(state).await);
    v.push(verificacao_config(&state.config, config_dir));
    v.push(verificacao_credenciais_providers(&state.config));
    v.push(verificacao_credenciais_canais(&state.config));
    v.push(verificacao_credenciais_vault(config_dir));
    v.push(verificacao_credenciais_jwt());
    v.push(verificacao_logs(config_dir));
    v.extend(verificacoes_dos_componentes(state).await);
    v
}

fn serializar(verificacoes: &[Verificacao]) -> String {
    let relatorio = serde_json::json!({ "verificacoes": verificacoes });
    serde_json::to_string_pretty(&relatorio).unwrap_or_else(|_| relatorio.to_string())
}

// ─── 1. versao instalada ────────────────────────────────────────────────────

fn verificacao_versao(uptime_secs: u64) -> Verificacao {
    Verificacao::nova(
        "versao.instalada",
        EstadoDiag::Ok,
        serde_json::json!({
            "versao": env!("CARGO_PKG_VERSION"),
            "uptime_secs": uptime_secs,
        }),
        format!(
            "Garra {} (uptime {}s)",
            env!("CARGO_PKG_VERSION"),
            uptime_secs
        ),
    )
}

// ─── 2. estado do gateway ───────────────────────────────────────────────────

/// O gateway esta no ar (esta tool respondeu de dentro dele); o que pode
/// degradar a linha e o bind exposto (#1261) ou uma porta invalida.
fn verificacao_gateway(host: &str, porta: u16, tls: bool) -> Verificacao {
    let exposto = host == "0.0.0.0" || host == "::";
    let porta_valida = (1..=65535).contains(&porta);
    let estado = if !porta_valida {
        EstadoDiag::Erro
    } else if exposto {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    let detalhe = if !porta_valida {
        format!("porta {porta} fora de 1..=65535")
    } else if exposto {
        format!(
            "no ar em {host}:{porta}, mas ligado em TODAS as interfaces — exige firewall ou \
             gateway.api_key"
        )
    } else {
        format!("no ar em {host}:{porta}")
    };
    Verificacao::nova(
        "gateway.estado",
        estado,
        serde_json::json!({ "host": host, "porta": porta, "tls": tls }),
        detalhe,
    )
}

// ─── 3. estado dos canais (todos, com estado) ───────────────────────────────

/// O estado de UM canal, na vocabulario do diagnostico. `offline` e
/// "configurado e caido" — `degradado`; `optional` e "ninguem ligou" —
/// `nao_configurado`; `unknown` e defeito de codigo — `erro`. iMessage fora
/// de macOS nao existe nesta plataforma (`build_imessage_channels` e
/// `#![cfg(target_os = "macos")]`).
pub(crate) fn estado_do_canal(id: &str, status: &str) -> EstadoDiag {
    if id == "imessage" && !cfg!(target_os = "macos") {
        return EstadoDiag::NaoSuportado;
    }
    match status {
        "active" => EstadoDiag::Ok,
        "offline" => EstadoDiag::Degradado,
        "optional" => EstadoDiag::NaoConfigurado,
        _ => EstadoDiag::Erro,
    }
}

async fn verificacoes_dos_canais(state: &AppState, push: PushMounted) -> Vec<Verificacao> {
    // A MESMA funcao do `GET /api/channels` e do `garra_status`: uma fonte so
    // para as tres superficies nao poderem discordar sobre o mesmo canal.
    let rows = crate::channels_view::channel_rows(state, push).await;
    let mut out = Vec::new();
    out.push(Verificacao::nova(
        "canais.superficies",
        EstadoDiag::Ok,
        serde_json::json!({
            "web": "servido pelo listener deste gateway",
            "api": "servido pelo listener deste gateway",
            "cli": "processo a parte, fora deste processo",
            "mcp": "processo a parte, fora deste processo",
        }),
        "web e api sao servidos por este processo (no ar); cli e mcp sao programas a parte que \
         este gateway nao inspeciona",
    ));
    for row in rows {
        if crate::channels_view::FORA_DO_RELATORIO_DO_AGENTE.contains(&row.id) {
            continue;
        }
        let status = row.status_do_agente.unwrap_or("optional");
        let estado = estado_do_canal(row.id, status);
        let detalhe = match estado {
            EstadoDiag::Ok => format!("{}: ativo", row.id),
            EstadoDiag::Degradado => format!("{}: configurado, mas fora do ar", row.id),
            EstadoDiag::NaoConfigurado => {
                format!("{}: nunca configurado (canal opcional)", row.id)
            }
            EstadoDiag::NaoSuportado => {
                format!("{}: nao suportado nesta plataforma (exige macOS)", row.id)
            }
            EstadoDiag::Erro => format!(
                "{}: estado desconhecido no registro de canais (defeito de codigo)",
                row.id
            ),
            EstadoDiag::Bloqueado => format!("{}: consulta negada por politica", row.id),
        };
        out.push(Verificacao::nova(
            format!("canais.{}", row.id),
            estado,
            serde_json::json!({
                "canal": row.id,
                "nome": row.display_name,
                "status": status,
            }),
            detalhe,
        ));
    }
    // #1650: cadencia entre turnos — contagem viva da memoria deste processo.
    // O `garraia doctor` de outro processo nao enxerga este estado (memoria de
    // outro processo); quem mostra e o diagnostico do gateway.
    out.push(match state.whatsapp_linked.cadencia_diagnostico() {
        Some(d) => Verificacao::nova(
            "canais.whatsapp_linked.cadencia",
            EstadoDiag::Ok,
            serde_json::json!({
                "turnos_ultima_hora": d.turnos_ultima_hora,
                "chats_ativos": d.chats_ativos,
                "max_por_janela": d.max_por_janela,
                "janela_s": d.janela_s,
            }),
            format!(
                "cadencia entre turnos: {} turno(s) admitido(s) na ultima hora; teto {}/{}s por chat",
                d.turnos_ultima_hora, d.max_por_janela, d.janela_s
            ),
        ),
        None => Verificacao::nova(
            "canais.whatsapp_linked.cadencia",
            EstadoDiag::Erro,
            serde_json::json!({ "estado": "mutex envenenado" }),
            "estado da cadencia ilegivel (mutex envenenado) — contagem nao inventada",
        ),
    });
    out
}

// ─── 4. estado dos servidores MCP ───────────────────────────────────────────

/// Conectado e `ok`; desconectado/retentando e `degradado` (o supervisor
/// ainda tenta); `failed` (restarts esgotados) e `erro`.
pub(crate) fn estado_do_mcp(estado: garraia_agents::McpServerState) -> EstadoDiag {
    use garraia_agents::McpServerState as M;
    match estado {
        M::Connected => EstadoDiag::Ok,
        M::Disconnected | M::Retrying => EstadoDiag::Degradado,
        M::Failed => EstadoDiag::Erro,
    }
}

async fn verificacoes_mcp(state: &AppState) -> Vec<Verificacao> {
    let statuses = match &state.mcp_manager_arc {
        Some(mgr) => mgr.server_statuses().await,
        None => Vec::new(),
    };
    if statuses.is_empty() {
        return vec![Verificacao::nova(
            "mcp.servidores",
            EstadoDiag::NaoConfigurado,
            serde_json::json!({ "servidores": [] }),
            "nenhum servidor MCP configurado",
        )];
    }
    statuses
        .iter()
        .map(|s| {
            let estado = estado_do_mcp(s.state);
            // So nome, estado, contagens e o CODIGO da causa. `last_error` e o
            // `Display` da causa podem citar caminho/comando da config do
            // operador — ficam de fora.
            let causa = s.cause.as_ref().map(|c| c.as_str().to_string());
            let detalhe = match estado {
                EstadoDiag::Ok => format!("{} conectado ({} tools)", s.name, s.tool_count),
                EstadoDiag::Degradado => format!(
                    "{} fora do ar, reconectando ({}/{} tentativas, causa: {})",
                    s.name,
                    s.attempts,
                    s.max_restarts,
                    causa.as_deref().unwrap_or("desconhecida")
                ),
                EstadoDiag::Erro => format!(
                    "{} falhou: reinicios esgotados ({}/{} tentativas, causa: {})",
                    s.name,
                    s.attempts,
                    s.max_restarts,
                    causa.as_deref().unwrap_or("desconhecida")
                ),
                outro => format!("{}: {outro:?}", s.name),
            };
            Verificacao::nova(
                format!("mcp.{}", s.name),
                estado,
                serde_json::json!({
                    "nome": s.name,
                    "estado": s.state.as_str(),
                    "tools": s.tool_count,
                    "tentativas": s.attempts,
                    "max_restarts": s.max_restarts,
                    "causa": causa,
                }),
                detalhe,
            )
        })
        .collect()
}

// ─── 5. capacidades e ferramentas disponiveis ───────────────────────────────

fn verificacao_ferramentas(state: &AppState) -> Verificacao {
    let mut tools = garraia_agents::tools::turn_tools::ferramentas_do_turno()
        .unwrap_or_else(|| state.agents.tool_names());
    tools.sort();
    let total = tools.len();
    Verificacao::nova(
        "ferramentas.disponiveis",
        EstadoDiag::Ok,
        serde_json::json!({ "total": total, "nomes": tools }),
        format!("{total} ferramenta(s) disponivel(is) neste turno"),
    )
}

/// O registro de capacidades — a mesma visao do `/api/diagnostics` e do
/// console, sem portao de sessao (aqui a sessao ja e do operador).
async fn verificacao_capacidades(state: &AppState) -> Verificacao {
    let inventario = state.agents.tool_inventory();
    let permite = |_: &str| true;
    let disponibilidade = |n: &str| state.agents.disponibilidade_de(n);
    let mcp: Vec<garraia_agents::McpServerStatus> = match &state.mcp_manager_arc {
        Some(mgr) => mgr.server_statuses().await,
        None => Vec::new(),
    };
    let politica = crate::bootstrap::politica_de_execucao(&state.config);
    let exposicao = crate::bootstrap::exposicao_do_bash(
        politica.perfil,
        &crate::bootstrap::sandbox_policy_from(&state.config.agent.sandbox),
        !state.config.agent.bash_allowlist.is_empty(),
    );
    let bash_desligado = match exposicao {
        crate::bootstrap::ExposicaoDoBash::Desligado { .. } => Some((
            exposicao.descricao(),
            crate::bootstrap::COMO_LIGAR_O_BASH.to_string(),
        )),
        _ => None,
    };
    let contexto = crate::capacidades_registro::ContextoDaSessao {
        tem_raiz: Some(!matches!(
            state.raizes_das_file_tools.fonte,
            crate::bootstrap::FonteDasRaizesDasFileTools::SomenteSessao
        )),
        tem_repositorio: None,
    };
    let registro = crate::capacidades_registro::registro(&crate::capacidades_registro::Entradas {
        inventario: &inventario,
        permite: &permite,
        disponibilidade: &disponibilidade,
        mcp: &mcp,
        bash_desligado,
        restrito: false,
        contexto,
    });
    let contagens = crate::capacidades_registro::contagens(&registro);
    let itens: Vec<serde_json::Value> = registro
        .iter()
        .map(|l| {
            serde_json::json!({
                "nome": l.name,
                "estado": l.state.as_str(),
                "motivo": l.reason_code,
            })
        })
        .collect();
    let estado = if contagens.unavailable > 0 || contagens.unhealthy > 0 {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    Verificacao::nova(
        "capacidades.estado",
        estado,
        serde_json::json!({
            "visiveis": contagens.visible,
            "indisponiveis": contagens.unavailable,
            "fora_do_ar": contagens.unhealthy,
            "nao_configuradas": contagens.not_configured,
            "itens": itens,
        }),
        format!(
            "{} visivel(is), {} indisponivel(is), {} fora do ar, {} nao configurada(s)",
            contagens.visible, contagens.unavailable, contagens.unhealthy, contagens.not_configured
        ),
    )
}

// ─── 6. configuracao efetiva (validada) ─────────────────────────────────────

/// A mesma validacao do `garraia config check` (`garraia_config::run_check`),
/// sobre a config EFETIVA em memoria. O contrato do modulo garante que nenhum
/// valor de segredo entra nos achados (so presenca); o texto ainda passa pelo
/// redator por garantia.
fn verificacao_config(config: &garraia_config::AppConfig, config_dir: &Path) -> Verificacao {
    use garraia_config::Severity;

    let loader = garraia_config::ConfigLoader::with_dir(config_dir);
    let check = garraia_config::run_check(&loader, config);
    let erros = check
        .findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .count();
    let avisos = check.findings.len() - erros;
    let estado = if erros > 0 {
        EstadoDiag::Erro
    } else if avisos > 0 {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    let achados: Vec<serde_json::Value> = check
        .findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "severidade": match f.severity {
                    Severity::Error => "erro",
                    Severity::Warning => "aviso",
                },
                "campo": f.field,
                "mensagem": f.message,
            })
        })
        .collect();
    // So o NOME do arquivo de config — nunca o caminho absoluto do host.
    let arquivo = check
        .source
        .file_used
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let resumo = serde_json::to_value(&check.summary).unwrap_or_default();
    Verificacao::nova(
        "config.efetiva",
        estado,
        serde_json::json!({
            "erros": erros,
            "avisos": avisos,
            "achados": achados,
            "fonte": {
                "arquivo": arquivo,
                "usou_defaults": check.source.used_defaults,
                "env_detectadas": check.source.env_vars_detected,
                "mcp_json_presente": check.source.mcp_json_present,
            },
            "resumo": resumo,
        }),
        format!("config efetiva validada: {erros} erro(s), {avisos} aviso(s)"),
    )
}

// ─── 7. presenca e ORIGEM de credenciais (nunca valores) ────────────────────

/// Provedores locais que nao levam credencial nenhuma. E o mesmo whitelist do
/// `check.rs`: `provider_key_env` nao distingue "keyless" de "desconhecido".
const PROVEDORES_SEM_CREDENCIAL: &[&str] = &["ollama", "llamacpp", "echo"];

/// Uma entrada por provedor LLM: a ORIGEM da credencial, nunca o valor.
/// `resolve_provider_key_source` e a fonte unica da precedencia
/// vault → config.yml → env (a mesma que decide se o provedor sobe no boot).
fn verificacao_credenciais_providers(config: &garraia_config::AppConfig) -> Verificacao {
    let mut itens = Vec::new();
    let mut faltando = 0usize;
    for (id, llm) in &config.llm {
        let (origem, resolvida) = match garraia_config::provider_key_env(&llm.provider) {
            _ if PROVEDORES_SEM_CREDENCIAL.contains(&llm.provider.as_str()) => {
                ("sem credencial necessaria".to_string(), true)
            }
            None => (
                "provedor desconhecido do runtime (ignorado no boot)".to_string(),
                false,
            ),
            Some(_) => {
                let fonte = garraia_config::resolve_provider_key_source(
                    &llm.provider,
                    llm.api_key.as_deref(),
                );
                (
                    format!("chave vinda do {}", fonte.label()),
                    fonte.is_resolved(),
                )
            }
        };
        if !resolvida {
            faltando += 1;
        }
        itens.push(serde_json::json!({
            "id": id,
            "provider": llm.provider,
            "origem": origem,
        }));
    }
    let estado = if itens.is_empty() {
        EstadoDiag::NaoConfigurado
    } else if faltando > 0 {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    let detalhe = if itens.is_empty() {
        "nenhum provedor LLM configurado em `llm:`".to_string()
    } else {
        let linhas: Vec<String> = itens
            .iter()
            .filter_map(|i| {
                Some(format!(
                    "{}: {}",
                    i.get("id")?.as_str()?,
                    i.get("origem")?.as_str()?
                ))
            })
            .collect();
        linhas.join("; ")
    };
    Verificacao::nova(
        "credenciais.providers",
        estado,
        serde_json::json!({ "providers": itens }),
        detalhe,
    )
}

/// Slots de credencial por tipo de canal: `(tipo, campo na config, env var)`.
/// A cadeia de resolucao e a MESMA dos `build_*_channels`
/// (vault → config.yml → env), consultada so pela ORIGEM.
const SLOTS_DE_CREDENCIAL: &[(&str, &str, &str)] = &[
    ("telegram", "bot_token", "TELEGRAM_BOT_TOKEN"),
    ("discord", "bot_token", "DISCORD_BOT_TOKEN"),
    ("slack", "bot_token", "SLACK_BOT_TOKEN"),
    ("slack", "app_token", "SLACK_APP_TOKEN"),
    ("whatsapp", "access_token", "WHATSAPP_ACCESS_TOKEN"),
    ("whatsapp", "app_secret", "WHATSAPP_APP_SECRET"),
    ("line", "channel_access_token", "LINE_CHANNEL_ACCESS_TOKEN"),
    ("line", "channel_secret", "LINE_CHANNEL_SECRET"),
    ("teams", "app_secret", "TEAMS_APP_SECRET"),
    (
        "google_chat",
        "service_account_token",
        "GOOGLE_CHAT_SERVICE_ACCOUNT_TOKEN",
    ),
    ("matrix", "access_token", "MATRIX_ACCESS_TOKEN"),
];

/// Uma entrada com a ORIGEM de cada credencial de canal declarada. O valor
/// nunca sai: so `chave vinda do <vault|config.yml|environment variable>`.
fn verificacao_credenciais_canais(config: &garraia_config::AppConfig) -> Verificacao {
    let mut itens = Vec::new();
    let mut faltando = 0usize;
    for (secao, canal) in &config.channels {
        if canal.enabled == Some(false) {
            continue;
        }
        if canal.channel_type == crate::bootstrap::WHATSAPP_LINKED_CONFIG_KEY {
            // A credencial deste canal nao e valor de config: e um blob
            // cifrado no data dir do produto (#1238).
            itens.push(serde_json::json!({
                "canal": canal.channel_type,
                "secao": secao,
                "campo": "sessao",
                "origem": "blob cifrado no data dir do produto",
            }));
            continue;
        }
        for (tipo, campo, env) in SLOTS_DE_CREDENCIAL {
            if *tipo != canal.channel_type {
                continue;
            }
            let valor_na_config = canal.settings.get(*campo).and_then(|v| v.as_str());
            // So o lado da ORIGEM: o valor resolvido (`.1`) e descartado aqui.
            let fonte = garraia_config::resolve_api_key_source(valor_na_config, env, env).0;
            if !fonte.is_resolved() {
                faltando += 1;
            }
            itens.push(serde_json::json!({
                "canal": canal.channel_type,
                "secao": secao,
                "campo": campo,
                "origem": format!("chave vinda do {}", fonte.label()),
            }));
        }
    }
    let estado = if itens.is_empty() {
        EstadoDiag::NaoConfigurado
    } else if faltando > 0 {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    let detalhe = if itens.is_empty() {
        "nenhum canal com credencial declarada".to_string()
    } else if faltando > 0 {
        format!(
            "{} slot(s) de credencial declarado(s), {} sem valor em lugar nenhum (o canal nao \
             sobe)",
            itens.len(),
            faltando
        )
    } else {
        format!(
            "{} slot(s) de credencial resolvido(s) — valores nunca saem daqui",
            itens.len()
        )
    };
    Verificacao::nova(
        "credenciais.canais",
        estado,
        serde_json::json!({ "slots": itens }),
        detalhe,
    )
}

/// O cofre de credenciais: presenca e disponibilidade, nunca o conteudo. O
/// caminho e resolvido pelo mesmo `config_dir` do resto do diagnostico (em
/// producao, `ConfigLoader::default_config_dir()` — a fonte unica).
fn verificacao_credenciais_vault(config_dir: &Path) -> Verificacao {
    let vault = config_dir.join("credentials").join("vault.json");
    let presente = vault.exists();
    let trancado = presente && garraia_config::vault_present_but_locked(&vault);
    let estado = if !presente {
        EstadoDiag::NaoConfigurado
    } else if trancado {
        EstadoDiag::Degradado
    } else {
        EstadoDiag::Ok
    };
    let detalhe = if !presente {
        "sem cofre de credenciais (credentials/vault.json)".to_string()
    } else if trancado {
        "cofre presente mas TRANCADO: sem GARRAIA_VAULT_PASSPHRASE as chaves la guardadas nao \
         resolvem e os provedores/canais nao sobem (fail-closed)"
            .to_string()
    } else {
        "cofre presente e disponivel".to_string()
    };
    Verificacao::nova(
        "credenciais.vault",
        estado,
        serde_json::json!({
            "arquivo": "credentials/vault.json",
            "presente": presente,
            "trancado": trancado,
        }),
        detalhe,
    )
}

/// O segredo de assinatura JWT: so PRESENCA (mesma regra do
/// `/api/diagnostics`), nunca o valor.
fn verificacao_credenciais_jwt() -> Verificacao {
    let configurado = std::env::var("GARRAIA_JWT_SECRET").is_ok()
        || std::env::var("GARRAIA_VAULT_PASSPHRASE").is_ok()
        || std::env::var("GarraIA_VAULT_PASSPHRASE").is_ok();
    Verificacao::nova(
        "credenciais.jwt",
        if configurado {
            EstadoDiag::Ok
        } else {
            EstadoDiag::NaoConfigurado
        },
        serde_json::json!({ "configurado": configurado }),
        if configurado {
            "segredo de assinatura JWT presente (valor mascarado)".to_string()
        } else {
            "sem GARRAIA_JWT_SECRET — /v1/auth/* e /auth/* respondem 503 (opcional num gateway \
             local single-user)"
                .to_string()
        },
    )
}

// ─── 8. logs sanitizados e limitados ────────────────────────────────────────

/// O arquivo de log esta DENTRO do diretorio de configuracao do produto?
/// Canonicaliza os dois lados (um symlink `garraia.log -> /etc/...` nao passa)
/// e qualquer falha de leitura responde `false`: file jail fail-closed.
pub(crate) fn dentro_do_config_dir(config_dir: &Path, arquivo: &Path) -> bool {
    match (
        std::fs::canonicalize(arquivo),
        std::fs::canonicalize(config_dir),
    ) {
        (Ok(a), Ok(d)) => a.starts_with(&d),
        _ => false,
    }
}

/// As ultimas [`ULTIMAS_LINHAS_DE_LOG`] linhas do `garraia.log` do produto,
/// redigidas. Leitura unica e limitada (o mesmo `read_log_tail` do
/// `GET /api/logs`, com teto de bytes), dentro do file jail.
fn verificacao_logs(config_dir: &Path) -> Verificacao {
    let caminho = config_dir.join("garraia.log");
    let vazio = |estado, detalhe: &str| {
        Verificacao::nova(
            "logs.recentes",
            estado,
            serde_json::json!({ "arquivo": "garraia.log", "linhas": 0, "amostras": [] }),
            detalhe,
        )
    };
    if !caminho.exists() {
        return vazio(
            EstadoDiag::NaoConfigurado,
            "nenhum garraia.log ainda (instalacao que nunca gravou log em arquivo)",
        );
    }
    if !dentro_do_config_dir(config_dir, &caminho) {
        return vazio(
            EstadoDiag::Bloqueado,
            "leitura negada: o garraia.log nao resolve dentro do diretorio de configuracao do \
             produto (file jail, fail-closed)",
        );
    }
    match crate::logs_handler::read_log_tail(&caminho, crate::logs_handler::MAX_TAIL_BYTES) {
        Ok(conteudo) => {
            let mut linhas: Vec<String> = conteudo
                .lines()
                .rev()
                .take(ULTIMAS_LINHAS_DE_LOG)
                .map(sanear)
                .collect();
            linhas.reverse();
            let n = linhas.len();
            Verificacao::nova(
                "logs.recentes",
                EstadoDiag::Ok,
                serde_json::json!({ "arquivo": "garraia.log", "linhas": n, "amostras": linhas }),
                format!(
                    "ultimas {n} linha(s) do garraia.log (redigidas: segredos viram [REDACTED], \
                     identificadores longos sao mascarados)"
                ),
            )
        }
        // Fail-closed em qualquer falha de leitura: nada de conteudo
        // reconstruido, nada de "ok" fabricado.
        Err(_) => vazio(
            EstadoDiag::Erro,
            "falha ao ler o garraia.log — fail-closed, nada divulgado",
        ),
    }
}

// ─── 9. componentes/dependencias relevantes ─────────────────────────────────

/// A ponte Node do WhatsApp vinculado, pela MESMA classificacao do
/// `/api/diagnostics` e do `garra whatsapp status`
/// (`whatsapp_linked::health::classify`).
pub(crate) fn estado_da_ponte(
    saude: garraia_channels::whatsapp_linked::health::LinkHealth,
) -> EstadoDiag {
    use garraia_channels::whatsapp_linked::health::LinkHealth as L;
    match saude {
        L::NotLinked => EstadoDiag::NaoConfigurado,
        // Ligou o canal e falta dependencia (`npm ci`): quebrado, nao neutro.
        L::MissingDependencies => EstadoDiag::Erro,
        // Ha sessao e a ponte nao esta conectada: configurado e caido.
        L::BridgeDown => EstadoDiag::Degradado,
        L::Connected | L::Linked => EstadoDiag::Ok,
    }
}

async fn verificacoes_dos_componentes(state: &AppState) -> Vec<Verificacao> {
    vec![
        verificacao_bridge_whatsapp(state),
        verificacao_banco(state).await,
    ]
}

fn verificacao_bridge_whatsapp(state: &AppState) -> Verificacao {
    use garraia_channels::whatsapp_linked::health::LinkHealth as L;

    let (saude, bridge_dir) =
        crate::bootstrap::whatsapp_linked_health(&state.config, &state.whatsapp_linked);
    let estado = estado_da_ponte(saude);
    let nome = match saude {
        L::NotLinked => "not_linked",
        L::MissingDependencies => "missing_dependencies",
        L::BridgeDown => "bridge_down",
        L::Connected => "connected",
        L::Linked => "linked",
    };
    // Presenca fisica da arvore da ponte (`bridge/node_modules`), so metadata:
    // e o "bridge Node do WhatsApp presente?" da #1669.
    let deps = std::fs::metadata(bridge_dir.join("node_modules"))
        .map(|m| m.is_dir())
        .unwrap_or(false);
    let detalhe = match estado {
        EstadoDiag::Ok => "ponte do WhatsApp saudavel".to_string(),
        EstadoDiag::NaoConfigurado => {
            "nenhum aparelho do WhatsApp vinculado (canal opcional)".to_string()
        }
        EstadoDiag::Erro => {
            "sessao vinculada mas a ponte esta SEM dependencias (node_modules ausente — rode \
             `npm ci` no diretorio da ponte)"
                .to_string()
        }
        EstadoDiag::Degradado => "sessao vinculada e a ponte nao esta conectada".to_string(),
        outro => format!("estado da ponte: {outro:?}"),
    };
    Verificacao::nova(
        "componentes.bridge_whatsapp",
        estado,
        serde_json::json!({
            "vinculo": nome,
            "dependencias_instaladas": deps,
        }),
        detalhe,
    )
}

/// O banco de sessoes (`sessions.db`): presente? responde consulta de leitura?
async fn verificacao_banco(state: &AppState) -> Verificacao {
    let Some(store) = &state.session_store else {
        return Verificacao::nova(
            "componentes.banco",
            EstadoDiag::NaoConfigurado,
            serde_json::json!({ "presente": false, "consultavel": false }),
            "sem banco de sessoes configurado (sessoes em memoria)",
        );
    };
    // Sonda de leitura pura (nenhuma escrita): se o banco responde, esta
    // alcancavel.
    let consulta = store
        .lock()
        .await
        .get_sources_for_session("garra-diagnostics-sonda");
    match consulta {
        Ok(_) => Verificacao::nova(
            "componentes.banco",
            EstadoDiag::Ok,
            serde_json::json!({ "presente": true, "consultavel": true }),
            "banco de sessoes alcancavel (consulta de leitura ok)",
        ),
        Err(_) => Verificacao::nova(
            "componentes.banco",
            EstadoDiag::Erro,
            serde_json::json!({ "presente": true, "consultavel": false }),
            "banco de sessoes presente mas nao responde consulta (fail-closed)",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_agents::McpServerState;
    use garraia_channels::ChannelRegistry;
    use garraia_channels::whatsapp_linked::health::LinkHealth;
    use garraia_config::AppConfig;

    fn config_no(dir: &Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    fn state_com(config: AppConfig, dir: &Path) -> Arc<AppState> {
        // Com config dir de teste: o `AppState` provisiona `mcp.json` nele, e
        // sem isto gravaria no config dir do desenvolvedor.
        Arc::new(AppState::with_config_dir(
            config,
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir,
        ))
    }

    fn state(dir: &Path) -> Arc<AppState> {
        state_com(config_no(dir), dir)
    }

    fn tool(st: &Arc<AppState>, dir: &Path) -> GarraDiagnosticsTool {
        GarraDiagnosticsTool::new(st, PushMounted::default(), dir.to_path_buf())
    }

    fn ctx_na_sessao(session_id: &str) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        }
    }

    /// Grava a sessao como o web chat grava antes de um turno: superficie
    /// `web` (do operador) + porta do operador fechada = sessao do operador.
    async fn sessao_do_web(st: &Arc<AppState>, sid: &str) {
        st.hydrate_session_history(sid, Some("web"), None).await;
    }

    /// Executa dentro de um turno NAO restrito (o que o runtime faz em volta
    /// do `execute` de toda tool).
    async fn executar(
        tool: &GarraDiagnosticsTool,
        ctx: &ToolContext,
    ) -> (serde_json::Value, String) {
        let out = garraia_agents::tools::turn_tools::com_restricao_do_turno(
            false,
            tool.execute(ctx, serde_json::json!({})),
        )
        .await
        .expect("executa");
        assert!(!out.is_error, "{}", out.content);
        (
            serde_json::from_str(&out.content).expect("json"),
            out.content,
        )
    }

    fn verificacoes(json: &serde_json::Value) -> Vec<serde_json::Value> {
        json["verificacoes"]
            .as_array()
            .expect("verificacoes e lista")
            .clone()
    }

    fn achar<'a>(json: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
        json["verificacoes"]
            .as_array()?
            .iter()
            .find(|v| v["id"] == id)
    }

    // ─── autorizacao (fail-closed) ─────────────────────────────────────────

    /// A recusa e o MINIMO absoluto: uma entrada `acesso`/`bloqueado` e nada
    /// mais — sem versao, sem canal, sem nome de provedor, sem caminho.
    fn assert_minimo_absoluto(json: &serde_json::Value, texto: &str) {
        let lista = verificacoes(json);
        assert_eq!(lista.len(), 1, "recusa so pode ter UMA entrada: {json}");
        assert_eq!(lista[0]["id"], "acesso", "{json}");
        assert_eq!(lista[0]["estado"], "bloqueado", "{json}");
        for proibido in [
            env!("CARGO_PKG_VERSION"),
            "canais.",
            "mcp.",
            "credenciais",
            "providers",
            "logs",
            "gateway",
            "versao",
        ] {
            assert!(
                !texto.contains(proibido),
                "{proibido} vazou na recusa: {texto}"
            );
        }
    }

    /// Sessao remota (WhatsApp), sessao desconhecida e sessao sem turno
    /// gravado recebem recusa — e nada da instalacao aparece.
    #[tokio::test]
    async fn sessao_nao_operadora_recebe_recusa_e_nada_mais() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state(dir.path());
        let tool = tool(&st, dir.path());
        for sid in [
            "whatsapp-linked-5511987654321@s.whatsapp.net",
            "sessao-desconhecida",
            "mobile-user-1",
        ] {
            let ctx = ctx_na_sessao(sid);
            let (json, texto) = executar(&tool, &ctx).await;
            assert_minimo_absoluto(&json, &texto);
        }
    }

    /// Fora de um turno despachado nao ha evidencia de quem fala: o `None` do
    /// portao conta como restrito (fail-closed), diferente do `garra_status`.
    #[tokio::test]
    async fn fora_de_turno_e_recusa_fail_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let out = tool
            .execute(&ctx_na_sessao("sessao-web"), serde_json::json!({}))
            .await
            .expect("executa");
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_minimo_absoluto(&json, &out.content);
    }

    /// Com o opt-out do #1261 e sem chave, o web chat responde a quem alcanca
    /// a porta: "e o web chat" deixa de provar "e o operador".
    #[tokio::test]
    async fn porta_aberta_sem_chave_restringe_ate_o_web_chat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config_no(dir.path());
        config.gateway.allow_unauthenticated_network_bind = true;
        let st = state_com(config, dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, texto) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        assert_minimo_absoluto(&json, &texto);
    }

    /// O operador, numa superficie local com a porta fechada, ve o relatorio
    /// inteiro — e a chave `diagnostics` do `garra_status` aponta pra ca.
    #[tokio::test]
    async fn operador_local_ve_o_relatorio_inteiro() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, _) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        let lista = verificacoes(&json);
        assert!(lista.len() > 10, "relatorio completo: {json}");
        assert!(achar(&json, "versao.instalada").is_some());
        assert!(achar(&json, "config.efetiva").is_some());
        assert!(achar(&json, "logs.recentes").is_some());
    }

    // ─── sanitizacao (segredos plantados) ──────────────────────────────────

    /// Varredura no padrao da `varredura_de_segredos_plantados` do
    /// `garra_status_tool`: config cheia de segredo plantado, relatorio do
    /// operador inteiro, nenhum valor plantado pode aparecer. O que SAO os
    /// fatos legitimos do relatorio (host/porta do bind, nomes de env var,
    /// origem da credencial) continua dito.
    #[tokio::test]
    async fn varredura_de_segredos_plantados() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config_no(dir.path());
        config.gateway.api_key = Some("gw-chave-plantada-0001".to_string());
        config.channels.insert(
            "telegram".to_string(),
            serde_json::from_value(serde_json::json!({
                "type": "telegram",
                "bot_token": "123456789:TELEGRAM-TOKEN-PLANTADO",
            }))
            .expect("canal"),
        );
        config.llm.insert(
            "openrouter".to_string(),
            garraia_config::LlmProviderConfig {
                provider: "openrouter".to_string(),
                model: None,
                api_key: Some("sk-or-...0002".to_string()),
                base_url: Some("https://usuario:senha@llm.interno.example/v1".to_string()),
                extra: Default::default(),
            },
        );
        config.mcp.insert(
            "segredo".to_string(),
            serde_json::from_value(serde_json::json!({
                "command": "/opt/mcp-plantado/bin",
                "env": { "MCP_TOKEN": "mcp-token-plantado-0003" },
            }))
            .expect("mcp"),
        );
        let st = state_com(config, dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, texto) = executar(&tool, &ctx_na_sessao("sessao-web")).await;

        let plantados = [
            "gw-chave-plantada-0001",
            "TELEGRAM-TOKEN-PLANTADO",
            "123456789",
            "sk-or-...0002",
            "usuario:senha",
            "llm.interno.example",
            "mcp-plantado",
            "mcp-token-plantado-0003",
        ];
        for p in plantados {
            assert!(!texto.contains(p), "{p} vazou: {texto}");
        }
        // O que o relatorio existe para dizer continua dito — so a ORIGEM.
        let providers = achar(&json, "credenciais.providers").expect("providers");
        assert!(
            providers["evidencia"]["providers"]
                .to_string()
                .contains("config.yml"),
            "origem da credencial dita, valor nao: {providers}"
        );
    }

    /// O redator cobre o que os padroes do `garraia_security` reconhecem, e a
    /// mascara pega identificador longo (telefone/LID). Segredos SEM formato
    /// reconhecivel nao tem regex que os diferencie de texto comum — a
    /// garantia deles e nunca entrar no relatorio (teste acima).
    #[test]
    fn sanear_redige_segredos_conhecidos_e_mascara_digitos_longos() {
        // Montado em partes para nenhum literal de segredo viver no fonte.
        let segredo = format!("sk-ant-api{}", "0123456789abcdef012345");
        let texto = format!("falha com {segredo} no numero 5511987654321");
        let limpo = sanear(&texto);
        assert!(!limpo.contains(&segredo), "{limpo}");
        assert!(limpo.contains("[REDACTED]"), "{limpo}");
        assert!(!limpo.contains("5511987654321"), "{limpo}");
        // A mascara do `mascarar_numeros_longos` deixa os 4 ultimos digitos
        // apos `…` (a mesma convencao `phone_last4` dos logs).
        assert!(limpo.contains("…4321"), "{limpo}");
    }

    // ─── logs: jail + sanitizacao + limite ─────────────────────────────────

    /// As amostras de log vem redigidas e limitadas a
    /// [`ULTIMAS_LINHAS_DE_LOG`] linhas.
    #[tokio::test]
    async fn logs_vem_redigidos_e_limitados() {
        let dir = tempfile::tempdir().expect("tempdir");
        let segredo = format!("sk-ant-api{}", "0123456789abcdef012345");
        let mut conteudo = String::new();
        for i in 0..(ULTIMAS_LINHAS_DE_LOG + 10) {
            conteudo.push_str(&format!("linha {i} do daemon\n"));
        }
        conteudo.push_str(&format!("ultimo: {segredo} ligou de 5511987654321\n"));
        std::fs::write(dir.path().join("garraia.log"), conteudo).expect("grava log");

        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, texto) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        let logs = achar(&json, "logs.recentes").expect("logs");
        assert_eq!(logs["estado"], "ok", "{logs}");
        let amostras = logs["evidencia"]["amostras"].as_array().expect("amostras");
        assert_eq!(amostras.len(), ULTIMAS_LINHAS_DE_LOG, "{logs}");
        // A ultima linha (a do segredo) esta na amostra... redigida.
        let ultima = amostras.last().expect("ultima").to_string();
        assert!(ultima.contains("[REDACTED]"), "{ultima}");
        assert!(!texto.contains(&segredo), "{texto}");
        assert!(!texto.contains("5511987654321"), "{texto}");
    }

    /// File jail: um `garraia.log` que resolve PARA FORA do diretorio do
    /// produto (symlink) e negado — `bloqueado`, sem conteudo.
    #[cfg(unix)]
    #[tokio::test]
    async fn log_fora_do_config_dir_e_bloqueado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fora = tempfile::tempdir().expect("fora");
        std::fs::write(fora.path().join("segredo.log"), "conteudo privado").expect("fora");
        std::os::unix::fs::symlink(
            fora.path().join("segredo.log"),
            dir.path().join("garraia.log"),
        )
        .expect("symlink");

        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, texto) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        let logs = achar(&json, "logs.recentes").expect("logs");
        assert_eq!(logs["estado"], "bloqueado", "{logs}");
        assert!(!texto.contains("conteudo privado"), "{texto}");
    }

    /// O guard e fail-closed: arquivo que nao existe, config dir que nao
    /// existe, qualquer duvida — `false`.
    #[test]
    fn dentro_do_config_dir_e_fail_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dentro = dir.path().join("garraia.log");
        std::fs::write(&dentro, "x").expect("grava");
        assert!(dentro_do_config_dir(dir.path(), &dentro));
        assert!(!dentro_do_config_dir(
            dir.path(),
            &dir.path().join("nao-existe.log")
        ));
        assert!(!dentro_do_config_dir(
            &dir.path().join("config-dir-que-nao-existe"),
            &dentro
        ));
        assert!(!dentro_do_config_dir(dir.path(), Path::new("/etc/passwd")));
    }

    // ─── estados diferenciados ─────────────────────────────────────────────

    /// A tabela inteira de estados de canal, escrita: `offline` (configurado e
    /// caido) e `degradado`; `optional` (ninguem ligou) e `nao_configurado`;
    /// `unknown` (defeito de codigo) e `erro`.
    #[test]
    fn estados_de_canal_sao_casos_distintos() {
        assert_eq!(estado_do_canal("telegram", "active"), EstadoDiag::Ok);
        assert_eq!(
            estado_do_canal("telegram", "offline"),
            EstadoDiag::Degradado
        );
        assert_eq!(
            estado_do_canal("telegram", "optional"),
            EstadoDiag::NaoConfigurado
        );
        assert_eq!(estado_do_canal("whatsapp", "unknown"), EstadoDiag::Erro);
        // iMessage fora de macOS nao existe nesta plataforma.
        if cfg!(target_os = "macos") {
            assert_eq!(
                estado_do_canal("imessage", "optional"),
                EstadoDiag::NaoConfigurado
            );
        } else {
            assert_eq!(
                estado_do_canal("imessage", "optional"),
                EstadoDiag::NaoSuportado
            );
            assert_eq!(
                estado_do_canal("imessage", "active"),
                EstadoDiag::NaoSuportado
            );
        }
    }

    /// A tabela de estados do MCP: ausente e assunto de outra entrada
    /// (`nao_configurado`); conectado `ok`; retentando `degradado`; esgotado
    /// `erro`.
    #[test]
    fn estados_de_mcp_sao_casos_distintos() {
        assert_eq!(estado_do_mcp(McpServerState::Connected), EstadoDiag::Ok);
        assert_eq!(
            estado_do_mcp(McpServerState::Disconnected),
            EstadoDiag::Degradado
        );
        assert_eq!(
            estado_do_mcp(McpServerState::Retrying),
            EstadoDiag::Degradado
        );
        assert_eq!(estado_do_mcp(McpServerState::Failed), EstadoDiag::Erro);
    }

    /// A tabela da ponte do WhatsApp: nao vinculado e `nao_configurado`;
    /// sem dependencia e `erro`; ponte caida com sessao e `degradado`.
    #[test]
    fn estados_da_ponte_sao_casos_distintos() {
        assert_eq!(
            estado_da_ponte(LinkHealth::NotLinked),
            EstadoDiag::NaoConfigurado
        );
        assert_eq!(
            estado_da_ponte(LinkHealth::MissingDependencies),
            EstadoDiag::Erro
        );
        assert_eq!(
            estado_da_ponte(LinkHealth::BridgeDown),
            EstadoDiag::Degradado
        );
        assert_eq!(estado_da_ponte(LinkHealth::Connected), EstadoDiag::Ok);
        assert_eq!(estado_da_ponte(LinkHealth::Linked), EstadoDiag::Ok);
    }

    /// Integracao dos estados diferenciados no relatorio de verdade: MCP
    /// ausente vira `nao_configurado`; canal DECLARADO mas fora do ar vira
    /// `degradado`; canal imessage em plataforma sem suporte vira
    /// `nao_suportado`; e recusa (consulta negada) ja foi provada acima como
    /// `bloqueado`.
    #[tokio::test]
    async fn relatorio_diferencia_nao_configurado_degradado_e_nao_suportado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config_no(dir.path());
        config.channels.insert(
            "meu-telegram".to_string(),
            serde_json::from_value(serde_json::json!({
                "type": "telegram",
                "bot_token": "123456789:TELEGRAM-TOKEN-PLANTADO",
            }))
            .expect("canal"),
        );
        config.channels.insert(
            "meu-imessage".to_string(),
            serde_json::from_value(serde_json::json!({ "type": "imessage" })).expect("canal"),
        );
        let st = state_com(config, dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, _) = executar(&tool, &ctx_na_sessao("sessao-web")).await;

        // MCP ausente: ninguem configurou servidor nenhum.
        assert_eq!(
            achar(&json, "mcp.servidores").expect("mcp")["estado"],
            "nao_configurado"
        );
        // Telegram declarado mas sem adapter no ar: configurado e caido.
        assert_eq!(
            achar(&json, "canais.telegram").expect("telegram")["estado"],
            "degradado"
        );
        // iMessage fora de macOS.
        let imessage = achar(&json, "canais.imessage").expect("imessage");
        if cfg!(target_os = "macos") {
            assert_eq!(imessage["estado"], "nao_configurado");
        } else {
            assert_eq!(imessage["estado"], "nao_suportado");
        }
    }

    // ─── regressao: instalacao saudavel nao muda ───────────────────────────

    /// Instalacao nova e saudavel: nenhuma entrada em `erro` ou `bloqueado`, o
    /// conjunto de ids e estavel e os neutros caem nos estados certos.
    #[tokio::test]
    async fn saudavel_nao_muda() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, _) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        let lista = verificacoes(&json);

        let ids: Vec<String> = lista
            .iter()
            .map(|v| v["id"].as_str().expect("id").to_string())
            .collect();
        let esperados = [
            "versao.instalada",
            "gateway.estado",
            "canais.superficies",
            "canais.telegram",
            "canais.discord",
            "canais.slack",
            "canais.whatsapp",
            "canais.whatsapp_linked",
            "canais.imessage",
            "canais.google_chat",
            "canais.teams",
            "canais.line",
            "canais.irc",
            "canais.signal",
            "canais.matrix",
            "canais.openclaw",
            "canais.whatsapp_linked.cadencia",
            "mcp.servidores",
            "ferramentas.disponiveis",
            "capacidades.estado",
            "config.efetiva",
            "credenciais.providers",
            "credenciais.canais",
            "credenciais.vault",
            "credenciais.jwt",
            "logs.recentes",
            "componentes.bridge_whatsapp",
            "componentes.banco",
        ];
        assert_eq!(ids, esperados, "conjunto/ordem dos ids mudou");

        for v in &lista {
            let estado = v["estado"].as_str().expect("estado");
            assert!(
                !matches!(estado, "erro" | "bloqueado"),
                "instalacao saudavel sem erro/bloqueado: {v}"
            );
        }
        // Os neutros da instalacao nova, fixos: sao o "nao muda" do contrato.
        for id in [
            "mcp.servidores",
            "credenciais.providers",
            "credenciais.canais",
            "credenciais.vault",
            "credenciais.jwt",
            "logs.recentes",
            "componentes.bridge_whatsapp",
            "componentes.banco",
        ] {
            assert_eq!(
                achar(&json, id).expect(id)["estado"],
                "nao_configurado",
                "{id}"
            );
        }
        assert_eq!(achar(&json, "versao.instalada").expect("v")["estado"], "ok");
        assert_eq!(achar(&json, "gateway.estado").expect("g")["estado"], "ok");
    }

    // ─── formato + fiação ──────────────────────────────────────────────────

    /// Todo item do contrato da #1669: exatamente `{id, estado, evidencia,
    /// detalhe}`, `estado` na vocabulario fechada e ids sem repeticao.
    #[tokio::test]
    async fn formato_da_saida_respeita_o_contrato() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state(dir.path());
        sessao_do_web(&st, "sessao-web").await;
        let tool = tool(&st, dir.path());
        let (json, _) = executar(&tool, &ctx_na_sessao("sessao-web")).await;
        let lista = verificacoes(&json);
        let mut ids = std::collections::HashSet::new();
        for v in &lista {
            let obj = v.as_object().expect("entrada e objeto");
            // A ordem das chaves nao e contrato (o `serde_json::Map` pode
            // ordenar); o conjunto e.
            let mut chaves: Vec<&str> = obj.keys().map(String::as_str).collect();
            chaves.sort_unstable();
            assert_eq!(chaves, ["detalhe", "estado", "evidencia", "id"], "{v}");
            assert!(
                matches!(
                    v["estado"].as_str().expect("estado"),
                    "ok" | "erro" | "nao_configurado" | "bloqueado" | "degradado" | "nao_suportado"
                ),
                "{v}"
            );
            assert!(ids.insert(v["id"].as_str().expect("id").to_string()), "{v}");
        }
    }

    /// Fiação: o `server.rs` registra a tool DEPOIS de montar os canais push
    /// (elas precisam das contagens) e exatamente uma vez — a mesma janela do
    /// `garra_status`.
    #[test]
    fn server_registra_a_tool_depois_dos_canais_push() {
        let fonte = include_str!("../server.rs");
        let push = fonte
            .find("let push_channels = crate::push_channels::PushChannelStates {")
            .expect("o server monta os canais push numa variavel");
        assert_eq!(
            fonte.matches("GarraDiagnosticsTool::new(").count(),
            1,
            "um registro so"
        );
        let registro = fonte
            .find("GarraDiagnosticsTool::new(")
            .expect("o server registra garra_diagnostics");
        assert!(registro > push, "registro antes dos canais push");
    }

    /// A nota de orientacao (que um `garra_status` ou console pode publicar)
    /// nomeia a tool certa e o contrato dela, e vive AQUI — uma fonte so.
    #[test]
    fn nota_diagnostics_nomeia_a_tool_e_o_contrato() {
        let nota = nota_diagnostics();
        assert!(nota.contains("garra_diagnostics"), "{nota}");
        assert!(nota.contains("operator-only"), "{nota}");
        assert!(nota.contains("refusal"), "{nota}");
    }
}
