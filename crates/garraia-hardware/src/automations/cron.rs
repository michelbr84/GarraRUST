//! Cron para os gatilhos de automação — casca fina sobre [`croner`], o
//! mesmo parser que `garraia-db` já usa para recorrência de tarefas.
//!
//! O fuso é o do host (`chrono::Local`): uma casa funciona no fuso de quem
//! mora nela, e o config do usuário versiona regras tipo "todo dia às 19h".

use chrono::{DateTime, Local};
use std::str::FromStr;

/// Parseia e valida uma expressão cron (5 campos; 6 com segundos também
/// aceito, como o `garraia-db`).
pub fn parse_cron(expr: &str) -> Result<Cron, String> {
    Cron::from_str(expr).map_err(|e| format!("'{expr}': {e}"))
}

/// Primeira ocorrência estritamente depois de `depois`, no fuso local.
pub fn proxima_ocorrencia(expr: &str, depois: DateTime<Local>) -> Result<DateTime<Local>, String> {
    let cron = parse_cron(expr)?;
    cron.find_next_occurrence(&depois, false)
        .map_err(|e| format!("sem ocorrência futura para '{expr}': {e}"))
}

pub use croner::Cron;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parseia_cron_valido_e_rejeita_lixo() {
        assert!(parse_cron("0 19 * * *").is_ok());
        assert!(parse_cron("*/15 * * * *").is_ok());
        assert!(parse_cron("nao-e-cron").is_err());
        assert!(parse_cron("").is_err());
    }

    #[test]
    fn proxima_ocorrencia_e_no_futuro() {
        let agora = Local::now();
        let proximo = proxima_ocorrencia("* * * * *", agora).expect("a cada minuto");
        assert!(
            proximo > agora,
            "ocorrência tem que ser estritamente futura"
        );
    }
}
