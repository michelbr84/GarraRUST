//! Cron para os gatilhos de automação — casca fina sobre [`croner`], o
//! mesmo parser que `garraia-db` já usa para recorrência de tarefas.
//!
//! O fuso é o do host (`jiff::tz::TimeZone::system()`, que é o de
//! `ZonedDateTime::now()`): uma casa funciona no fuso de quem mora nela, e o
//! config do usuário versiona regras tipo "todo dia às 19h". Sem fuso
//! configurado no host, cai em UTC, como o `chrono::Local` de antes (#1938).

use jiff::ZonedDateTime;
use std::str::FromStr;

/// Parseia e valida uma expressão cron (5 campos; 6 com segundos também
/// aceito, como o `garraia-db`).
pub fn parse_cron(expr: &str) -> Result<Cron, String> {
    Cron::from_str(expr).map_err(|e| format!("'{expr}': {e}"))
}

/// Primeira ocorrência estritamente depois de `depois`, no fuso de `depois`.
pub fn proxima_ocorrencia(expr: &str, depois: &ZonedDateTime) -> Result<ZonedDateTime, String> {
    let cron = parse_cron(expr)?;
    cron.find_next_occurrence(depois, false)
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

    /// Antes do croner 5, `jan` no campo de dia da semana virava 1
    /// (segunda-feira): a regra "todo janeiro" disparava toda segunda.
    #[test]
    fn rejeita_nome_fora_do_proprio_campo() {
        assert!(parse_cron("0 19 * * jan").is_err());
        assert!(parse_cron("mon * * * *").is_err());
        assert!(parse_cron("0 19 * * mon-fri").is_ok());
    }

    #[test]
    fn proxima_ocorrencia_e_no_futuro() {
        let agora = ZonedDateTime::now();
        let proximo = proxima_ocorrencia("* * * * *", &agora).expect("a cada minuto");
        assert!(
            proximo > agora,
            "ocorrência tem que ser estritamente futura"
        );
    }
}
