//! Markdown legivel no terminal, sem quebrar o streaming (#939).
//!
//! # O problema que decide o desenho
//!
//! O texto do modelo chega em pedacos, e um construto de Markdown pode ser
//! partido entre dois deltas: `**neg` num, `rito**` no outro. E a mesma classe
//! de problema do [`super::ansi_filter`], e a solucao tem a mesma forma —
//! estado que atravessa as chamadas.
//!
//! A tentacao e bufferizar ate a quebra de linha e renderizar a linha inteira.
//! Isso da renderizacao correta e zero cintilacao, mas **para a prosa**: o
//! modelo costuma emitir um paragrafo inteiro como uma unica linha, entao o
//! usuario ficaria olhando para o nada ate o paragrafo terminar. Streaming que
//! nao aparece nao e streaming.
//!
//! Entao a regra e outra: **segurar so o que ainda pode mudar de significado.**
//!
//! - No comeco da linha, segura ate dar para decidir o tipo do bloco — sao
//!   poucos caracteres (`# `, `- `, `> `, ```` ``` ````, `1. `). Um `-` sozinho
//!   ainda pode virar lista (`- item`) ou regra (`---`), entao espera o
//!   proximo.
//! - Decidido o bloco, o resto da linha sai incrementalmente, segurando apenas
//!   a cauda que ainda pode fazer parte de um delimitador inline nao fechado
//!   (`` ` ``, `**`, `*`, `_`, `[`).
//!
//! O atraso maximo e de uma palavra, e nunca de uma linha inteira.
//!
//! # Quebra na largura do terminal
//!
//! A prosa quebra na largura detectada, e a continuacao recebe o recuo do
//! bloco: a segunda linha de um item de lista fica alinhada com a primeira em
//! vez de voltar a coluna zero e se confundir com o item seguinte. E esse
//! alinhamento, e nao a quebra em si, que justifica quebrar — o terminal ja
//! quebra sozinho no limite direito, mas sempre na coluna zero.
//!
//! E por isso que a emissao e por palavra inteira: para decidir se a palavra
//! cabe e preciso conhece-la inteira. Quem mede e
//! [`console::measure_text_width`], que ignora escape e conta largura visual —
//! contar bytes erraria com acento e com ideograma, e para o lado errado.
//!
//! Bloco cercado **nao** quebra: um `\n` que o modelo nao escreveu vira um
//! `\n` que o usuario cola. La a decisao fica com o terminal.
//!
//! # O que nao e estilizado
//!
//! Dentro de bloco cercado (```` ``` ````) nao ha estilo inline nem quebra de
//! linha por largura: o criterio de aceite pede que codigo continue facil de
//! copiar, e um `*` no meio de um programa e um `*`, nao enfase.
//!
//! Sem cor — `NO_COLOR`, saida redirecionada, `TERM=dumb` — nada disto emite
//! escape nenhum: o texto sai como veio. Isso e o que mantem
//! `garra chat > arquivo` util para automacao.

use super::conversation::Style;

/// Sequencias usadas na renderizacao. Sao as mesmas famílias que o resto da
/// interface ja usa, para o visual nao destoar.
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALICO: &str = "\x1b[3m";
const CIANO: &str = "\x1b[36m";
const AMARELO: &str = "\x1b[33m";

/// Quantos caracteres bastam para decidir o tipo de bloco de uma linha.
///
/// O maior prefixo que interessa e ```` ```lang ```` e `###### `; sete cobre
/// os dois com folga. E o teto tambem serve de rede: uma linha que comece com
/// muitos `-` (uma regra longa) nao fica presa esperando decisao.
const PREFIXO_MAX: usize = 7;

/// O tipo de bloco em que a linha corrente esta.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Bloco {
    /// Ainda juntando caracteres para decidir.
    Indeciso,
    /// Prosa comum — inclui item de lista e citacao depois do marcador.
    Texto,
    /// Dentro de bloco cercado: sai literal.
    Codigo,
}

/// O que `decide_traco_ou_lista` concluiu.
///
/// Precisa dos tres estados: sem separar "nao e comigo" de "e comigo mas ainda
/// nao sei", o chamador lia a duvida como negativa e um `- ` virava texto comum
/// antes de o espaco chegar.
enum TracoOuLista {
    NaoEhMeuCaso,
    Esperando,
    Decidido(String),
}

/// Renderiza Markdown incrementalmente para o terminal.
#[derive(Debug)]
pub struct MarkdownStream {
    style: Style,
    /// Largura util do terminal. `0` desliga a quebra.
    largura: usize,
    /// A linha em construcao, do inicio ate o que chegou.
    linha: String,
    /// Quantos **bytes** de `linha` ja foram emitidos.
    emitido: usize,
    bloco: Bloco,
    /// `true` entre a cerca de abertura e a de fechamento.
    em_fence: bool,
    /// Colunas visiveis ja escritas na linha **fisica** corrente.
    ///
    /// Nao e o mesmo que `emitido`: a quebra insere `\n` no meio de uma linha
    /// logica, e o que decide a proxima quebra e a fisica.
    coluna: usize,
    /// A ultima coisa escrita foi parte de uma palavra que pode continuar no
    /// proximo pedaco?
    ///
    /// Um bloco unico maior que a linha sai em varias emissoes, e sem esta
    /// marca a segunda emissao parecia uma palavra nova e ganhava uma quebra
    /// no meio — bem no unico caso em que quebrar nao ajuda.
    palavra_em_curso: bool,
    /// Com o que comeca a continuacao quando a linha quebra.
    ///
    /// E o que faz um item de lista longo continuar parecendo um item: sem
    /// isto a segunda linha volta a coluna zero e se confunde com o proximo.
    recuo: String,
}

impl MarkdownStream {
    /// `largura` e a largura util do terminal; `0` desliga a quebra.
    pub fn new(style: Style, largura: usize) -> Self {
        Self {
            style,
            largura,
            linha: String::new(),
            emitido: 0,
            bloco: Bloco::Indeciso,
            em_fence: false,
            coluna: 0,
            palavra_em_curso: false,
            recuo: String::new(),
        }
    }

    /// Consome um pedaco e devolve o que ja da para escrever na tela.
    pub fn push(&mut self, delta: &str) -> String {
        // Sem cor, nao ha o que renderizar: o texto sai como veio, e nenhum
        // escape e inventado. E o caminho de pipe, arquivo e `NO_COLOR`.
        if !self.style.color {
            return delta.to_string();
        }

        let mut saida = String::new();
        for ch in delta.chars() {
            if ch == '\n' {
                saida.push_str(&self.fecha_linha());
                continue;
            }
            self.linha.push(ch);
            saida.push_str(&self.avanca());
        }
        saida
    }

    /// Fecha o que sobrou. Chamado no fim do turno.
    pub fn finish(&mut self) -> String {
        if !self.style.color {
            return String::new();
        }
        let mut saida = self.emite_resto_da_linha();
        self.linha.clear();
        self.emitido = 0;
        self.coluna = 0;
        self.palavra_em_curso = false;
        self.recuo.clear();
        self.bloco = Bloco::Indeciso;
        // A cerca aberta nao e fechada a forca: o texto acabou assim, e
        // inventar um fim mudaria o que o modelo disse.
        if !saida.is_empty() && self.em_fence {
            saida.push_str(RESET);
        }
        saida
    }

    /// Emite o que a chegada de mais um caractere liberou.
    fn avanca(&mut self) -> String {
        if self.bloco == Bloco::Indeciso {
            match self.decide_bloco() {
                None => return String::new(), // ainda esperando
                Some(prefixo) => return prefixo,
            }
        }
        if self.bloco == Bloco::Codigo {
            // Codigo sai literal, sem segurar nada.
            return self.emite_ate(self.linha.len());
        }
        let limite = self.limite_de_emissao();
        self.emite_ate(limite)
    }

    /// Tenta decidir o tipo de bloco. `Some(prefixo_renderizado)` quando
    /// decidiu; `None` quando ainda faltam caracteres.
    fn decide_bloco(&mut self) -> Option<String> {
        let l = self.linha.clone();
        let l = l.as_str();

        if self.em_fence {
            // Dentro de cerca, so a propria cerca fecha.
            if l == "```" || (l.len() >= 3 && l.starts_with("```")) {
                self.em_fence = false;
                self.bloco = Bloco::Texto;
                self.emitido = self.linha.len();
                self.coluna = 3;
                return Some(format!("{DIM}```{RESET}"));
            }
            if l.chars().count() >= 3 || !"`".starts_with(&l[..l.len().min(1)]) {
                self.bloco = Bloco::Codigo;
                return Some(self.emite_ate(self.linha.len()));
            }
            return None;
        }

        // Cerca de abertura.
        if l.starts_with("```") {
            self.em_fence = true;
            self.bloco = Bloco::Codigo;
            self.emitido = self.linha.len();
            self.coluna = l.chars().count();
            return Some(format!("{DIM}{l}{RESET}"));
        }
        if "```".starts_with(l) {
            return None; // "`" ou "``": ainda pode virar cerca
        }

        // Titulo: `#` ate `######` seguido de espaco.
        if l.starts_with('#') {
            let sustenidos = l.chars().take_while(|c| *c == '#').count();
            if sustenidos <= 6 && l.len() > sustenidos {
                if l.as_bytes()[sustenidos] == b' ' {
                    self.bloco = Bloco::Texto;
                    self.emitido = sustenidos + 1;
                    // O `# ` some da tela, entao a coluna nao anda.
                    self.coluna = 0;
                    return Some(format!("{BOLD}{CIANO}"));
                }
                // `#texto` nao e titulo em Markdown.
                self.bloco = Bloco::Texto;
                return Some(self.emite_ate(self.limite_de_emissao()));
            }
            if sustenidos > 6 {
                self.bloco = Bloco::Texto;
                return Some(self.emite_ate(self.limite_de_emissao()));
            }
            return None;
        }

        // Citacao.
        if l.starts_with("> ") {
            self.bloco = Bloco::Texto;
            self.emitido = 2;
            self.coluna = 2;
            self.recuo = "  ".to_string();
            let barra = if self.style.unicode { "│ " } else { "| " };
            return Some(format!("{DIM}{barra}"));
        }
        if l == ">" {
            return None;
        }

        // Regra horizontal e lista compartilham o primeiro caractere.
        match self.decide_traco_ou_lista() {
            TracoOuLista::NaoEhMeuCaso => {}
            // Sem esta distincao, "ainda nao sei" era lido como "nao e lista" e
            // um `- ` virava texto comum antes de o espaco chegar.
            TracoOuLista::Esperando => return None,
            TracoOuLista::Decidido(r) => return Some(r),
        }

        // Lista numerada: digitos seguidos de `. `.
        let digitos = l.chars().take_while(|c| c.is_ascii_digit()).count();
        if digitos > 0 {
            if l.len() == digitos {
                return if digitos < PREFIXO_MAX {
                    None
                } else {
                    self.vira_texto()
                };
            }
            if l.as_bytes()[digitos] == b'.' {
                if l.len() == digitos + 1 {
                    return None;
                }
                if l.as_bytes()[digitos + 1] == b' ' {
                    self.bloco = Bloco::Texto;
                    self.emitido = digitos + 2;
                    // "  " + digitos + "." + " "
                    self.coluna = digitos + 4;
                    self.recuo = " ".repeat(digitos + 4);
                    let n = &l[..digitos];
                    return Some(format!("  {AMARELO}{n}.{RESET} "));
                }
            }
            return self.vira_texto();
        }

        self.vira_texto()
    }

    /// `-`, `*` e `_` no inicio da linha podem ser lista, regra ou enfase.
    fn decide_traco_ou_lista(&mut self) -> TracoOuLista {
        let l = self.linha.clone();
        let l = l.as_str();
        let Some(primeiro) = l.chars().next() else {
            return TracoOuLista::NaoEhMeuCaso;
        };
        if !matches!(primeiro, '-' | '*' | '_' | '+') {
            return TracoOuLista::NaoEhMeuCaso;
        }

        // `- ` / `* ` / `+ ` -> item de lista.
        if l.len() >= 2 && l.as_bytes()[1] == b' ' && primeiro != '_' {
            self.bloco = Bloco::Texto;
            self.emitido = 2;
            // "  " + marcador + " "
            self.coluna = 4;
            self.recuo = "    ".to_string();
            let ponto = if self.style.unicode { "•" } else { "*" };
            return TracoOuLista::Decidido(format!("  {AMARELO}{ponto}{RESET} "));
        }

        // Tres ou mais iguais -> regra horizontal.
        let iguais = l.chars().take_while(|c| *c == primeiro).count();
        if iguais >= 3 {
            self.bloco = Bloco::Texto;
            self.emitido = l.len();
            self.coluna = 24;
            let traco = if self.style.unicode { "─" } else { "-" };
            return TracoOuLista::Decidido(format!("{DIM}{}{RESET}", traco.repeat(24)));
        }
        if l.len() == iguais && iguais < 3 {
            // `-` ou `--` ainda pode virar `- item` ou `---`.
            return TracoOuLista::Esperando;
        }

        match self.vira_texto() {
            Some(t) => TracoOuLista::Decidido(t),
            None => TracoOuLista::Esperando,
        }
    }

    fn vira_texto(&mut self) -> Option<String> {
        self.bloco = Bloco::Texto;
        Some(self.emite_ate(self.limite_de_emissao()))
    }

    /// Ate onde da para emitir: nem partir construto, nem partir palavra.
    ///
    /// # Por que uma varredura so
    ///
    /// Sao dois limites, e eles **nao** sao independentes. O primeiro segura
    /// da ultima abertura nao fechada em diante: se `**neg` chegou e o `**` de
    /// fechamento nao, emitir agora mostraria os asteriscos crus e nao daria
    /// mais para estiliza-los. O segundo, quando ha quebra por largura, segura
    /// a palavra incompleta — sem conhece-la inteira nao da para saber se ela
    /// cabe.
    ///
    /// O segundo sozinho corta dentro do primeiro: em
    /// `[o site](https://exemplo.org)` o ultimo espaco fica **dentro** do
    /// link, e cortar ali entrega ao estilizador um `[o` sem o `](`, que sai
    /// cru na tela. Por isso o espaco so conta quando esta fora de codigo
    /// inline, enfase e link — o que exige a mesma varredura que acha a
    /// abertura pendente.
    fn limite_de_emissao(&self) -> usize {
        let bytes = self.linha.as_bytes();
        let mut i = self.emitido;
        // Posicao e largura do delimitador aberto ainda sem par.
        let mut aberto: Option<(usize, usize)> = None;
        let mut em_codigo = false;
        // Inicio do ultimo grupo de espacos visto **fora** de qualquer
        // construto. E onde a palavra incompleta comeca a ser segurada.
        let mut espaco_livre: Option<usize> = None;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'`' {
                if em_codigo {
                    em_codigo = false;
                    aberto = None;
                } else {
                    em_codigo = true;
                    aberto = Some((i, 1));
                }
                i += 1;
                continue;
            }
            if em_codigo {
                i += 1;
                continue;
            }
            match b {
                b'*' | b'_' => {
                    // A largura do fechamento tem de casar com a da abertura.
                    //
                    // Sem isso, o `*` final de `**forte*` — que ainda espera o
                    // segundo asterisco — fechava o `**` do inicio, e o trecho
                    // saia emitido no meio do delimitador. O estilo ficava
                    // errado e nao dava mais para consertar: o texto ja estava
                    // na tela.
                    let largura = if i + 1 < bytes.len() && bytes[i + 1] == b {
                        2
                    } else {
                        1
                    };
                    match aberto {
                        Some((_, w)) if w == largura => aberto = None,
                        Some(_) => {}
                        None => aberto = Some((i, largura)),
                    }
                    i += largura;
                }
                b'[' => {
                    aberto = Some((i, 1));
                    i += 1;
                }
                b')' => {
                    aberto = None;
                    i += 1;
                }
                _ => {
                    if aberto.is_none() && (b as char).is_whitespace() {
                        // So o **inicio** do grupo: assim o espaco que separa
                        // duas palavras viaja com a segunda, e a quebra
                        // consegue descarta-lo em vez de deixa-lo pendurado.
                        let anterior_era_espaco = i > self.emitido
                            && (bytes[i - 1] as char).is_whitespace()
                            && espaco_livre.is_some();
                        if !anterior_era_espaco {
                            espaco_livre = Some(i);
                        }
                    }
                    i += 1;
                }
            }
        }

        let construto = aberto.map(|(pos, _)| pos).unwrap_or(bytes.len());
        if self.largura == 0 {
            return construto;
        }
        match espaco_livre {
            Some(pos) => construto.min(pos),
            // Sem espaco livre nenhum: segurar ate o `\n` pararia o streaming
            // num bloco unico maior que a linha (URL longa, base64), entao ele
            // sai e o terminal decide onde parte.
            None if console::measure_text_width(&self.linha[self.emitido..]) > self.largura => {
                construto
            }
            None => self.emitido,
        }
    }

    /// Escreve um trecho ja estilizado quebrando na largura do terminal.
    ///
    /// Mede com [`console::measure_text_width`], que ignora escape e conta
    /// largura visual — um `\x1b[1m` ocupa zero colunas e um ideograma ocupa
    /// duas. Contar bytes quebraria nos dois casos, e para o lado errado.
    fn quebra(&mut self, estilizado: &str) -> String {
        if self.largura == 0 {
            self.coluna += console::measure_text_width(estilizado);
            return estilizado.to_string();
        }
        let recuo_largura = self.recuo.chars().count();
        let mut saida = String::new();
        for grupo in grupos(estilizado) {
            let largura_do_grupo = console::measure_text_width(grupo);
            if grupo.starts_with(char::is_whitespace) {
                saida.push_str(grupo);
                self.coluna += largura_do_grupo;
                self.palavra_em_curso = false;
                continue;
            }
            // Grupo que nao caberia nem numa linha vazia (URL longa, base64):
            // quebrar antes dele nao resolve nada e so gasta uma linha. Passa
            // a decisao ao terminal.
            let cabe_sozinho = largura_do_grupo + recuo_largura <= self.largura;
            if !self.palavra_em_curso
                && cabe_sozinho
                && self.coluna + largura_do_grupo > self.largura
                && self.coluna > recuo_largura
            {
                apara_espacos(&mut saida, &mut self.coluna);
                saida.push('\n');
                saida.push_str(&self.recuo);
                self.coluna = recuo_largura;
            }
            saida.push_str(grupo);
            self.coluna += largura_do_grupo;
            self.palavra_em_curso = true;
        }
        saida
    }

    /// Emite `linha[emitido..limite]` com estilo inline aplicado.
    fn emite_ate(&mut self, limite: usize) -> String {
        let limite = limite.min(self.linha.len());
        if limite <= self.emitido {
            return String::new();
        }
        // Nao corta no meio de um caractere multibyte.
        let mut fim = limite;
        while fim > self.emitido && !self.linha.is_char_boundary(fim) {
            fim -= 1;
        }
        if fim <= self.emitido {
            return String::new();
        }
        let trecho = self.linha[self.emitido..fim].to_string();
        self.emitido = fim;
        if self.bloco == Bloco::Codigo {
            // Codigo nao quebra: o criterio de aceite pede que continue facil
            // de copiar, e um `\n` que o modelo nao escreveu vira um `\n` que
            // o usuario cola. Linha longa passa a decisao ao terminal.
            self.coluna += trecho.chars().count();
            return trecho;
        }
        let estilizado = estiliza_inline(&trecho, self.style);
        self.quebra(&estilizado)
    }

    fn emite_resto_da_linha(&mut self) -> String {
        if self.bloco == Bloco::Indeciso {
            self.bloco = Bloco::Texto;
        }
        self.emite_ate(self.linha.len())
    }

    /// Termina a linha corrente e devolve o que faltava mais o `\n`.
    fn fecha_linha(&mut self) -> String {
        let era_titulo = self.linha.starts_with('#')
            && !self.em_fence
            && self.linha.chars().take_while(|c| *c == '#').count() <= 6;
        let mut saida = self.emite_resto_da_linha();
        if era_titulo || self.linha.starts_with("> ") {
            saida.push_str(RESET);
        }
        saida.push('\n');
        self.linha.clear();
        self.emitido = 0;
        self.coluna = 0;
        self.palavra_em_curso = false;
        self.recuo.clear();
        // Toda linha nova comeca indecisa, dentro ou fora de cerca: e a
        // propria linha que diz se e a cerca de fechamento ou codigo.
        self.bloco = Bloco::Indeciso;
        saida
    }
}

/// Aplica negrito, enfase e codigo inline a um trecho ja delimitado.
///
/// Funciona sobre trecho, e nao sobre linha, porque o chamador so entrega o que
/// ja esta fechado — ver `MarkdownStream::limite_de_emissao`.
fn estiliza_inline(trecho: &str, style: Style) -> String {
    if !style.color {
        return trecho.to_string();
    }
    let mut fora = String::with_capacity(trecho.len());
    let bytes = trecho.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Codigo inline vem primeiro: dentro dele, `*` e `*`.
        //
        // O `fim > i + 1` exige conteudo, como o `achar_par` ja exigia para
        // `*` e `_`. Sem ele, uma crase colada na outra virava um vao vazio
        // colorido: um ``` no meio da linha saia como um par de escapes
        // seguido de uma crase solta. Achado rodando o binario.
        if bytes[i] == b'`'
            && let Some(fim) = trecho[i + 1..].find('`').map(|p| i + 1 + p)
            && fim > i + 1
        {
            fora.push_str(CIANO);
            fora.push_str(&trecho[i + 1..fim]);
            fora.push_str(RESET);
            i = fim + 1;
            continue;
        }
        if (bytes[i] == b'*' || bytes[i] == b'_')
            && i + 1 < bytes.len()
            && bytes[i + 1] == bytes[i]
            && let Some(fim) = achar_par(trecho, i + 2, bytes[i], 2)
        {
            fora.push_str(BOLD);
            fora.push_str(&trecho[i + 2..fim]);
            fora.push_str(RESET);
            i = fim + 2;
            continue;
        }
        if (bytes[i] == b'*' || bytes[i] == b'_')
            && let Some(fim) = achar_par(trecho, i + 1, bytes[i], 1)
        {
            fora.push_str(ITALICO);
            fora.push_str(&trecho[i + 1..fim]);
            fora.push_str(RESET);
            i = fim + 1;
            continue;
        }
        // Link `[texto](url)`: mostra os dois. Esconder a URL num terminal
        // tiraria justamente o que da para copiar.
        if bytes[i] == b'['
            && let Some(fecha) = trecho[i..].find("](").map(|p| i + p)
            && let Some(fim) = trecho[fecha + 2..].find(')').map(|p| fecha + 2 + p)
        {
            fora.push_str(&trecho[i + 1..fecha]);
            fora.push(' ');
            fora.push_str(DIM);
            fora.push('(');
            fora.push_str(&trecho[fecha + 2..fim]);
            fora.push(')');
            fora.push_str(RESET);
            i = fim + 1;
            continue;
        }
        let ch_len = trecho[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        fora.push_str(&trecho[i..i + ch_len]);
        i += ch_len;
    }
    fora
}

/// Parte o trecho em grupos alternados de espaco e nao-espaco.
///
/// Uma sequencia de escape nunca contem espaco, entao ela sempre acompanha a
/// palavra a que se aplica — o estilo nunca fica orfao numa linha por causa de
/// uma quebra.
fn grupos(s: &str) -> Vec<&str> {
    let mut saida = Vec::new();
    let mut inicio = 0;
    let mut atual: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let espaco = c.is_whitespace();
        match atual {
            Some(anterior) if anterior != espaco => {
                saida.push(&s[inicio..i]);
                inicio = i;
                atual = Some(espaco);
            }
            None => atual = Some(espaco),
            _ => {}
        }
    }
    if inicio < s.len() {
        saida.push(&s[inicio..]);
    }
    saida
}

/// Tira os espacos que ficaram no fim da linha antes de quebra-la.
///
/// O espaco que separava duas palavras nao deve virar o ultimo caractere da
/// linha: em selecao de texto e em `cat -A` ele aparece, e ninguem o escreveu.
fn apara_espacos(saida: &mut String, coluna: &mut usize) {
    let aparado = saida.trim_end_matches(char::is_whitespace).len();
    if aparado < saida.len() {
        *coluna -= console::measure_text_width(&saida[aparado..]);
        saida.truncate(aparado);
    }
}

/// Acha o fechamento de um delimitador de `largura` bytes iguais a `delim`.
fn achar_par(s: &str, de: usize, delim: u8, largura: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = de;
    while i + largura <= b.len() {
        if b[i] == delim && (largura == 1 || b[i + 1] == delim) {
            // Delimitador colado no abre (`**` vazio) nao conta.
            return if i > de { Some(i) } else { None };
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Larga o bastante para os testes que nao sao sobre quebra nao quebrarem.
    const LARGURA_DE_TESTE: usize = 200;

    fn rico() -> MarkdownStream {
        MarkdownStream::new(Style::RICH, LARGURA_DE_TESTE)
    }

    fn renderiza(texto: &str) -> String {
        let mut m = rico();
        let mut s = m.push(texto);
        s.push_str(&m.finish());
        s
    }

    /// Renderiza o mesmo texto cortado em **todo** tamanho de pedaco possivel.
    ///
    /// E o teste que importa num renderizador de streaming: um construto pode
    /// chegar partido em qualquer ponto, e escolher um corte a dedo testa o
    /// corte que o autor imaginou — que e justamente o que nao quebra.
    fn renderiza_em_todos_os_cortes(texto: &str) -> Vec<String> {
        let chars: Vec<char> = texto.chars().collect();
        (1..=chars.len())
            .map(|n| {
                let mut m = rico();
                let mut saida = String::new();
                for pedaco in chars.chunks(n) {
                    saida.push_str(&m.push(&pedaco.iter().collect::<String>()));
                }
                saida.push_str(&m.finish());
                saida
            })
            .collect()
    }

    #[test]
    fn o_resultado_nao_depende_de_como_o_texto_foi_partido() {
        for texto in [
            "**negrito** no meio\n",
            "um `codigo` inline\n",
            "# Titulo\ntexto depois\n",
            "- item um\n- item dois\n",
            "1. primeiro\n2. segundo\n",
            "> uma citacao\n",
            "---\n",
            "veja [o site](https://exemplo.org) ali\n",
            "```rust\nlet x = *y;\n```\n",
            "misto: **forte** e `codigo` e *enfase*\n",
        ] {
            let saidas = renderiza_em_todos_os_cortes(texto);
            let primeira = &saidas[0];
            for (i, s) in saidas.iter().enumerate() {
                assert_eq!(
                    s,
                    primeira,
                    "corte de {} caractere(s) mudou o resultado de {texto:?}",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn negrito_enfase_e_codigo_inline() {
        let s = renderiza("**forte** e *fraco* e `codigo`\n");
        assert!(s.contains(&format!("{BOLD}forte{RESET}")));
        assert!(s.contains(&format!("{ITALICO}fraco{RESET}")));
        assert!(s.contains(&format!("{CIANO}codigo{RESET}")));
        assert!(!s.contains("**"), "os marcadores nao ficam na tela");
    }

    #[test]
    fn titulo_lista_citacao_e_regra() {
        let t = renderiza("# Um titulo\n");
        assert!(t.contains("Um titulo") && t.contains(BOLD));
        assert!(!t.contains("# "), "o sustenido nao vai para a tela");

        let l = renderiza("- primeiro\n");
        assert!(l.contains("primeiro") && l.contains('•'));

        let n = renderiza("1. um\n");
        assert!(n.contains("um") && n.contains("1."));

        let c = renderiza("> citado\n");
        assert!(c.contains("citado") && c.contains('│'));

        let r = renderiza("---\n");
        assert!(r.contains('─') && !r.contains("---"));
    }

    /// Codigo cercado sai literal — e o criterio "facil de copiar".
    #[test]
    fn bloco_de_codigo_sai_sem_estilo_inline() {
        let s = renderiza("```rust\nlet a = *b + **c;\nlet s = `x`;\n```\n");
        assert!(
            s.contains("let a = *b + **c;"),
            "os asteriscos do codigo tem de sobreviver: {s:?}"
        );
        assert!(
            s.contains("let s = `x`;"),
            "as crases do codigo tambem: {s:?}"
        );
        assert!(!s.contains(ITALICO), "nada de enfase dentro de codigo");
    }

    /// Sem cor, nao sai um unico escape — e o texto e identico ao original.
    ///
    /// E o criterio "redirected output does not contain unwanted ANSI escapes"
    /// e o `NO_COLOR` de uma vez.
    #[test]
    fn sem_cor_o_texto_atravessa_intacto() {
        let entrada = "# Titulo\n- item **forte**\n```\ncodigo\n```\n> citado\n";
        let mut m = MarkdownStream::new(Style::PLAIN, LARGURA_DE_TESTE);
        let mut s = m.push(entrada);
        s.push_str(&m.finish());

        assert_eq!(s, entrada, "sem cor o texto sai como veio");
        assert!(!s.contains('\x1b'), "nenhum escape");
    }

    /// O link mostra texto **e** URL: esconder a URL tira o que da para copiar.
    #[test]
    fn link_mostra_texto_e_url() {
        let s = renderiza("veja [o site](https://exemplo.org) ali\n");
        assert!(s.contains("o site"));
        assert!(s.contains("https://exemplo.org"));
        assert!(!s.contains("]("), "a sintaxe crua nao fica");
    }

    /// A prosa nao espera a quebra de linha para aparecer.
    ///
    /// E a razao de o renderizador nao bufferizar por linha: o modelo emite
    /// paragrafo inteiro como uma linha so, e o usuario ficaria olhando para o
    /// nada ate o ponto final.
    #[test]
    fn prosa_aparece_antes_da_quebra_de_linha() {
        let mut m = rico();
        let saida = m.push("Este e um paragrafo longo que ainda nao terminou");
        assert!(
            saida.contains("paragrafo longo"),
            "o texto tem de sair enquanto chega, e nao so no \\n: {saida:?}"
        );
    }

    /// So a cauda ambigua fica retida, e ela sai no `finish`.
    #[test]
    fn delimitador_nao_fechado_sai_no_fim() {
        let mut m = rico();
        let durante = m.push("texto **aberto");
        // Sem o espaco: com a quebra ligada, o espaco que separa duas palavras
        // viaja com a segunda, para a quebra poder descarta-lo se ela cair ali.
        assert!(durante.contains("texto"), "o que e certo sai na hora");
        assert!(
            !durante.contains("aberto"),
            "o que ainda pode virar negrito espera: {durante:?}"
        );

        let fim = m.finish();
        assert!(fim.contains("aberto"), "e nada se perde no fim: {fim:?}");
    }

    /// Acento nao pode ser cortado no meio.
    #[test]
    fn corte_nao_quebra_caractere_multibyte() {
        for saida in renderiza_em_todos_os_cortes("ação e coração **ç**\n") {
            assert!(saida.contains("ação"), "saiu: {saida:?}");
            assert!(saida.contains("coração"));
        }
    }

    /// `#texto` sem espaco nao e titulo, e `#######` tambem nao.
    #[test]
    fn sustenido_sem_espaco_nao_vira_titulo() {
        let s = renderiza("#hashtag e nao titulo\n");
        assert!(s.contains("#hashtag"), "o sustenido fica: {s:?}");

        let sete = renderiza("####### sete\n");
        assert!(
            sete.contains("#######"),
            "sete sustenidos nao e titulo: {sete:?}"
        );
    }

    fn estreito(largura: usize) -> MarkdownStream {
        MarkdownStream::new(Style::RICH, largura)
    }

    fn sem_escape(s: &str) -> String {
        let mut fora = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            fora.push(c);
        }
        fora
    }

    /// Nenhuma linha passa da largura, e nenhuma palavra e partida ao meio.
    #[test]
    fn a_prosa_quebra_na_largura() {
        let mut m = estreito(24);
        let mut s = m.push("uma frase razoavelmente longa que precisa quebrar em varias linhas\n");
        s.push_str(&m.finish());
        let limpo = sem_escape(&s);

        for linha in limpo.lines() {
            assert!(
                console::measure_text_width(linha) <= 24,
                "linha passou de 24 colunas: {linha:?}"
            );
        }
        assert!(limpo.lines().count() > 1, "quebrou mesmo: {limpo:?}");
        // Nenhuma palavra do original foi partida.
        for palavra in
            "uma frase razoavelmente longa que precisa quebrar em varias linhas".split_whitespace()
        {
            assert!(
                limpo.contains(palavra),
                "palavra {palavra:?} partida: {limpo:?}"
            );
        }
    }

    /// A continuacao de um item de lista fica alinhada com o texto do item.
    ///
    /// E o motivo de existir a quebra: o terminal tambem quebraria, mas na
    /// coluna zero, e a segunda linha pareceria um item novo.
    #[test]
    fn a_continuacao_do_item_recebe_o_recuo() {
        let mut m = estreito(24);
        let mut s = m.push("- um item de lista bastante comprido aqui\n");
        s.push_str(&m.finish());
        let limpo = sem_escape(&s);
        let linhas: Vec<&str> = limpo.lines().collect();

        assert!(linhas.len() > 1, "precisava quebrar: {limpo:?}");
        for continuacao in &linhas[1..] {
            assert!(
                continuacao.starts_with("    "),
                "continuacao sem recuo: {continuacao:?}"
            );
        }
    }

    /// Codigo cercado nao ganha quebra: um `\n` inventado vira um `\n` colado.
    #[test]
    fn bloco_de_codigo_nao_quebra_na_largura() {
        let longa = "let resultado = funcao_com_nome_bem_longo(argumento_um, argumento_dois);";
        let mut m = estreito(24);
        let mut s = m.push(&format!("```rust\n{longa}\n```\n"));
        s.push_str(&m.finish());
        let limpo = sem_escape(&s);

        assert!(
            limpo.lines().any(|l| l == longa),
            "a linha de codigo tem de sair inteira: {limpo:?}"
        );
    }

    /// Uma palavra maior que a linha inteira sai — nao trava o streaming, e
    /// nao ganha uma quebra que ninguem escreveu.
    ///
    /// Segurar ate o `\n` deixaria a tela parada numa URL longa ou num base64.
    /// E quebra-la nao ajudaria: ela estoura a linha de qualquer jeito, entao
    /// quem parte e o terminal — e uma URL colada de volta continua inteira.
    #[test]
    fn palavra_maior_que_a_linha_nao_prende_a_saida() {
        let gigante = "a".repeat(60);
        let mut m = estreito(24);
        let durante = m.push(&gigante);
        assert!(
            durante.len() > 24,
            "o bloco unico maior que a linha tem de ir saindo: {durante:?}"
        );

        let tudo = format!("{durante}{}", m.finish());
        assert!(tudo.contains(&gigante), "e sair inteiro: {tudo:?}");
        assert!(!tudo.contains('\n'), "sem quebra inventada: {tudo:?}");
    }

    /// Com a quebra ligada, o corte do delta continua sem mudar o resultado.
    #[test]
    fn a_quebra_nao_depende_de_como_o_texto_foi_partido() {
        let texto = "- um item de lista bastante comprido aqui\nprosa longa que tambem quebra em mais de uma linha\n";
        let chars: Vec<char> = texto.chars().collect();
        let saidas: Vec<String> = (1..=chars.len())
            .map(|n| {
                let mut m = estreito(24);
                let mut saida = String::new();
                for pedaco in chars.chunks(n) {
                    saida.push_str(&m.push(&pedaco.iter().collect::<String>()));
                }
                saida.push_str(&m.finish());
                saida
            })
            .collect();
        for (i, s) in saidas.iter().enumerate() {
            assert_eq!(
                s,
                &saidas[0],
                "corte de {} caractere(s) mudou a quebra",
                i + 1
            );
        }
    }

    /// Nenhuma linha quebrada termina em espaco.
    #[test]
    fn a_quebra_nao_deixa_espaco_pendurado() {
        let mut m = estreito(24);
        let mut s = m.push("uma frase razoavelmente longa que precisa quebrar\n");
        s.push_str(&m.finish());
        for linha in sem_escape(&s).lines() {
            assert_eq!(
                linha,
                linha.trim_end(),
                "linha com espaco no fim: {linha:?}"
            );
        }
    }

    /// Crase colada em crase nao vira um vao vazio colorido.
    ///
    /// Achado rodando o binario: uma linha com ``` no meio saia como um par de
    /// escapes seguido de uma crase solta e do resto do texto.
    #[test]
    fn crase_sem_conteudo_fica_literal() {
        let s = renderiza("use ``` para cercar\n");
        assert!(s.contains("```"), "as tres crases ficam: {s:?}");
        assert!(s.contains("para cercar"));
        assert!(
            !s.contains(&format!("{CIANO}{RESET}")),
            "nada de vao vazio colorido: {s:?}"
        );

        let vazio = renderiza("um `` vazio\n");
        assert!(vazio.contains("``"), "as duas crases ficam: {vazio:?}");
    }

    /// Uma linha que so tem `-` ou `--` nao trava esperando virar regra.
    #[test]
    fn traco_curto_nao_fica_preso() {
        let mut m = rico();
        let mut tudo = m.push("--");
        tudo.push_str(&m.finish());
        assert!(tudo.contains("--"), "o que chegou tem de sair: {tudo:?}");
    }
}
