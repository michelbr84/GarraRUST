//! As pecas OAuth do Sign in with ChatGPT: PKCE S256, `state`/`nonce`, URL de
//! autorizacao, troca do codigo, renovacao, revogacao e validacao do ID
//! token (RS256 contra o JWKS publicado em `auth.openai.com`).
//!
//! Tudo aqui e sem estado: quem guarda tentativa e credencial sao
//! [`super::login`] e [`super::credenciais`]. Nenhuma funcao loga token,
//! codigo de autorizacao ou corpo de resposta do endpoint de token.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{CLIENTE_DE_REGISTRO, ESCOPOS, Endpoints, NOME_DO_AGENTE, RECURSO};

/// Codigos de erro de renovacao que tornam o refresh token inutilizavel
/// (`errors-and-recovery`): limpar os tokens e refazer o login com o mesmo
/// client id emitido.
pub const ERROS_TERMINAIS_DE_RENOVACAO: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

/// Erros do fluxo OAuth. As mensagens nunca carregam token nem codigo.
#[derive(Debug, thiserror::Error)]
pub enum ErroOAuth {
    #[error("falha de rede ao falar com {destino}: {detalhe}")]
    Rede {
        destino: &'static str,
        detalhe: String,
    },
    #[error("o endpoint de token recusou o pedido (HTTP {status}, erro {codigo})")]
    Recusado { status: u16, codigo: String },
    #[error("resposta inesperada do servidor de autorizacao: {0}")]
    RespostaInvalida(String),
    #[error("ID token invalido: {0}")]
    IdTokenInvalido(String),
}

impl ErroOAuth {
    /// O erro e um dos que exigem novo login (refresh token inutilizavel).
    pub fn exige_novo_login(&self) -> bool {
        matches!(self, ErroOAuth::Recusado { codigo, .. } if ERROS_TERMINAIS_DE_RENOVACAO.contains(&codigo.as_str()))
    }
}

/// Bytes aleatorios do CSPRNG do sistema em base64url sem padding. Sem
/// fonte de entropia o login nao segue: valores fracos de state/nonce/PKCE
/// seriam pior que falhar.
pub fn aleatorio_b64(n: usize) -> Result<String, ErroOAuth> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf)
        .map_err(|e| ErroOAuth::RespostaInvalida(format!("CSPRNG do sistema indisponivel: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Par PKCE: verificador de 43 caracteres (32 bytes) e o desafio S256.
#[derive(Clone)]
pub struct Pkce {
    pub verificador: String,
    pub desafio: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkce")
            .field("verificador", &"<redigido>")
            .field("desafio", &self.desafio)
            .finish()
    }
}

impl Pkce {
    pub fn novo() -> Result<Self, ErroOAuth> {
        let verificador = aleatorio_b64(32)?;
        let desafio = desafio_s256(&verificador);
        Ok(Self {
            verificador,
            desafio,
        })
    }
}

/// `code_challenge` = base64url(SHA-256(verificador)) sem padding (RFC 7636).
pub fn desafio_s256(verificador: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verificador.as_bytes()))
}

/// Os parametros de uma tentativa de autorizacao.
#[derive(Debug, Clone)]
pub struct PedidoDeAutorizacao<'a> {
    /// `dynamic_agent_client` no primeiro login da conta; o `oaiapp_...`
    /// emitido nos seguintes.
    pub client_id: &'a str,
    pub ext_agent_host_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub desafio: &'a str,
    /// So na reautorizacao: o ID token guardado do ultimo login.
    pub id_token_hint: Option<&'a str>,
    /// So na reautorizacao: o e-mail validado da conta.
    pub login_hint: Option<&'a str>,
}

/// A URL que o navegador do sistema abre. `agent_name_hint` vai so no
/// registro inicial; `id_token_hint`/`login_hint`, so na reautorizacao.
pub fn url_de_autorizacao(endpoints: &Endpoints, p: &PedidoDeAutorizacao<'_>) -> String {
    let registro = p.client_id == CLIENTE_DE_REGISTRO;
    let mut pares: Vec<(&str, &str)> = vec![
        ("client_id", p.client_id),
        ("response_type", "code"),
        ("redirect_uri", p.redirect_uri),
        ("scope", ESCOPOS),
        ("resource", RECURSO),
        ("state", p.state),
        ("nonce", p.nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", p.desafio),
        ("ext_agent_host_id", p.ext_agent_host_id),
    ];
    if registro {
        pares.push(("agent_name_hint", NOME_DO_AGENTE));
    } else {
        if let Some(h) = p.id_token_hint {
            pares.push(("id_token_hint", h));
        }
        if let Some(h) = p.login_hint {
            pares.push(("login_hint", h));
        }
    }
    match reqwest::Url::parse_with_params(&endpoints.authorize, &pares) {
        Ok(url) => url.to_string(),
        // O endpoint vem de constante ou de teste; se nao parseia, devolve
        // algo que o navegador recusa em vez de um link quebrado silencioso.
        Err(_) => endpoints.authorize.clone(),
    }
}

/// A URL de autorizacao com os parametros sensiveis redigidos, para log e
/// diagnostico (`id_token_hint` nunca vai para log).
pub fn url_redigida(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            let pares: Vec<(String, String)> = u
                .query_pairs()
                .map(|(k, v)| {
                    let sensivel = matches!(
                        k.as_ref(),
                        "id_token_hint" | "login_hint" | "state" | "nonce" | "code_challenge"
                    );
                    (
                        k.into_owned(),
                        if sensivel {
                            "<redigido>".into()
                        } else {
                            v.into_owned()
                        },
                    )
                })
                .collect();
            u.query_pairs_mut().clear().extend_pairs(pares);
            u.to_string()
        }
        Err(_) => "<url invalida>".into(),
    }
}

/// O que volta no callback (`?code=...&state=...&client_id=...&scope=...`)
/// ou o erro OAuth (`?error=access_denied&state=...`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RespostaDoCallback {
    pub code: Option<String>,
    pub state: Option<String>,
    pub client_id: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl RespostaDoCallback {
    /// Le a query string do callback (`code=...&state=...`), sem o `?`.
    pub fn da_query(query: &str) -> Self {
        let mut r = Self::default();
        for (k, v) in url::form_urlencoded_pares(query) {
            match k.as_str() {
                "code" => r.code = Some(v),
                "state" => r.state = Some(v),
                "client_id" => r.client_id = Some(v),
                "scope" => r.scope = Some(v),
                "error" => r.error = Some(v),
                "error_description" => r.error_description = Some(v),
                _ => {}
            }
        }
        r
    }
}

/// Decodificacao de `application/x-www-form-urlencoded` sem crate novo: o
/// `reqwest::Url` ja traz o parser, so precisa de uma base qualquer.
mod url {
    pub fn form_urlencoded_pares(query: &str) -> Vec<(String, String)> {
        let base = format!("http://127.0.0.1/?{query}");
        match reqwest::Url::parse(&base) {
            Ok(u) => u
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// O corpo de sucesso do endpoint de token (`token-reference`).
#[derive(Clone, Deserialize)]
pub struct RespostaDeToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default = "bearer")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    /// Escopos concedidos, separados por espaco.
    #[serde(default)]
    pub scope: Option<String>,
    /// Listado na documentacao sem descricao; guardado como veio.
    #[serde(default)]
    pub earliest_refresh_at: Option<serde_json::Value>,
}

fn bearer() -> String {
    "Bearer".into()
}

impl std::fmt::Debug for RespostaDeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RespostaDeToken")
            .field("access_token", &"<redigido>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redigido>"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "<redigido>"))
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish()
    }
}

impl RespostaDeToken {
    pub fn escopos(&self) -> Vec<String> {
        self.scope
            .as_deref()
            .map(escopos_da_lista)
            .unwrap_or_default()
    }
}

/// `"a b  c"` → `["a","b","c"]` (o callback usa `+`, ja decodificado como espaco).
pub fn escopos_da_lista(s: &str) -> Vec<String> {
    s.split([' ', '+'])
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

async fn postar_formulario(
    cliente: &reqwest::Client,
    url: &str,
    destino: &'static str,
    form: &[(&str, &str)],
) -> Result<reqwest::Response, ErroOAuth> {
    cliente
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| ErroOAuth::Rede {
            destino,
            detalhe: e.without_url().to_string(),
        })
}

/// Le o corpo de erro do endpoint de token so para extrair o codigo:
/// `{"error":"invalid_grant"}` ou `{"error":{"code":"..."}}`.
fn codigo_de_erro(corpo: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(corpo) else {
        return "desconhecido".into();
    };
    match v.get("error") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(obj) => obj
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or("desconhecido")
            .to_string(),
        None => "desconhecido".into(),
    }
}

async fn ler_resposta_de_token(resp: reqwest::Response) -> Result<RespostaDeToken, ErroOAuth> {
    let status = resp.status();
    let corpo = resp.text().await.map_err(|e| ErroOAuth::Rede {
        destino: "o endpoint de token",
        detalhe: e.without_url().to_string(),
    })?;
    if !status.is_success() {
        return Err(ErroOAuth::Recusado {
            status: status.as_u16(),
            codigo: codigo_de_erro(&corpo),
        });
    }
    serde_json::from_str::<RespostaDeToken>(&corpo).map_err(|_| {
        ErroOAuth::RespostaInvalida("o endpoint de token respondeu 200 sem access_token".into())
    })
}

/// Troca o codigo de autorizacao (`grant_type=authorization_code`) com o
/// client id emitido, o verificador PKCE, o mesmo `redirect_uri` e o mesmo
/// `resource`. Cliente publico: sem segredo.
pub async fn trocar_codigo(
    cliente: &reqwest::Client,
    endpoints: &Endpoints,
    client_id: &str,
    code: &str,
    verificador: &str,
    redirect_uri: &str,
) -> Result<RespostaDeToken, ErroOAuth> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verificador),
        ("redirect_uri", redirect_uri),
        ("resource", RECURSO),
    ];
    let resp = postar_formulario(cliente, &endpoints.token, "o endpoint de token", &form).await?;
    ler_resposta_de_token(resp).await
}

/// Renova o access token (`grant_type=refresh_token`) com o client id
/// salvo e o `resource`; sem `scope`, para manter o grant.
pub async fn renovar(
    cliente: &reqwest::Client,
    endpoints: &Endpoints,
    client_id: &str,
    refresh_token: &str,
) -> Result<RespostaDeToken, ErroOAuth> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("resource", RECURSO),
    ];
    let resp = postar_formulario(cliente, &endpoints.token, "o endpoint de token", &form).await?;
    ler_resposta_de_token(resp).await
}

/// Revoga a sessao: `token=<refresh>`, `token_type_hint=refresh_token` e o
/// client id emitido. HTTP 200 (mesmo vazio) e sucesso, inclusive para token
/// ja invalido. Rede/5xx sao tentados de novo com espera crescente.
pub async fn revogar(
    cliente: &reqwest::Client,
    endpoints: &Endpoints,
    client_id: &str,
    refresh_token: &str,
) -> Result<(), ErroOAuth> {
    let form = [
        ("token", refresh_token),
        ("token_type_hint", "refresh_token"),
        ("client_id", client_id),
    ];
    let mut ultimo = None;
    for tentativa in 0..3u32 {
        if tentativa > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(400 * 2u64.pow(tentativa))).await;
        }
        match postar_formulario(cliente, &endpoints.revoke, "o endpoint de revogacao", &form).await
        {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) if r.status().is_server_error() => {
                ultimo = Some(ErroOAuth::Recusado {
                    status: r.status().as_u16(),
                    codigo: "server_error".into(),
                });
            }
            Ok(r) => {
                let status = r.status().as_u16();
                let corpo = r.text().await.unwrap_or_default();
                return Err(ErroOAuth::Recusado {
                    status,
                    codigo: codigo_de_erro(&corpo),
                });
            }
            Err(e) => ultimo = Some(e),
        }
    }
    Err(ultimo.unwrap_or(ErroOAuth::RespostaInvalida("revogacao sem resposta".into())))
}

/// A identidade validada do ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identidade {
    pub sub: String,
    pub email: Option<String>,
    pub nome: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaimsDoIdToken {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

/// Valida o ID token com uma chave ja escolhida: assinatura RS256, `iss`,
/// `aud` = client id emitido, `exp`, e o `nonce` desta tentativa.
pub fn validar_id_token_com_chave(
    id_token: &str,
    chave: &DecodingKey,
    issuer: &str,
    client_id: &str,
    nonce: &str,
) -> Result<Identidade, ErroOAuth> {
    let mut validacao = Validation::new(Algorithm::RS256);
    validacao.set_issuer(&[issuer]);
    validacao.set_audience(&[client_id]);
    validacao.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validacao.validate_exp = true;
    let dados = decode::<ClaimsDoIdToken>(id_token, chave, &validacao)
        .map_err(|e| ErroOAuth::IdTokenInvalido(format!("{:?}", e.kind())))?;
    let claims = dados.claims;
    if claims.nonce.as_deref() != Some(nonce) {
        return Err(ErroOAuth::IdTokenInvalido(
            "nonce diferente do desta tentativa".into(),
        ));
    }
    if claims.sub.trim().is_empty() {
        return Err(ErroOAuth::IdTokenInvalido("sub vazio".into()));
    }
    Ok(Identidade {
        sub: claims.sub,
        email: claims.email,
        nome: claims.name,
    })
}

/// Escolhe no JWK Set a chave do `kid` do cabecalho.
pub fn chave_do_set(set: &JwkSet, kid: &str) -> Result<DecodingKey, ErroOAuth> {
    let jwk = set.find(kid).ok_or_else(|| {
        ErroOAuth::IdTokenInvalido(format!("kid '{kid}' nao esta no JWKS publicado"))
    })?;
    DecodingKey::from_jwk(jwk)
        .map_err(|e| ErroOAuth::IdTokenInvalido(format!("JWK inutilizavel: {:?}", e.kind())))
}

/// Valida o ID token buscando a chave no JWKS publicado pela OpenAI.
pub async fn validar_id_token(
    cliente: &reqwest::Client,
    endpoints: &Endpoints,
    id_token: &str,
    client_id: &str,
    nonce: &str,
) -> Result<Identidade, ErroOAuth> {
    let cabecalho = decode_header(id_token)
        .map_err(|e| ErroOAuth::IdTokenInvalido(format!("{:?}", e.kind())))?;
    if cabecalho.alg != Algorithm::RS256 {
        return Err(ErroOAuth::IdTokenInvalido(format!(
            "algoritmo {:?} nao aceito (so RS256)",
            cabecalho.alg
        )));
    }
    let kid = cabecalho
        .kid
        .ok_or_else(|| ErroOAuth::IdTokenInvalido("cabecalho sem kid".into()))?;
    let set: JwkSet = cliente
        .get(&endpoints.jwks)
        .send()
        .await
        .map_err(|e| ErroOAuth::Rede {
            destino: "o JWKS",
            detalhe: e.without_url().to_string(),
        })?
        .error_for_status()
        .map_err(|e| ErroOAuth::Rede {
            destino: "o JWKS",
            detalhe: e.without_url().to_string(),
        })?
        .json()
        .await
        .map_err(|_| ErroOAuth::RespostaInvalida("JWKS ilegivel".into()))?;
    let chave = chave_do_set(&set, &kid)?;
    validar_id_token_com_chave(id_token, &chave, &endpoints.issuer, client_id, nonce)
}

#[cfg(test)]
pub(crate) mod testes_rsa {
    //! Um par RSA gerado na hora (nada de chave fixa no repositorio) e um
    //! emissor de ID tokens assinados com ele, para os testes de validacao.
    use aws_lc_rs::encoding::AsDer;
    use aws_lc_rs::rsa::{KeyPair, KeySize, PublicKeyComponents};
    use aws_lc_rs::signature::KeyPair as _;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, encode};

    pub struct ParDeTeste {
        pub codificacao: EncodingKey,
        pub n: String,
        pub e: String,
    }

    /// Le um TLV DER a partir de `i`: devolve (inicio do conteudo, tamanho).
    fn tlv(der: &[u8], i: usize) -> (usize, usize) {
        let primeiro = der[i + 1] as usize;
        if primeiro < 0x80 {
            return (i + 2, primeiro);
        }
        let n = primeiro & 0x7f;
        let tamanho = der[i + 2..i + 2 + n]
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | *b as usize);
        (i + 2 + n, tamanho)
    }

    /// `PrivateKeyInfo ::= SEQUENCE { version, algorithm, privateKey OCTET STRING }`:
    /// o conteudo do OCTET STRING e a `RSAPrivateKey` (PKCS#1).
    fn pkcs1_de_pkcs8(der: &[u8]) -> Vec<u8> {
        let (mut i, _) = tlv(der, 0); // SEQUENCE externa
        let (c, t) = tlv(der, i); // version INTEGER
        i = c + t;
        let (c, t) = tlv(der, i); // AlgorithmIdentifier SEQUENCE
        i = c + t;
        assert_eq!(der[i], 0x04, "esperava o OCTET STRING da chave");
        let (c, t) = tlv(der, i);
        der[c..c + t].to_vec()
    }

    impl ParDeTeste {
        pub fn novo() -> Self {
            let par = KeyPair::generate(KeySize::Rsa2048).expect("gera RSA de teste");
            let der = par.as_der().expect("PKCS#8 do par de teste");
            // O jsonwebtoken assina RSA com a chave PKCS#1; o aws-lc-rs
            // exporta PKCS#8, que embrulha a PKCS#1 num OCTET STRING.
            let codificacao = EncodingKey::from_rsa_der(&pkcs1_de_pkcs8(der.as_ref()));
            let comp: PublicKeyComponents<Vec<u8>> = par.public_key().into();
            Self {
                codificacao,
                n: URL_SAFE_NO_PAD.encode(&comp.n),
                e: URL_SAFE_NO_PAD.encode(&comp.e),
            }
        }

        pub fn decodificacao(&self) -> DecodingKey {
            DecodingKey::from_rsa_components(&self.n, &self.e).expect("componentes RSA")
        }

        pub fn jwks(&self, kid: &str) -> serde_json::Value {
            serde_json::json!({"keys": [{"kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256", "n": self.n, "e": self.e}]})
        }

        pub fn assinar(&self, kid: &str, claims: &serde_json::Value) -> String {
            let mut h = Header::new(Algorithm::RS256);
            h.kid = Some(kid.to_string());
            encode(&h, claims, &self.codificacao).expect("assina ID token de teste")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testes_rsa::ParDeTeste;
    use super::*;

    fn agora() -> i64 {
        chrono::Utc::now().timestamp()
    }

    #[test]
    fn pkce_s256_segue_o_vetor_da_rfc_7636() {
        // RFC 7636, apendice B.
        assert_eq!(
            desafio_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = Pkce::novo().expect("pkce");
        assert_eq!(p.verificador.len(), 43);
        assert_eq!(p.desafio, desafio_s256(&p.verificador));
        assert!(!format!("{p:?}").contains(&p.verificador));
    }

    #[test]
    fn aleatorios_sao_distintos_e_url_safe() {
        let a = aleatorio_b64(32).expect("csprng");
        let b = aleatorio_b64(32).expect("csprng");
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    fn pedido<'a>(client_id: &'a str) -> PedidoDeAutorizacao<'a> {
        PedidoDeAutorizacao {
            client_id,
            ext_agent_host_id: "urn:uuid:00000000-0000-4000-8000-000000000000",
            redirect_uri: "http://127.0.0.1:1455/auth/callback",
            state: "st",
            nonce: "no",
            desafio: "ch",
            id_token_hint: Some("eyJ.hint"),
            login_hint: Some("a@b.c"),
        }
    }

    #[test]
    fn url_de_registro_tem_todos_os_parametros_oficiais() {
        let url = url_de_autorizacao(&Endpoints::default(), &pedido(CLIENTE_DE_REGISTRO));
        let u = reqwest::Url::parse(&url).expect("url");
        assert_eq!(u.host_str(), Some("auth.openai.com"));
        assert_eq!(u.path(), "/api/accounts/authorize");
        let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "dynamic_agent_client");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["scope"], ESCOPOS);
        assert_eq!(q["resource"], "https://api.openai.com/v1");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!(q["agent_name_hint"], "GarraIA");
        assert!(
            q.contains_key("ext_agent_host_id")
                && q.contains_key("state")
                && q.contains_key("nonce")
        );
        // Hints sao so da reautorizacao.
        assert!(!q.contains_key("id_token_hint") && !q.contains_key("login_hint"));
    }

    #[test]
    fn url_de_reautorizacao_usa_o_cliente_emitido_e_os_hints_sem_nome() {
        let url = url_de_autorizacao(&Endpoints::default(), &pedido("oaiapp_123"));
        let q: std::collections::HashMap<String, String> = reqwest::Url::parse(&url)
            .expect("url")
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(q["client_id"], "oaiapp_123");
        assert_eq!(q["id_token_hint"], "eyJ.hint");
        assert_eq!(q["login_hint"], "a@b.c");
        assert!(!q.contains_key("agent_name_hint"));
        let redigida = url_redigida(&url);
        assert!(!redigida.contains("eyJ.hint") && !redigida.contains("a%40b.c"));
    }

    #[test]
    fn callback_le_codigo_estado_cliente_e_escopos() {
        let r = RespostaDoCallback::da_query(
            "code=abc&scope=chatgpt.tokens.use.direct+email+offline_access+openid+profile+resource.invoke&state=s1&client_id=oaiapp_9",
        );
        assert_eq!(r.code.as_deref(), Some("abc"));
        assert_eq!(r.state.as_deref(), Some("s1"));
        assert_eq!(r.client_id.as_deref(), Some("oaiapp_9"));
        let escopos = escopos_da_lista(r.scope.as_deref().unwrap_or_default());
        assert!(escopos.iter().any(|s| s == "chatgpt.tokens.use.direct"));
        assert_eq!(escopos.len(), 6);
        let negado = RespostaDoCallback::da_query("error=access_denied&state=s1");
        assert_eq!(negado.error.as_deref(), Some("access_denied"));
        assert!(negado.code.is_none());
    }

    #[test]
    fn erros_terminais_de_renovacao_pedem_novo_login() {
        for c in ERROS_TERMINAIS_DE_RENOVACAO {
            assert!(
                ErroOAuth::Recusado {
                    status: 400,
                    codigo: c.into()
                }
                .exige_novo_login()
            );
        }
        assert!(
            !ErroOAuth::Recusado {
                status: 400,
                codigo: "invalid_client".into()
            }
            .exige_novo_login()
        );
        assert_eq!(
            codigo_de_erro(r#"{"error":"invalid_grant","error_description":"x"}"#),
            "invalid_grant"
        );
        assert_eq!(
            codigo_de_erro(r#"{"error":{"code":"refresh_token_reused"}}"#),
            "refresh_token_reused"
        );
    }

    #[test]
    fn resposta_de_token_nao_vaza_segredos_no_debug() {
        let r: RespostaDeToken = serde_json::from_str(
            r#"{"access_token":"AT-secreto","refresh_token":"RT-secreto","id_token":"ID-secreto","token_type":"Bearer","expires_in":3600,"scope":"openid chatgpt.tokens.use.direct"}"#,
        )
        .expect("json");
        let d = format!("{r:?}");
        assert!(
            !d.contains("AT-secreto") && !d.contains("RT-secreto") && !d.contains("ID-secreto")
        );
        assert_eq!(r.escopos(), vec!["openid", "chatgpt.tokens.use.direct"]);
    }

    #[test]
    fn id_token_valido_passa_e_cada_regra_recusa_o_que_deve() {
        let par = ParDeTeste::novo();
        let chave = par.decodificacao();
        let iss = "https://auth.openai.com";
        let base = |aud: &str, nonce: &str, exp: i64, iss: &str| serde_json::json!({"sub": "user-1", "email": "a@b.c", "aud": aud, "iss": iss, "exp": exp, "iat": agora(), "nonce": nonce});
        let ok = par.assinar("k1", &base("oaiapp_1", "n1", agora() + 600, iss));
        let id = validar_id_token_com_chave(&ok, &chave, iss, "oaiapp_1", "n1").expect("valido");
        assert_eq!(id.sub, "user-1");
        assert_eq!(id.email.as_deref(), Some("a@b.c"));

        // Audiencia de outro cliente.
        assert!(validar_id_token_com_chave(&ok, &chave, iss, "oaiapp_2", "n1").is_err());
        // Nonce de outra tentativa.
        assert!(validar_id_token_com_chave(&ok, &chave, iss, "oaiapp_1", "n2").is_err());
        // Expirado (alem da folga de 60 s).
        let velho = par.assinar("k1", &base("oaiapp_1", "n1", agora() - 3600, iss));
        assert!(validar_id_token_com_chave(&velho, &chave, iss, "oaiapp_1", "n1").is_err());
        // Emissor errado.
        let outro = par.assinar(
            "k1",
            &base("oaiapp_1", "n1", agora() + 600, "https://evil.example"),
        );
        assert!(validar_id_token_com_chave(&outro, &chave, iss, "oaiapp_1", "n1").is_err());
        // Assinado por outra chave.
        let impostor =
            ParDeTeste::novo().assinar("k1", &base("oaiapp_1", "n1", agora() + 600, iss));
        assert!(validar_id_token_com_chave(&impostor, &chave, iss, "oaiapp_1", "n1").is_err());
    }

    #[test]
    fn chave_e_escolhida_pelo_kid_do_jwks() {
        let par = ParDeTeste::novo();
        let set: JwkSet = serde_json::from_value(par.jwks("k-atual")).expect("jwks");
        assert!(chave_do_set(&set, "k-atual").is_ok());
        assert!(chave_do_set(&set, "k-outro").is_err());
    }
}
