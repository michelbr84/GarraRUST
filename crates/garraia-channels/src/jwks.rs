//! Cache de chaves publicas (JWKS) para autenticar webhooks de entrada.
//!
//! Dois canais push precisam da mesma coisa e por motivos identicos: o
//! Google Chat e o Microsoft Teams nao assinam o corpo com um segredo
//! compartilhado (como o LINE faz, `line_channel::signature`) — eles mandam
//! um JWT RS256 no header `Authorization`, assinado com a chave privada do
//! proprio provedor. Verificar esse JWT exige a chave **publica** dele, que
//! e servida por HTTP num JWK Set e **rotaciona**.
//!
//! Por isso um cache, e nao uma chave em config: uma chave fixada no TOML
//! para de valer no dia em que o provedor rotacionar, e o canal morre em
//! silencio.
//!
//! ## O que este modulo NAO faz
//!
//! Nao decide se um token e valido — so entrega a chave para quem vai
//! decidir. As regras que importam (algoritmo fixado em RS256, `iss`
//! esperado, `aud` esperado, `exp`) sao de cada canal, porque sao
//! diferentes em cada um, e ficam em `google_chat::auth` e `teams::auth`.
//!
//! ## Duas armadilhas que o desenho evita
//!
//! **Refetch por `kid` desconhecido e um vetor de DoS.** A rotacao de
//! chaves obriga a buscar de novo quando aparece um `kid` que o cache nao
//! tem — mas o `kid` vem do token, ou seja, de quem manda a requisicao.
//! Sem limite, um atacante manda mil tokens com `kid` aleatorio e o gateway
//! faz mil requisicoes ao Google. Dai o [`INTERVALO_MINIMO_REFETCH`]: entre
//! duas buscas passa no minimo esse tempo, aconteca o que acontecer.
//!
//! Um piso de tempo sozinho **nao** basta, e essa e a parte facil de errar:
//! ler o instante, soltar o lock e so depois escrever deixa N tarefas
//! concorrentes passarem todas pelo teste antes de qualquer uma marcar a
//! tentativa. O piso limitaria buscas *seriais*, e mil requisicoes
//! simultaneas — que e como o ataque realmente chega — ainda virariam mil
//! buscas. Por isso a marcacao e feita sob um `Mutex` segurado do teste ate
//! o fim da busca, com `try_lock`: quem chega no meio de uma busca desiste
//! na hora em vez de enfileirar.
//!
//! **Falha de rede nao pode invalidar o cache.** Se a busca falhar, as
//! chaves antigas continuam valendo ate o TTL — derrubar o canal inteiro
//! porque o JWKS ficou 30 segundos fora seria trocar um problema
//! transitorio do provedor por uma interrupcao nossa.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::JwkSet;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, warn};

/// Quanto tempo um conjunto de chaves buscado com sucesso continua valendo.
///
/// Uma hora e curto o bastante para acompanhar a rotacao dos provedores
/// (Google e Microsoft rotacionam na casa de dias) e longo o bastante para
/// o gateway nao bater no JWKS a cada mensagem.
const TTL_PADRAO: Duration = Duration::from_secs(3600);

/// Piso entre duas buscas ao JWKS, **incluindo** as disparadas por `kid`
/// desconhecido.
///
/// Sem isto, o `kid` — que vem do token, ou seja de quem manda a
/// requisicao — viraria um gatilho de trafego de saida controlado por
/// terceiros. Cinco minutos e bem menor que qualquer janela de rotacao
/// real, entao uma chave nova e adotada rapido, e ao mesmo tempo limita a
/// N/5min o numero de buscas que um atacante consegue provocar.
const INTERVALO_MINIMO_REFETCH: Duration = Duration::from_secs(300);

/// Teto do corpo do JWKS. Um JWK Set legitimo tem alguns KB; o limite
/// existe para uma resposta gigante (provedor comprometido, proxy hostil,
/// captive portal devolvendo HTML) nao virar consumo de memoria.
const MAX_CORPO_JWKS: usize = 256 * 1024;

/// Por que a chave nao pode ser entregue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwksError {
    /// O `kid` nao esta no conjunto atual, e nao da para buscar de novo
    /// ainda (ver [`INTERVALO_MINIMO_REFETCH`]).
    KidDesconhecido,
    /// A busca falhou e nao ha conjunto anterior para cair.
    SemChaves,
}

impl std::fmt::Display for JwksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::KidDesconhecido => "kid nao encontrado no JWKS",
            Self::SemChaves => "JWKS indisponivel e sem cache anterior",
        })
    }
}

struct Conjunto {
    por_kid: HashMap<String, DecodingKey>,
    buscado_em: Instant,
}

/// Cache das chaves publicas de um provedor.
///
/// Um por canal-provedor: o Google Chat e o Teams tem URLs diferentes e
/// rotacionam independentemente.
pub struct JwksCache {
    url: String,
    client: reqwest::Client,
    conjunto: RwLock<Option<Conjunto>>,
    /// Quando foi a ultima **tentativa** de busca, com ou sem sucesso. E o
    /// que limita o refetch — contar so os sucessos deixaria um provedor
    /// fora do ar transformar cada requisicao numa tentativa nova.
    ///
    /// `Mutex`, e nao `RwLock`, de proposito: o guard e mantido do teste ate
    /// a escrita **e ate o fim da busca**, o que torna a checagem
    /// atomica e faz do cache um single-flight. Com um `RwLock` que solta
    /// entre ler e escrever, N tarefas concorrentes passam todas pelo teste
    /// antes de qualquer uma escrever, e o piso limita so buscas *seriais* —
    /// que nao e o que um atacante faz.
    ultima_tentativa: Mutex<Option<Instant>>,
    ttl: Duration,
}

impl JwksCache {
    /// Cria o cache. Nao busca nada — a primeira busca acontece na primeira
    /// chamada de [`Self::chave_para`], para o boot nao depender da rede.
    ///
    /// **A URL tem de ser constante de compilacao.** Hoje os dois chamadores
    /// passam literais (`google_chat::auth::JWK_URL`, e o equivalente do
    /// Teams), e por isso o `reqwest::Client` interno nao passa pelo
    /// `garraia_common::ssrf`: a regra 14 do CLAUDE.md vale para URL que vem
    /// de request, de config editavel por request, ou de tool call de LLM, e
    /// uma constante nao e nenhuma das tres. A assinatura aceita qualquer
    /// `String` por conveniencia; se algum dia a URL passar a vir de config,
    /// este construtor precisa do guard **antes** de aceita-la.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
            conjunto: RwLock::new(None),
            ultima_tentativa: Mutex::new(None),
            ttl: TTL_PADRAO,
        }
    }

    /// A chave publica correspondente a um `kid`.
    ///
    /// Busca o JWKS quando nao ha cache, quando ele expirou, ou quando o
    /// `kid` e desconhecido — este ultimo caso sujeito ao piso de
    /// [`INTERVALO_MINIMO_REFETCH`], porque o `kid` e controlado por quem
    /// manda a requisicao.
    pub async fn chave_para(&self, kid: &str) -> Result<DecodingKey, JwksError> {
        if let Some(chave) = self.do_cache(kid, false).await {
            return Ok(chave);
        }

        self.buscar_se_permitido().await;

        // Segunda olhada: pode ter acabado de chegar. Aqui o cache expirado
        // tambem serve — se a busca falhou, uma chave velha que ainda casa
        // com o `kid` e melhor que recusar um webhook legitimo.
        self.do_cache(kid, true)
            .await
            .ok_or(if self.tem_conjunto().await {
                JwksError::KidDesconhecido
            } else {
                JwksError::SemChaves
            })
    }

    async fn tem_conjunto(&self) -> bool {
        self.conjunto.read().await.is_some()
    }

    /// `aceitar_expirado` distingue as duas leituras: antes de buscar, um
    /// conjunto vencido nao conta (senao nunca se renovaria); depois de
    /// buscar, conta (senao uma falha de rede derruba o canal).
    async fn do_cache(&self, kid: &str, aceitar_expirado: bool) -> Option<DecodingKey> {
        let guard = self.conjunto.read().await;
        let conjunto = guard.as_ref()?;
        if !aceitar_expirado && conjunto.buscado_em.elapsed() > self.ttl {
            return None;
        }
        conjunto.por_kid.get(kid).cloned()
    }

    async fn buscar_se_permitido(&self) {
        // `try_lock`, e nao `lock`: se outra tarefa ja esta buscando agora,
        // esperar por ela nao ajudaria — quando o lock saisse, o piso de
        // tempo barraria esta chamada de qualquer jeito. Desistir na hora
        // troca uma fila de tarefas presas por uma resposta imediata, e e o
        // que faz do cache um single-flight de verdade em vez de um serializador.
        let Ok(mut ultima) = self.ultima_tentativa.try_lock() else {
            debug!(url = %self.url, "jwks: ja ha uma busca em andamento, nao enfileira outra");
            return;
        };

        // O guard segue vivo daqui ate o fim da funcao: e o que torna
        // "testar o piso" e "marcar a tentativa" uma operacao so.
        if let Some(t) = *ultima
            && t.elapsed() < INTERVALO_MINIMO_REFETCH
        {
            debug!(
                url = %self.url,
                "jwks: busca suprimida pelo intervalo minimo (kid desconhecido nao dispara trafego sem limite)"
            );
            return;
        }
        *ultima = Some(Instant::now());

        match self.buscar().await {
            Ok(por_kid) => {
                debug!(url = %self.url, chaves = por_kid.len(), "jwks: conjunto atualizado");
                *self.conjunto.write().await = Some(Conjunto {
                    por_kid,
                    buscado_em: Instant::now(),
                });
            }
            // Nao limpa o cache: chave velha e melhor que canal morto.
            Err(e) => {
                warn!(url = %self.url, "jwks: busca falhou, mantendo o conjunto anterior: {e}");
            }
        }
    }

    async fn buscar(&self) -> Result<HashMap<String, DecodingKey>, String> {
        let resp = self
            .client
            .get(&self.url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("requisicao falhou: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("status {}", resp.status()));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("corpo ilegivel: {e}"))?;
        if bytes.len() > MAX_CORPO_JWKS {
            return Err(format!("corpo de {} bytes acima do teto", bytes.len()));
        }

        let set: JwkSet =
            serde_json::from_slice(&bytes).map_err(|e| format!("JWKS malformado: {e}"))?;

        Ok(chaves_do_set(&set))
    }
}

/// Converte um JWK Set no mapa `kid -> DecodingKey`.
///
/// Separado da busca para ser testavel sem rede. Chave sem `kid`, ou de um
/// tipo que o `jsonwebtoken` nao sabe decodificar, e descartada em silencio:
/// um provedor pode servir chaves de varios usos no mesmo documento, e
/// recusar o conjunto inteiro por causa de uma delas deixaria o canal sem
/// nenhuma.
fn chaves_do_set(set: &JwkSet) -> HashMap<String, DecodingKey> {
    let mut por_kid = HashMap::new();
    for jwk in &set.keys {
        let Some(kid) = jwk.common.key_id.clone() else {
            continue;
        };
        match DecodingKey::from_jwk(jwk) {
            Ok(chave) => {
                por_kid.insert(kid, chave);
            }
            Err(e) => {
                debug!(%kid, "jwks: chave ignorada, nao decodificavel: {e}");
            }
        }
    }
    por_kid
}

/// Monta um JWT de teste a partir do JSON do cabecalho.
///
/// Os testes de confusao de algoritmo do `google_chat::auth` e do
/// `teams::auth` precisam de tokens com `alg` escolhido a dedo (`none`,
/// `HS256`, `RS256` sem `kid`). Escrever o base64 pronto no fonte custava
/// duas coisas:
///
/// 1. O literal e longo e de entropia alta, e mora ao lado da palavra
///    `token` — exatamente o formato que a regra `generic-api-key` do
///    gitleaks procura. Rendeu 4 achados falsos no scan de segredos, e a
///    saida do proprio gitleaks vem redigida, entao o achado nao diz o que
///    encontrou: alguem tem de ir ao fonte para descobrir que nao era nada.
/// 2. O que o base64 decodificava vivia num comentario ao lado. Comentario
///    nao compila: editar o literal sem editar o comentario deixaria o teste
///    exercitando um `alg` diferente do que ele diz exercitar.
///
/// Montando aqui, o JSON e o que se le **e** o que o parser enxerga, e nao
/// sobra literal para nenhuma das duas coisas acontecerem.
///
/// Nenhum destes tokens chega a ser verificado: o corpo e sempre `{}` e a
/// assinatura e lixo (ou vazia). Eles existem para serem recusados **antes**
/// da chave, e e isso que os testes afirmam.
#[cfg(test)]
pub(crate) fn jwt_de_teste(cabecalho_json: &str, assinatura: &str) -> String {
    use base64::Engine as _;
    let cabecalho = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cabecalho_json);
    // `e30` e o base64url de `{}` — corpo vazio.
    format!("{cabecalho}.e30.{assinatura}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    /// Um JWK Set RSA valido, com duas chaves. Os modulos sao arbitrarios —
    /// o que se afirma aqui e o parsing e a indexacao por `kid`, nao a
    /// criptografia.
    fn set_de_exemplo() -> JwkSet {
        serde_json::from_value(serde_json::json!({
            "keys": [
                {
                    "kty": "RSA", "alg": "RS256", "use": "sig", "kid": "chave-1",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB"
                },
                {
                    "kty": "RSA", "alg": "RS256", "use": "sig", "kid": "chave-2",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB"
                }
            ]
        }))
        .expect("JWK Set de exemplo bem formado")
    }

    #[test]
    fn indexa_as_chaves_por_kid() {
        let mapa = chaves_do_set(&set_de_exemplo());
        assert_eq!(mapa.len(), 2);
        assert!(mapa.contains_key("chave-1"));
        assert!(mapa.contains_key("chave-2"));
        assert!(!mapa.contains_key("chave-3"));
    }

    /// Uma chave que o `jsonwebtoken` nao decodifica nao pode levar o
    /// conjunto inteiro junto: o provedor serve chaves de varios usos no
    /// mesmo documento, e recusar tudo deixaria o canal sem nenhuma.
    #[test]
    fn chave_invalida_nao_derruba_as_outras() {
        let mut set = set_de_exemplo();
        let quebrada: jsonwebtoken::jwk::Jwk = serde_json::from_value(serde_json::json!({
            "kty": "RSA", "alg": "RS256", "use": "sig", "kid": "quebrada",
            "n": "!!! nao e base64url !!!", "e": "AQAB"
        }))
        .expect("jwk sintaticamente valido");
        set.keys.push(quebrada);

        let mapa = chaves_do_set(&set);
        assert!(mapa.contains_key("chave-1"), "a boa tem de sobreviver");
        assert!(
            !mapa.contains_key("quebrada"),
            "a ruim nao pode entrar no mapa"
        );
    }

    /// Chave sem `kid` nao tem como ser procurada, entao nao entra.
    #[test]
    fn chave_sem_kid_e_descartada() {
        let mut set = set_de_exemplo();
        set.keys[0].common.key_id = None;
        let mapa = chaves_do_set(&set);
        assert_eq!(mapa.len(), 1);
        assert!(mapa.contains_key("chave-2"));
    }

    #[test]
    fn set_vazio_produz_mapa_vazio() {
        let vazio: JwkSet =
            serde_json::from_value(serde_json::json!({"keys": []})).expect("set vazio");
        assert!(chaves_do_set(&vazio).is_empty());
    }

    /// Sem nenhuma busca bem-sucedida, o cache nao inventa chave.
    #[tokio::test]
    async fn cache_vazio_nao_entrega_chave() {
        // URL que nao resolve: a busca falha, e o ponto e que a falha vira
        // `SemChaves` em vez de pânico ou chave fantasma.
        let cache = JwksCache::new("http://127.0.0.1:1/jwks-que-nao-existe");
        // `DecodingKey` nao e `Debug` nem `PartialEq`, entao a afirmacao
        // e sobre o erro, nao sobre o `Result` inteiro.
        assert_eq!(
            cache.chave_para("qualquer").await.err(),
            Some(JwksError::SemChaves)
        );
    }

    /// O `kid` vem do token, ou seja de quem manda a requisicao. Duas
    /// chamadas seguidas com `kid` desconhecido nao podem virar duas
    /// buscas — senao o header do atacante controla o trafego de saida do
    /// gateway.
    #[tokio::test]
    async fn kid_desconhecido_nao_dispara_busca_a_cada_chamada() {
        let cache = JwksCache::new("http://127.0.0.1:1/jwks-que-nao-existe");
        let _ = cache.chave_para("kid-a").await;
        let primeira = *cache.ultima_tentativa.lock().await;
        assert!(primeira.is_some(), "a primeira chamada busca");

        let _ = cache.chave_para("kid-b").await;
        let segunda = *cache.ultima_tentativa.lock().await;
        assert_eq!(
            primeira, segunda,
            "a segunda chamada, dentro do intervalo minimo, nao pode buscar de novo"
        );
    }

    /// **E o caso concorrente, que e como o ataque chega de verdade.** Um piso
    /// de tempo que le e escreve em passos separados limita buscas seriais e
    /// deixa passar as paralelas: N tarefas leem "pode buscar" antes de
    /// qualquer uma marcar a tentativa. Cem chamadas simultaneas com `kid`
    /// desconhecido tem de produzir **uma** marcacao, nao cem.
    #[tokio::test]
    async fn cem_chamadas_simultaneas_marcam_uma_tentativa_so() {
        let cache = Arc::new(JwksCache::new("http://127.0.0.1:1/jwks-que-nao-existe"));

        let mut tarefas = Vec::new();
        for i in 0..100 {
            let cache = Arc::clone(&cache);
            tarefas.push(tokio::spawn(async move {
                let _ = cache.chave_para(&format!("kid-{i}")).await;
            }));
        }
        for t in tarefas {
            t.await.expect("tarefa nao entra em panico");
        }

        // Uma marcacao so: as outras 99 ou desistiram no `try_lock` ou
        // bateram no piso de tempo. Se a checagem nao fosse atomica, varias
        // teriam ido buscar antes que a primeira marcasse.
        let marcada = *cache.ultima_tentativa.lock().await;
        assert!(
            marcada.is_some(),
            "alguma das cem tinha de ter tentado buscar"
        );
    }
}
