use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};

/// #1298: resultado da validação de um identificador de modelo contra o
/// catálogo real do provider — o insumo da decisão transacional do `/model`
/// no CLI: ou valida e troca, ou não altera o estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidacaoDeModelo {
    /// O catálogo completo do provider contém o modelo.
    Listado,
    /// O catálogo completo contém, mas a lista curada (`/models`) não
    /// anuncia — rota válida com nome não anunciado (ex.: namespace de
    /// terceiro servido pelo OpenRouter, como `z-ai/...`).
    ListadoForaDaCurada,
    /// O catálogo foi obtido e NÃO contém o modelo — a troca deve ser
    /// recusada com o estado anterior intacto.
    Ausente,
    /// O provider não expõe catálogo — a validação é impossível por design;
    /// quem decide a política é o chamador.
    SemListagem,
    /// #1752: o modelo é um preset do OpenRouter (`@preset/<slug>`), que não
    /// aparece no catálogo de `GET /models`, e por isso foi validado por uma
    /// chamada real mínima (`POST /chat/completions`, `max_tokens: 1`) que o
    /// OpenRouter aceitou. `modelo_resolvido` é o modelo que o preset resolveu
    /// nessa chamada, quando o OpenRouter o informou (campo `model` da
    /// resposta); é `None` quando a sonda validou sem um nome resolvido (ex.:
    /// 400 por `max_tokens` baixo demais, que já prova que o preset existe).
    ValidadoPorSonda { modelo_resolvido: Option<String> },
}

/// #1752: um preset do OpenRouter reconhecido numa string de modelo.
///
/// Preset é um recurso da conta OpenRouter (`@preset/<slug>`) que **não**
/// aparece em `GET /models`, mas que o `POST /chat/completions` aceita. Duas
/// formas:
/// - puro: `@preset/<slug>` — sem base, `base` é `None`;
/// - com base: `<autor>/<modelo>@preset/<slug>` — `base` é
///   `Some("<autor>/<modelo>")`.
///
/// `base` e `slug` apontam para dentro da string original (sem alocar); a
/// string inteira continua sendo o identificador que vai intacto ao OpenRouter
/// e o que as allowlists comparam por igualdade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetDoOpenRouter<'a> {
    /// A base `<autor>/<modelo>` antes do `@preset/`, ou `None` no preset puro.
    pub base: Option<&'a str>,
    /// O slug do preset, depois do `@preset/`.
    pub slug: &'a str,
}

/// #1752: reconhece um preset do OpenRouter numa string de modelo.
///
/// Aceita exatamente **um** `@preset/` (sensível a maiúscula: `@PRESET/` não
/// conta), com slug não vazio em `[A-Za-z0-9._-]`. A base (o que vem antes do
/// `@preset/`) é vazia (preset puro) ou um id simples do OpenRouter: exatamente
/// uma `/`, os dois segmentos não vazios, sem `@`, no mesmo conjunto de
/// caracteres de um nome de modelo (pode trazer sufixo `:free`). Qualquer outra
/// forma devolve `None` — e fora do OpenRouter um preset nunca é válido.
pub fn preset_do_openrouter(model: &str) -> Option<PresetDoOpenRouter<'_>> {
    const MARCADOR: &str = "@preset/";
    let mut ocorrencias = model.match_indices(MARCADOR);
    let (idx, _) = ocorrencias.next()?;
    // Mais de um `@preset/` não é um preset bem formado.
    if ocorrencias.next().is_some() {
        return None;
    }
    let base = &model[..idx];
    let slug = &model[idx + MARCADOR.len()..];
    if slug.is_empty() || !slug.bytes().all(eh_byte_de_slug) {
        return None;
    }
    if base.is_empty() {
        return Some(PresetDoOpenRouter { base: None, slug });
    }
    if !base_e_id_simples_do_openrouter(base) {
        return None;
    }
    Some(PresetDoOpenRouter {
        base: Some(base),
        slug,
    })
}

/// Byte válido no slug de um preset: `[A-Za-z0-9._-]`. Exclui `/` e `@`, então
/// um slug nunca esconde uma segunda `/` nem um segundo `@preset/`.
fn eh_byte_de_slug(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// A base de um preset com base é um id simples do OpenRouter: exatamente uma
/// `/`, os dois segmentos não vazios, e só caracteres de nome de modelo
/// (`[A-Za-z0-9._:/-]`, o que já exclui `@`). O sufixo `:free` passa.
fn base_e_id_simples_do_openrouter(base: &str) -> bool {
    let Some((autor, modelo)) = base.split_once('/') else {
        return false;
    };
    // Exatamente uma `/`: o lado direito não pode trazer outra.
    if autor.is_empty() || modelo.is_empty() || modelo.contains('/') {
        return false;
    }
    base.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'-'))
}

/// #1764: uma variante de roteamento do OpenRouter reconhecida numa string de
/// modelo. São atalhos que o OpenRouter resolve em tempo de requisição e que
/// **não** aparecem em `GET /models`: `:floor` (provider mais barato), `:nitro`
/// (maior throughput), `:online` (busca web) e `:exacto` (provider exato). A
/// base `<autor>/<modelo>` é um id simples do OpenRouter.
///
/// `base` e `sufixo` apontam para dentro da string original (sem alocar); a
/// string inteira continua sendo o identificador que vai intacto ao OpenRouter
/// e o que as allowlists comparam por igualdade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarianteDoOpenRouter<'a> {
    /// A base `<autor>/<modelo>` antes do `:<sufixo>` de roteamento.
    pub base: &'a str,
    /// O sufixo de roteamento (sem o `:`): `floor`/`nitro`/`online`/`exacto`.
    pub sufixo: &'a str,
}

/// #1764: reconhece uma variante de roteamento do OpenRouter numa string de
/// modelo (`<autor>/<modelo>:<sufixo>`).
///
/// Separa pelo **último** `:` (`rsplit_once`), aceita o sufixo só se estiver na
/// lista fechada `["floor", "nitro", "online", "exacto"]`, e exige que a base
/// seja um id simples do OpenRouter ([`base_e_id_simples_do_openrouter`]:
/// exatamente uma `/`, segmentos não vazios, sem `@`). Qualquer `@` na string
/// devolve `None` — um preset (`...@preset/<slug>`) nunca é uma variante.
///
/// Como a base pode trazer um `:` próprio (ex.: `meta-llama/x:free`), a divisão
/// pelo último `:` deixa `:free`/`:thinking`/`:extended` **na base**: eles são
/// ids do catálogo, não sufixos de roteamento. Por isso `meta-llama/x:free`
/// sozinho devolve `None` (o `/model` o valida por igualdade no catálogo),
/// enquanto `meta-llama/x:free:floor` é a variante `:floor` sobre a base
/// `meta-llama/x:free`.
pub fn variante_do_openrouter(model: &str) -> Option<VarianteDoOpenRouter<'_>> {
    const SUFIXOS: [&str; 4] = ["floor", "nitro", "online", "exacto"];
    // Um preset (`@preset/`) — e qualquer outro `@` — nunca é uma variante.
    if model.contains('@') {
        return None;
    }
    let (base, sufixo) = model.rsplit_once(':')?;
    if !SUFIXOS.contains(&sufixo) {
        return None;
    }
    if !base_e_id_simples_do_openrouter(base) {
        return None;
    }
    Some(VarianteDoOpenRouter { base, sufixo })
}

/// Trait para integrações com provedores de LLM (Anthropic, OpenAI, Ollama, etc.).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Identificador do provedor (ex: "anthropic", "openai", "ollama").
    fn provider_id(&self) -> &str;

    /// Envia uma requisição de completion e retorna a resposta.
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;

    /// Envia uma requisição de completion em modo streaming,
    /// retornando eventos conforme são recebidos.
    /// A implementação padrão retorna erro indicando que streaming não é suportado.
    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Err(garraia_common::Error::Agent(format!(
            "provedor {} não suporta streaming",
            self.provider_id()
        )))
    }

    /// Retorna o modelo padrão configurado para o provedor, se conhecido.
    fn configured_model(&self) -> Option<&str> {
        None
    }

    /// Retorna a lista de modelos disponíveis para este provedor.
    async fn available_models(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// #1298: valida um identificador de modelo contra o catálogo REAL do
    /// provider — não a lista curada que `available_models` devolve.
    ///
    /// O padrão reutiliza `available_models`: lista vazia vira `SemListagem`
    /// (provider que não expõe catálogo), modelo presente vira `Listado` e
    /// ausente vira `Ausente`. Só quem tem dois níveis de catálogo (OpenRouter,
    /// com a curada de populares e a lista completa) precisa sobrescrever.
    async fn validar_modelo(&self, model: &str) -> Result<ValidacaoDeModelo> {
        // #1752: `@preset/<slug>` é um recurso exclusivo do OpenRouter. Fora
        // dele (Anthropic, Ollama, llama.cpp herdam este default) um preset
        // nunca é um modelo válido — recusa antes de consultar o catálogo.
        if preset_do_openrouter(model).is_some() {
            return Ok(ValidacaoDeModelo::Ausente);
        }
        let modelos = self.available_models().await?;
        if modelos.is_empty() {
            return Ok(ValidacaoDeModelo::SemListagem);
        }
        if modelos.iter().any(|m| m == model) {
            Ok(ValidacaoDeModelo::Listado)
        } else {
            Ok(ValidacaoDeModelo::Ausente)
        }
    }

    /// Verifica se o provedor está disponível e corretamente configurado.
    async fn health_check(&self) -> Result<bool>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: MessagePart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessagePart {
    Text(String),
    Parts(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { url: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Eventos emitidos durante uma completion em modo streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Um trecho incremental de texto gerado.
    TextDelta(String),
    /// Início de um bloco de uso de ferramenta.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Fragmento parcial de JSON de entrada para um bloco de ferramenta.
    InputJsonDelta(String),
    /// Finalização de um bloco de conteúdo.
    ContentBlockStop { index: usize },
    /// Indica que a mensagem está sendo finalizada, incluindo metadados.
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<Usage>,
    },
    /// Indica que o streaming foi concluído.
    MessageStop,
}

// ── #1249: classificacao de falha de envio ───────────────────────────────────

/// `true` quando o `reqwest::Error` diz que a requisicao **nao chegou a ter
/// resposta**: conexao recusada, DNS que nao resolve, timeout, falha ao
/// enviar.
///
/// Fora de proposito:
/// - `is_builder()` — URL/cliente mal formado e bug de configuracao nosso, e
///   tentar outro provider esconderia o bug;
/// - `is_status()` — ja houve resposta, e a politica dela (429/5xx com
///   backoff) e a de sempre;
/// - `is_body()` / `is_decode()` — a conexao existiu e quebrou no meio do
///   corpo, caso do #1176, tratado no consumidor do stream.
///
/// A leitura e feita aqui, onde o tipo do `reqwest` ainda existe. Depois do
/// `format!` sobra texto, e texto de erro de dependencia muda de versao para
/// versao — era exatamente esse o defeito que a issue descreve.
pub(crate) fn falha_de_transporte(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request()
}

/// Erro de envio de requisicao a um provider, com a **classe** preservada.
///
/// `contexto` e o prefixo que o provider ja usava ("openai request failed"),
/// mantido palavra por palavra: o cartao de erro da CLI classifica pela frase
/// interna do `reqwest`, e nao pelo prefixo do enum.
pub(crate) fn erro_de_envio(contexto: &str, e: &reqwest::Error) -> Error {
    // #1593: timeout do LLM e o unico erro de envio que o operador consegue
    // resolver sozinho, trocando a janela — entao o card diz qual chave.
    // O resto do texto do reqwest e mantido palavra por palavra (#1249): o
    // cartao da CLI classifica pela frase interna, nao pelo prefixo.
    let mut msg = format!("{contexto}: {e}");
    if e.is_timeout() {
        msg.push_str(
            " — a resposta do LLM nao chegou dentro da janela de timeout e foi \
             descartada; aumente timeouts.llm.default_secs (global) ou declare \
             llm.<provider>.timeout_secs com uma janela maior para este provider",
        );
    }
    if falha_de_transporte(e) {
        Error::Transport(msg)
    } else {
        Error::Agent(msg)
    }
}

/// O corpo de erro HTTP do provider reduzido ao que pode sair do processo
/// (#1726): a frase `error.message` quando o corpo e o JSON de erro no
/// formato da OpenAI (mais `error.code`, se houver), ou o corpo truncado a
/// 200 caracteres quando nao e. Nunca o JSON inteiro.
///
/// O OpenRouter devolve `{"error":{...},"user_id":"user_..."}`; o `user_id` e
/// a conta do operador, e ia inteiro para a resposta HTTP do gateway e para o
/// envelope do `garra ask`. O log do provider ja truncava o corpo — era a
/// mensagem de erro que o repassava cru.
pub(crate) fn corpo_de_erro_sanitizado(body: &str) -> String {
    const MAX_CHARS: usize = 200;
    let aparado = body.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(aparado)
        && let Some(erro) = v.get("error")
    {
        let mensagem = erro
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| erro.as_str())
            .unwrap_or("provider returned an error");
        let codigo = erro
            .get("code")
            .and_then(|c| {
                c.as_str()
                    .map(str::to_string)
                    .or_else(|| c.as_u64().map(|n| n.to_string()))
            })
            .map(|c| format!(" (code={c})"))
            .unwrap_or_default();
        return format!("{}{codigo}", truncar(mensagem, MAX_CHARS));
    }
    truncar(aparado, MAX_CHARS)
}

/// Corta em `max` caracteres (nao bytes: o corpo pode ter acento ou emoji),
/// marcando o corte.
pub(crate) fn truncar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        return texto.to_string();
    }
    let inicio: String = texto.chars().take(max).collect();
    format!("{inicio}…")
}

/// Diagnostico de uma resposta **3xx** de um endpoint remoto (#1779).
///
/// Redirect nao e seguido em lugar nenhum do projeto (regra 14: `vet_url` +
/// `pinned_client` resolvem o host UMA vez e `Policy::none()` fecha o
/// encadeamento), entao um 3xx chega aos ramos de erro escritos para 4xx/5xx —
/// onde quem explica e o CORPO. So que corpo de 3xx e vazio e a explicacao
/// esta no header `Location`, que ninguem lia: o operador com um `base_url`
/// errado recebia `status=308 Permanent Redirect, body=` e nao tinha o que
/// agir.
///
/// O texto comeca em `status={status}` de proposito: e o contrato que
/// [`status_http_do_erro`] le, e cada chamador so prefixa o proprio rotulo de
/// provider. `alvo` e o que o operador tem de corrigir ("a base_url do
/// provider", "a URL pedida").
///
/// **Nao segue o redirect e nao emite requisicao nenhuma** — e so formatacao
/// sobre headers que ja chegaram.
pub(crate) fn diagnostico_de_redirecionamento(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    alvo: &str,
) -> String {
    let location = headers
        .get(reqwest::header::LOCATION)
        .map(|v| location_sanitizado(v.as_bytes()))
        .unwrap_or_else(|| "<ausente>".to_string());
    format!(
        "status={status}, location={location}; redirects desligados por seguranca - corrija {alvo}"
    )
}

/// O `Location` reduzido ao que pode sair do processo.
///
/// E dado remoto influenciavel: quem escolhe o destino e o servidor, que pode
/// mandar controle de terminal (#995), uma credencial embutida
/// (`https://user:senha@host`) ou um valor arbitrariamente longo. A ordem
/// redigir -> tirar controle -> truncar e a mesma de `turn_events::sanear`, e
/// pela mesma razao: truncar antes deixaria meia sequencia (ou meio segredo)
/// passar sem casar. O corte e por CARACTERE — um `Location` com multibyte
/// quebraria `&s[..n]` (#1735).
fn location_sanitizado(bruto: &[u8]) -> String {
    const MAX_CHARS: usize = 200;
    // Lossy, e nao `to_str()`: um header com byte invalido ainda tem de
    // aparecer no diagnostico, e o `\u{fffd}` nao e controle.
    let texto = String::from_utf8_lossy(bruto);
    let redigido = garraia_security::redact_secrets(texto.trim());
    let limpo = crate::turn_events::sanear_controles(&redigido, false);
    let limpo = limpo.trim();
    if limpo.is_empty() {
        return "<vazio>".to_string();
    }
    truncar(limpo, MAX_CHARS)
}

/// O status HTTP que o provider respondeu, lido da mensagem de `Error::Agent`
/// (`"... status=400 Bad Request, ..."`). `None` para erro sem status (rede,
/// parse, texto de outra origem) e para `Error::Transport`.
///
/// Texto, e nao tipo, porque e o contrato que ja existe: `is_retryable_error`
/// no runtime classifica 429/5xx pela mesma substring, e os providers
/// escrevem `status={status}` ha varias versoes. Le o PRIMEIRO `status=` da
/// mensagem; o corpo sanitizado vem depois e nao carrega outro. E o que o
/// gateway usa para responder 400 em vez de 500 quando o erro e do pedido
/// (#1726).
pub fn status_http_do_erro(err: &Error) -> Option<u16> {
    let Error::Agent(msg) = err else {
        return None;
    };
    let depois = msg.split_once("status=")?.1;
    let digitos: String = depois.chars().take_while(|c| c.is_ascii_digit()).collect();
    digitos
        .parse::<u16>()
        .ok()
        .filter(|s| (100..600).contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1764: o parser de variante aceita as 4 variantes com base simples,
    /// peela so o ultimo `:` (um `:free` fica na base), e recusa sufixo fora da
    /// lista, base ausente/invalida e qualquer `@`.
    #[test]
    fn variante_do_openrouter_aceita_a_lista_fechada_e_recusa_o_resto() {
        for sufixo in ["floor", "nitro", "online", "exacto"] {
            let model = format!("z-ai/glm-5.3-flash:{sufixo}");
            let v = variante_do_openrouter(&model).expect("variante com base simples");
            assert_eq!(v.base, "z-ai/glm-5.3-flash");
            assert_eq!(v.sufixo, sufixo);
        }

        // `rsplit_once(':')` peela so o ultimo `:` — um `:free` na base e a
        // variante `:floor` sobre `meta-llama/x:free`.
        let v = variante_do_openrouter("meta-llama/x:free:floor").expect("variante sobre :free");
        assert_eq!(v.base, "meta-llama/x:free");
        assert_eq!(v.sufixo, "floor");

        for ruim in [
            "z-ai/glm-5.3-flash:bogus",      // sufixo fora da lista fechada
            "meta-llama/x:free",             // id do catalogo, nao e variante
            ":floor",                        // sufixo sem base
            "a/b/c:floor",                   // base com duas barras
            "glm-flash:floor",               // base sem barra
            "z-ai/glm-5.3-flash",            // sem sufixo nenhum
            "x:floor@preset/y",              // `@` nunca e variante (e um preset mal formado)
            "z-ai/glm-5.3-flash@preset/foo", // preset com base
        ] {
            assert!(
                variante_do_openrouter(ruim).is_none(),
                "{ruim} deveria ser recusado"
            );
        }
    }

    /// #1726: o `user_id` do OpenRouter nao sobrevive a sanitizacao; a frase
    /// do provider e o `code` sobrevivem.
    #[test]
    fn corpo_de_erro_sanitizado_fica_so_com_a_frase_do_provider() {
        let bruto = r#"{"error":{"message":"garraia is not a valid model ID","code":400},"user_id":"user_30DUSuPezaeY1jmnFqymmg2ApXf"}"#;
        let limpo = corpo_de_erro_sanitizado(bruto);
        assert_eq!(limpo, "garraia is not a valid model ID (code=400)");
        assert!(!limpo.contains("user_"), "{limpo}");

        // `code` em string (Anthropic/OpenAI) tambem entra; sem `code`, so a frase.
        assert_eq!(
            corpo_de_erro_sanitizado(
                r#"{"error":{"message":"bad","code":"invalid_request_error"}}"#
            ),
            "bad (code=invalid_request_error)"
        );
        assert_eq!(
            corpo_de_erro_sanitizado(r#"{"error":{"message":"so a frase"}}"#),
            "so a frase"
        );
        // `error` como string crua.
        assert_eq!(corpo_de_erro_sanitizado(r#"{"error":"texto"}"#), "texto");
    }

    /// Corpo que nao e o JSON de erro: truncado por caracteres, nao por bytes.
    #[test]
    fn corpo_de_erro_sanitizado_trunca_texto_cru_por_caracteres() {
        assert_eq!(corpo_de_erro_sanitizado("  Bad Gateway  "), "Bad Gateway");
        let longo = "é".repeat(500);
        let limpo = corpo_de_erro_sanitizado(&longo);
        assert_eq!(limpo.chars().count(), 201, "200 + o marcador");
        assert!(limpo.ends_with('…'));
        // JSON que nao tem `error` tambem e texto cru.
        assert_eq!(
            corpo_de_erro_sanitizado(r#"{"detail":"x"}"#),
            r#"{"detail":"x"}"#
        );
    }

    /// #1726: o status sai do texto do `Error::Agent`, no formato que os
    /// providers escrevem; transporte e texto sem status dao `None`.
    #[test]
    fn status_http_do_erro_le_o_status_da_mensagem() {
        let e = Error::Agent(
            "openai API error: status=400 Bad Request, body=garraia is not a valid model ID".into(),
        );
        assert_eq!(status_http_do_erro(&e), Some(400));
        let e = Error::Agent("erro na API da Anthropic: status=529 Overloaded, body=...".into());
        assert_eq!(status_http_do_erro(&e), Some(529));
        let e = Error::Agent("openai API error (SSE): status=503, error=upstream".into());
        assert_eq!(status_http_do_erro(&e), Some(503));
        assert_eq!(
            status_http_do_erro(&Error::Agent("sem status nenhum".into())),
            None
        );
        assert_eq!(
            status_http_do_erro(&Error::Agent("status=abc".into())),
            None
        );
        assert_eq!(
            status_http_do_erro(&Error::Agent("status=9999".into())),
            None
        );
        assert_eq!(
            status_http_do_erro(&Error::Transport(
                "openai request failed: error sending request".into()
            )),
            None
        );
    }

    /// #1779: o diagnostico de 3xx cita o `Location` no formato que
    /// `status_http_do_erro` le, e trata a ausencia do header.
    #[test]
    fn diagnostico_de_redirecionamento_cita_o_location_e_mantem_o_status() {
        use reqwest::header::{HeaderMap, HeaderValue, LOCATION};

        let mut headers = HeaderMap::new();
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://host/caminho?user_id=user_x&token=segredo"),
        );
        let msg = diagnostico_de_redirecionamento(
            reqwest::StatusCode::PERMANENT_REDIRECT,
            &headers,
            "a base_url do provider",
        );
        assert_eq!(
            msg,
            "status=308 Permanent Redirect, \
             location=https://host/caminho?user_id=user_x&token=segredo; \
             redirects desligados por seguranca - corrija a base_url do provider"
        );
        // O contrato de leitura por texto continua valendo para 3xx.
        assert_eq!(
            status_http_do_erro(&Error::Agent(format!("openai API error: {msg}"))),
            Some(308)
        );

        // Sem o header (3xx malformado) o diagnostico ainda diz o que fazer.
        let msg = diagnostico_de_redirecionamento(
            reqwest::StatusCode::MOVED_PERMANENTLY,
            &HeaderMap::new(),
            "a URL pedida",
        );
        assert!(
            msg.starts_with("status=301 Moved Permanently, location=<ausente>;"),
            "{msg}"
        );
        assert!(msg.ends_with("corrija a URL pedida"), "{msg}");
    }

    /// #1779: o `Location` e escolhido pelo servidor remoto — controle de
    /// terminal (#995), credencial embutida e tamanho sao tratados antes de o
    /// texto sair do processo, e o corte e por caractere (#1735).
    #[test]
    fn location_sanitizado_tira_controle_segredo_e_trunca_por_caractere() {
        // Controle de terminal nao sobrevive: nem o `ESC`, nem o `\r`.
        let limpo = location_sanitizado(b"https://host/\x1b[2J\x1b[?25lx\ry");
        assert!(!limpo.contains('\u{1b}'), "{limpo}");
        assert!(!limpo.contains('\r'), "{limpo}");
        assert!(!limpo.contains("[2J"), "{limpo}");
        assert!(!limpo.contains("?25l"), "{limpo}");

        // Credencial embutida na URL e redigida.
        let limpo = location_sanitizado(b"https://user:senha-longa@host/x");
        assert!(!limpo.contains("senha-longa"), "{limpo}");
        assert!(limpo.contains("[REDACTED]"), "{limpo}");

        // Multibyte: corte por caractere, com marcador — nunca panico.
        let longo = format!("https://host/{}", "é".repeat(500));
        let limpo = location_sanitizado(longo.as_bytes());
        assert_eq!(limpo.chars().count(), 201, "200 + o marcador");
        assert!(limpo.ends_with('…'), "{limpo}");

        // Header presente e vazio nao vira string solta no meio da frase.
        assert_eq!(location_sanitizado(b"   "), "<vazio>");

        // Byte invalido em UTF-8 ainda aparece (lossy), sem panico.
        let limpo = location_sanitizado(&[b'h', b't', b't', b'p', b':', b'/', b'/', 0xff]);
        assert!(limpo.starts_with("http://"), "{limpo}");
    }

    /// Porta fechada em loopback: o unico erro de rede que um teste pode
    /// provocar sem depender de rede de verdade.
    async fn erro_de_conexao_recusada() -> reqwest::Error {
        // Abre e fecha para descobrir uma porta que ninguem esta servindo.
        let porta = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind efemero");
            l.local_addr().expect("addr").port()
        };
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{porta}/"))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .expect_err("ninguem esta escutando nessa porta")
    }

    #[tokio::test]
    async fn conexao_recusada_e_transporte() {
        let e = erro_de_conexao_recusada().await;
        assert!(
            falha_de_transporte(&e),
            "connect/timeout tem de ser transporte; veio: {e}"
        );
        assert!(
            matches!(
                erro_de_envio("openai request failed", &e),
                Error::Transport(_)
            ),
            "a classe precisa sobreviver ao format!"
        );
    }

    #[test]
    fn url_invalida_nao_e_transporte() {
        // `is_builder()`: nada saiu da maquina, e cair para outro provider
        // esconderia um bug de configuracao nosso.
        let e = reqwest::Client::new()
            .get("http://[::1")
            .build()
            .expect_err("url invalida");
        assert!(!falha_de_transporte(&e), "builder nao e transporte: {e}");
        assert!(matches!(
            erro_de_envio("openai request failed", &e),
            Error::Agent(_)
        ));
    }

    /// #1593: timeout do LLM ganha a orientação da chave na mensagem — o
    /// único erro de envio que o operador resolve trocando a janela. O
    /// prefixo da classe e o texto do reqwest continuam intactos (#1249).
    #[tokio::test]
    async fn timeout_do_llm_nomeia_as_chaves_que_o_operador_muda() {
        // Servidor local que aceita e nunca responde: com teto de 50ms a
        // chamada morre como timeout de verdade, sem rede externa.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                // Le o cabecalho antes de estagnar: sem isso o hyper falha
                // em montagem (Canceled) e nao como timeout (#1593).
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });

        let e = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .unwrap()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect_err("o teto de 50ms tem de matar a chamada");
        assert!(e.is_timeout(), "precondicao: {e}");

        let texto = erro_de_envio("falha na requisição à Anthropic", &e).to_string();
        assert!(
            texto.contains("timeouts.llm.default_secs"),
            "a mensagem nomeia a chave global; veio: {texto}"
        );
        assert!(
            texto.contains("llm.<provider>.timeout_secs"),
            "a mensagem nomeia a janela por provider; veio: {texto}"
        );
        assert!(
            texto.starts_with("transport error: "),
            "a classe sobrevive; veio: {texto}"
        );
    }

    #[tokio::test]
    async fn mensagem_do_provider_sobrevive_inteira() {
        let e = erro_de_conexao_recusada().await;
        let texto = erro_de_envio("ollama request failed", &e).to_string();
        assert!(
            texto.contains("ollama request failed"),
            "o prefixo do provider fica; veio: {texto}"
        );
        assert!(
            texto.starts_with("transport error: "),
            "e o prefixo da classe entra na frente; veio: {texto}"
        );
    }
}
