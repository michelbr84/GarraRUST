//! A fiacao do boot gate do #1247 na CLI.
//!
//! `garraia start`, `restart` e `start -d` chamam [`rodar`] uma vez, logo
//! depois do `ConfigLoader::load()` e ANTES de qualquer efeito do boot (fork,
//! PID file, stop do daemon atual, bind). A decisao e a de
//! `garraia_config::boot_gate`; aqui so mora o que imprime:
//!
//! - recusa: a mensagem vai para stderr e o processo sai com `EX_CONFIG` (78)
//!   — antes do fork, entao `start -d` e o gerente de servico veem a falha;
//! - `start -d`: os achados vao para stderr antes do fork (depois dele o
//!   tracing aponta para o arquivo de log, invisivel ao terminal) e tambem
//!   para o log do daemon, via [`logar`];
//! - foreground: [`logar`] depois do `init_tracing`, uma linha por achado.

use std::sync::OnceLock;

use garraia_config::Severity;
use garraia_config::boot_gate::{
    EX_CONFIG, RelatorioDoBoot, Veredito, avaliar, escape_do_ambiente,
};

/// O relatorio deste boot, calculado uma vez. `OnceLock` sobrevive ao fork do
/// daemon (a memoria e copiada), entao o filho loga o mesmo relatorio.
static RELATORIO: OnceLock<RelatorioDoBoot> = OnceLock::new();

/// Roda o check, decide, e recusa o boot quando for o caso.
///
/// `stderr` pede os achados tambem em stderr (modo daemon, antes do fork).
pub fn rodar(
    loader: &garraia_config::ConfigLoader,
    config: &garraia_config::AppConfig,
    stderr: bool,
) {
    let check = garraia_config::run_check(loader, config);
    let relatorio = avaliar(&check, escape_do_ambiente());
    let bin = crate::binario::nome();
    if relatorio.veredito == Veredito::Recusa {
        eprintln!("{}", relatorio.mensagem_de_recusa(&bin));
        std::process::exit(EX_CONFIG);
    }
    if stderr {
        for (_, linha) in relatorio.linhas(&bin) {
            eprintln!("{linha}");
        }
    }
    let _ = RELATORIO.set(relatorio);
}

/// Loga o relatorio (uma vez por processo) no subscriber de tracing ja
/// iniciado: `error!` para Error, `warn!` para Warning.
pub fn logar() {
    static LOGADO: OnceLock<()> = OnceLock::new();
    if LOGADO.set(()).is_err() {
        return;
    }
    let Some(relatorio) = RELATORIO.get() else {
        return;
    };
    for (sev, linha) in relatorio.linhas(&crate::binario::nome()) {
        match sev {
            Severity::Error => tracing::error!("{linha}"),
            Severity::Warning => tracing::warn!("{linha}"),
        }
    }
}
