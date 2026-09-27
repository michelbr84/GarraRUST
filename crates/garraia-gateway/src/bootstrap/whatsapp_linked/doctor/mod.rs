//! O motor do `doctor whatsapp` (#1419, #1420): o caminho do WhatsApp pessoal
//! de ponta a ponta, numa passada — fatos → linhas, puro.
//!
//! # Um motor, dois consumidores
//!
//! O `garraia doctor whatsapp` (CLI) e o card "Test WhatsApp" do Web Console
//! (`GET /admin/api/whatsapp/doctor`) rodam **esta** tabela. O que difere e a
//! colheita: a CLI le disco, config e processo e pergunta ao gateway vivo pelo
//! `/api/diagnostics` por HTTP; o gateway monta os mesmos [`Fatos`] em
//! processo (ele sabe a view da ponte, e o `/api/diagnostics` e uma chamada
//! de funcao). A classificacao ([`classificar`]), o agregado e o exit code sao
//! um so — e por isso o console nunca diz "verde" onde a CLI diz "vermelho".
//!
//! Cada linha vem da MESMA fonte que o `whatsapp status`, o boot e o
//! `/api/diagnostics` ja usam — a classificacao `LinkHealth`, o leitor de
//! acesso, `raizes_das_file_tools`, o perfil de execucao, o `ToolGate` do
//! piso. O que so o gateway sabe (ponte conectada, provider registrado) vem
//! do `/api/diagnostics`; sem ele, a linha diz que nao sabe, em vez de
//! inventar.
//!
//! # Segredos e identidades nunca saem
//!
//! Contagens de autorizados e donos (nunca numeros ou LIDs), a origem da
//! chave (nunca a chave), nomes de servidor MCP e de provider (nunca URL com
//! credencial). O vocabulario de status e o do `/api/diagnostics` (`ok` /
//! `warning` / `error` / `not_configured`), para o `--json` da CLI e a
//! resposta do console serem lidos pelo mesmo consumidor.
//!
//! # Idioma
//!
//! A CLI tem o seu proprio `Lang` (detectado do ambiente); o console manda
//! `?lang=` pelo idioma do navegador. Os dois convergem no [`Lang`] daqui,
//! que e o unico que a tabela conhece.
//!
//! Tres arquivos, pelo teto de 700 linhas do Quality Ratchet: aqui os tipos,
//! o idioma e a leitura pura da config; `tabela.rs` a classificacao;
//! `tests.rs` a tabela testada linha a linha.

use std::path::PathBuf;

use garraia_agents::modes::{ModeEngine, ToolGate};
use garraia_channels::whatsapp_linked::health::LinkHealth;
use garraia_config::AppConfig;
use serde::Serialize;

mod tabela;
#[cfg(test)]
mod tests;

pub use tabela::classificar;

/// Tudo verde (ou neutro).
pub const EX_OK: i32 = 0;
/// Aviso sob `--strict` — o mesmo codigo do `config check` e do `doctor`.
pub const EX_CONFIG: i32 = 2;
/// Algo vermelho: o canal nao funciona como esta (sysexits EX_UNAVAILABLE).
pub const EX_UNAVAILABLE: i32 = 69;

// ---------------------------------------------------------------------------
// Idioma
// ---------------------------------------------------------------------------

/// Idioma das frases do relatorio. pt-BR e o default do projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Pt,
    En,
}

impl Lang {
    /// `en`, `en-US`, `EN_gb`… viram [`Lang::En`]; qualquer outra coisa (ou
    /// vazio) e pt-BR. E a regra do `Lang::detect` da CLI, aplicada a um
    /// valor que veio de fora (`?lang=`, `navigator.language`).
    pub fn parse(valor: &str) -> Self {
        if valor.trim().to_ascii_lowercase().starts_with("en") {
            Lang::En
        } else {
            Lang::Pt
        }
    }

    /// A grafia que a resposta HTTP devolve.
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Pt => "pt",
            Lang::En => "en",
        }
    }
}

/// Escolhe entre as duas versoes de uma frase.
pub fn t(lang: Lang, pt: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Pt => pt,
        Lang::En => en,
    }
}

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

/// O semaforo de uma linha. Mesma grafia serializada do `/api/diagnostics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Semaforo {
    Ok,
    Warning,
    Error,
    NotConfigured,
}

impl Semaforo {
    /// A mesma grafia que o `Serialize` emite — para quem compara sem
    /// serializar.
    pub fn as_str(self) -> &'static str {
        match self {
            Semaforo::Ok => "ok",
            Semaforo::Warning => "warning",
            Semaforo::Error => "error",
            Semaforo::NotConfigured => "not_configured",
        }
    }
}

/// Uma linha do relatorio. O shape serializado (`id`, `status`, `detail`,
/// `next_step` so quando ha) e o contrato do `--json` da CLI **e** do
/// `checks` do console — o mesmo, por construcao.
#[derive(Debug, Clone, Serialize)]
pub struct Linha {
    pub id: &'static str,
    pub status: Semaforo,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

impl Linha {
    pub(crate) fn nova(
        id: &'static str,
        status: Semaforo,
        detail: String,
        next_step: Option<String>,
    ) -> Self {
        Self {
            id,
            status,
            detail,
            next_step,
        }
    }
}

/// O que ha em disco sobre o vinculo.
#[derive(Debug, Clone)]
pub enum Sessao {
    /// O diretorio da sessao nao abriu (permissao, disco): nada mais e afirmavel.
    Indisponivel(String),
    Lida {
        saude: LinkHealth,
        /// `None` quando nao ha sessao (nada a abrir).
        chave: Option<Chave>,
    },
}

impl Default for Sessao {
    fn default() -> Self {
        Sessao::Lida {
            saude: LinkHealth::NotLinked,
            chave: None,
        }
    }
}

/// A chave da sessao: de onde veio e se abre o blob de verdade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chave {
    /// Derivada da passphrase do cofre (nunca toca o disco).
    pub do_cofre: bool,
    /// `store.load(&key)` abriu — o unico teste que prova que a passphrase
    /// e a certa.
    pub legivel: bool,
}

/// De onde vem as raizes das file tools (o mesmo enum do boot, achatado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Raizes {
    Declaradas(usize),
    #[default]
    WorkspacePadrao,
    SomenteSessao,
}

/// O que o piso do remetente comum (`search`) enxerga de um servidor MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibilidade {
    /// `servidor/*` na `allowed`: toda ferramenta dele.
    Inteiro,
    /// So operacoes nomeadas (`*/read_file`, …): a contagem delas.
    SoOperacoes(usize),
    /// Nenhuma entrada cobre o servidor: o modelo nem o ve.
    Escondido,
}

#[derive(Debug, Clone)]
pub struct ServidorMcp {
    pub nome: String,
    pub visibilidade: Visibilidade,
}

#[derive(Debug, Clone)]
pub struct Provedor {
    pub nome: String,
    pub tipo: String,
    pub keyless: bool,
    /// So para daemon local keyless: o probe TCP respondeu? `None` quando
    /// ninguem sondou — o gateway nao sonda (ele julga o provider pelo que
    /// registrou); a CLI preenche depois de [`fatos_da_config`].
    pub alcancavel: Option<bool>,
}

/// O que a config diz. `None` no [`Fatos`] quando ela nao carregou.
#[derive(Debug, Clone, Default)]
pub struct ConfigFatos {
    pub canal_ligado: bool,
    pub autorizados: usize,
    pub donos: usize,
    pub perfil: String,
    pub origem: String,
    pub isolado: bool,
    pub piso_do_dono: String,
    pub raizes: Raizes,
    pub mcp: Vec<ServidorMcp>,
    pub provedores: Vec<Provedor>,
    pub provedor_padrao: Option<String>,
}

/// Uma linha do `/api/diagnostics` do gateway vivo, como veio.
#[derive(Debug, Clone)]
pub struct LinhaViva {
    pub id: String,
    pub status: String,
    pub detail: String,
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Gateway {
    /// Pid do daemon quando o pidfile aponta para um processo vivo (CLI), ou
    /// o do proprio processo (console).
    pub pid: Option<u32>,
    pub ouvindo: bool,
    pub host: String,
    pub porta: u16,
    /// As linhas do `/api/diagnostics`, quando o gateway respondeu.
    pub ao_vivo: Vec<LinhaViva>,
    /// O gateway respondeu 401: tem chave de API e a CLI nao a mandou.
    /// Um "nao sei" diferente do "nao respondeu" — pede a chave, nao o log.
    pub recusou_credencial: bool,
}

/// Os fatos, colhidos uma vez; a classificacao ([`classificar`]) e pura.
#[derive(Debug, Clone, Default)]
pub struct Fatos {
    pub sessao: Sessao,
    pub bridge_dir: PathBuf,
    pub config: Option<ConfigFatos>,
    pub gateway: Gateway,
}

// ---------------------------------------------------------------------------
// A config, pelos MESMOS leitores do boot
// ---------------------------------------------------------------------------

/// O recorte da config que a tabela precisa — pelos MESMOS leitores do boot
/// (`settings_from_config`, `raizes_das_file_tools`, o `ToolGate` do piso).
///
/// Sem probe de rede: `alcancavel` sai `None` em todo provider, e a CLI
/// sobrepoe o resultado do probe local dela. O unico toque em disco e o
/// `mcp.json` do provisionamento (`McpPersistenceService::with_default_path`),
/// porque a lista de servidores declarados e a uniao dele com a secao `mcp:`
/// do config.yml — exatamente o que o boot spawna. Nomes, nunca args (o
/// `filesystem` leva caminhos do host) e nunca env (pode carregar token).
pub fn fatos_da_config(config: &AppConfig) -> ConfigFatos {
    use crate::bootstrap::FonteDasRaizesDasFileTools as Fonte;

    let settings = super::settings_from_config(config);
    let perfil = config.execution.perfil();
    let piso_do_dono = settings.modo_padrao_efetivo(perfil);
    let raizes = {
        let r = crate::bootstrap::raizes_das_file_tools(config);
        match r.fonte {
            Fonte::Declaradas => Raizes::Declaradas(r.jail.roots().len()),
            Fonte::WorkspacePadrao => Raizes::WorkspacePadrao,
            Fonte::SomenteSessao => Raizes::SomenteSessao,
        }
    };

    // Servidores MCP declarados: a secao `mcp:` do config.yml (que vence) e
    // o mcp.json do provisionamento.
    let mut nomes: Vec<String> = config.mcp.keys().cloned().collect();
    if let Ok(persistido) =
        crate::mcp::persistence::McpPersistenceService::with_default_path().load()
    {
        nomes.extend(persistido.mcp_servers.keys().cloned());
    }
    nomes.sort();
    nomes.dedup();
    let mcp = visibilidade_no_piso(nomes);

    let mut nomes_llm: Vec<&str> = config.llm.keys().map(String::as_str).collect();
    nomes_llm.sort_unstable();
    let provedores = nomes_llm
        .into_iter()
        .filter_map(|nome| {
            let entrada = config.llm.get(nome)?;
            let tipo = entrada.provider.clone();
            Some(Provedor {
                nome: nome.to_string(),
                keyless: provider_keyless(&tipo),
                tipo,
                alcancavel: None,
            })
        })
        .collect();
    let provedor_padrao =
        config.agent.default_provider.clone().filter(|d| {
            config.llm.contains_key(d) || config.llm.values().any(|e| e.provider == *d)
        });

    ConfigFatos {
        canal_ligado: settings.enabled,
        autorizados: settings.autorizados(),
        donos: settings.donos(),
        perfil: perfil.to_string(),
        origem: config.execution.origem().to_string(),
        isolado: perfil.is_isolated_pod(),
        piso_do_dono,
        raizes,
        mcp,
        provedores,
        provedor_padrao,
    }
}

/// Um provider e "keyless" quando e um daemon local sem credencial (`ollama`,
/// `llamacpp`) — a mesma tabela do `doctor` geral: quem tem env de chave na
/// tabela de `provider_key_env` precisa de credencial; quem nao tem e nao e
/// daemon local (`echo`, tipos desconhecidos) tambem conta como "precisa",
/// e o `config check` e quem reclama do desconhecido.
fn provider_keyless(tipo: &str) -> bool {
    garraia_config::provider_key_env(tipo).is_none() && matches!(tipo, "ollama" | "llamacpp")
}

/// O que o piso do remetente comum (`search`) enxerga de cada servidor.
///
/// As entradas `*/<operacao>` do piso sao as operacoes de leitura do
/// `@modelcontextprotocol/server-filesystem` (#1384): valem para qualquer
/// servidor que TENHA essas operacoes, e sem inventario ao vivo so o
/// `filesystem` e afirmavel. Outro servidor sem `nome/*` nem entrada exata
/// e, na pratica, escondido — e e isso que o operador precisa ler.
fn visibilidade_no_piso(nomes: Vec<String>) -> Vec<ServidorMcp> {
    let allowed = ModeEngine::new()
        .get_profile("search")
        .map(|p| p.tool_policy.allowed.clone())
        .unwrap_or_default();
    let operacoes = allowed
        .iter()
        .filter(|e| ToolGate::operacao_em_qualquer_servidor(e).is_some())
        .count();
    nomes
        .into_iter()
        .map(|nome| {
            let prefixo = format!("{nome}__");
            let inteiro = allowed
                .iter()
                .any(|e| ToolGate::prefixo_de_servidor(e).as_deref() == Some(prefixo.as_str()));
            let exatas = allowed.iter().filter(|e| e.starts_with(&prefixo)).count();
            let nomeadas = if nome == "filesystem" { operacoes } else { 0 };
            let visibilidade = if inteiro {
                Visibilidade::Inteiro
            } else if exatas + nomeadas > 0 {
                Visibilidade::SoOperacoes(exatas + nomeadas)
            } else {
                Visibilidade::Escondido
            };
            ServidorMcp { nome, visibilidade }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Agregado e exit code
// ---------------------------------------------------------------------------

/// `error` → 69; `warning` so conta sob `--strict` (2); o resto e 0.
pub fn exit_code(linhas: &[Linha], strict: bool) -> i32 {
    if linhas.iter().any(|l| l.status == Semaforo::Error) {
        EX_UNAVAILABLE
    } else if strict && linhas.iter().any(|l| l.status == Semaforo::Warning) {
        EX_CONFIG
    } else {
        EX_OK
    }
}

/// O agregado, no vocabulario do `/api/diagnostics`.
pub fn agregado(linhas: &[Linha]) -> &'static str {
    if linhas.iter().any(|l| l.status == Semaforo::Error) {
        "error"
    } else if linhas.iter().any(|l| l.status == Semaforo::Warning) {
        "warning"
    } else {
        "ok"
    }
}
