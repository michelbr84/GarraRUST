//! #1793 — as sondas que a escolha de provider da CLI faz ANTES do primeiro
//! turno, e o aviso quando essa escolha degrada.
//!
//! `garra chat`, `garra --model <tag>` e `garra ask` sem `-p` escolhem o
//! provider em [`crate::chat::detect_provider`]: o `agent.default_provider`
//! configurado primeiro, a heuristica de autodeteccao (#1180) depois. Duas
//! paradas dessa escolha perguntam a um daemon local se ele esta de pe — o
//! default configurado quando e Ollama/llama.cpp, e o Ollama no fim da cadeia —
//! e as duas chamavam `health_check()` no cliente de INFERENCIA: a janela de
//! `timeouts.llm.default_secs` (120s) ou, pior, a de `llm.<nome>.timeout_secs`
//! (no Termux, dezenas de minutos). Um daemon que aceita a conexao e nao
//! responde segurava a abertura do REPL pela janela inteira so para descobrir
//! que nao servia.
//!
//! A janela de uma sonda e `timeouts.health.default_secs` (5s por padrao) — a
//! mesma chave que ja limita as sondas do gateway (`/health`, a do STT no
//! boot), com o mesmo piso de 1s: `0` nao vira "sem sonda".
//!
//! E quando o default configurado nao serve, a escolha cai na heuristica de
//! autodeteccao — antes em silencio: quem configurou `default_provider: local`
//! via outro provider responder sem uma linha de log dizendo por que.
//! [`avisar_default_descartado`] diz, em `warn`, com o motivo.

use std::time::Duration;

use garraia_agents::LlmProvider;
use garraia_config::TimeoutConfig;

/// A janela de uma sonda de disponibilidade: `timeouts.health.default_secs`,
/// com piso de 1s — a mesma regra da sonda do STT no boot do gateway
/// (`server.rs`): `0` nao pode significar "sem sonda nenhuma".
pub(crate) fn janela_de_sonda(timeouts: &TimeoutConfig) -> Duration {
    Duration::from_secs(timeouts.health.default_secs.max(1))
}

/// O que uma sonda com janela propria descobriu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sonda {
    /// Respondeu saudavel dentro da janela.
    Disponivel,
    /// Respondeu que nao esta de pe, ou a chamada falhou antes da janela
    /// (conexao recusada, status de erro).
    Indisponivel,
    /// Nao respondeu dentro da janela.
    Esgotada(Duration),
}

/// `health_check()` do provider, limitado pela `janela` — e NAO pela janela
/// do cliente HTTP do provider, que e a de inferencia.
///
/// O cliente de inferencia continua sendo o do provider devolvido: a sonda so
/// poe um teto por fora, sem montar um cliente novo (o que trocaria a politica
/// de redirect e de conexao que o `cliente_llm` fixou).
pub(crate) async fn sondar(provider: &dyn LlmProvider, janela: Duration) -> Sonda {
    match tokio::time::timeout(janela, provider.health_check()).await {
        Ok(Ok(true)) => Sonda::Disponivel,
        Ok(Ok(false) | Err(_)) => Sonda::Indisponivel,
        Err(_) => Sonda::Esgotada(janela),
    }
}

/// Por que o `agent.default_provider` configurado ficou de fora da escolha.
///
/// Os motivos sao classificacoes, nao o texto do erro: a construcao do
/// provider passa pela credencial da entrada, e o log nao pode carregar nada
/// que tenha saido daquele escopo (GAR-576, regra 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DefaultDescartado {
    /// `decide_default_provider` recusou antes de montar: entrada ausente em
    /// `llm:`, tipo que a CLI nao escolhe sozinha, ou sem credencial.
    Decisao(&'static str),
    /// A entrada nao pode ser montada (`build_provider` recusou).
    Construcao,
    /// O daemon local respondeu que nao esta de pe, ou a chamada falhou.
    Indisponivel,
    /// O daemon local nao respondeu dentro da janela de sonda.
    SondaEsgotada(Duration),
}

impl DefaultDescartado {
    /// O resultado de uma sonda que nao achou o daemon de pe.
    pub(crate) fn da_sonda(sonda: Sonda) -> Option<Self> {
        match sonda {
            Sonda::Disponivel => None,
            Sonda::Indisponivel => Some(Self::Indisponivel),
            Sonda::Esgotada(janela) => Some(Self::SondaEsgotada(janela)),
        }
    }

    /// O motivo, em uma linha, para o campo `motivo` do `warn`.
    pub(crate) fn motivo(&self) -> String {
        match self {
            Self::Decisao(razao) => (*razao).to_string(),
            Self::Construcao => "a entrada nao pode ser montada (credencial ausente ou tipo que a \
                                 CLI nao constroi; `garra config check` mostra o detalhe)"
                .to_string(),
            Self::Indisponivel => {
                "o daemon local nao respondeu saudavel a sonda de disponibilidade".to_string()
            }
            Self::SondaEsgotada(janela) => format!(
                "o daemon local nao respondeu a sonda de disponibilidade em {}s \
                 (timeouts.health.default_secs)",
                janela.as_secs()
            ),
        }
    }
}

/// O `warn` da degradacao: o default configurado nao serviu e a escolha caiu
/// na heuristica de autodeteccao. `default_provider` e a chave do config —
/// nunca a credencial nem a URL da entrada.
pub(crate) fn avisar_default_descartado(default_provider: &str, motivo: &DefaultDescartado) {
    tracing::warn!(
        default_provider = %default_provider,
        motivo = %motivo.motivo(),
        "agent.default_provider configurado nao foi usado; a escolha de provider caiu na \
         autodeteccao"
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use garraia_agents::{LlmRequest, LlmResponse};
    use std::io;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::writer::MakeWriter;

    /// Destino de log afirmavel: o `fmt` escreve num `Vec<u8>` compartilhado.
    /// `garraia-cli` nao tem `tracing-test`; o molde e o de `tracing_setup`.
    #[derive(Clone, Default)]
    pub(crate) struct Logs(Arc<Mutex<Vec<u8>>>);

    impl Logs {
        pub(crate) fn texto(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("logs envenenado")).into_owned()
        }
    }

    impl io::Write for Logs {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("logs envenenado")
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Logs {
        type Writer = Logs;
        fn make_writer(&'a self) -> Logs {
            self.clone()
        }
    }

    /// Instala um subscriber que captura tudo (`trace` para cima) nesta
    /// thread enquanto o guard viver. `#[tokio::test]` roda numa thread so,
    /// entao os eventos das tasks do teste caem aqui.
    pub(crate) fn capturar_logs() -> (Logs, tracing::subscriber::DefaultGuard) {
        let logs = Logs::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let guard = tracing::subscriber::set_default(sub);
        (logs, guard)
    }

    /// Servidor local que aceita, le o pedido como um servidor de verdade e
    /// nunca responde — o daemon "de pe mas travado" da issue.
    pub(crate) async fn servidor_que_nao_responde() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    drop(sock);
                });
            }
        });
        addr
    }

    /// Provider cuja sonda devolve o que o teste mandar — ou nunca volta.
    struct SondaFixa(Option<garraia_common::Result<bool>>);

    #[async_trait::async_trait]
    impl LlmProvider for SondaFixa {
        fn provider_id(&self) -> &str {
            "sonda-fixa"
        }
        async fn complete(&self, _r: &LlmRequest) -> garraia_common::Result<LlmResponse> {
            unreachable!("a sonda nao faz inferencia")
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            match &self.0 {
                Some(Ok(v)) => Ok(*v),
                Some(Err(_)) => Err(garraia_common::Error::Agent("fora".into())),
                None => std::future::pending().await,
            }
        }
    }

    #[test]
    fn janela_de_sonda_e_timeouts_health_com_piso_de_um_segundo() {
        let mut t = TimeoutConfig::default();
        assert_eq!(janela_de_sonda(&t), Duration::from_secs(5), "default");
        t.health.default_secs = 7;
        assert_eq!(janela_de_sonda(&t), Duration::from_secs(7));
        t.health.default_secs = 0;
        assert_eq!(
            janela_de_sonda(&t),
            Duration::from_secs(1),
            "0 nao pode virar sonda sem teto nem sonda que ja nasce vencida"
        );
        // A janela de inferencia nao entra na conta.
        t.llm.default_secs = 1800;
        assert_eq!(janela_de_sonda(&t), Duration::from_secs(1));
    }

    /// O teto e o da sonda, com relogio pausado: um daemon que nunca responde
    /// devolve `Esgotada` exatamente na janela de 5s, nao na de 120s.
    #[tokio::test(start_paused = true)]
    async fn sonda_que_nao_volta_esgota_na_janela_da_sonda() {
        let inicio = tokio::time::Instant::now();
        let r = sondar(&SondaFixa(None), Duration::from_secs(5)).await;
        assert_eq!(r, Sonda::Esgotada(Duration::from_secs(5)));
        assert_eq!(inicio.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn sonda_distingue_saudavel_de_fora() {
        let j = Duration::from_secs(5);
        assert_eq!(
            sondar(&SondaFixa(Some(Ok(true))), j).await,
            Sonda::Disponivel
        );
        assert_eq!(
            sondar(&SondaFixa(Some(Ok(false))), j).await,
            Sonda::Indisponivel
        );
        assert_eq!(
            sondar(
                &SondaFixa(Some(Err(garraia_common::Error::Agent("x".into())))),
                j
            )
            .await,
            Sonda::Indisponivel
        );
    }

    /// A sonda de verdade contra um Ollama travado: o provider sai com o
    /// cliente de INFERENCIA (120s, como o `detect_provider` monta), e mesmo
    /// assim a sonda morre na janela dela. E a sonda do fim da cadeia de
    /// autodeteccao — a outra (`try_build_default_provider`) tem teste ponta a
    /// ponta no `chat`.
    #[tokio::test]
    async fn ollama_travado_com_cliente_de_inferencia_esgota_na_janela_da_sonda() {
        let addr = servidor_que_nao_responde().await;
        let inferencia =
            garraia_agents::cliente_llm::cliente_compartilhado(120).expect("o cliente monta");
        let ollama = garraia_agents::OllamaProvider::new(None, Some(format!("http://{addr}")))
            .with_client(inferencia);

        let inicio = std::time::Instant::now();
        let r = tokio::time::timeout(
            Duration::from_secs(20),
            sondar(&ollama, Duration::from_secs(1)),
        )
        .await
        .expect("a sonda herdou a janela de inferencia (120s) em vez da dela");
        assert_eq!(r, Sonda::Esgotada(Duration::from_secs(1)));
        assert!(
            inicio.elapsed() < Duration::from_secs(5),
            "a janela de 1s tem de valer: {:?}",
            inicio.elapsed()
        );
    }

    #[test]
    fn motivo_da_sonda_esgotada_nomeia_a_janela_e_a_chave() {
        let m = DefaultDescartado::SondaEsgotada(Duration::from_secs(5)).motivo();
        assert!(m.contains("5s"), "{m}");
        assert!(m.contains("timeouts.health.default_secs"), "{m}");
        assert_eq!(DefaultDescartado::da_sonda(Sonda::Disponivel), None);
        assert_eq!(
            DefaultDescartado::da_sonda(Sonda::Indisponivel),
            Some(DefaultDescartado::Indisponivel)
        );
    }

    #[test]
    fn aviso_do_default_descartado_sai_em_warn_com_o_motivo() {
        let (logs, _guard) = capturar_logs();
        avisar_default_descartado("local", &DefaultDescartado::Indisponivel);
        let texto = logs.texto();
        assert!(texto.contains("WARN"), "{texto}");
        assert!(texto.contains("default_provider=local"), "{texto}");
        assert!(
            texto.contains("motivo=o daemon local nao respondeu saudavel"),
            "{texto}"
        );
    }
}
