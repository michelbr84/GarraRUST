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
//! **Contrato do `with_client`:** os `::new()` dos providers montam um cliente
//! com `redirect(none)` (issue #1248, regra 14 do projeto) e, nos de nuvem,
//! `connect_timeout`. Como `with_client` SUBSTITUI esse cliente, quem chama
//! precisa entregar um que ja tenha as duas coisas — e por isso que elas sao
//! aplicadas aqui, em um lugar so, e nao repetidas em cada chamador.

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
pub fn cliente_compartilhado(default_secs: u64) -> reqwest::Client {
    let builder = base();
    let builder = if default_secs == 0 {
        builder
    } else {
        builder.timeout(Duration::from_secs(default_secs))
    };
    builder.build().unwrap_or_default()
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
) -> reqwest::Client {
    match timeout_secs {
        // 0 = sem timeout, a mesma semantica documentada do valor global
        // (#1593: `Duration::ZERO` seria um sleep ja vencido, nao "sem teto").
        Some(0) => base().build().unwrap_or_else(|_| compartilhado.clone()),
        Some(segs) => base()
            .timeout(Duration::from_secs(segs))
            .build()
            .unwrap_or_else(|_| compartilhado.clone()),
        None => compartilhado.clone(),
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
        let cliente = cliente_compartilhado(1);
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
        let cliente = cliente_compartilhado(0);
        let addr = servidor_lento(Duration::from_millis(200)).await;

        let resp = cliente
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect("sem teto nao pode falhar numa resposta de 200ms");
        assert!(resp.status().is_success());
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
        for cliente in [
            cliente_compartilhado(30),
            cliente_do_llm(&cliente_compartilhado(30), Some(5)),
            cliente_do_llm(&cliente_compartilhado(30), Some(0)),
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
