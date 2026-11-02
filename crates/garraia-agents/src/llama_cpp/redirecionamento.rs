//! #1792: o cliente padrao do `llama-server` nao segue redirect, e o 3xx vira
//! erro que cita o `Location` redigido. Arquivo proprio, e nao mais um teste em
//! `llama_cpp.rs`, pelo teto de 700 linhas do Quality Ratchet.

use super::*;
use crate::cliente_llm::servidor_que_redireciona as srv;

/// O cliente padrao (`::new()`) era `Client::new()`: seguia ate 10
/// redirects e nao tinha teto nem de conexao. Agora o 3xx volta como erro
/// nas tres chamadas HTTP do provider, com o `Location` citado e redigido
/// e o status no formato que `status_http_do_erro` le.
#[tokio::test]
async fn redirect_nao_e_seguido_e_o_erro_cita_o_location() {
    let s = srv::subir(308).await;
    let provedor = LlamaCppProvider::new(None, Some(s.base.clone()), None);
    let pedido = LlmRequest {
        model: String::new(),
        messages: vec![crate::providers::ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("oi".to_string()),
        }],
        system: None,
        max_tokens: Some(8),
        temperature: None,
        tools: vec![],
    };

    let erros = [
        (
            "complete",
            provedor
                .complete(&pedido)
                .await
                .expect_err("3xx tem de virar erro"),
        ),
        (
            "stream_complete",
            match provedor.stream_complete(&pedido).await {
                Ok(_) => panic!("3xx tem de virar erro antes do stream"),
                Err(e) => e,
            },
        ),
        (
            "available_models",
            provedor
                .available_models()
                .await
                .expect_err("3xx tem de virar erro"),
        ),
    ];

    assert_eq!(s.pedidos(), erros.len(), "algum redirect foi seguido");
    for (chamada, erro) in erros {
        let msg = erro.to_string();
        assert_eq!(
            crate::providers::status_http_do_erro(&erro),
            Some(308),
            "{chamada}: {msg}"
        );
        assert!(
            msg.contains(&format!("location={}", s.location_redigido())),
            "{chamada}: o Location tem de ser citado, redigido: {msg}"
        );
        assert!(!msg.contains(srv::SEGREDO), "{chamada}: {msg}");
        assert!(
            msg.contains("redirects desligados por seguranca"),
            "{chamada}: {msg}"
        );
        assert!(
            msg.contains("corrija a base_url do provider"),
            "{chamada}: {msg}"
        );
    }
}
