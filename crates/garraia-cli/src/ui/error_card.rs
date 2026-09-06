//! Erro que diz o que houve e o que fazer a seguir (#941).
//!
//! O #933 tirou o tracing do console. Isso deixou a conversa limpa e criou uma
//! divida: silenciar o log de rotina nao pode significar esconder a falha. Ate
//! aqui o `garra chat` imprimia `Erro: {mensagem crua do provedor}` e, para
//! dois casos, uma dica escolhida por `err_str.contains(...)` solto no meio do
//! laco.
//!
//! # O que este modulo decide, e o que ele nao decide
//!
//! Ele decide **classe** e **acoes**: "isso e credencial faltando, e o proximo
//! passo e configurar a chave". Ele nao decide cor, largura, glifo nem
//! posicao — isso e do `TerminalRenderer` (ADR 0017). Por isso o
//! [`ErrorCard`] e dado, nao texto formatado.
//!
//! # A classificacao olha a mensagem, e isso e o unico sinal que existe
//!
//! O `garraia_common::Error` tem variantes grossas (`Agent(String)`,
//! `Config(String)`): a distincao que interessa — timeout, 401, conexao
//! recusada — vive **dentro** da String. Nao ha tipo para casar.
//!
//! Entao a regra que vale aqui e outra: **nao reconhecer nunca pode perder
//! informacao**. Toda classe desconhecida cai num cartao generico que mostra a
//! mensagem original inteira. O contrario — engolir o texto cru porque a
//! classificacao falhou — trocaria um erro feio por um erro invisivel, que e
//! pior. Ha teste afirmando exatamente isso.
//!
//! # Segredo nunca aparece no cartao
//!
//! Criterio de aceite explicito da #941, e nao e teorico: a mensagem de erro
//! de um provedor e corpo de resposta HTTP, e ha provedor que ecoa o pedido —
//! com o `Authorization` dentro. O detalhe passa por
//! [`garraia_security::redact_secrets`] **e** pelo filtro de controle do #996,
//! porque texto de provedor e texto de fora como qualquer outro.

use crate::ui::ansi_filter::AnsiFilter;

/// O que mostrar quando algo falha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCard {
    /// Uma linha dizendo o que quebrou, nomeando o componente quando se sabe.
    pub titulo: String,
    /// O detalhe — a mensagem do provedor, redigida e saneada.
    pub detalhe: String,
    /// O que o usuario pode fazer agora. Vazio quando nao ha o que sugerir:
    /// inventar acao que nao ajuda e pior que nao sugerir nenhuma.
    pub acoes: Vec<String>,
}

/// A classe da falha, para o titulo e as acoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classe {
    Credencial,
    Timeout,
    Inalcancavel,
    ModeloIndisponivel,
    LimiteDeTaxa,
    Permissao,
    Desconhecida,
}

impl ErrorCard {
    /// Monta o cartao para um erro de turno.
    ///
    /// `provider` nomeia o componente no titulo — "identify the failed
    /// component when known", da issue. Vazio quando nao se sabe, e ai o
    /// titulo fica generico em vez de mentir um nome.
    pub fn from_error(mensagem: &str, provider: &str) -> Self {
        let classe = classificar(mensagem);
        let detalhe = sanear(mensagem);
        let quem = if provider.is_empty() {
            "O provedor".to_string()
        } else {
            provider.to_string()
        };

        let (titulo, acoes) = match classe {
            Classe::Credencial => (
                format!("Credencial de {quem} invalida ou ausente"),
                vec![
                    "Configure a chave e tente de novo".to_string(),
                    "garra doctor — mostra qual variavel esta faltando".to_string(),
                ],
            ),
            Classe::Timeout => (
                format!("{quem} nao respondeu a tempo"),
                vec![
                    "Tente de novo — pode ter sido carga momentanea".to_string(),
                    "--timeout-secs <n> para esperar mais".to_string(),
                    "/provider ou /model para trocar".to_string(),
                ],
            ),
            Classe::Inalcancavel => {
                // O caso local tem conserto proprio e obvio, e dizer "verifique
                // a rede" para quem esqueceu de subir o Ollama seria mandar a
                // pessoa investigar a coisa errada.
                let local = quem.to_lowercase().contains("ollama")
                    || quem.to_lowercase().contains("llamacpp");
                let acoes = if local {
                    vec![
                        "ollama serve — o provedor local nao esta rodando".to_string(),
                        "/provider para usar um provedor de nuvem".to_string(),
                    ]
                } else {
                    vec![
                        "Verifique a conexao de rede".to_string(),
                        "/provider para trocar de provedor".to_string(),
                    ]
                };
                (format!("Sem conexao com {quem}"), acoes)
            }
            Classe::ModeloIndisponivel => (
                "Modelo indisponivel".to_string(),
                vec![
                    "/models para ver o que este provedor oferece".to_string(),
                    "/model <nome> para trocar".to_string(),
                ],
            ),
            Classe::LimiteDeTaxa => (
                format!("{quem} recusou por limite de uso"),
                vec![
                    "Espere alguns instantes e tente de novo".to_string(),
                    "/provider para usar outro provedor".to_string(),
                ],
            ),
            Classe::Permissao => (
                "Permissao negada".to_string(),
                vec!["Verifique as permissoes do arquivo ou diretorio".to_string()],
            ),
            // Sem acao inventada: a mensagem crua vai inteira no detalhe, e e
            // isso que o usuario tem para trabalhar.
            Classe::Desconhecida => (format!("{quem} falhou"), Vec::new()),
        };

        Self {
            titulo,
            detalhe,
            acoes,
        }
    }

    /// O turno estourou o tempo do proprio GarraIA (nao do provedor).
    pub fn timeout_local(timeout_secs: u64) -> Self {
        Self {
            titulo: "Tempo esgotado".to_string(),
            detalhe: format!(
                "A resposta passou de {timeout_secs}s e foi descartada. O histórico da \
                 conversa continua intacto."
            ),
            acoes: vec![
                "Tente de novo".to_string(),
                format!("--timeout-secs <n> para esperar mais que {timeout_secs}s"),
            ],
        }
    }
}

/// Classifica pela mensagem, que e o unico sinal disponivel.
///
/// Cada padrao e minusculo e comparado sobre a mensagem em minusculo: os
/// provedores nao concordam em maiuscula ("Unauthorized", "unauthorized",
/// "UNAUTHORIZED" aparecem os tres na pratica).
fn classificar(mensagem: &str) -> Classe {
    let m = mensagem.to_lowercase();

    // Credencial antes de tudo: um 401 costuma vir junto de outras palavras
    // ("connection closed after 401"), e a acao certa e a da credencial.
    if m.contains("401")
        || m.contains("unauthorized")
        || m.contains("invalid api key")
        || m.contains("api key not found")
        || m.contains("missing api key")
        || m.contains("no api key")
    {
        return Classe::Credencial;
    }
    if m.contains("429") || m.contains("rate limit") || m.contains("too many requests") {
        return Classe::LimiteDeTaxa;
    }
    if m.contains("timed out") || m.contains("timeout") || m.contains("deadline exceeded") {
        return Classe::Timeout;
    }
    if m.contains("connection refused")
        || m.contains("connect error")
        || m.contains("dns error")
        || m.contains("no route to host")
        || m.contains("network is unreachable")
        || m.contains("failed to connect")
        // A frase do `reqwest` para falha de transporte. Ela **nao** contem
        // "connect", e por isso a falha mais comum do projeto — Ollama local
        // que nao esta rodando — nao era reconhecida. Rodar o binario de
        // verdade contra um Ollama desligado foi o que mostrou isso; a lista
        // de padroes acima, escrita de cabeca, passava em todos os testes e
        // errava o caso do dia a dia.
        || m.contains("error sending request")
    {
        return Classe::Inalcancavel;
    }
    if m.contains("model not found")
        || m.contains("unknown model")
        || m.contains("does not exist")
        || m.contains("model_not_found")
    {
        return Classe::ModeloIndisponivel;
    }
    if m.contains("permission denied") || m.contains("access denied") || m.contains("eacces") {
        return Classe::Permissao;
    }

    Classe::Desconhecida
}

/// Redige segredo e tira controle de terminal — nessa ordem, como em todo
/// caminho que imprime texto de fora.
fn sanear(mensagem: &str) -> String {
    AnsiFilter::sanitize_once(&garraia_security::redact_secrets(mensagem.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credencial_nomeia_o_provedor_e_sugere_o_conserto() {
        let c = ErrorCard::from_error("HTTP 401 Unauthorized", "openrouter");
        assert!(c.titulo.contains("openrouter"), "{}", c.titulo);
        assert!(
            c.acoes.iter().any(|a| a.contains("doctor")),
            "{:?}",
            c.acoes
        );
    }

    /// A frase real do `reqwest`, capturada rodando o binario contra um
    /// Ollama desligado. Ela nao contem "connect" nem "refused", entao a lista
    /// de padroes escrita de cabeca a perdia — e era o caso mais comum.
    #[test]
    fn a_frase_do_reqwest_e_reconhecida_como_inalcancavel() {
        let bruto = "agent error: ollama request failed: error sending request for url (http://localhost:11434/api/chat)";
        let c = ErrorCard::from_error(bruto, "ollama (offline)");
        assert!(c.titulo.contains("Sem conexao"), "{}", c.titulo);
        assert!(
            c.acoes.iter().any(|a| a.contains("ollama serve")),
            "nao sugeriu subir o servico local: {:?}",
            c.acoes
        );
    }

    /// Provedor local tem conserto proprio: mandar "verifique a rede" para
    /// quem esqueceu de subir o Ollama e mandar investigar a coisa errada.
    #[test]
    fn provedor_local_sugere_subir_o_servico() {
        let c = ErrorCard::from_error("Connection refused (os error 111)", "ollama");
        assert!(
            c.acoes.iter().any(|a| a.contains("ollama serve")),
            "{:?}",
            c.acoes
        );
    }

    #[test]
    fn provedor_de_nuvem_sugere_rede_e_troca() {
        let c = ErrorCard::from_error("dns error: failed to lookup address", "openai");
        assert!(c.acoes.iter().any(|a| a.contains("rede")), "{:?}", c.acoes);
        assert!(
            !c.acoes.iter().any(|a| a.contains("ollama serve")),
            "sugeriu servico local para provedor de nuvem: {:?}",
            c.acoes
        );
    }

    /// A regra central do modulo: nao reconhecer nao pode perder informacao.
    #[test]
    fn classe_desconhecida_preserva_a_mensagem_inteira() {
        let bruto = "algo muito especifico quebrou no meio do caminho, codigo XYZ-42";
        let c = ErrorCard::from_error(bruto, "openrouter");
        assert_eq!(c.detalhe, bruto, "a mensagem original foi perdida");
        assert!(
            c.acoes.is_empty(),
            "inventou acao para erro desconhecido: {:?}",
            c.acoes
        );
    }

    /// Criterio de aceite da #941, e nao e teorico: ha provedor que ecoa o
    /// pedido inteiro na mensagem de erro, com o `Authorization` dentro.
    #[test]
    fn segredo_na_mensagem_do_provedor_nao_aparece() {
        let bruto = "request failed: Authorization: Bearer sk-ant-api03-aaaaaaaaaaaaaaaaaaaa";
        let c = ErrorCard::from_error(bruto, "anthropic");
        assert!(
            !c.detalhe.contains("sk-ant-api03-aaaaaaaaaaaaaaaaaaaa"),
            "segredo vazou no cartao: {}",
            c.detalhe
        );
    }

    /// Mensagem de provedor e texto de fora: nao pode escrever comando no
    /// terminal (#995/#996).
    #[test]
    fn mensagem_do_provedor_nao_injeta_sequencia() {
        let c = ErrorCard::from_error("falhou\u{1b}[2J\u{1b}[?25l", "openai");
        assert!(
            !c.detalhe.contains('\u{1b}'),
            "ESC sobreviveu: {}",
            c.detalhe
        );
    }

    /// Os provedores nao concordam em maiuscula.
    #[test]
    fn classificacao_ignora_caixa() {
        for m in ["UNAUTHORIZED", "Unauthorized", "unauthorized"] {
            let c = ErrorCard::from_error(m, "x");
            assert!(c.titulo.contains("Credencial"), "{m}: {}", c.titulo);
        }
    }

    /// Um 401 que venha embrulhado em outras palavras ainda e credencial — e
    /// a acao da credencial e a util, nao a da conexao.
    #[test]
    fn credencial_ganha_de_conexao_quando_os_dois_aparecem() {
        let c = ErrorCard::from_error("connection closed after 401 Unauthorized", "openai");
        assert!(c.titulo.contains("Credencial"), "{}", c.titulo);
    }

    #[test]
    fn sem_provedor_conhecido_o_titulo_nao_inventa_nome() {
        let c = ErrorCard::from_error("timed out", "");
        assert!(c.titulo.contains("O provedor"), "{}", c.titulo);
    }

    #[test]
    fn timeout_local_diz_que_o_historico_sobrevive() {
        let c = ErrorCard::timeout_local(120);
        assert!(c.detalhe.contains("120s"), "{}", c.detalhe);
        assert!(c.detalhe.contains("histórico"), "{}", c.detalhe);
        assert!(c.acoes.iter().any(|a| a.contains("--timeout-secs")));
    }

    #[test]
    fn modelo_e_limite_tem_acoes_proprias() {
        let modelo = ErrorCard::from_error("model not found: gpt-9", "openai");
        assert!(modelo.acoes.iter().any(|a| a.contains("/models")));

        let limite = ErrorCard::from_error("429 Too Many Requests", "openrouter");
        assert!(limite.acoes.iter().any(|a| a.contains("Espere")));
    }
}
