//! #1403: a validacao de numero que a CLI ja fazia (`garraia whatsapp allow`)
//! e que a API admin (e o console) precisam fazer IGUAL. Um numero sem o `+`
//! do codigo do pais e indistinguivel de um numero com codigo do pais de
//! outro tamanho — e um numero gravado sem codigo nunca casa com o remetente:
//! o portao e fail-closed, e o operador so descobriria pela resposta que
//! nao chega. Aqui a regra, uma vez; a CLI re-exporta e poe a frase no
//! idioma do terminal, a API devolve `error_code` estavel.
//!
//! Puro. `Ok` e so digitos, com codigo do pais, 6 a 15 de comprimento — ou um
//! `<digitos>@lid` como veio — e e byte a byte o que
//! [`super::normalizar_identidade`] devolve para a mesma entrada.

/// Por que um texto nao e um numero autorizavel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumeroInvalido {
    Vazio,
    /// Veio na forma `…@s.whatsapp.net` (ou qualquer JID que nao `<id>@lid`).
    Jid,
    /// Numero sem o `+` do codigo do pais.
    SemMais,
    /// Letra ou outro caractere fora de `+ -().` e espaco.
    Caractere,
    /// `*` (ou `+*`, `**`…): a tentativa de abrir o canal para todo mundo
    /// (#1389). O acesso aberto existe (`access open`, ADR 0025), mas nao e
    /// um numero na lista — e o erro generico de caractere faria isso parecer
    /// erro de digitacao.
    Curinga,
    /// Comeca com `0`: prefixo de discagem local, nao codigo de pais.
    ZeroInicial,
    /// Fora de 6 a 15 digitos.
    Tamanho(usize),
}

impl NumeroInvalido {
    /// Codigo estavel, legivel por maquina (o `error_code` da API admin).
    pub fn codigo(&self) -> &'static str {
        match self {
            Self::Vazio => "identity_empty",
            Self::Jid => "identity_is_jid",
            Self::SemMais => "identity_missing_country_code",
            Self::Caractere => "identity_invalid_character",
            Self::Curinga => "identity_wildcard",
            Self::ZeroInicial => "identity_leading_zero",
            Self::Tamanho(_) => "identity_length",
        }
    }

    /// A descricao (pt-BR sem acento, como o resto da API), sem repetir o
    /// que foi digitado.
    pub fn descricao(&self) -> String {
        match self {
            Self::Vazio => "o numero esta vazio".to_string(),
            Self::Jid => "use o numero com + e codigo do pais, sem @s.whatsapp.net (so um LID, <id>@lid, vai com o @)".to_string(),
            Self::SemMais => "comece com + e o codigo do pais (ex.: +55 11 99999-8888): sem ele o numero nunca casa com quem manda a mensagem".to_string(),
            Self::Caractere => "o numero so pode ter digitos (e, opcionalmente, +, espacos, hifens, pontos e parenteses)".to_string(),
            Self::Curinga => "`*` nao e um numero: para admitir qualquer numero use a admissao `open`, declarada".to_string(),
            Self::ZeroInicial => "o numero comeca com 0: use o codigo do pais no lugar do prefixo local".to_string(),
            Self::Tamanho(n) => format!("o numero tem {n} digitos; com o codigo do pais ele precisa ter de 6 a 15"),
        }
    }
}

/// Separadores que o numero pode trazer e que sao descartados.
pub fn separador(c: char) -> bool {
    matches!(c, ' ' | '-' | '.' | '(' | ')')
}

/// `*`, `**`, `+*`: a tentativa de dizer "todo mundo" (#1389).
pub fn e_curinga(s: &str) -> bool {
    let corpo = s.strip_prefix('+').unwrap_or(s);
    !corpo.is_empty() && corpo.chars().all(|c| c == '*')
}

/// `<digitos>@lid`, exatamente: o LID que a ponte entrega quando o servidor
/// nao manda o numero junto.
pub fn e_lid_valido(s: &str) -> bool {
    s.strip_suffix("@lid")
        .is_some_and(|id| (6..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Normaliza um numero digitado para a forma que o portao do canal compara.
///
/// Aceita `+<codigo do pais><numero>` com espacos, hifens, pontos e
/// parenteses no meio (formato BR `+55 (11) 99999-8888`, US `+1 (415)
/// 555-0100`, E.164 curto `+376 312 345`), e `<digitos>@lid` como veio.
/// Recusa numero sem `+` (nao da para saber se o codigo do pais veio), letra,
/// curinga, zero inicial, JID de outro tipo e fora de 6 a 15 digitos.
pub fn normalizar_numero(raw: &str) -> Result<String, NumeroInvalido> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(NumeroInvalido::Vazio);
    }
    if e_curinga(s) {
        return Err(NumeroInvalido::Curinga);
    }
    if e_lid_valido(s) {
        return Ok(super::normalizar_identidade(s));
    }
    if s.contains('@') {
        return Err(NumeroInvalido::Jid);
    }
    let Some(corpo) = s.strip_prefix('+') else {
        return Err(if s.chars().all(|c| c.is_ascii_digit() || separador(c)) {
            NumeroInvalido::SemMais
        } else {
            NumeroInvalido::Caractere
        });
    };
    let mut digitos = String::with_capacity(corpo.len());
    for c in corpo.chars() {
        if c.is_ascii_digit() {
            digitos.push(c);
        } else if !separador(c) {
            return Err(NumeroInvalido::Caractere);
        }
    }
    if digitos.is_empty() {
        return Err(NumeroInvalido::Vazio);
    }
    if digitos.starts_with('0') {
        return Err(NumeroInvalido::ZeroInicial);
    }
    if !(6..=15).contains(&digitos.len()) {
        return Err(NumeroInvalido::Tamanho(digitos.len()));
    }
    Ok(super::normalizar_identidade(&digitos))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Brasil, EUA (com e sem parenteses/hifens), Reino Unido e E.164 curto:
    /// tudo vira so digitos com o codigo do pais — a forma que o portao compara.
    #[test]
    fn formatos_br_us_e_e164_viram_so_digitos() {
        for (entrada, esperado) in [
            ("+55 11 99999-8888", "5511999998888"),
            ("+55 (11) 99999.8888", "5511999998888"),
            ("  +5511999998888  ", "5511999998888"),
            ("+1 (415) 555-0100", "14155550100"),
            ("+1-415-555-0100", "14155550100"),
            ("+1 555 123 4567", "15551234567"),
            ("+44 20 7946 0958", "442079460958"),
            ("+376 312 345", "376312345"),
            ("+683 1234", "6831234"),
        ] {
            let n = normalizar_numero(entrada).expect(entrada);
            assert_eq!(n, esperado, "{entrada}");
            assert_eq!(
                n,
                super::super::normalizar_identidade(entrada),
                "a forma gravada e a que o portao compara: {entrada}"
            );
        }
        assert_eq!(
            normalizar_numero("87654321098765@lid").as_deref(),
            Ok("87654321098765@lid")
        );
    }

    /// Sem `+` (BR e US), letra, curinga, zero inicial, JID e tamanho: cada
    /// recusa com o seu motivo e o seu codigo estavel.
    #[test]
    fn entradas_invalidas_tem_motivo_e_codigo_proprios() {
        for (entrada, esperado, codigo) in [
            ("", NumeroInvalido::Vazio, "identity_empty"),
            ("+", NumeroInvalido::Vazio, "identity_empty"),
            (
                "21 98888-7777",
                NumeroInvalido::SemMais,
                "identity_missing_country_code",
            ),
            (
                "(415) 555-0100",
                NumeroInvalido::SemMais,
                "identity_missing_country_code",
            ),
            (
                "415-555-0100",
                NumeroInvalido::SemMais,
                "identity_missing_country_code",
            ),
            (
                "5511999998888",
                NumeroInvalido::SemMais,
                "identity_missing_country_code",
            ),
            (
                "+1 (415) 555-01O0",
                NumeroInvalido::Caractere,
                "identity_invalid_character",
            ),
            (
                "abc",
                NumeroInvalido::Caractere,
                "identity_invalid_character",
            ),
            ("*", NumeroInvalido::Curinga, "identity_wildcard"),
            ("+*", NumeroInvalido::Curinga, "identity_wildcard"),
            (
                "+011 99999-8888",
                NumeroInvalido::ZeroInicial,
                "identity_leading_zero",
            ),
            ("+12345", NumeroInvalido::Tamanho(5), "identity_length"),
            (
                "+1234567890123456",
                NumeroInvalido::Tamanho(16),
                "identity_length",
            ),
            (
                "5511999998888@s.whatsapp.net",
                NumeroInvalido::Jid,
                "identity_is_jid",
            ),
        ] {
            let erro = normalizar_numero(entrada).expect_err(entrada);
            assert_eq!(erro, esperado, "{entrada:?}");
            assert_eq!(erro.codigo(), codigo, "{entrada:?}");
            // A descricao nunca repete um numero digitado (so um simbolo como `*`
            // pode aparecer, porque a frase e sobre ele).
            if entrada.chars().filter(|c| c.is_ascii_digit()).count() >= 5 {
                assert!(
                    !erro.descricao().contains(entrada.trim()),
                    "a descricao nao repete a entrada: {entrada:?}"
                );
            }
        }
    }
}
