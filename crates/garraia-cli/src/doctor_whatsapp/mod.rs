//! `garraia doctor whatsapp` — o caminho do WhatsApp pessoal de ponta a ponta,
//! numa passada (#1419).
//!
//! # O motor mora no gateway (#1420)
//!
//! A tabela (fatos → linhas), os tipos, o agregado e o exit code vivem em
//! [`garraia_gateway::bootstrap::whatsapp_linked_doctor`], porque o card
//! "Test WhatsApp" do Web Console roda o MESMO motor — e a direcao de
//! dependencia permitida e gateway → CLI, nunca o contrario. O que fica aqui e
//! o que so a CLI faz:
//!
//! - **a colheita** (`colheita.rs`): disco, config, processo e o
//!   `/api/diagnostics` do gateway vivo por HTTP (com a credencial do gateway,
//!   quando a config a tem), mais o probe TCP dos daemons locais keyless;
//! - **a saida**: a tabela humana, o `--json` e os exit codes — que nao mudam.
//!
//! Com o gateway de pe, o que so ele sabe (ponte conectada, provider
//! registrado) vem do proprio `/api/diagnostics`; sem ele, a CLI diz o que da
//! para saber do disco e da config — e diz que nao sabe o resto, em vez de
//! inventar.
//!
//! Segredos e identidades nunca saem: contagens de autorizados e donos (nunca
//! numeros ou LIDs), a origem da chave (nunca a chave), nomes de servidor MCP
//! e de provider (nunca URL com credencial). O vocabulario de status e o do
//! `/api/diagnostics` (`ok` / `warning` / `error` / `not_configured`), para o
//! `--json` ser lido pelo mesmo consumidor.

use garraia_gateway::bootstrap::whatsapp_linked_doctor as doctor;

use crate::whatsapp::{Context, Lang, t};

mod colheita;
#[cfg(test)]
mod tests;

use colheita::colher;
use doctor::{Linha, Semaforo, agregado, classificar, exit_code};

/// O `Lang` da CLI (detectado do ambiente) e o da tabela sao a mesma coisa
/// com dois nomes; a conversao existe para a CLI seguir dona do seu.
impl From<Lang> for doctor::Lang {
    fn from(lang: Lang) -> Self {
        match lang {
            Lang::Pt => doctor::Lang::Pt,
            Lang::En => doctor::Lang::En,
        }
    }
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

/// O payload do `--json`. `report` e o MESMO shape que o
/// `GET /admin/api/whatsapp/doctor` devolve (menos o `lang`), por construcao:
/// as linhas sao o mesmo tipo.
fn payload_json(linhas: &[Linha], code: i32) -> serde_json::Value {
    serde_json::json!({
        "ok": code == 0,
        "exit_code": code,
        "report": {
            "status": agregado(linhas),
            "version": env!("CARGO_PKG_VERSION"),
            "checks": linhas,
        },
    })
}

/// Ponto de entrada do `garraia doctor whatsapp`. Devolve o exit code.
pub fn run(json: bool, strict: bool) -> anyhow::Result<i32> {
    let ctx = Context::from_env();
    let fatos = colher(&ctx);
    let bin = crate::binario::nome();
    let linhas = classificar(&fatos, ctx.lang.into(), &bin);
    let code = exit_code(&linhas, strict);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload_json(&linhas, code))?
        );
    } else {
        imprimir(&linhas, ctx.lang, ctx.unicode, code);
    }
    Ok(code)
}
