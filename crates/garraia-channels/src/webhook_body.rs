//! Corpo de webhook: o tamanho é conferido **antes** de o corpo virar JSON.
//!
//! Os webhooks push (WhatsApp, LINE, Teams, Google Chat) recebem o corpo como
//! `Bytes` e fazem o `serde_json::from_slice` à mão, porque a autenticação
//! vem primeiro — HMAC sobre os bytes crus, ou JWT — e o extrator `Json<T>`
//! parsearia antes dela. O preço é que o limite de tamanho deixava de estar
//! escrito em algum lugar: valia o teto *default* do Axum para `Bytes`, que
//! ninguém declarou e que nenhum teste segurava.
//!
//! Este módulo é o único caminho até o parser para esses quatro handlers, e
//! ele carrega as três cercas que fazem do parse de um corpo remoto uma
//! operação de custo limitado:
//!
//! 1. **Tamanho.** `dentro_do_teto` recusa o que passa de
//!    [`MAX_WEBHOOK_BODY_BYTES`]. O mesmo número vai no `DefaultBodyLimit`
//!    das rotas (`garraia-gateway`, `router.rs`), que corta o corpo ainda no
//!    buffer; a conferência aqui é a que não depende de camada nenhuma.
//! 2. **Profundidade.** O `serde_json` recusa aninhamento acima de 128
//!    níveis, então `[[[[…` não estoura a pilha. É o default da crate e o
//!    teste `aninhamento_profundo_e_recusado` segura.
//! 3. **Tipo fechado.** O destino é sempre `serde_json::Value` — um enum de
//!    seis variantes. Não há `typetag`, objeto de trait nem tipo escolhido
//!    pelo corpo, que é de onde vem a execução de código nos CWE-502 de
//!    outras linguagens.
//!
//! ## CodeQL
//!
//! `rust/unsafe-deserialization` não enxerga checagem de tamanho: a query só
//! conhece barreiras por tipo (número, booleano, enum sem campos) ou
//! declaradas por modelo. `dentro_do_teto` está declarada como barreira em
//! `.github/codeql/extensions/garraia-rust/` — e é por isso que ela é uma
//! função separada, com o corpo validado no valor de retorno. Quem parsear um
//! corpo remoto **sem** passar por ela continua sendo alertado. Ver
//! `docs/security/codeql-setup.md`, seção "Barreiras declaradas".
//!
//! A barreira vale para qualquer sink de desserialização, seja qual for o
//! tipo de destino — o modelo do CodeQL não condiciona ao tipo. Tamanho
//! conferido não torna seguro desserializar num tipo perigoso, e é por isso
//! que `dentro_do_teto` é **privada**: o único consumidor é
//! [`json_limitado`], cujo destino é fixo em `serde_json::Value`. Torná-la
//! pública entregaria a quem a chamasse um salvo-conduto que ela não merece.

/// Teto do corpo de um webhook, em bytes (2 MiB).
///
/// É o mesmo valor que o Axum já aplicava por default ao extrator `Bytes`,
/// então declarar o teto não muda o que é aceito — muda quem responde por
/// ele. Mensagem de texto de qualquer uma das quatro plataformas fica ordens
/// de grandeza abaixo disto.
pub const MAX_WEBHOOK_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Por que o corpo não virou JSON. Os dois casos têm respostas HTTP
/// diferentes (413 e 400), e por isso não são um `Option`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpoRecusado {
    /// Passou de [`MAX_WEBHOOK_BODY_BYTES`]; o parser nem foi chamado.
    GrandeDemais,
    /// Coube no teto, mas não é JSON — ou aninha além do que o `serde_json`
    /// aceita.
    JsonInvalido,
}

/// Devolve o próprio corpo se ele cabe em [`MAX_WEBHOOK_BODY_BYTES`].
///
/// O corpo validado sai no **retorno** de propósito: é esse valor que segue
/// para o parser, e é nele que a barreira do CodeQL está declarada. Uma
/// versão que devolvesse `bool` deixaria o chamador livre para parsear o
/// slice original sem ter olhado a resposta.
///
/// Privada de propósito — ver "CodeQL" no topo do módulo.
fn dentro_do_teto(corpo: &[u8]) -> Option<&[u8]> {
    (corpo.len() <= MAX_WEBHOOK_BODY_BYTES).then_some(corpo)
}

/// Parseia o corpo de um webhook **já autenticado**, com o tamanho conferido
/// antes.
///
/// Chamar só depois de a assinatura ou o token conferirem: um POST forjado
/// não deve alcançar o parser, e não deve receber um 413 que o distinga do
/// 401/403 uniforme.
pub fn json_limitado(corpo: &[u8]) -> Result<serde_json::Value, CorpoRecusado> {
    let corpo = dentro_do_teto(corpo).ok_or(CorpoRecusado::GrandeDemais)?;
    serde_json::from_slice(corpo).map_err(|_| CorpoRecusado::JsonInvalido)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpo_pequeno_vira_json() {
        let v = json_limitado(br#"{"events":[]}"#).expect("json valido e pequeno");
        assert!(v.get("events").is_some());
    }

    #[test]
    fn corpo_que_nao_e_json_e_recusado_como_invalido() {
        assert_eq!(
            json_limitado(b"nao sou json"),
            Err(CorpoRecusado::JsonInvalido)
        );
    }

    /// A fronteira exata: `MAX` bytes passam, `MAX + 1` não. Um `<` no lugar
    /// do `<=` (ou o contrário) cai aqui.
    #[test]
    fn o_teto_e_inclusivo() {
        let no_teto = vec![b' '; MAX_WEBHOOK_BODY_BYTES];
        assert_eq!(
            dentro_do_teto(&no_teto).map(<[u8]>::len),
            Some(no_teto.len())
        );

        let acima = vec![b' '; MAX_WEBHOOK_BODY_BYTES + 1];
        assert!(dentro_do_teto(&acima).is_none());
    }

    /// **A prova de que o tamanho é conferido antes do parse.** O corpo abaixo
    /// é JSON válido — uma string gigante — e mesmo assim sai como
    /// `GrandeDemais`, não como `Ok`. Sem a checagem ele seria parseado, e o
    /// teste perceberia.
    #[test]
    fn json_valido_acima_do_teto_nem_chega_ao_parser() {
        let mut corpo = Vec::with_capacity(MAX_WEBHOOK_BODY_BYTES + 3);
        corpo.push(b'"');
        corpo.resize(MAX_WEBHOOK_BODY_BYTES + 1, b'a');
        corpo.push(b'"');
        assert!(
            serde_json::from_slice::<serde_json::Value>(&corpo).is_ok(),
            "o teste so tem valor se o corpo for JSON valido"
        );

        assert_eq!(json_limitado(&corpo), Err(CorpoRecusado::GrandeDemais));
    }

    /// A segunda cerca não é nossa, é do `serde_json` — e por isso mesmo tem
    /// teste: se um dia a crate (ou uma feature dela) deixar de limitar a
    /// recursão, este módulo passa a prometer mais do que entrega.
    #[test]
    fn aninhamento_profundo_e_recusado() {
        let fundo = 10_000;
        let mut corpo = vec![b'['; fundo];
        corpo.extend(std::iter::repeat_n(b']', fundo));
        assert!(corpo.len() < MAX_WEBHOOK_BODY_BYTES);

        assert_eq!(json_limitado(&corpo), Err(CorpoRecusado::JsonInvalido));
    }
}
