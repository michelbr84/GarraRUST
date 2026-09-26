//! `garraia doctor whatsapp` — o caminho do WhatsApp pessoal de ponta a ponta,
//! numa passada (#1419).
//!
//! Um pipeline, dois consumidores: cada linha vem da MESMA fonte que o
//! `whatsapp status`, o boot e o `/api/diagnostics` ja usam — a classificacao
//! `LinkHealth`, o leitor de acesso, `raizes_das_file_tools`, o perfil de
//! execucao, o `ToolGate` do piso. Com o gateway de pe, o que so ele sabe
//! (ponte conectada, provider registrado) vem do proprio `/api/diagnostics`;
//! sem ele, a CLI diz o que da para saber do disco e da config — e diz que
//! nao sabe o resto, em vez de inventar.
//!
//! Segredos e identidades nunca saem: contagens de autorizados e donos (nunca
//! numeros ou LIDs), a origem da chave (nunca a chave), nomes de servidor MCP
//! e de provider (nunca URL com credencial). O vocabulario de status e o do
//! `/api/diagnostics` (`ok` / `warning` / `error` / `not_configured`), para o
//! `--json` ser lido pelo mesmo consumidor.
//!
//! Um diretorio, quatro arquivos, pelo teto de 700 linhas do Quality Ratchet:
//! aqui os tipos e a saida; `tabela.rs` a classificacao pura; `colheita.rs`
//! o I/O (disco, config, processo, `/api/diagnostics`); `tests.rs` a tabela
//! testada linha a linha.

use std::path::PathBuf;

use garraia_channels::whatsapp_linked::health::LinkHealth;
use serde::Serialize;

use crate::whatsapp::{Context, Lang, t};

mod colheita;
mod tabela;
#[cfg(test)]
mod tests;

use colheita::colher;
pub(crate) use tabela::{agregado, classificar, exit_code};

const EX_OK: i32 = 0;
/// Aviso sob `--strict` — o mesmo codigo do `config check` e do `doctor`.
const EX_CONFIG: i32 = 2;
/// Algo vermelho: o canal nao funciona como esta (sysexits EX_UNAVAILABLE).
const EX_UNAVAILABLE: i32 = 69;

/// O semaforo de uma linha. Mesma grafia serializada do `/api/diagnostics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Semaforo {
    Ok,
    Warning,
    Error,
    NotConfigured,
}

/// Uma linha do relatorio.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Linha {
    pub id: &'static str,
    pub status: Semaforo,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

/// O que ha em disco sobre o vinculo.
#[derive(Debug, Clone)]
pub(crate) enum Sessao {
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
pub(crate) struct Chave {
    /// Derivada da passphrase do cofre (nunca toca o disco).
    pub do_cofre: bool,
    /// `store.load(&key)` abriu — o unico teste que prova que a passphrase
    /// e a certa.
    pub legivel: bool,
}

/// De onde vem as raizes das file tools (o mesmo enum do boot, achatado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Raizes {
    Declaradas(usize),
    #[default]
    WorkspacePadrao,
    SomenteSessao,
}

/// O que o piso do remetente comum (`search`) enxerga de um servidor MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibilidade {
    /// `servidor/*` na `allowed`: toda ferramenta dele.
    Inteiro,
    /// So operacoes nomeadas (`*/read_file`, …): a contagem delas.
    SoOperacoes(usize),
    /// Nenhuma entrada cobre o servidor: o modelo nem o ve.
    Escondido,
}

#[derive(Debug, Clone)]
pub(crate) struct ServidorMcp {
    pub nome: String,
    pub visibilidade: Visibilidade,
}

#[derive(Debug, Clone)]
pub(crate) struct Provedor {
    pub nome: String,
    pub tipo: String,
    pub keyless: bool,
    /// So para daemon local keyless: o probe TCP respondeu?
    pub alcancavel: Option<bool>,
}

/// O que a config diz. `None` no [`Fatos`] quando ela nao carregou.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigFatos {
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
pub(crate) struct LinhaViva {
    pub id: String,
    pub status: String,
    pub detail: String,
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Gateway {
    /// Pid do daemon quando o pidfile aponta para um processo vivo.
    pub pid: Option<u32>,
    pub ouvindo: bool,
    pub host: String,
    pub porta: u16,
    /// As linhas do `/api/diagnostics`, quando o gateway respondeu.
    pub ao_vivo: Vec<LinhaViva>,
    /// O gateway respondeu 401: tem chave de API e esta CLI nao a mandou.
    /// Um "nao sei" diferente do "nao respondeu" — pede a chave, nao o log.
    pub recusou_credencial: bool,
}

/// Os fatos, colhidos uma vez; a classificacao ([`classificar`]) e pura.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fatos {
    pub sessao: Sessao,
    pub bridge_dir: PathBuf,
    pub config: Option<ConfigFatos>,
    pub gateway: Gateway,
}

// ---------------------------------------------------------------------------
// Saida
// ---------------------------------------------------------------------------

fn simbolo(status: Semaforo, unicode: bool) -> &'static str {
    match (status, unicode) {
        (Semaforo::Ok, true) => "✔",
        (Semaforo::Warning, true) => "⚠",
        (Semaforo::Error, true) => "✖",
        (Semaforo::NotConfigured, true) => "○",
        (Semaforo::Ok, false) => "OK ",
        (Semaforo::Warning, false) => "!! ",
        (Semaforo::Error, false) => "XX ",
        (Semaforo::NotConfigured, false) => "-- ",
    }
}

fn imprimir(linhas: &[Linha], lang: Lang, unicode: bool, code: i32) {
    println!("🩺 GarraIA doctor whatsapp v{}", env!("CARGO_PKG_VERSION"));
    for l in linhas {
        println!("  {} {:<22} {}", simbolo(l.status, unicode), l.id, l.detail);
        if let Some(p) = &l.next_step {
            println!("        → {p}");
        }
    }
    println!();
    println!(
        "{} {} (exit {code})",
        t(lang, "Resumo:", "Summary:"),
        agregado(linhas)
    );
}

/// Ponto de entrada do `garraia doctor whatsapp`. Devolve o exit code.
pub fn run(json: bool, strict: bool) -> anyhow::Result<i32> {
    let ctx = Context::from_env();
    let fatos = colher(&ctx);
    let bin = crate::binario::nome();
    let linhas = classificar(&fatos, ctx.lang, &bin);
    let code = exit_code(&linhas, strict);
    if json {
        let payload = serde_json::json!({
            "ok": code == 0,
            "exit_code": code,
            "report": {
                "status": agregado(&linhas),
                "version": env!("CARGO_PKG_VERSION"),
                "checks": linhas,
            },
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        imprimir(&linhas, ctx.lang, ctx.unicode, code);
    }
    Ok(code)
}
