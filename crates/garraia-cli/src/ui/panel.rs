//! Painel de rotulo e valor — a superficie das telas de status (#940).
//!
//! # Por que uma funcao pura, e nao um `println!` no `chat.rs`
//!
//! O `/context` de hoje escreve `println!("{DIM}Diretorio: {cwd}{RESET}")`:
//! cor incondicional, largura ignorada, e um escape que aparece em
//! `garra chat | cat`. E a divida que a ADR 0017 registra, e ela cresce a cada
//! superficie nova — que e exatamente o que a #940 pede para acrescentar.
//!
//! Aqui vale a mesma regra do [`super::error_card`]: quem sabe **o que**
//! mostrar e o chamador, quem sabe **como desenhar** e a interface. Um painel
//! ja formatado do outro lado dessa fronteira ignoraria
//! [`super::Capabilities`] — largura, cor, Unicode —, que sao justamente os
//! tres criterios de aceite da issue.
//!
//! Sendo funcao pura de `(titulo, linhas, style, largura)` para `String`, cada
//! um desses criterios vira uma assercao sobre um valor de retorno, sem
//! terminal e sem processo.

use super::conversation::{BOLD, CYAN, DIM, RESET, Style};

/// Espaco entre a coluna do rotulo e a do valor.
const ESPACO: usize = 2;

/// Abaixo disto nao ha duas colunas: o valor vai para a linha de baixo.
///
/// Nao e um numero escolhido no olho — e o ponto em que a coluna do valor
/// ficaria mais estreita que uma palavra comum, e o texto passaria a quebrar
/// a cada duas silabas. Nesse caso empilhar le melhor do que alinhar.
const MIN_COLUNA_DE_VALOR: usize = 16;

/// Uma linha do painel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linha<'a> {
    pub rotulo: &'a str,
    pub valor: String,
}

/// Atalho para montar uma linha sem cerimonia no chamador.
pub fn linha<'a>(rotulo: &'a str, valor: impl Into<String>) -> Linha<'a> {
    Linha {
        rotulo,
        valor: valor.into(),
    }
}

/// Desenha o painel inteiro, com `\n` no fim.
///
/// O valor **e saneado aqui**, e nao so na origem. Nome de modelo vem da API
/// do provider, nome de ramo vem do disco, caminho vem do ambiente: os tres
/// entram no painel e nenhum e nosso. Sanear no chamador funcionaria ate
/// alguem escrever o proximo chamador — a mesma razao pela qual o
/// `write_error_card` saneia de novo o que o `ErrorCard::from_error` ja
/// saneou.
pub fn render(titulo: &str, linhas: &[Linha<'_>], style: Style, largura: usize) -> String {
    let (bold, cyan, dim, reset) = if style.color {
        (BOLD, CYAN, DIM, RESET)
    } else {
        ("", "", "", "")
    };
    let traco = if style.unicode { "\u{2500}" } else { "-" };

    let titulo = super::ansi_filter::AnsiFilter::sanitize_once(titulo);
    let saneadas: Vec<(String, String)> = linhas
        .iter()
        .map(|l| {
            (
                super::ansi_filter::AnsiFilter::sanitize_once(l.rotulo),
                super::ansi_filter::AnsiFilter::sanitize_once(&l.valor),
            )
        })
        .collect();

    let maior_rotulo = saneadas
        .iter()
        .map(|(r, _)| r.chars().count())
        .max()
        .unwrap_or(0);
    let coluna = maior_rotulo + ESPACO;
    // Duas colunas so quando sobra largura para a segunda ser util.
    let empilha = largura.saturating_sub(coluna) < MIN_COLUNA_DE_VALOR;

    let mut saida = String::new();
    saida.push_str(&format!("{bold}{cyan}{titulo}{reset}\n"));
    saida.push_str(&format!(
        "{dim}{}{reset}\n",
        traco.repeat(regua(&titulo, &saneadas, coluna, largura, empilha))
    ));

    for (rotulo, valor) in &saneadas {
        if empilha {
            saida.push_str(&format!("{dim}{rotulo}{reset}\n"));
            for (i, pedaco) in quebra(valor, largura.saturating_sub(ESPACO).max(1))
                .iter()
                .enumerate()
            {
                let _ = i;
                saida.push_str(&format!("{}{pedaco}\n", " ".repeat(ESPACO)));
            }
            continue;
        }
        let recuo = " ".repeat(coluna);
        let pedacos = quebra(valor, largura.saturating_sub(coluna).max(1));
        for (i, pedaco) in pedacos.iter().enumerate() {
            if i == 0 {
                let preenche = " ".repeat(coluna - rotulo.chars().count());
                saida.push_str(&format!("{dim}{rotulo}{reset}{preenche}{pedaco}\n"));
            } else {
                // Continuacao alinhada com o valor, e nao com a margem: senao
                // a segunda linha de um caminho longo parece um rotulo novo.
                saida.push_str(&format!("{recuo}{pedaco}\n"));
            }
        }
    }
    saida
}

/// Desenha uma lista simples sob o mesmo cabecalho do painel.
///
/// `/help`, `/models` e o indice do `/tool` sao listas, nao pares. Passariam
/// como painel de rotulo vazio, mas o alinhamento existe para relacionar duas
/// colunas e aqui ha uma — o resultado seria uma margem sem motivo. O que
/// precisa ser igual e a moldura, e e ela que esta compartilhada.
pub fn render_list(titulo: &str, itens: &[String], style: Style, largura: usize) -> String {
    let (bold, cyan, dim, reset) = if style.color {
        (BOLD, CYAN, DIM, RESET)
    } else {
        ("", "", "", "")
    };
    let traco = if style.unicode { "\u{2500}" } else { "-" };

    let titulo = super::ansi_filter::AnsiFilter::sanitize_once(titulo);
    let itens: Vec<String> = itens
        .iter()
        .map(|i| super::ansi_filter::AnsiFilter::sanitize_once(i))
        .collect();

    let comprimento = itens
        .iter()
        .map(|i| i.chars().count())
        .max()
        .unwrap_or(0)
        .max(titulo.chars().count())
        .clamp(1, largura.max(1));

    let mut saida = String::new();
    saida.push_str(&format!("{bold}{cyan}{titulo}{reset}\n"));
    saida.push_str(&format!("{dim}{}{reset}\n", traco.repeat(comprimento)));
    for item in &itens {
        for pedaco in quebra(item, largura.max(1)) {
            saida.push_str(&pedaco);
            saida.push('\n');
        }
    }
    saida
}

/// Comprimento da regua: acompanha o conteudo, sem passar da largura.
///
/// Uma regua de largura fixa num painel de tres linhas curtas fica maior que
/// o painel; uma que acompanha o conteudo emoldura o que existe.
fn regua(
    titulo: &str,
    linhas: &[(String, String)],
    coluna: usize,
    largura: usize,
    empilha: bool,
) -> usize {
    let maior = linhas
        .iter()
        .map(|(r, v)| {
            if empilha {
                r.chars().count().max(v.chars().count() + ESPACO)
            } else {
                coluna + v.chars().count()
            }
        })
        .max()
        .unwrap_or(0)
        .max(titulo.chars().count());
    maior.clamp(1, largura.max(1))
}

/// Quebra o valor em pedacos que cabem em `largura`, preferindo o espaco.
///
/// Devolve **sempre** ao menos um pedaco, inclusive para valor vazio: um
/// rotulo sem linha nenhuma sumiria do painel, e "sem valor" e uma informacao.
fn quebra(valor: &str, largura: usize) -> Vec<String> {
    if valor.chars().count() <= largura {
        return vec![valor.to_string()];
    }
    let mut pedacos = Vec::new();
    let mut atual = String::new();
    for palavra in valor.split(' ') {
        let cabe =
            atual.is_empty() || atual.chars().count() + 1 + palavra.chars().count() <= largura;
        if !cabe {
            pedacos.push(std::mem::take(&mut atual));
        }
        if !atual.is_empty() {
            atual.push(' ');
        }
        // Palavra maior que a linha inteira (um caminho fundo, uma URL): entra
        // e estoura. Parti-la ao meio quebraria justamente o que se quer
        // copiar do painel.
        atual.push_str(palavra);
    }
    if !atual.is_empty() {
        pedacos.push(atual);
    }
    if pedacos.is_empty() {
        pedacos.push(String::new());
    }
    pedacos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amostra() -> Vec<Linha<'static>> {
        vec![
            linha("Diretorio", "/home/user/GarraRUST"),
            linha("Ramo", "main"),
            linha("Provider", "ollama"),
        ]
    }

    /// Sem cor, nao sai um unico escape — o criterio "non-TTY behavior remains
    /// deterministic" medido no valor de retorno.
    #[test]
    fn sem_cor_nao_sai_escape() {
        let s = render("Contexto", &amostra(), Style::PLAIN, 72);
        assert!(!s.contains('\x1b'), "saiu: {s:?}");
        assert!(s.contains("Diretorio  /home/user/GarraRUST"));
        assert!(!s.contains('\u{2500}'), "sem Unicode a regua e ASCII");
    }

    #[test]
    fn com_cor_o_titulo_ganha_destaque_e_o_rotulo_fica_discreto() {
        let s = render("Contexto", &amostra(), Style::RICH, 72);
        assert!(s.contains(&format!("{BOLD}{CYAN}Contexto{RESET}")));
        assert!(s.contains(&format!("{DIM}Ramo{RESET}")));
        assert!(s.contains('\u{2500}'));
    }

    /// Os valores comecam todos na mesma coluna.
    #[test]
    fn os_valores_ficam_alinhados() {
        let s = render("Contexto", &amostra(), Style::PLAIN, 72);
        let colunas: Vec<usize> = s
            .lines()
            .skip(2)
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| {
                l.find("/home")
                    .or_else(|| l.find("main"))
                    .or_else(|| l.find("ollama"))
            })
            .collect();
        assert_eq!(colunas.len(), 3, "tres valores: {s:?}");
        assert!(
            colunas.windows(2).all(|p| p[0] == p[1]),
            "colunas diferentes {colunas:?} em {s:?}"
        );
    }

    /// Nenhuma linha passa da largura, e a continuacao fica alinhada.
    #[test]
    fn valor_longo_quebra_e_a_continuacao_alinha() {
        let linhas = vec![linha(
            "Projeto",
            "Rust Cargo Docker Compose Kubernetes Terraform Ansible Vagrant",
        )];
        let s = render("Contexto", &linhas, Style::PLAIN, 40);
        let corpo: Vec<&str> = s.lines().skip(2).filter(|l| !l.is_empty()).collect();
        assert!(corpo.len() > 1, "precisava quebrar: {s:?}");
        for l in &corpo {
            assert!(l.chars().count() <= 40, "linha longa demais: {l:?}");
        }
        let coluna = "Projeto".chars().count() + ESPACO;
        for c in &corpo[1..] {
            assert!(
                c.starts_with(&" ".repeat(coluna)),
                "continuacao sem alinhamento: {c:?}"
            );
        }
    }

    /// Num terminal muito estreito o painel empilha em vez de espremer.
    #[test]
    fn terminal_estreito_empilha_em_vez_de_espremer() {
        let s = render("Contexto", &amostra(), Style::PLAIN, 20);
        assert!(
            s.contains("Diretorio\n  /home/user/GarraRUST"),
            "devia empilhar: {s:?}"
        );
    }

    /// Escape vindo de nome de ramo, de modelo ou de caminho nao atravessa.
    ///
    /// Nenhum dos tres e nosso: ramo vem do disco, modelo vem da API do
    /// provider, caminho vem do ambiente.
    #[test]
    fn valor_hostil_nao_injeta_escape() {
        let linhas = vec![
            linha("Ramo", "main\x1b[2J\x1b[H"),
            linha("Modelo", "gpt\x1b]0;titulo\x07-4"),
        ];
        let s = render("Status\x1b[31m", &linhas, Style::PLAIN, 72);
        assert!(!s.contains('\x1b'), "escape atravessou: {s:?}");
        assert!(s.contains("main"), "o texto util fica: {s:?}");
    }

    /// Valor vazio nao faz o rotulo sumir.
    #[test]
    fn valor_vazio_mantem_a_linha() {
        let linhas = vec![linha("Ramo", "")];
        let s = render("Contexto", &linhas, Style::PLAIN, 72);
        assert!(s.contains("Ramo"), "o rotulo fica: {s:?}");
    }

    /// A lista usa a mesma moldura do painel, e sem cor nao sai escape.
    #[test]
    fn a_lista_compartilha_a_moldura_e_respeita_o_no_color() {
        let itens = vec![
            "/help    mostra isto".to_string(),
            "/exit    sair".to_string(),
        ];
        let plano = render_list("Comandos", &itens, Style::PLAIN, 72);
        assert!(!plano.contains('\x1b'), "saiu: {plano:?}");
        assert!(plano.starts_with("Comandos\n"));
        assert!(plano.lines().nth(1).is_some_and(|l| l.starts_with('-')));

        let rico = render_list("Comandos", &itens, Style::RICH, 72);
        assert!(rico.contains(&format!("{BOLD}{CYAN}Comandos{RESET}")));
        assert!(rico.contains('\u{2500}'));
    }

    /// Item hostil na lista tambem nao injeta escape.
    #[test]
    fn item_de_lista_hostil_nao_injeta_escape() {
        let itens = vec!["modelo\x1b[2J-x".to_string()];
        let s = render_list("Modelos", &itens, Style::PLAIN, 72);
        assert!(!s.contains('\x1b'), "escape atravessou: {s:?}");
        assert!(s.contains("modelo"));
    }

    /// A regua acompanha o conteudo e nunca passa da largura.
    #[test]
    fn a_regua_acompanha_o_conteudo() {
        let curto = render("Ct", &[linha("A", "b")], Style::PLAIN, 72);
        let regua = curto.lines().nth(1).expect("regua");
        assert!(regua.chars().count() < 20, "regua grande demais: {regua:?}");

        for largura in [24usize, 30, 40, 72] {
            let p = render("Contexto", &amostra(), Style::PLAIN, largura);
            let regua = p.lines().nth(1).expect("regua");
            assert!(
                regua.chars().count() <= largura,
                "regua passou da largura {largura}: {regua:?}"
            );
        }
    }

    /// Valor sem espaco maior que a largura **estoura**, e nao e truncado.
    ///
    /// Um caminho e um nome de ramo nao tem onde quebrar, e cortar com `…`
    /// destruiria justamente o que se copia do painel. Quem quebra e o
    /// terminal — a mesma decisao do renderizador de Markdown para URL longa.
    /// Medido rodando o binario com `COLUMNS=30`: e o comportamento, e nao um
    /// descuido.
    #[test]
    fn valor_sem_espaco_estoura_em_vez_de_ser_truncado() {
        let linhas = vec![linha("Ramo", "claude/6-issues-strategy-5mufs3-e5")];
        let s = render("Contexto", &linhas, Style::PLAIN, 30);
        assert!(
            s.contains("claude/6-issues-strategy-5mufs3-e5"),
            "o valor tem de sair inteiro: {s:?}"
        );
        assert!(!s.contains('\u{2026}'), "nada de reticencia: {s:?}");
    }
}
