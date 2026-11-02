//! O cliente HTTP dos providers de LLM, montado a partir de `timeouts`.
//!
//! Mora aqui, e nao no bootstrap do gateway, porque a CLI precisa do MESMO
//! cliente (#1778): `garra chat` e `garra ask` montavam os providers com
//! `::new()` cru, entao `timeouts.llm.default_secs` era inerte na CLI e um
//! turno contra Ollama ou llama.cpp nao tinha teto de tempo em lugar nenhum.
//! `garraia-ask` (de onde sai o provider da CLI) nao pode depender de
//! `garraia-gateway` — o gateway e que depende dela —, e `garraia-agents` e a
//! crate que as duas ja alcancam e onde o `reqwest` ja e dependencia direta.
//!
//! Duas funcoes, as duas puras: nenhuma le config, nenhuma faz I/O. A casca
//! que le `config.timeouts` fica em quem tem o `AppConfig` na mao.
//!
//! **Falha de montagem e devolvida, nunca trocada (#1790).** Um cliente que
//! nao monta vira `Err` para quem chamou. Antes ele virava, em silencio, o
//! cliente padrao do `reqwest` — que SEGUE redirect (regra 14 / #1248) e nao
//! tem a janela de resposta — ou o compartilhado, com a janela errada. Nao
//! existe cliente "quase certo" para cair: quem chama decide o que fazer sem
//! ele (a CLI falha com a causa, o gateway nao registra o provider).
//!
//! **Contrato do `with_client`:** os `::new()` dos providers montam um cliente
//! com `redirect(none)` (issue #1248, regra 14 do projeto) e, nos de nuvem,
//! `connect_timeout`. Como `with_client` SUBSTITUI esse cliente, quem chama
//! precisa entregar um que ja tenha as duas coisas — e por isso que elas sao
//! aplicadas aqui, em um lugar so, e nao repetidas em cada chamador.
//!
//! **#1792:** a mesma politica vale para os clientes que NAO recebem o da
//! config — os `::new()` dos providers, embeddings, A2A e `web_search` —, por
//! [`cliente_padrao`]. Todo cliente HTTP desta crate sai daqui ou de
//! `garraia_common::ssrf::pinned_client`, que tambem desliga redirect. A
//! excecao e o transporte HTTP do MCP: o `rmcp` monta o proprio cliente em
//! reqwest 0.13 e nao aceita o nosso (ver `mcp::manager::validate_mcp_url`).

use std::time::Duration;

/// Teto para ESTABELECER a conexao, nao para a resposta.
///
/// As respostas de LLM transmitem por minutos, entao o teto total e a janela
/// da config; o que nao pode e um host morto segurar o chamador para sempre.
/// Mesmo valor que os `::new()` de `OpenAiProvider` e `AnthropicProvider`
/// usam nos clientes padrao deles.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A base comum: sem seguir redirect, com teto de conexao. O teto total e a
/// unica coisa que varia entre os dois construtores abaixo.
fn base() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
}

/// O cliente compartilhado por todos os providers sem janela propria, a
/// partir de `timeouts.llm.default_secs`.
///
/// `0` documenta "sem timeout" e e o que este ramo entrega: sem chamar
/// `.timeout()`, o reqwest fica com o default dele (sem teto). Antes o 0
/// virava `Duration::ZERO`, que e um sleep ja vencido — TODA chamada LLM
/// falhava em ~1ms, o oposto do que a mensagem do `config check` promete
/// (#1593).
pub fn cliente_compartilhado(default_secs: u64) -> reqwest::Result<reqwest::Client> {
    let builder = base();
    let builder = if default_secs == 0 {
        builder
    } else {
        builder.timeout(Duration::from_secs(default_secs))
    };
    builder.build()
}

/// #1593: o cliente HTTP de um provider LLM — janela propria quando
/// `llm.<nome>.timeout_secs` esta declarada, o compartilhado caso contrario.
///
/// Puro de proposito: a decisao "qual timeout vale para este provider" fica
/// testavel sem subir runtime nenhum. `Some(x)` constroi um cliente com a
/// janela propria; o `config check` ja avisa quando o valor declarado e 0
/// (mesmo aviso do valor global).
pub fn cliente_do_llm(
    compartilhado: &reqwest::Client,
    timeout_secs: Option<u64>,
) -> reqwest::Result<reqwest::Client> {
    match timeout_secs {
        // 0 = sem timeout, a mesma semantica documentada do valor global
        // (#1593: `Duration::ZERO` seria um sleep ja vencido, nao "sem teto").
        Some(0) => base().build(),
        Some(segs) => base().timeout(Duration::from_secs(segs)).build(),
        None => Ok(compartilhado.clone()),
    }
}

// ─── #1792: o cliente de quem NAO recebe o da config ──────────────────────

/// Janela de resposta de quem nao tem config nenhuma que a traga (o cliente
/// A2A): o default de `timeouts.llm.default_secs` em `garraia_config`.
///
/// Repetido aqui, e nao lido de la, porque esta crate nao depende de
/// `garraia-config` — o mesmo motivo pelo qual as funcoes acima recebem o
/// numero em vez do `AppConfig`.
pub const JANELA_PADRAO: Duration = Duration::from_secs(120);

/// O cliente HTTP com a politica comum — **sem seguir redirect** e com teto
/// de conexao ([`CONNECT_TIMEOUT`]) — para todo cliente desta crate que nasce
/// SEM o da config (#1792): os `::new()` dos providers (OpenAI, Anthropic,
/// Ollama, llama.cpp e os tres de embeddings), o A2A e a `web_search`.
///
/// Antes cada um montava o seu. Os de fora do caminho principal usavam
/// `Client::new()`, que segue ate 10 redirects (e, no mesmo host, leva o
/// bearer junto) e nao tem teto nenhum; os de dentro tinham `Policy::none()`,
/// mas com `.unwrap_or_else(|_| Client::new())`, que trocava a politica pelo
/// default do reqwest justamente quando o builder falhava.
///
/// `janela` e o teto TOTAL da resposta:
/// - `None` nos `::new()` dos providers: a janela deles e a da config, que
///   chega por `with_client` ([`cliente_compartilhado`] / [`cliente_do_llm`]).
///   Um teto no `::new()` mataria o `pull` do Ollama no meio — ele usa o
///   `::new()` de proposito, porque um download frio leva minutos (#1778);
/// - `Some(..)` quando a janela e conhecida ao montar: a da config (o
///   `embed_client` do gateway, de `timeouts.embeddings`) ou a propria de
///   quem nao tem config que a traga (A2A, `web_search`, credenciais do
///   `openai-codex`).
///
/// `Some(Duration::ZERO)` vale como `None`, a mesma semantica de "0 = sem
/// teto" da config (#1593): `Duration::ZERO` no reqwest e um sleep ja
/// vencido, e toda chamada falharia em ~1ms.
pub fn cliente_padrao(janela: Option<Duration>) -> reqwest::Client {
    let builder = match janela.filter(|j| !j.is_zero()) {
        Some(janela) => base().timeout(janela),
        None => base(),
    };
    montar_sem_degradar(builder)
}

/// Monta o cliente sem NUNCA cair no default do reqwest (#1792).
///
/// Se o builder pedido falhar, o erro vai para o log e o reserva e o builder
/// MINIMO que ainda nao segue redirect: perde o teto, nunca a politica. Se nem
/// o minimo monta, o reqwest nao consegue montar cliente nenhum (backend TLS
/// indisponivel) — e exatamente o caso em que o `Client::new()` dos reservas
/// antigos ja entrava em panico, porque ele E `builder().build().expect(..)`.
/// O `expect` abaixo nao cria um modo de falha novo; o que deixa de existir e
/// o reserva que voltava SEGUINDO redirect.
pub fn montar_sem_degradar(builder: reqwest::ClientBuilder) -> reqwest::Client {
    match builder.build() {
        Ok(cliente) => cliente,
        Err(e) => {
            tracing::error!(
                erro = %e,
                "cliente HTTP com a politica comum nao montou; usando o minimo, \
                 sem seguir redirect e sem teto"
            );
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("o reqwest nao monta cliente HTTP nenhum (backend TLS indisponivel)")
        }
    }
}

/// Servidor de teste que responde todo pedido com um 3xx (#1792).
///
/// Compartilhado pelos testes de embeddings, llama.cpp, A2A e `web_search`:
/// cada superficie prova a MESMA coisa (o redirect nao e seguido e o erro cita
/// o `Location` redigido), entao o servidor e um so.
#[cfg(test)]
pub(crate) mod servidor_que_redireciona {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// O segredo que o `Location` carrega na query. Nao pode aparecer em
    /// mensagem de erro nenhuma: o `Location` e escolhido pelo servidor
    /// remoto e o diagnostico o ecoa.
    pub(crate) const SEGREDO: &str = "segredo-do-location";

    pub(crate) struct Servidor {
        /// `http://127.0.0.1:<porta>`: a `base_url` a configurar.
        pub(crate) base: String,
        pedidos: Arc<AtomicUsize>,
    }

    impl Servidor {
        /// Quantos pedidos chegaram. O `Location` aponta para o proprio
        /// servidor, entao um cliente que segue redirect volta aqui e a conta
        /// passa de um por chamada.
        pub(crate) fn pedidos(&self) -> usize {
            self.pedidos.load(Ordering::SeqCst)
        }

        /// O `Location` como o diagnostico tem de cita-lo: com a credencial da
        /// query redigida e o resto intacto.
        pub(crate) fn location_redigido(&self) -> String {
            format!("{}/seguido?user_id=u1&token=[REDACTED]", self.base)
        }
    }

    /// Sobe o servidor; todo pedido recebe `status` com
    /// `Location: <base>/seguido?user_id=u1&token=<SEGREDO>`.
    pub(crate) async fn subir(status: u16) -> Servidor {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let pedidos = Arc::new(AtomicUsize::new(0));
        let contador = pedidos.clone();
        let location = format!("{base}/seguido?user_id=u1&token={SEGREDO}");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let contador = contador.clone();
                let location = location.clone();
                tokio::spawn(async move {
                    ler_pedido(&mut sock).await;
                    contador.fetch_add(1, Ordering::SeqCst);
                    let resposta = format!(
                        "HTTP/1.1 {status} Redirect\r\nlocation: {location}\r\n\
                         content-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    let _ = sock.write_all(resposta.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Servidor { base, pedidos }
    }

    /// Le o pedido inteiro (cabecalho e corpo, pelo `content-length`) antes
    /// de responder: responder no meio do upload faria o cliente ver conexao
    /// quebrada em vez do 3xx.
    async fn ler_pedido(sock: &mut tokio::net::TcpStream) {
        let mut lido = Vec::new();
        let mut pedaco = [0u8; 4096];
        loop {
            let n = match sock.read(&mut pedaco).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            lido.extend_from_slice(&pedaco[..n]);
            let Some(fim) = lido.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let cabecalho = String::from_utf8_lossy(&lido[..fim]).to_lowercase();
            let tamanho = cabecalho
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let mut corpo = lido.len() - (fim + 4);
            while corpo < tamanho {
                match sock.read(&mut pedaco).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => corpo += n,
                }
            }
            return;
        }
    }
}

#[cfg(test)]
mod testes_do_cliente_padrao {
    use super::servidor_que_redireciona as srv;
    use super::*;

    /// Responde 200 so depois de `atraso`, lendo o pedido antes (sem isso o
    /// hyper falha em montagem, e nao como timeout).
    async fn servidor_lento(atraso: Duration) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    tokio::time::sleep(atraso).await;
                    let _ = sock
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                        .await;
                });
            }
        });
        format!("http://{addr}/")
    }

    /// #1792: nenhuma variante do cliente padrao segue redirect — com janela,
    /// sem janela e com a janela zero que vale como "sem teto".
    #[tokio::test]
    async fn cliente_padrao_nao_segue_redirect() {
        let s = srv::subir(302).await;
        let variantes = [
            cliente_padrao(None),
            cliente_padrao(Some(Duration::from_secs(5))),
            cliente_padrao(Some(Duration::ZERO)),
        ];
        for cliente in &variantes {
            let resp = cliente
                .get(&s.base)
                .send()
                .await
                .expect("o 302 tem de voltar como resposta");
            assert_eq!(resp.status(), reqwest::StatusCode::FOUND);
        }
        assert_eq!(s.pedidos(), variantes.len(), "algum redirect foi seguido");
    }

    /// A janela pedida vale (1s mata uma resposta de 3s), e `Some(ZERO)` e
    /// "sem teto" (#1593), nao um sleep ja vencido que falharia em ~1ms.
    #[tokio::test]
    async fn cliente_padrao_aplica_a_janela_e_zero_e_sem_teto() {
        let url = servidor_lento(Duration::from_secs(3)).await;
        let erro = cliente_padrao(Some(Duration::from_secs(1)))
            .get(&url)
            .send()
            .await
            .expect_err("o teto de 1s tem de matar a resposta de 3s");
        assert!(
            erro.is_timeout(),
            "tem de morrer como timeout; veio: {erro}"
        );

        let url = servidor_lento(Duration::from_millis(200)).await;
        let resp = cliente_padrao(Some(Duration::ZERO))
            .get(&url)
            .send()
            .await
            .expect("zero = sem teto: nao pode falhar numa resposta de 200ms");
        assert!(resp.status().is_success());
    }

    /// O reserva de um builder que FALHA continua sem seguir redirect. Era o
    /// buraco do `.unwrap_or_else(|_| Client::new())` dos `::new()`: o
    /// reserva era o default do reqwest. Versao TLS minima acima da maxima e
    /// um jeito deterministico de fazer o `build()` falhar.
    #[tokio::test]
    async fn reserva_de_builder_que_falha_nao_segue_redirect() {
        let quebrado = || {
            base()
                .min_tls_version(reqwest::tls::Version::TLS_1_3)
                .max_tls_version(reqwest::tls::Version::TLS_1_2)
        };
        assert!(
            quebrado().build().is_err(),
            "o builder tem de falhar, senao este teste nao exercita o reserva"
        );
        let cliente = montar_sem_degradar(quebrado());

        let s = srv::subir(302).await;
        let resp = cliente
            .get(&s.base)
            .send()
            .await
            .expect("o 302 tem de voltar como resposta");
        assert_eq!(resp.status(), reqwest::StatusCode::FOUND);
        assert_eq!(s.pedidos(), 1, "o reserva seguiu o redirect");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Servidor local que aceita, le o cabecalho como um servidor de verdade
    /// (sem isso o hyper falha em montagem, `Canceled`, e nao como timeout) e
    /// so responde depois de `atraso`.
    async fn servidor_lento(atraso: Duration) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                tokio::time::sleep(atraso).await;
                let _ = tokio::io::AsyncWriteExt::write_all(
                    &mut sock,
                    b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok",
                )
                .await;
            }
        });
        addr
    }

    /// A janela global vale: 1s mata uma resposta de 3s. E o ramo que a CLI
    /// nao tinha (#1778) e que o bootstrap do gateway montava inline.
    #[tokio::test]
    async fn compartilhado_aplica_a_janela_global() {
        let cliente = cliente_compartilhado(1).expect("o cliente monta");
        let addr = servidor_lento(Duration::from_secs(3)).await;

        let inicio = std::time::Instant::now();
        let erro = cliente
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect_err("o teto de 1s tem de matar a resposta de 3s");
        assert!(
            erro.is_timeout(),
            "tem de morrer como timeout; veio: {erro}"
        );
        assert!(
            inicio.elapsed() < Duration::from_secs(3),
            "a janela tem de disparar ANTES da resposta lenta"
        );
    }

    /// `default_secs: 0` = sem teto. Com `Duration::ZERO` esta chamada
    /// falharia em ~1ms; passar aqui prova que 0 nao virou teto.
    #[tokio::test]
    async fn compartilhado_com_zero_nao_tem_teto() {
        let cliente = cliente_compartilhado(0).expect("o cliente monta");
        let addr = servidor_lento(Duration::from_millis(200)).await;

        let resp = cliente
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect("sem teto nao pode falhar numa resposta de 200ms");
        assert!(resp.status().is_success());
    }

    /// #1790 item 8: falha de montagem do cliente nunca vira um cliente
    /// "qualquer". O `Client::default()` segue redirect (regra 14 / #1248) e
    /// nao tem a janela de resposta; cair nele em silencio trocava a policy
    /// sem aviso. Varre so a parte de producao do arquivo.
    #[test]
    fn montagem_nunca_cai_num_cliente_degradado_em_silencio() {
        // O erro chega a quem chama: a assinatura e o contrato.
        let _: fn(u64) -> reqwest::Result<reqwest::Client> = cliente_compartilhado;
        let _: fn(&reqwest::Client, Option<u64>) -> reqwest::Result<reqwest::Client> =
            cliente_do_llm;
        let fonte = include_str!("cliente_llm.rs");
        let producao = fonte.split("#[cfg(test)]").next().unwrap_or(fonte);
        for proibido in [
            ".unwrap_or_default()",
            ".unwrap_or_else(",
            ".unwrap()",
            ".ok()",
        ] {
            assert!(
                !producao.contains(proibido),
                "cliente_llm.rs nao pode engolir erro de montagem com `{proibido}` (#1790)"
            );
        }
    }

    /// Regra 14 / #1248: o cliente da config NAO segue redirect. Sem isto,
    /// entrega-lo por `with_client` apagaria o `redirect(none)` que os
    /// `::new()` dos providers montam, e um 302 levaria a credencial para
    /// outro host.
    #[tokio::test]
    async fn clientes_da_config_nao_seguem_redirect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = tokio::io::AsyncWriteExt::write_all(
                    &mut sock,
                    b"HTTP/1.1 302 Found\r\nlocation: http://exemplo.invalido/\r\n\
                      content-length: 0\r\n\r\n",
                )
                .await;
            }
        });

        let url = format!("http://{addr}/");
        let compartilhado = cliente_compartilhado(30).expect("o cliente monta");
        for cliente in [
            compartilhado.clone(),
            cliente_do_llm(&compartilhado, Some(5)).expect("o cliente monta"),
            cliente_do_llm(&compartilhado, Some(0)).expect("o cliente monta"),
        ] {
            let resp = cliente
                .get(&url)
                .send()
                .await
                .expect("o 302 tem de voltar como resposta, nao virar outra chamada");
            assert_eq!(
                resp.status(),
                reqwest::StatusCode::FOUND,
                "seguiu o redirect — host inalcancavel teria dado erro de rede"
            );
        }
    }
}
