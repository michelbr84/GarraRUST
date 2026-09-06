//! Politica de ruido na ingestao da memoria semantica (#952).
//!
//! O sintoma relatado: numa base de ~7.100 entradas, a consulta "quem e
//! Michel" trazia entradas `"oi"` no top-K. Vetor de texto curto tem cosseno
//! alto com quase tudo, entao "oi" competia com memoria de verdade — e a
//! proporcao ruido/sinal so cresce com o tempo.
//!
//! # O que esta politica faz, e o que ela NAO faz
//!
//! Ela decide **se vale gastar um vetor** com um conteudo. Nada mais.
//!
//! - A entrada **continua sendo gravada**. O historico nao perde nada, e o
//!   caminho textual do recall continua achando-a.
//! - Nada e apagado. Desligar a politica e reindexar devolve o vetor.
//! - Vetores de ruido que ja estao no indice **ficam**: isto e um filtro de
//!   ingestao, nao uma limpeza retroativa. Quem quiser limpar o passado usa
//!   `garra memory compact` / TTL (#956, #959), que apagam de verdade e por
//!   isso pedem decisao explicita do operador.
//!
//! # Por que ela e conservadora
//!
//! Errar para o lado de embeddar ruido custa ranking. Errar para o outro lado
//! custa **memoria que o agente deveria ter e nao tem** — e o usuario nao tem
//! como saber que perdeu. Por isso a regra de frase so casa quando a frase e
//! o **conteudo inteiro**: `"obrigado"` e ruido, `"obrigado pela ajuda com o
//! deploy do gateway"` nao e.

/// Conteudo que nao merece vetor.
///
/// Construida a partir da config (`memory.ingestion`) no bootstrap; o
/// `garraia-agents` nao depende de `garraia-config`, entao a politica chega
/// como dado puro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoisePolicy {
    enabled: bool,
    min_chars: usize,
    /// Frases que, sozinhas, sao o conteudo inteiro de uma entrada de ruido.
    /// Ja normalizadas na construcao.
    phrases: Vec<String>,
}

/// Piso de caracteres uteis abaixo do qual o conteudo nao ganha vetor.
///
/// Quatro, e nao os 10 ou 12 que pegariam "bom dia" por comprimento: a lista
/// de frases faz esse trabalho com precisao, e um piso alto derrubaria junto
/// um fato curto de verdade ("moro em SP" tem 10). O piso cobre o caso que a
/// issue relata — `oi`, `ok`, `ta`, `sim`, `nao`, `kkk` — sem apostar em
/// tamanho como proxy de conteudo.
pub const DEFAULT_MIN_CHARS: usize = 4;

/// Faixa aceita para `memory.ingestion.min_chars`.
///
/// Zero desliga o piso sem desligar a politica (a lista de frases segue
/// valendo). O teto de 40 nao e arbitrario: acima disso o numero deixa de ser
/// "filtro de ruido" e vira uma politica de o que lembrar, que ninguem
/// consegue prever pelo nome da chave.
pub const MIN_CHARS_MAX: usize = 40;

/// Frases que sao ruido quando sao o conteudo **inteiro**.
///
/// PT primeiro porque e a lingua do projeto; o punhado de EN cobre o teclado
/// de quem alterna. Cada uma esta aqui por ser um ato de fala sem conteudo
/// proprio — saudacao, confirmacao, agradecimento, despedida. Nenhuma delas
/// diz nada que o agente queira lembrar seis meses depois.
pub const DEFAULT_NOISE_PHRASES: &[&str] = &[
    // saudacao / despedida
    "oi",
    "ola",
    "opa",
    "e ai",
    "eai",
    "fala",
    "bom dia",
    "boa tarde",
    "boa noite",
    "ate mais",
    "ate logo",
    "tchau",
    "falou",
    "abraco",
    "abracos",
    // confirmacao / negacao
    "ok",
    "okay",
    "ta",
    "ta bom",
    "tabom",
    "ta bem",
    "tudo bem",
    "tudo certo",
    "certo",
    "claro",
    "sim",
    "nao",
    "pode ser",
    "pode sim",
    "isso",
    "isso mesmo",
    "exato",
    "beleza",
    "blz",
    "show",
    "perfeito",
    "otimo",
    "legal",
    "entendi",
    "entendido",
    "saquei",
    "uhum",
    "aham",
    "hmm",
    "hm",
    // agradecimento / cortesia
    "obrigado",
    "obrigada",
    "obg",
    "vlw",
    "valeu",
    "de nada",
    "por nada",
    "imagina",
    "por favor",
    "pfv",
    "desculpa",
    "desculpe",
    // EN de teclado alternado
    "hi",
    "hello",
    "hey",
    "yes",
    "no",
    "yep",
    "nope",
    "sure",
    "thanks",
    "thank you",
    "thx",
    "cool",
    "nice",
    "great",
    "got it",
    "bye",
];

impl Default for NoisePolicy {
    fn default() -> Self {
        Self::new(true, DEFAULT_MIN_CHARS, &[])
    }
}

impl NoisePolicy {
    /// `extras` sao acrescentadas a [`DEFAULT_NOISE_PHRASES`], nunca a
    /// substituem: a config e aditiva de proposito, para que uma lista
    /// escrita a mao nao apague silenciosamente a cobertura padrao numa
    /// atualizacao.
    pub fn new(enabled: bool, min_chars: usize, extras: &[String]) -> Self {
        let mut phrases: Vec<String> = DEFAULT_NOISE_PHRASES
            .iter()
            .map(|p| normalize(p))
            .filter(|p| !p.is_empty())
            .collect();
        phrases.extend(
            extras
                .iter()
                .map(|p| normalize(p))
                .filter(|p| !p.is_empty()),
        );
        phrases.sort();
        phrases.dedup();
        Self {
            enabled,
            min_chars,
            phrases,
        }
    }

    /// Politica que nunca filtra nada — o comportamento anterior ao #952.
    pub fn disabled() -> Self {
        Self::new(false, 0, &[])
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn min_chars(&self) -> usize {
        self.min_chars
    }

    /// `true` quando o conteudo nao deve receber vetor.
    ///
    /// Com a politica desligada devolve `false` para **tudo**, inclusive para
    /// conteudo vazio: quem desliga o filtro pediu o comportamento anterior
    /// inteiro, e a recusa de conteudo vazio ja e do `MemoryStore`.
    pub fn is_noise(&self, content: &str) -> bool {
        if !self.enabled {
            return false;
        }

        let normalizado = normalize(content);

        // Conteudo que some na normalizacao e so pontuacao ou emoji: "?!",
        // "...", "👍". Nao ha o que embeddar.
        if normalizado.is_empty() {
            return true;
        }
        if normalizado.chars().count() < self.min_chars {
            return true;
        }
        if self.phrases.iter().any(|p| p == &normalizado) {
            return true;
        }
        e_so_risada(&normalizado)
    }
}

/// Reduz o conteudo ao que interessa para comparar: minusculas, sem acento,
/// sem pontuacao nem emoji, espacos colapsados.
///
/// Nao e normalizacao Unicode completa (nao ha `unicode-normalization` na
/// arvore e nao vale uma dependencia para isto): cobre a faixa latina que o
/// portugues usa, que e o alfabeto em que o ruido deste projeto e escrito.
fn normalize(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    let mut espaco_pendente = false;

    for c in s.chars() {
        let c = sem_acento(c.to_lowercase().next().unwrap_or(c));
        if c.is_alphanumeric() {
            if espaco_pendente && !saida.is_empty() {
                saida.push(' ');
            }
            espaco_pendente = false;
            saida.push(c);
        } else {
            // Pontuacao, emoji e espaco viram o mesmo separador: "ok!!!" e
            // "ok" precisam colidir, e "bom-dia" tambem.
            espaco_pendente = true;
        }
    }

    saida
}

fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        outro => outro,
    }
}

/// `true` quando o conteudo inteiro e risada.
///
/// `kkkkkk`, `hahaha`, `rsrsrs` e `hehe` sao o ruido mais comum de chat em
/// portugues e passariam pelo piso de caracteres por serem longos. So casa a
/// **string inteira**: "kkkk pode ser" nao e risada pura e nao entra aqui —
/// a lista de frases e o piso ja tiveram a chance deles.
fn e_so_risada(normalizado: &str) -> bool {
    let compacto: String = normalizado.chars().filter(|c| !c.is_whitespace()).collect();
    if compacto.chars().count() < 3 {
        return false;
    }

    if compacto.chars().all(|c| c == 'k') {
        return true;
    }

    // Repeticao de silaba: hahaha, hehe, rsrsrs, huehue.
    //
    // Daqui para baixo o codigo conta **bytes** (`len`, `chunks`) e nao
    // caracteres, e isso e seguro por uma razao que vale escrever: todas as
    // silabas sao ASCII, e todo byte de um caractere UTF-8 multi-byte tem o
    // bit alto ligado (>= 0x80). Nenhum pedaco de texto grego, cirilico ou
    // CJK pode entao casar com uma silaba, mesmo que o comprimento em bytes
    // seja multiplo de `n` — os bytes nao coincidem. `chunks(n)` com `n >= 1`
    // tambem nunca entra em panico. Se um dia entrar aqui uma silaba
    // nao-ASCII, esta invariante cai e a comparacao precisa passar a ser por
    // caractere.
    for silaba in ["ha", "he", "hi", "rs", "hue"] {
        let n = silaba.len();
        if compacto.len() >= n * 2
            && compacto.len().is_multiple_of(n)
            && compacto
                .as_bytes()
                .chunks(n)
                .all(|ch| ch == silaba.as_bytes())
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padrao() -> NoisePolicy {
        NoisePolicy::default()
    }

    #[test]
    fn o_ruido_relatado_na_issue_e_filtrado() {
        let p = padrao();
        for ruido in ["oi", "ok", "kkk", "tá", "Oi!", "OK.", "ok!!!", "  ok  "] {
            assert!(p.is_noise(ruido), "deveria ser ruido: {ruido:?}");
        }
    }

    #[test]
    fn saudacao_e_agradecimento_inteiros_sao_ruido() {
        let p = padrao();
        for ruido in [
            "bom dia",
            "Bom dia!",
            "boa noite",
            "obrigado",
            "Obrigada :)",
            "valeu",
            "de nada",
            "beleza",
            "entendi",
            "tudo bem",
            "thanks",
            "got it",
        ] {
            assert!(p.is_noise(ruido), "deveria ser ruido: {ruido:?}");
        }
    }

    /// O ponto mais importante do modulo: a frase so e ruido quando e o
    /// conteudo **inteiro**. Uma regra de substring apagaria o eixo semantico
    /// de memoria de verdade.
    #[test]
    fn frase_de_ruido_dentro_de_conteudo_real_nao_filtra() {
        let p = padrao();
        for real in [
            "obrigado pela ajuda com o deploy do gateway",
            "bom dia, preciso do relatorio de vendas de marco",
            "ok, entao vamos usar Postgres em vez de SQLite",
            "beleza mas o teste de RLS ainda falha no CI",
            "sim, meu nome e Michel e eu moro na Florida",
        ] {
            assert!(!p.is_noise(real), "nao deveria ser ruido: {real:?}");
        }
    }

    #[test]
    fn fato_curto_de_verdade_sobrevive() {
        let p = padrao();
        for fato in ["moro em SP", "uso Rust", "sou o Michel", "prefiro pt-BR"] {
            assert!(!p.is_noise(fato), "nao deveria ser ruido: {fato:?}");
        }
    }

    #[test]
    fn risada_inteira_e_ruido_mas_risada_com_conteudo_nao_e() {
        let p = padrao();
        for risada in ["kkkk", "kkkkkkkkkk", "hahaha", "hehe", "rsrsrs", "huehue"] {
            assert!(p.is_noise(risada), "deveria ser ruido: {risada:?}");
        }
        for real in [
            "kkkk pode ser, mas o build quebrou",
            "hahaha esse teste passou de primeira",
        ] {
            assert!(!p.is_noise(real), "nao deveria ser ruido: {real:?}");
        }
    }

    /// Palavra que apenas *comeca* com uma silaba de risada nao pode casar.
    #[test]
    fn palavra_que_parece_risada_nao_e_risada() {
        let p = padrao();
        for real in ["hardware", "heroku", "historico", "hierarquia"] {
            assert!(!p.is_noise(real), "nao deveria ser ruido: {real:?}");
        }
    }

    /// Ancora a invariante documentada em `e_so_risada`: texto nao-ASCII com
    /// comprimento em bytes multiplo do tamanho de uma silaba nao pode ser
    /// confundido com risada. Apontada pela auditoria do #952.
    #[test]
    fn texto_nao_ascii_nao_vira_risada_por_coincidencia_de_bytes() {
        let p = padrao();
        for texto in [
            "ключевое",   // cirilico, 2 bytes/char
            "αβγδεζ",     // grego, 2 bytes/char
            "日本語です", // CJK, 3 bytes/char
            "한국어입니다",
        ] {
            assert!(
                !p.is_noise(texto),
                "texto nao-ASCII classificado como risada: {texto:?}"
            );
        }
    }

    #[test]
    fn so_pontuacao_ou_emoji_e_ruido() {
        let p = padrao();
        for vazio in ["?!", "...", "👍", "🙂🙂", "   ", "!!!"] {
            assert!(p.is_noise(vazio), "deveria ser ruido: {vazio:?}");
        }
    }

    #[test]
    fn desligada_nao_filtra_nada() {
        let p = NoisePolicy::disabled();
        for qualquer in ["oi", "ok", "kkkk", "?!", "", "bom dia"] {
            assert!(
                !p.is_noise(qualquer),
                "filtrou com a politica desligada: {qualquer:?}"
            );
        }
    }

    #[test]
    fn min_chars_zero_desliga_o_piso_e_mantem_as_frases() {
        let p = NoisePolicy::new(true, 0, &[]);
        // "eh" passa: nao esta na lista e o piso esta desligado.
        assert!(!p.is_noise("eh"));
        // A lista continua valendo.
        assert!(p.is_noise("bom dia"));
        assert!(p.is_noise("ok"));
    }

    #[test]
    fn extras_somam_e_nao_substituem_a_lista_padrao() {
        let p = NoisePolicy::new(true, DEFAULT_MIN_CHARS, &["salve familia".to_string()]);
        assert!(p.is_noise("Salve, familia!"), "extra nao entrou");
        assert!(p.is_noise("bom dia"), "extra apagou a lista padrao");
    }

    #[test]
    fn extras_sao_normalizadas_como_o_conteudo() {
        let p = NoisePolicy::new(true, DEFAULT_MIN_CHARS, &["  ATÉ  Mais!! ".to_string()]);
        assert!(p.is_noise("ate mais"));
        assert!(p.is_noise("Até mais."));
    }

    #[test]
    fn extra_vazia_e_ignorada_e_nao_engole_conteudo_normal() {
        // Uma entrada em branco na config nao pode virar uma frase que casa
        // com tudo que normaliza para vazio — isso ja e coberto pelo ramo de
        // string vazia, mas a lista nao pode ganhar um elemento fantasma.
        let p = NoisePolicy::new(
            true,
            DEFAULT_MIN_CHARS,
            &["".to_string(), "   ".to_string()],
        );
        assert!(!p.is_noise("preciso lembrar do deploy de sexta"));
    }

    #[test]
    fn normalize_colapsa_espaco_e_pontuacao() {
        assert_eq!(normalize("Bom   dia!!!"), "bom dia");
        assert_eq!(normalize("bom-dia"), "bom dia");
        assert_eq!(normalize("  OK  "), "ok");
        assert_eq!(normalize("Até logo…"), "ate logo");
        assert_eq!(normalize("ção"), "cao");
        assert_eq!(normalize("👍"), "");
    }

    /// Acento nao pode mudar a decisao: `tá` e `ta` sao a mesma coisa.
    #[test]
    fn acento_nao_muda_a_decisao() {
        let p = padrao();
        assert_eq!(p.is_noise("tá"), p.is_noise("ta"));
        assert_eq!(p.is_noise("está tudo bem"), p.is_noise("esta tudo bem"));
        assert_eq!(p.is_noise("ótimo"), p.is_noise("otimo"));
    }
}
