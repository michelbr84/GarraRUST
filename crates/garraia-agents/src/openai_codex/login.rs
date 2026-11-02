//! O login completo do Sign in with ChatGPT: callback loopback em
//! `127.0.0.1`, validacao do `state`, troca do codigo, validacao do ID token
//! e gravacao da credencial.
//!
//! Quem chama recebe a URL de autorizacao (para abrir no navegador do
//! sistema: a CLI abre sozinha, o painel da porta 3888 abre numa aba) e uma
//! tarefa que termina quando o navegador volta ao callback. O consentimento
//! e sempre da pessoa, no site da OpenAI — nada aqui simula isso.
//!
//! Ordem dos passos, como na documentacao (`sign-in` §2–§5):
//! 1. listener no ar **antes** de abrir o navegador;
//! 2. `state`, `nonce` e PKCE novos por tentativa;
//! 3. `state` validado primeiro; `error=access_denied` encerra sem troca;
//! 4. registro novo exige o `client_id` emitido; reautorizacao recusa um
//!    `client_id` diferente do salvo;
//! 5. troca com o client id emitido, o mesmo `redirect_uri` e `resource`;
//! 6. ID token validado (assinatura, `iss`, `aud`, `exp`, `nonce`) e, na
//!    reautorizacao, o mesmo `sub` da conta escolhida;
//! 7. credencial gravada (0600) com os escopos concedidos.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::credenciais::{
    ArmazemDeCredenciais, ContaChatGpt, ErroDeConta, RegistroDeCredencial, agora_iso,
};
use super::oauth::{self, ErroOAuth, PedidoDeAutorizacao, Pkce, RespostaDoCallback};
use super::{CAMINHO_DE_CALLBACK, CLIENTE_DE_REGISTRO, PORTA_DE_CALLBACK};

/// Quanto o callback espera o navegador voltar.
pub const ESPERA_PADRAO: Duration = Duration::from_secs(300);
const LIMITE_DO_CABECALHO: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct OpcoesDeLogin {
    /// Registrar outra conta/workspace em vez de reautorizar a conta em uso.
    pub nova_conta: bool,
    /// Porta do callback; sem ela, 1455 ou, se ocupada, uma porta livre.
    pub porta: Option<u16>,
    pub espera: Duration,
}

impl Default for OpcoesDeLogin {
    fn default() -> Self {
        Self {
            nova_conta: false,
            porta: None,
            espera: ESPERA_PADRAO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResultadoDoLogin {
    pub conta: ContaChatGpt,
    pub registro_novo: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ErroDeLogin {
    #[error(
        "o login foi cancelado ou o uso do plano nao foi autorizado no navegador (access_denied)"
    )]
    AcessoNegado,
    #[error(
        "o callback voltou com um `state` diferente desta tentativa; o resultado foi descartado"
    )]
    EstadoInvalido,
    #[error("o servidor de autorizacao devolveu o erro `{0}`")]
    ErroDoServidor(String),
    #[error(
        "o registro voltou sem o client id emitido; o registro ficou incompleto, tente de novo"
    )]
    RegistroIncompleto,
    #[error(
        "o callback trouxe um client id diferente do salvo para esta conta; o resultado foi recusado"
    )]
    ClienteDivergente,
    #[error("o callback voltou sem codigo de autorizacao")]
    SemCodigo,
    #[error("o codigo de autorizacao foi recusado (invalid_grant); comece um novo login")]
    CodigoRecusado,
    #[error("a conta que entrou nao e a conta escolhida; as credenciais salvas nao foram trocadas")]
    IdentidadeDivergente,
    #[error("o navegador nao voltou ao GarraIA em {0} s; o login expirou")]
    TempoEsgotado(u64),
    #[error("{0}")]
    OAuth(#[from] ErroOAuth),
    #[error("{0}")]
    Conta(#[from] ErroDeConta),
    #[error("callback local: {0}")]
    Io(String),
}

/// Um login aberto: a URL para o navegador e a espera pelo callback.
pub struct LoginEmAndamento {
    pub url: String,
    pub redirect_uri: String,
    pub registro_novo: bool,
    tarefa: tokio::task::JoinHandle<Result<ResultadoDoLogin, ErroDeLogin>>,
}

impl std::fmt::Debug for LoginEmAndamento {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginEmAndamento")
            .field("url", &oauth::url_redigida(&self.url))
            .field("redirect_uri", &self.redirect_uri)
            .field("registro_novo", &self.registro_novo)
            .finish()
    }
}

impl LoginEmAndamento {
    /// Espera o navegador voltar e devolve a conta conectada.
    pub async fn concluir(self) -> Result<ResultadoDoLogin, ErroDeLogin> {
        match self.tarefa.await {
            Ok(r) => r,
            Err(e) => Err(ErroDeLogin::Io(format!(
                "tarefa do callback interrompida: {e}"
            ))),
        }
    }

    pub fn terminou(&self) -> bool {
        self.tarefa.is_finished()
    }

    pub fn cancelar(&self) {
        self.tarefa.abort();
    }
}

struct Tentativa {
    client_id: String,
    registro_novo: bool,
    sujeito_esperado: Option<String>,
    host_id: String,
    redirect_uri: String,
    state: String,
    nonce: String,
    pkce: Pkce,
}

/// Abre o callback e devolve a URL de autorizacao. A conta em uso e
/// reautorizada (com o client id emitido e as dicas); sem conta, ou com
/// `nova_conta`, e um registro novo (`dynamic_agent_client`).
pub async fn iniciar(
    armazem: &ArmazemDeCredenciais,
    opcoes: OpcoesDeLogin,
) -> Result<LoginEmAndamento, ErroDeLogin> {
    let host_id = armazem.host_id()?;
    let ativo: Option<RegistroDeCredencial> = if opcoes.nova_conta {
        None
    } else {
        armazem.registro_ativo()?
    };

    let listener = abrir_callback(opcoes.porta).await?;
    let porta = listener
        .local_addr()
        .map_err(|e| ErroDeLogin::Io(e.to_string()))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{porta}{CAMINHO_DE_CALLBACK}");

    let (client_id, registro_novo, sujeito_esperado, id_token_hint, login_hint) = match &ativo {
        Some(r) => (
            r.client_id.clone(),
            false,
            Some(r.subject.clone()),
            r.id_token.clone(),
            r.email.clone(),
        ),
        None => (CLIENTE_DE_REGISTRO.to_string(), true, None, None, None),
    };
    let tentativa = Tentativa {
        client_id,
        registro_novo,
        sujeito_esperado,
        host_id,
        redirect_uri: redirect_uri.clone(),
        state: oauth::aleatorio_b64(32)?,
        nonce: oauth::aleatorio_b64(32)?,
        pkce: Pkce::novo()?,
    };
    let url = oauth::url_de_autorizacao(
        armazem.endpoints(),
        &PedidoDeAutorizacao {
            client_id: &tentativa.client_id,
            ext_agent_host_id: &tentativa.host_id,
            redirect_uri: &tentativa.redirect_uri,
            state: &tentativa.state,
            nonce: &tentativa.nonce,
            desafio: &tentativa.pkce.desafio,
            id_token_hint: id_token_hint.as_deref(),
            login_hint: login_hint.as_deref(),
        },
    );

    let armazem = armazem.clone();
    let espera = opcoes.espera;
    let tarefa = tokio::spawn(async move {
        let (resposta, conexao) = aguardar_callback(listener, espera).await?;
        let resultado = processar(&armazem, &tentativa, resposta).await;
        let _ = responder(conexao, &pagina_do_resultado(&resultado)).await;
        resultado
    });
    Ok(LoginEmAndamento {
        url,
        redirect_uri,
        registro_novo,
        tarefa,
    })
}

async fn abrir_callback(porta: Option<u16>) -> Result<TcpListener, ErroDeLogin> {
    let preferida = porta.unwrap_or(PORTA_DE_CALLBACK);
    match TcpListener::bind(("127.0.0.1", preferida)).await {
        Ok(l) => Ok(l),
        // A documentacao permite outra porta nos logins seguintes; so a
        // porta muda — esquema, host e caminho ficam fixos.
        Err(_) if porta.is_none() => TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| {
            ErroDeLogin::Io(format!("nao consegui abrir o callback em 127.0.0.1: {e}"))
        }),
        Err(e) => Err(ErroDeLogin::Io(format!(
            "porta {preferida} do callback indisponivel: {e}"
        ))),
    }
}

async fn processar(
    armazem: &ArmazemDeCredenciais,
    t: &Tentativa,
    resposta: RespostaDoCallback,
) -> Result<ResultadoDoLogin, ErroDeLogin> {
    // 1. `state` antes de qualquer outra coisa, inclusive do erro.
    if resposta.state.as_deref() != Some(t.state.as_str()) {
        return Err(ErroDeLogin::EstadoInvalido);
    }
    if let Some(erro) = resposta.error {
        return Err(if erro == "access_denied" {
            ErroDeLogin::AcessoNegado
        } else {
            ErroDeLogin::ErroDoServidor(erro)
        });
    }
    // 2. O client id: emitido no registro; igual ao salvo na reautorizacao.
    let client_id = if t.registro_novo {
        match resposta.client_id {
            Some(c) if !c.is_empty() && c != CLIENTE_DE_REGISTRO => c,
            _ => return Err(ErroDeLogin::RegistroIncompleto),
        }
    } else {
        if let Some(c) = &resposta.client_id
            && c != &t.client_id
        {
            return Err(ErroDeLogin::ClienteDivergente);
        }
        t.client_id.clone()
    };
    let code = resposta.code.ok_or(ErroDeLogin::SemCodigo)?;

    // 3. Troca do codigo.
    let token = oauth::trocar_codigo(
        armazem.cliente(),
        armazem.endpoints(),
        &client_id,
        &code,
        &t.pkce.verificador,
        &t.redirect_uri,
    )
    .await
    .map_err(|e| match e {
        ErroOAuth::Recusado { ref codigo, .. } if codigo == "invalid_grant" => {
            ErroDeLogin::CodigoRecusado
        }
        outro => ErroDeLogin::OAuth(outro),
    })?;

    // 4. ID token e identidade.
    let id_token = token.id_token.clone().ok_or_else(|| {
        ErroDeLogin::OAuth(ErroOAuth::RespostaInvalida(
            "a troca nao devolveu id_token".into(),
        ))
    })?;
    let identidade = oauth::validar_id_token(
        armazem.cliente(),
        armazem.endpoints(),
        &id_token,
        &client_id,
        &t.nonce,
    )
    .await?;
    if let Some(esperado) = &t.sujeito_esperado
        && esperado != &identidade.sub
    {
        return Err(ErroDeLogin::IdentidadeDivergente);
    }

    // 5. Escopos concedidos: os da resposta de token; se ela nao trouxer,
    // os do callback.
    let mut escopos = token.escopos();
    if escopos.is_empty() {
        escopos = resposta
            .scope
            .as_deref()
            .map(oauth::escopos_da_lista)
            .unwrap_or_default();
    }
    let registro = RegistroDeCredencial {
        email: identidade.email,
        name: identidade.nome,
        issuer: armazem.endpoints().issuer.clone(),
        subject: identidade.sub,
        client_id,
        ext_agent_host_id: t.host_id.clone(),
        id_token: Some(id_token),
        access_token: Some(token.access_token.clone()),
        refresh_token: token.refresh_token.clone(),
        token_type: token.token_type.clone(),
        expires_in: token.expires_in.unwrap_or(3600),
        scopes: escopos,
        saved_at: agora_iso(),
        earliest_refresh_at: token.earliest_refresh_at.clone(),
    };
    armazem.salvar_e_ativar(&registro)?;
    Ok(ResultadoDoLogin {
        conta: ContaChatGpt::from(&registro),
        registro_novo: t.registro_novo,
    })
}

/// Espera a primeira requisicao `GET /auth/callback` de um cliente local.
/// Outras rotas (favicon, sondas) recebem 404 e a espera continua.
async fn aguardar_callback(
    listener: TcpListener,
    espera: Duration,
) -> Result<(RespostaDoCallback, TcpStream), ErroDeLogin> {
    let prazo = tokio::time::Instant::now() + espera;
    loop {
        let aceito = tokio::time::timeout_at(prazo, listener.accept())
            .await
            .map_err(|_| ErroDeLogin::TempoEsgotado(espera.as_secs()))?;
        let (mut conexao, par) = match aceito {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !par.ip().is_loopback() {
            continue;
        }
        let Ok(Ok(cabecalho)) =
            tokio::time::timeout(Duration::from_secs(10), ler_cabecalho(&mut conexao)).await
        else {
            continue;
        };
        match alvo_do_callback(&cabecalho) {
            Some(query) => return Ok((RespostaDoCallback::da_query(&query), conexao)),
            None => {
                let _ = escrever_http(
                    &mut conexao,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    "nao encontrado",
                )
                .await;
            }
        }
    }
}

async fn ler_cabecalho(conexao: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut bloco = [0u8; 1024];
    loop {
        let n = conexao.read(&mut bloco).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&bloco[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= LIMITE_DO_CABECALHO {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// A query de `GET /auth/callback?...`, ou `None` para qualquer outra coisa.
fn alvo_do_callback(cabecalho: &str) -> Option<String> {
    let linha = cabecalho.lines().next()?;
    let mut partes = linha.split_whitespace();
    if partes.next()? != "GET" {
        return None;
    }
    let alvo = partes.next()?;
    let (caminho, query) = alvo.split_once('?').unwrap_or((alvo, ""));
    (caminho == CAMINHO_DE_CALLBACK).then(|| query.to_string())
}

async fn escrever_http(
    conexao: &mut TcpStream,
    status: &str,
    tipo: &str,
    corpo: &str,
) -> std::io::Result<()> {
    let resposta = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {tipo}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{corpo}",
        corpo.len()
    );
    conexao.write_all(resposta.as_bytes()).await?;
    conexao.flush().await
}

async fn responder(mut conexao: TcpStream, html: &str) -> std::io::Result<()> {
    escrever_http(&mut conexao, "200 OK", "text/html; charset=utf-8", html).await?;
    conexao.shutdown().await
}

fn escapar_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn pagina_do_resultado(r: &Result<ResultadoDoLogin, ErroDeLogin>) -> String {
    let (titulo, texto) = match r {
        Ok(ok) => {
            let quem = ok.conta.email.clone().unwrap_or_else(|| "sua conta".into());
            if ok.conta.plano_habilitado {
                (
                    "Você está usando seu plano do ChatGPT",
                    format!("O GarraIA está conectado como {quem}. Pode fechar esta aba."),
                )
            } else {
                (
                    "Entrou, mas sem o uso do plano",
                    format!(
                        "{quem} entrou sem autorizar o uso do plano do ChatGPT. Volte ao GarraIA para autorizar."
                    ),
                )
            }
        }
        Err(e) => ("Não foi possível conectar", e.to_string()),
    };
    format!(
        "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><title>GarraIA</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"font-family:Inter,system-ui,sans-serif;background:#0b1220;color:#e6edf3;display:grid;place-items:center;min-height:100vh;margin:0\">\
         <main style=\"max-width:520px;padding:24px\"><h1 style=\"color:#16d9ff;font-size:22px\">{}</h1><p>{}</p></main></body></html>",
        escapar_html(titulo),
        escapar_html(&texto)
    )
}

#[cfg(test)]
mod tests {
    use super::super::Endpoints;
    use super::super::oauth::testes_rsa::ParDeTeste;
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn so_get_no_caminho_do_callback_e_aceito() {
        assert_eq!(
            alvo_do_callback("GET /auth/callback?code=1&state=2 HTTP/1.1\r\n").as_deref(),
            Some("code=1&state=2")
        );
        assert_eq!(
            alvo_do_callback("GET /auth/callback HTTP/1.1\r\n").as_deref(),
            Some("")
        );
        assert!(alvo_do_callback("GET /callback?code=1 HTTP/1.1\r\n").is_none());
        assert!(alvo_do_callback("POST /auth/callback?code=1 HTTP/1.1\r\n").is_none());
        assert!(alvo_do_callback("GET /favicon.ico HTTP/1.1\r\n").is_none());
    }

    #[test]
    fn pagina_escapa_o_texto() {
        let r: Result<ResultadoDoLogin, ErroDeLogin> =
            Err(ErroDeLogin::ErroDoServidor("<script>".into()));
        let html = pagina_do_resultado(&r);
        assert!(html.contains("&lt;script&gt;") && !html.contains("<script>"));
    }

    /// Formularios recebidos pelo endpoint de token falso.
    type Formularios = Arc<Mutex<Vec<Vec<(String, String)>>>>;

    /// Servidor de autorizacao falso: token endpoint e JWKS. Guarda os
    /// formularios recebidos para os testes conferirem.
    struct Falso {
        endpoints: Endpoints,
        formularios: Formularios,
        _desligar: tokio::sync::oneshot::Sender<()>,
    }

    async fn servidor_falso(
        par: Arc<ParDeTeste>,
        sub: &'static str,
        escopos: &'static str,
    ) -> Falso {
        use axum::{Router, extract::State, routing::get, routing::post};
        let formularios: Formularios = Arc::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let issuer = base.clone();
        #[derive(Clone)]
        struct Estado {
            par: Arc<ParDeTeste>,
            formularios: Formularios,
            issuer: String,
            sub: &'static str,
            escopos: &'static str,
        }
        let estado = Estado {
            par,
            formularios: formularios.clone(),
            issuer: issuer.clone(),
            sub,
            escopos,
        };
        let app = Router::new()
            .route(
                "/token",
                post(|State(s): State<Estado>, corpo: String| async move {
                    let pares: Vec<(String, String)> = reqwest::Url::parse(&format!("http://x/?{corpo}"))
                        .map(|u| u.query_pairs().into_owned().collect())
                        .unwrap_or_default();
                    let client_id = pares.iter().find(|(k, _)| k == "client_id").map(|(_, v)| v.clone()).unwrap_or_default();
                    s.formularios.lock().expect("lock").push(pares);
                    // O nonce do ID token tem de ser o da tentativa: o teste o
                    // injeta pelo `code` (code = "c:<nonce>").
                    let nonce = s.formularios.lock().expect("lock").last().and_then(|f| {
                        f.iter().find(|(k, _)| k == "code").map(|(_, v)| v.trim_start_matches("c:").to_string())
                    });
                    let id = s.par.assinar(
                        "k1",
                        &serde_json::json!({"sub": s.sub, "email": "ana@exemplo.com", "aud": client_id, "iss": s.issuer,
                            "exp": chrono::Utc::now().timestamp() + 600, "iat": chrono::Utc::now().timestamp(), "nonce": nonce}),
                    );
                    axum::Json(serde_json::json!({"access_token": "AT1", "refresh_token": "RT1", "id_token": id,
                        "token_type": "Bearer", "expires_in": 3600, "scope": s.escopos}))
                }),
            )
            .route("/jwks", get(|State(s): State<Estado>| async move { axum::Json(s.par.jwks("k1")) }))
            .with_state(estado);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        Falso {
            endpoints: Endpoints {
                authorize: format!("{base}/authorize"),
                token: format!("{base}/token"),
                revoke: format!("{base}/revoke"),
                jwks: format!("{base}/jwks"),
                issuer,
                api_base: format!("{base}/v1"),
            },
            formularios,
            _desligar: tx,
        }
    }

    fn query_da_url(url: &str) -> std::collections::HashMap<String, String> {
        reqwest::Url::parse(url)
            .expect("url")
            .query_pairs()
            .into_owned()
            .collect()
    }

    /// Faz o papel do navegador voltando ao callback (o consentimento real
    /// e da pessoa; aqui so o redirecionamento final e simulado).
    async fn navegador_volta(redirect_uri: &str, query: &str) -> String {
        reqwest::get(format!("{redirect_uri}?{query}"))
            .await
            .expect("callback")
            .text()
            .await
            .expect("corpo")
    }

    #[tokio::test]
    async fn registro_novo_troca_o_codigo_valida_e_grava_a_conta() {
        let par = Arc::new(ParDeTeste::novo());
        let falso = servidor_falso(
            par,
            "user-1",
            "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        )
        .await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = ArmazemDeCredenciais::new(tmp.path().join("oc"))
            .with_endpoints(falso.endpoints.clone());
        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        assert!(login.registro_novo);
        let q = query_da_url(&login.url);
        assert_eq!(q["client_id"], "dynamic_agent_client");
        let pagina = navegador_volta(
            &login.redirect_uri,
            &format!(
                "code=c:{}&state={}&client_id=oaiapp_novo&scope=openid",
                q["nonce"], q["state"]
            ),
        )
        .await;
        let r = login.concluir().await.expect("login ok");
        assert!(pagina.contains("plano do ChatGPT"));
        assert_eq!(r.conta.client_id, "oaiapp_novo");
        assert_eq!(r.conta.email.as_deref(), Some("ana@exemplo.com"));
        assert!(r.conta.plano_habilitado);
        // A troca usou o cliente emitido, o PKCE e o mesmo redirect.
        let form = falso.formularios.lock().expect("lock")[0].clone();
        let campo = |k: &str| {
            form.iter()
                .find(|(c, _)| c == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(campo("grant_type"), "authorization_code");
        assert_eq!(campo("client_id"), "oaiapp_novo");
        assert_eq!(campo("redirect_uri"), login_redirect(&q));
        assert_eq!(campo("resource"), "https://api.openai.com/v1");
        assert_eq!(
            super::oauth::desafio_s256(&campo("code_verifier")),
            q["code_challenge"]
        );
        assert_eq!(armazem.access_token().await.expect("token"), "AT1");
    }

    fn login_redirect(q: &std::collections::HashMap<String, String>) -> String {
        q["redirect_uri"].clone()
    }

    #[tokio::test]
    async fn state_errado_e_access_denied_nao_trocam_codigo() {
        let par = Arc::new(ParDeTeste::novo());
        let falso = servidor_falso(par, "user-1", "openid chatgpt.tokens.use.direct").await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = ArmazemDeCredenciais::new(tmp.path().join("oc"))
            .with_endpoints(falso.endpoints.clone());

        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        navegador_volta(
            &login.redirect_uri,
            "code=c:x&state=forjado&client_id=oaiapp_x",
        )
        .await;
        assert!(matches!(
            login.concluir().await,
            Err(ErroDeLogin::EstadoInvalido)
        ));

        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        let q = query_da_url(&login.url);
        navegador_volta(
            &login.redirect_uri,
            &format!("error=access_denied&state={}", q["state"]),
        )
        .await;
        assert!(matches!(
            login.concluir().await,
            Err(ErroDeLogin::AcessoNegado)
        ));

        assert!(
            falso.formularios.lock().expect("lock").is_empty(),
            "nenhum codigo foi trocado"
        );
        assert!(armazem.conta_ativa().is_none());
    }

    #[tokio::test]
    async fn registro_sem_client_id_emitido_fica_incompleto() {
        let par = Arc::new(ParDeTeste::novo());
        let falso = servidor_falso(par, "user-1", "openid chatgpt.tokens.use.direct").await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = ArmazemDeCredenciais::new(tmp.path().join("oc"))
            .with_endpoints(falso.endpoints.clone());
        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        let q = query_da_url(&login.url);
        navegador_volta(
            &login.redirect_uri,
            &format!("code=c:{}&state={}", q["nonce"], q["state"]),
        )
        .await;
        assert!(matches!(
            login.concluir().await,
            Err(ErroDeLogin::RegistroIncompleto)
        ));
    }

    #[tokio::test]
    async fn reautorizacao_reusa_o_cliente_e_recusa_outra_identidade() {
        let par = Arc::new(ParDeTeste::novo());
        // O servidor sempre devolve user-2: a conta salva e user-1.
        let falso = servidor_falso(par, "user-2", "openid chatgpt.tokens.use.direct").await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = ArmazemDeCredenciais::new(tmp.path().join("oc"))
            .with_endpoints(falso.endpoints.clone());
        let mut salvo = super::super::credenciais::RegistroDeCredencial {
            email: Some("ana@exemplo.com".into()),
            name: None,
            issuer: falso.endpoints.issuer.clone(),
            subject: "user-1".into(),
            client_id: "oaiapp_salvo".into(),
            ext_agent_host_id: armazem.host_id().expect("host"),
            id_token: Some("ID-antigo".into()),
            access_token: Some("AT-antigo".into()),
            refresh_token: Some("RT-antigo".into()),
            token_type: "Bearer".into(),
            expires_in: 3600,
            scopes: vec!["chatgpt.tokens.use.direct".into()],
            saved_at: agora_iso(),
            earliest_refresh_at: None,
        };
        armazem.salvar_e_ativar(&salvo).expect("salva");

        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        assert!(!login.registro_novo);
        let q = query_da_url(&login.url);
        assert_eq!(q["client_id"], "oaiapp_salvo");
        assert_eq!(q["id_token_hint"], "ID-antigo");
        assert_eq!(q["login_hint"], "ana@exemplo.com");
        assert!(!q.contains_key("agent_name_hint"));

        // Client id diferente no callback: recusado.
        navegador_volta(
            &login.redirect_uri,
            &format!(
                "code=c:{}&state={}&client_id=oaiapp_outro",
                q["nonce"], q["state"]
            ),
        )
        .await;
        assert!(matches!(
            login.concluir().await,
            Err(ErroDeLogin::ClienteDivergente)
        ));

        // Mesmo cliente, mas a identidade que voltou e outra: recusado e
        // as credenciais salvas continuam as antigas.
        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        let q = query_da_url(&login.url);
        navegador_volta(
            &login.redirect_uri,
            &format!("code=c:{}&state={}", q["nonce"], q["state"]),
        )
        .await;
        assert!(matches!(
            login.concluir().await,
            Err(ErroDeLogin::IdentidadeDivergente)
        ));
        salvo = armazem
            .registro("oaiapp_salvo")
            .expect("le")
            .expect("existe");
        assert_eq!(salvo.access_token.as_deref(), Some("AT-antigo"));
    }

    #[tokio::test]
    async fn login_sem_escopo_do_plano_grava_conta_sem_plano() {
        let par = Arc::new(ParDeTeste::novo());
        let falso = servidor_falso(par, "user-1", "openid profile email").await;
        let tmp = tempfile::tempdir().expect("tmp");
        let armazem = ArmazemDeCredenciais::new(tmp.path().join("oc"))
            .with_endpoints(falso.endpoints.clone());
        let login = iniciar(
            &armazem,
            OpcoesDeLogin {
                porta: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect("inicia");
        let q = query_da_url(&login.url);
        let pagina = navegador_volta(
            &login.redirect_uri,
            &format!(
                "code=c:{}&state={}&client_id=oaiapp_n",
                q["nonce"], q["state"]
            ),
        )
        .await;
        let r = login.concluir().await.expect("login");
        assert!(!r.conta.plano_habilitado);
        assert!(pagina.contains("sem o uso do plano"));
        assert!(matches!(
            armazem.access_token().await,
            Err(ErroDeConta::PlanoNaoHabilitado { .. })
        ));
    }
}
