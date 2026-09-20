use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("channel error: {0}")]
    Channel(String),

    #[error("agent error: {0}")]
    Agent(String),

    /// A requisicao nao chegou a ter resposta: conexao recusada, DNS que nao
    /// resolve, timeout de conexao, falha ao enviar.
    ///
    /// Existe separada de `Agent` porque a **politica** e outra (#1249).
    /// Erro de HTTP 429/5xx pede retry com backoff no mesmo endereco; rede
    /// caida nao — insistir quatro vezes num endereco inalcancavel so gasta
    /// segundos antes de chegar ao provider local, que era o caminho certo
    /// desde a primeira tentativa (ADR 0022).
    ///
    /// Quem constroi esta variante e quem ainda tem o erro de transporte
    /// tipado na mao (ver `garraia_agents::providers::erro_de_envio`), e nao
    /// quem le a mensagem depois: classificar por texto de `reqwest` quebra
    /// quando a dependencia muda a frase.
    #[error("transport error: {0}")]
    Transport(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("security error: {0}")]
    Security(String),

    #[error("media error: {0}")]
    Media(String),

    #[error("gateway error: {0}")]
    Gateway(String),

    #[error("skill error: {0}")]
    Skill(String),

    #[error("mcp error: {0}")]
    Mcp(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn error_display_includes_context() {
        let e = Error::Config("bad yaml".into());
        assert_eq!(e.to_string(), "configuration error: bad yaml");

        let e = Error::Plugin("timeout".into());
        assert_eq!(e.to_string(), "plugin error: timeout");

        let e = Error::Security("blocked".into());
        assert_eq!(e.to_string(), "security error: blocked");

        // #1249: transporte tem prefixo proprio, e o texto do provider
        // continua inteiro dentro dele — a CLI classifica o cartao de erro
        // pela frase interna ("error sending request"), nao pelo prefixo.
        let e = Error::Transport("ollama request failed: error sending request".into());
        assert_eq!(
            e.to_string(),
            "transport error: ollama request failed: error sending request"
        );

        let e = Error::Other("misc".into());
        assert_eq!(e.to_string(), "misc");
    }
}
