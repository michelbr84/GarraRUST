//! #1228 slice B: o Swagger UI entra pela feature `vendored` do
//! `utoipa-swagger-ui`, e o build.rs dela nunca baixa nada da rede.
//!
//! Sem `vendored`, o build.rs baixa o zip do GitHub a cada build limpo (ou
//! exige `SWAGGER_UI_DOWNLOAD_URL=file://...`), o que quebra build offline e
//! poe um download sem pino de conteudo na cadeia de build. Este teste le o
//! manifesto e o lockfile para que a volta ao download seja uma decisao
//! explicita, nao um efeito colateral de editar a linha da dependencia.

use std::path::Path;

fn linha_da_dependencia(manifesto: &str) -> Option<&str> {
    manifesto
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("utoipa-swagger-ui ") || l.starts_with("utoipa-swagger-ui="))
}

fn ler(caminho: &Path) -> String {
    match std::fs::read_to_string(caminho) {
        Ok(s) => s,
        Err(e) => panic!("nao consegui ler {}: {e}", caminho.display()),
    }
}

#[test]
fn swagger_ui_usa_a_feature_vendored_e_nao_o_downloader() {
    let manifesto = ler(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    let linha = linha_da_dependencia(&manifesto)
        .expect("a dependencia utoipa-swagger-ui sumiu do Cargo.toml do gateway");
    assert!(
        linha.contains("\"vendored\""),
        "utoipa-swagger-ui sem a feature `vendored`: o build.rs volta a baixar o \
         Swagger UI da rede (#1228). Linha: {linha}"
    );
    assert!(
        !linha.contains("\"reqwest\""),
        "a feature `reqwest` religa o downloader HTTP do build.rs; com `vendored` \
         ela nao e necessaria (#1228). Linha: {linha}"
    );
}

#[test]
fn lockfile_resolve_a_crate_vendored() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = ler(&raiz.join("Cargo.lock"));
    assert!(
        lock.lines()
            .any(|l| l.trim() == "name = \"utoipa-swagger-ui-vendored\""),
        "Cargo.lock nao resolve utoipa-swagger-ui-vendored: a feature `vendored` \
         nao esta ligada em nenhum lugar do grafo (#1228)"
    );
}

/// Os assets embutidos pelo build.rs (a partir do zip vendorizado) servem de
/// verdade: `/docs/` responde a pagina do Swagger UI apontando para a spec.
#[tokio::test]
async fn swagger_ui_embutido_serve_a_pagina() {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use utoipa_swagger_ui::SwaggerUi;

    let app: axum::Router = axum::Router::new().merge(
        SwaggerUi::new("/docs").url("/v1/openapi.json", utoipa::openapi::OpenApi::default()),
    );
    let pedido = match axum::http::Request::get("/docs/swagger-initializer.js").body(Body::empty())
    {
        Ok(p) => p,
        Err(e) => panic!("pedido invalido: {e}"),
    };
    let resposta = match app.oneshot(pedido).await {
        Ok(r) => r,
        Err(e) => panic!("router falhou: {e}"),
    };
    assert_eq!(resposta.status(), axum::http::StatusCode::OK);
    let corpo = match resposta.into_body().collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => panic!("corpo ilegivel: {e}"),
    };
    let texto = String::from_utf8_lossy(&corpo);
    assert!(
        texto.contains("SwaggerUIBundle") && texto.contains("/v1/openapi.json"),
        "swagger-initializer.js nao veio do Swagger UI embutido: {texto}"
    );
}
