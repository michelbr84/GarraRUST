//! O avaliador de expressões das condições de automação (#1128).
//!
//! Uma linguagem pequena, deliberadamente: literais (número, string, bool,
//! `null`), caminhos de acesso (`to.state`, `from.attributes.temperature`),
//! comparações (`== != < <= > >=`), lógica (`&& || !` e as palavras `and`
//! `or` `not`) e aritmética (`+ - * / %`). Nada mais — sem chamada de
//! função, sem reflexão, sem acesso ao mundo fora do contexto passado.
//! Fail-closed em duas camadas: o `parse` rejeita sintaxe inválida na carga
//! da automação (regra ruim não sobe), e a `avaliar` devolve erro para
//! caminho ausente e tipo errado (condição quebrada nunca vira `true` por
//! engano — o motor registra `condition_error` e não executa).
//!
//! O contexto é o JSON do evento. Para um gatilho `state_changed`:
//!
//! ```json
//! {
//!   "to":   { "state": "33.5", "attributes": { ... } },
//!   "from": { "state": "31.0", "attributes": { ... } },
//!   "entity_id": "sensor.garagem_temperatura",
//!   "domain": "sensor"
//! }
//! ```
//!
//! Valores de estado do hub chegam como string (`"33.5"`); comparar com um
//! número coerciona quando a string parses (`to.state > 32`). Strings não
//! numéricas em comparação numérica são erro — `state == "on"` é o caminho
//! certo para domínios assim.

/// O que pode dar errado — na carga (parse) ou na avaliação.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    #[error("sintaxe inválida: {0}")]
    Sintaxe(String),
    #[error("caminho '{0}' não existe no contexto do evento")]
    Caminho(String),
    #[error("operandos incompatíveis: {0}")]
    Tipo(String),
    #[error("divisão por zero")]
    DivisaoPorZero,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CmpOp {
    Igual,
    Diferente,
    Menor,
    MenorIgual,
    Maior,
    MaiorIgual,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AritOp {
    Soma,
    Sub,
    Mul,
    Div,
    Mod,
}

/// A árvore da expressão. `parse` uma vez na carga; `avaliar` a cada evento.
#[derive(Debug, Clone)]
pub struct Expr {
    raiz: Node,
}

#[derive(Debug, Clone)]
enum Node {
    Num(f64),
    Str(String),
    Bool(bool),
    Nulo,
    Caminho(Vec<String>),
    Nao(Box<Node>),
    E(Box<Node>, Box<Node>),
    Ou(Box<Node>, Box<Node>),
    Cmp {
        op: CmpOp,
        esq: Box<Node>,
        dir: Box<Node>,
    },
    Arit {
        op: AritOp,
        esq: Box<Node>,
        dir: Box<Node>,
    },
    Neg(Box<Node>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Str(String),
    Path(Vec<String>),
    Verdadeiro,
    Falso,
    Nulo,
    Cmp(CmpOp),
    ELogico,
    OuLogico,
    NaoLogico,
    Arit(AritOp),
    AbrePar,
    FechaPar,
    Fim,
}

fn lexar(entrada: &str) -> Result<Vec<Token>, ExprError> {
    let chars: Vec<char> = entrada.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                tokens.push(Token::AbrePar);
                i += 1;
            }
            ')' => {
                tokens.push(Token::FechaPar);
                i += 1;
            }
            '&' if i + 1 < chars.len() && chars[i + 1] == '&' => {
                tokens.push(Token::ELogico);
                i += 2;
            }
            '|' if i + 1 < chars.len() && chars[i + 1] == '|' => {
                tokens.push(Token::OuLogico);
                i += 2;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Cmp(CmpOp::Igual));
                i += 2;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Cmp(CmpOp::Diferente));
                i += 2;
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Cmp(CmpOp::MenorIgual));
                    i += 2;
                } else {
                    tokens.push(Token::Cmp(CmpOp::Menor));
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Cmp(CmpOp::MaiorIgual));
                    i += 2;
                } else {
                    tokens.push(Token::Cmp(CmpOp::Maior));
                    i += 1;
                }
            }
            '+' => {
                tokens.push(Token::Arit(AritOp::Soma));
                i += 1;
            }
            '*' => {
                tokens.push(Token::Arit(AritOp::Mul));
                i += 1;
            }
            '/' => {
                tokens.push(Token::Arit(AritOp::Div));
                i += 1;
            }
            '%' => {
                tokens.push(Token::Arit(AritOp::Mod));
                i += 1;
            }
            '-' => {
                tokens.push(Token::Arit(AritOp::Sub));
                i += 1;
            }
            '!' => {
                tokens.push(Token::NaoLogico);
                i += 1;
            }
            '"' | '\'' => {
                let fecha = c;
                i += 1;
                let mut texto = String::new();
                let mut fechou = false;
                while i < chars.len() {
                    let c = chars[i];
                    if c == '\\' && i + 1 < chars.len() {
                        texto.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    if c == fecha {
                        fechou = true;
                        i += 1;
                        break;
                    }
                    texto.push(c);
                    i += 1;
                }
                if !fechou {
                    return Err(ExprError::Sintaxe("string sem fechamento".into()));
                }
                tokens.push(Token::Str(texto));
            }
            c if c.is_ascii_digit() => {
                let mut num = String::new();
                let mut ponto = false;
                while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !ponto))
                {
                    if chars[i] == '.' {
                        // `1.` e `1.2.3` são sintaxe — o número não engole
                        // o ponto que não vem seguido de dígito.
                        if i + 1 >= chars.len() || !chars[i + 1].is_ascii_digit() {
                            break;
                        }
                        ponto = true;
                    }
                    num.push(chars[i]);
                    i += 1;
                }
                let valor: f64 = num
                    .parse()
                    .map_err(|_| ExprError::Sintaxe(format!("número inválido '{num}'")))?;
                tokens.push(Token::Num(valor));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut segmento = String::new();
                let mut caminho = Vec::new();
                loop {
                    segmento.clear();
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        segmento.push(chars[i]);
                        i += 1;
                    }
                    caminho.push(segmento.clone());
                    if i + 1 < chars.len() && chars[i] == '.' {
                        let proximo = chars[i + 1];
                        if proximo.is_ascii_alphabetic() || proximo == '_' {
                            i += 1; // o '.' vira separador do caminho
                            continue;
                        }
                    }
                    break;
                }
                // Palavras reservadas só valem como token solto (sem ponto).
                if caminho.len() == 1 {
                    match caminho[0].as_str() {
                        "and" => {
                            tokens.push(Token::ELogico);
                            continue;
                        }
                        "or" => {
                            tokens.push(Token::OuLogico);
                            continue;
                        }
                        "not" => {
                            tokens.push(Token::NaoLogico);
                            continue;
                        }
                        "true" => {
                            tokens.push(Token::Verdadeiro);
                            continue;
                        }
                        "false" => {
                            tokens.push(Token::Falso);
                            continue;
                        }
                        "null" => {
                            tokens.push(Token::Nulo);
                            continue;
                        }
                        _ => {}
                    }
                }
                tokens.push(Token::Path(caminho));
            }
            outro => {
                return Err(ExprError::Sintaxe(format!(
                    "caractere inesperado '{outro}'"
                )));
            }
        }
    }
    tokens.push(Token::Fim);
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn olhar(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Fim)
    }

    fn avancar(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Fim);
        self.pos += 1;
        t
    }

    fn esperado(&mut self, o_que: &str) -> Result<(), ExprError> {
        Err(ExprError::Sintaxe(format!(
            "esperado {o_que}, encontrei {:?}",
            self.olhar()
        )))
    }

    fn parse_expr(&mut self) -> Result<Node, ExprError> {
        self.parse_ou()
    }

    fn parse_ou(&mut self) -> Result<Node, ExprError> {
        let mut esq = self.parse_e()?;
        while matches!(self.olhar(), Token::OuLogico) {
            self.avancar();
            let dir = self.parse_e()?;
            esq = Node::Ou(Box::new(esq), Box::new(dir));
        }
        Ok(esq)
    }

    fn parse_e(&mut self) -> Result<Node, ExprError> {
        let mut esq = self.parse_nao()?;
        while matches!(self.olhar(), Token::ELogico) {
            self.avancar();
            let dir = self.parse_nao()?;
            esq = Node::E(Box::new(esq), Box::new(dir));
        }
        Ok(esq)
    }

    fn parse_nao(&mut self) -> Result<Node, ExprError> {
        if matches!(self.olhar(), Token::NaoLogico) {
            self.avancar();
            return Ok(Node::Nao(Box::new(self.parse_nao()?)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Node, ExprError> {
        let esq = self.parse_arit()?;
        let op = match self.olhar() {
            Token::Cmp(op) => *op,
            _ => return Ok(esq),
        };
        self.avancar();
        let dir = self.parse_arit()?;
        Ok(Node::Cmp {
            op,
            esq: Box::new(esq),
            dir: Box::new(dir),
        })
    }

    fn parse_arit(&mut self) -> Result<Node, ExprError> {
        let mut esq = self.parse_mul()?;
        while let Token::Arit(op @ (AritOp::Soma | AritOp::Sub)) = self.olhar() {
            let op = *op;
            self.avancar();
            let dir = self.parse_mul()?;
            esq = Node::Arit {
                op,
                esq: Box::new(esq),
                dir: Box::new(dir),
            };
        }
        Ok(esq)
    }

    fn parse_mul(&mut self) -> Result<Node, ExprError> {
        let mut esq = self.parse_unario()?;
        while let Token::Arit(op @ (AritOp::Mul | AritOp::Div | AritOp::Mod)) = self.olhar() {
            let op = *op;
            self.avancar();
            let dir = self.parse_unario()?;
            esq = Node::Arit {
                op,
                esq: Box::new(esq),
                dir: Box::new(dir),
            };
        }
        Ok(esq)
    }

    fn parse_unario(&mut self) -> Result<Node, ExprError> {
        // O '-' na posição de prefixo é negação; no infix, o parse_arit
        // trata como subtração — a posição desambigua.
        if matches!(self.olhar(), Token::Arit(AritOp::Sub)) {
            self.avancar();
            return Ok(Node::Neg(Box::new(self.parse_unario()?)));
        }
        self.parse_primario()
    }

    fn parse_primario(&mut self) -> Result<Node, ExprError> {
        match self.avancar() {
            Token::Num(n) => Ok(Node::Num(n)),
            Token::Str(s) => Ok(Node::Str(s)),
            Token::Verdadeiro => Ok(Node::Bool(true)),
            Token::Falso => Ok(Node::Bool(false)),
            Token::Nulo => Ok(Node::Nulo),
            Token::Path(caminho) => Ok(Node::Caminho(caminho)),
            Token::AbrePar => {
                let dentro = self.parse_expr()?;
                if !matches!(self.avancar(), Token::FechaPar) {
                    self.esperado("')'")?;
                }
                Ok(dentro)
            }
            outro => Err(ExprError::Sintaxe(format!("token inesperado {outro:?}"))),
        }
    }
}

impl Expr {
    /// Compila a expressão uma vez — sintaxe ruim é rejeitada na carga da
    /// automação, não no meio da noite de um sensor barulhento.
    pub fn parse(entrada: &str) -> Result<Self, ExprError> {
        if entrada.trim().is_empty() {
            return Err(ExprError::Sintaxe("expressão vazia".into()));
        }
        let tokens = lexar(entrada)?;
        let mut parser = Parser { pos: 0, tokens };
        let raiz = parser.parse_expr()?;
        if !matches!(parser.olhar(), Token::Fim) {
            return Err(ExprError::Sintaxe(format!(
                "sobrou conteúdo após a expressão: {:?}",
                parser.olhar()
            )));
        }
        Ok(Self { raiz })
    }

    /// Avalia contra o contexto do evento. Caminho ausente e tipo errado
    /// são **erro** — nunca `false` silencioso.
    pub fn avaliar(&self, ctx: &serde_json::Value) -> Result<serde_json::Value, ExprError> {
        avaliar_node(&self.raiz, ctx)
    }
}

fn avaliar_node(node: &Node, ctx: &serde_json::Value) -> Result<serde_json::Value, ExprError> {
    match node {
        Node::Num(n) => Ok(serde_json::json!(n)),
        Node::Str(s) => Ok(serde_json::json!(s)),
        Node::Bool(b) => Ok(serde_json::json!(b)),
        Node::Nulo => Ok(serde_json::Value::Null),
        Node::Caminho(segmentos) => {
            let mut atual = ctx;
            for seg in segmentos {
                atual = match atual {
                    serde_json::Value::Object(mapa) => mapa
                        .get(seg)
                        .ok_or_else(|| ExprError::Caminho(segmentos.join(".")))?,
                    _ => {
                        return Err(ExprError::Caminho(segmentos.join(".")));
                    }
                };
            }
            Ok(atual.clone())
        }
        Node::Nao(dentro) => {
            let v = avaliar_node(dentro, ctx)?;
            let b = como_bool(&v)?;
            Ok(serde_json::json!(!b))
        }
        Node::E(esq, dir) => {
            let e = como_bool(&avaliar_node(esq, ctx)?)?;
            // Curto-circuito: o lado direito nem é avaliado quando já é falso.
            if !e {
                return Ok(serde_json::json!(false));
            }
            let d = como_bool(&avaliar_node(dir, ctx)?)?;
            Ok(serde_json::json!(d))
        }
        Node::Ou(esq, dir) => {
            let e = como_bool(&avaliar_node(esq, ctx)?)?;
            if e {
                return Ok(serde_json::json!(true));
            }
            let d = como_bool(&avaliar_node(dir, ctx)?)?;
            Ok(serde_json::json!(d))
        }
        Node::Neg(dentro) => {
            let v = avaliar_node(dentro, ctx)?;
            let n = como_num(&v)?;
            Ok(serde_json::json!(-n))
        }
        Node::Cmp { op, esq, dir } => {
            let a = avaliar_node(esq, ctx)?;
            let b = avaliar_node(dir, ctx)?;
            Ok(serde_json::json!(comparar(*op, &a, &b)?))
        }
        Node::Arit { op, esq, dir } => {
            let a = avaliar_node(esq, ctx)?;
            let b = avaliar_node(dir, ctx)?;
            let x = como_num(&a)?;
            let y = como_num(&b)?;
            let r = match op {
                AritOp::Soma => x + y,
                AritOp::Sub => x - y,
                AritOp::Mul => x * y,
                AritOp::Div => {
                    if y == 0.0 {
                        return Err(ExprError::DivisaoPorZero);
                    }
                    x / y
                }
                AritOp::Mod => {
                    if y == 0.0 {
                        return Err(ExprError::DivisaoPorZero);
                    }
                    x % y
                }
            };
            Ok(serde_json::json!(r))
        }
    }
}

fn como_bool(v: &serde_json::Value) -> Result<bool, ExprError> {
    match v {
        serde_json::Value::Bool(b) => Ok(*b),
        outro => Err(ExprError::Tipo(format!(
            "esperado booleano, encontrei {outro}"
        ))),
    }
}

/// Número direto, ou string que parses como número (estados do hub chegam
/// como string — `"33.5" > 32` tem que funcionar).
fn como_num(v: &serde_json::Value) -> Result<f64, ExprError> {
    match v {
        serde_json::Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| ExprError::Tipo(format!("número fora de faixa: {n}"))),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| ExprError::Tipo(format!("string '{s}' não é um número comparável"))),
        outro => Err(ExprError::Tipo(format!("valor não numérico: {outro}"))),
    }
}

fn comparar(op: CmpOp, a: &serde_json::Value, b: &serde_json::Value) -> Result<bool, ExprError> {
    // Coerção numérica quando UM dos lados é número e o outro parses.
    let numerico =
        matches!(a, serde_json::Value::Number(_)) || matches!(b, serde_json::Value::Number(_));
    if numerico {
        let (x, y) = (como_num(a)?, como_num(b)?);
        return Ok(match op {
            CmpOp::Igual => x == y,
            CmpOp::Diferente => x != y,
            CmpOp::Menor => x < y,
            CmpOp::MenorIgual => x <= y,
            CmpOp::Maior => x > y,
            CmpOp::MaiorIgual => x >= y,
        });
    }
    match (a, b) {
        (serde_json::Value::String(x), serde_json::Value::String(y)) => Ok(match op {
            CmpOp::Igual => x == y,
            CmpOp::Diferente => x != y,
            CmpOp::Menor => x < y,
            CmpOp::MenorIgual => x <= y,
            CmpOp::Maior => x > y,
            CmpOp::MaiorIgual => x >= y,
        }),
        (serde_json::Value::Bool(x), serde_json::Value::Bool(y)) => Ok(match op {
            CmpOp::Igual => x == y,
            CmpOp::Diferente => x != y,
            _ => {
                return Err(ExprError::Tipo(
                    "booleanos só comparam por igualdade, não por ordem".to_string(),
                ));
            }
        }),
        (serde_json::Value::Null, serde_json::Value::Null) => Ok(matches!(
            op,
            CmpOp::Igual | CmpOp::MenorIgual | CmpOp::MaiorIgual
        )),
        // Um dos lados null e o outro não: desigual por definição.
        (serde_json::Value::Null, _) | (_, serde_json::Value::Null) => {
            Ok(matches!(op, CmpOp::Diferente | CmpOp::Menor | CmpOp::Maior))
        }
        (x, y) => Err(ExprError::Tipo(format!(
            "tipos não comparáveis: {x} vs {y}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_exemplo() -> serde_json::Value {
        json!({
            "to": { "state": "33.5", "attributes": { "temperature": 34.1, "unit": "C" } },
            "from": { "state": "31.0", "attributes": {} },
            "entity_id": "sensor.garagem_temperatura",
            "domain": "sensor"
        })
    }

    fn ok(expr: &str, ctx: &serde_json::Value) -> serde_json::Value {
        Expr::parse(expr)
            .unwrap_or_else(|e| panic!("parse de '{expr}' falhou: {e}"))
            .avaliar(ctx)
            .unwrap_or_else(|e| panic!("avaliar '{expr}' falhou: {e}"))
    }

    #[test]
    fn numero_compara_com_estado_string_do_hub() {
        // O caso do issue: estado do hub é string; a comparação coerciona.
        assert_eq!(ok("to.state > 32", &ctx_exemplo()), json!(true));
        assert_eq!(ok("to.state < 32", &ctx_exemplo()), json!(false));
        assert_eq!(ok("from.state == 31", &ctx_exemplo()), json!(true));
    }

    #[test]
    fn string_nao_numerica_vs_numero_e_erro() {
        let ctx = json!({ "to": { "state": "on" } });
        let err = Expr::parse("to.state > 32")
            .unwrap()
            .avaliar(&ctx)
            .unwrap_err();
        assert!(matches!(err, ExprError::Tipo(_)), "{err}");
    }

    #[test]
    fn precedencia_aritmetica_e_comparacao() {
        let ctx = json!({});
        assert_eq!(ok("1 + 2 * 3 == 7", &ctx), json!(true));
        assert_eq!(ok("(1 + 2) * 3 == 9", &ctx), json!(true));
        assert_eq!(ok("10 / 4 == 2.5", &ctx), json!(true));
        assert_eq!(ok("7 % 3 == 1", &ctx), json!(true));
        assert_eq!(ok("10 - 4 == 6", &ctx), json!(true));
        assert_eq!(ok("10 - 4 - 3 == 3", &ctx), json!(true));
    }

    #[test]
    fn logica_com_simbolos_e_palavras() {
        let ctx = ctx_exemplo();
        assert_eq!(ok("to.state > 32 && from.state < 32", &ctx), json!(true));
        assert_eq!(ok("to.state > 32 and from.state < 32", &ctx), json!(true));
        assert_eq!(ok("to.state < 32 || from.state < 32", &ctx), json!(true));
        assert_eq!(ok("to.state > 32 or from.state < 32", &ctx), json!(true));
        assert_eq!(ok("!(to.state > 32)", &ctx), json!(false));
        assert_eq!(ok("not to.state > 32", &ctx), json!(false));
        // Curto-circuito: lado direito inválido não é avaliado.
        assert_eq!(ok("false && 1/0 == 1", &ctx), json!(false));
        assert_eq!(ok("true || 1/0 == 1", &ctx), json!(true));
    }

    #[test]
    fn caminho_profundo_ate_atributo() {
        assert_eq!(
            ok("to.attributes.temperature > 34", &ctx_exemplo()),
            json!(true)
        );
        assert_eq!(
            ok("to.attributes.unit == \"C\"", &ctx_exemplo()),
            json!(true)
        );
    }

    #[test]
    fn caminho_ausente_e_erro_nunca_false() {
        let err = Expr::parse("to.new_state.state > 32")
            .unwrap()
            .avaliar(&ctx_exemplo())
            .unwrap_err();
        assert!(matches!(err, ExprError::Caminho(p) if p == "to.new_state.state"));
    }

    #[test]
    fn booleanos_sao_estritos() {
        let ctx = json!({});
        for expr in [
            "1 && true",
            "true && 0",
            "false || 0",
            "!\"texto\"",
            "not 1",
        ] {
            let err = Expr::parse(expr).unwrap().avaliar(&ctx).unwrap_err();
            assert!(matches!(err, ExprError::Tipo(_)), "{expr}: {err}");
        }
    }

    #[test]
    fn resultado_nao_booleano_sai_como_valor() {
        // O motor exige Bool no topo; o avaliador só entrega o valor.
        assert_eq!(ok("1 + 1", &json!({})), json!(2.0));
        assert_eq!(ok("\"on\"", &json!({})), json!("on"));
    }

    #[test]
    fn igualdade_de_strings_e_null() {
        let ctx = json!({ "to": { "state": "on" }, "x": null });
        assert_eq!(ok("to.state == 'on'", &ctx), json!(true));
        assert_eq!(ok("to.state != \"off\"", &ctx), json!(true));
        assert_eq!(ok("x == null", &ctx), json!(true));
        assert_eq!(ok("x != null", &ctx), json!(false));
        assert_eq!(ok("to.state == null", &ctx), json!(false));
    }

    #[test]
    fn divisao_por_zero_e_erro() {
        let err = Expr::parse("1 / 0")
            .unwrap()
            .avaliar(&json!({}))
            .unwrap_err();
        assert!(matches!(err, ExprError::DivisaoPorZero));
    }

    #[test]
    fn unario_negativo_e_parenteses() {
        let ctx = json!({ "a": 5 });
        assert_eq!(ok("-a + 10 == 5", &ctx), json!(true));
        assert_eq!(ok("(a - 5) * -1 == 0", &ctx), json!(true));
    }

    #[test]
    fn comparacao_de_strings_ordena_lexicografico() {
        let ctx = json!({ "to": { "state": "heat" } });
        assert_eq!(ok("to.state >= \"cool\"", &ctx), json!(true));
        assert_eq!(ok("to.state < \"off\"", &ctx), json!(true));
    }

    #[test]
    fn sintaxe_ruim_rejeitada_no_parse() {
        for expr in [
            "",
            "to.. > 3",
            "\"sem fechar",
            "1 +",
            "a < b < c",
            "(1",
            "1 2",
            "== 3",
            "to.state =! 3",
        ] {
            assert!(Expr::parse(expr).is_err(), "'{expr}' deveria falhar");
        }
    }

    #[test]
    fn parser_aceita_a_espec_da_issue() {
        // A expressão canônica do exemplo do ventilador (docs do módulo).
        let ctx = ctx_exemplo();
        assert_eq!(ok("to.state > 32", &ctx), json!(true));
    }
}
