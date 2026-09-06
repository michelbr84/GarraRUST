//! Onde a saida completa das ferramentas do turno fica ate alguem pedir (#938).
//!
//! O #937 reduziu cada chamada de ferramenta a uma linha, que era o que a
//! conversa precisava. Mas resumo bom e resumo que esconde coisa, e esconder
//! sem dar como reaver e so perder: quando o `cargo test` diz "Failed" e o
//! resumo mostra a primeira linha, a causa costuma estar na linha 300.
//!
//! Este modulo e o "onde" dessa segunda metade. Ele **nao** decide o que e
//! seguro mostrar: a saida ja chega redigida, saneada e capada do
//! `garraia-agents` ([`garraia_agents::turn_events::capture_tool_output`]),
//! pela mesma razao de sempre — quem guarda nao pode precisar lembrar da
//! garantia.
//!
//! # Os dois limites, e por que sao dois
//!
//! Um teto de **entradas** sozinho nao segura memoria: vinte saidas de 64 KiB
//! sao 1,2 MiB grudados no processo do chat de quem so queria conversar. Um
//! teto de **bytes** sozinho tornaria o `/tool` imprevisivel — uma saida
//! gorda expulsaria dez pequenas e o indice que o usuario acabou de ver some.
//! Com os dois, o comportamento e dizivel: guarda as ultimas [`MAX_ENTRADAS`],
//! e menos se elas passarem de [`MAX_BYTES`].
//!
//! # Indice que nao se reaproveita
//!
//! Esta e a decisao que mais custa se for errada. Numerar as entradas por
//! posicao (`0..n`) faria `/tool 3` significar coisas diferentes conforme
//! outras chamadas acontecessem — o usuario le "3" na tela, roda mais um
//! comando, pede `/tool 3` e recebe outra saida, sem nada indicando a troca.
//!
//! Entao o indice e **monotonico** e nunca reusado. Pedir uma entrada ja
//! descartada devolve [`Busca::Expirada`], que o chamador transforma numa
//! mensagem dizendo isso — mostrar a entrada errada em silencio seria pior do
//! que nao mostrar nada.

/// Quantas chamadas de ferramenta ficam disponiveis para inspecao.
///
/// Vinte cobre a conversa recente com folga; a memoria de quem trabalhou o dia
/// todo na mesma sessao nao cresce por isso.
const MAX_ENTRADAS: usize = 20;

/// Teto de memoria do registro inteiro.
///
/// Meio mega e o bastante para varias saidas grandes e pequeno o bastante para
/// nao aparecer no consumo do processo.
///
/// # O acoplamento que este numero tem, e que nao aparece no tipo
///
/// Apontado pela auditoria do #938. Quem chama `registrar` em producao passa
/// sempre o resultado de `garraia_agents::turn_events::capture_tool_output`,
/// que ja capa cada saida em 64 KiB. E de la que vem a garantia de que uma
/// entrada sozinha nunca estoura este teto — **nao** deste modulo.
///
/// Se alguem subir o cap de la acima deste valor, o guard `len > 1` do
/// `registrar` passa a ser o unico impedindo que uma saida gigante entre e
/// saia na mesma chamada, deixando o `/tool` vazio logo depois do comando.
/// O guard existe justamente para esse dia; o teste
/// `entrada_unica_gigante_sobrevive` o exercita chamando `registrar` direto,
/// que e um caminho que a API normal nao produz hoje.
const MAX_BYTES: usize = 512 * 1024;

/// Uma chamada de ferramenta guardada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrada {
    /// O numero que o usuario digita no `/tool N`. Monotonico, nunca reusado.
    pub indice: usize,
    pub ferramenta: String,
    /// O que a chamada fazia — o `command` do bash, o `path` do read.
    pub detalhe: String,
    pub sucesso: bool,
    pub duracao: std::time::Duration,
    /// A saida inteira, ja segura, como veio do `garraia-agents`.
    pub saida: String,
}

impl Entrada {
    /// O que esta entrada custa de memoria. Aproximado de proposito: as
    /// `String` dominam, e contar o resto seria precisao sem uso.
    fn bytes(&self) -> usize {
        self.saida.len() + self.ferramenta.len() + self.detalhe.len()
    }
}

/// O resultado de procurar uma entrada — tres casos, nao dois.
///
/// "Nao achei" e ambiguo demais para uma coisa que envelhece: o usuario
/// precisa saber se digitou um numero que nunca existiu ou se a entrada saiu
/// do registro.
#[derive(Debug, PartialEq, Eq)]
pub enum Busca<'a> {
    Achou(&'a Entrada),
    /// O indice ja existiu e foi descartado para caber no limite.
    Expirada,
    /// O indice nunca existiu.
    Inexistente,
}

/// Registro limitado das saidas de ferramenta da sessao.
#[derive(Debug, Default)]
pub struct ToolLog {
    entradas: std::collections::VecDeque<Entrada>,
    /// Proximo indice a distribuir. So cresce.
    proximo: usize,
    bytes: usize,
}

impl ToolLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Guarda uma saida e devolve o indice que o usuario vai digitar.
    pub fn registrar(
        &mut self,
        ferramenta: &str,
        detalhe: &str,
        sucesso: bool,
        duracao: std::time::Duration,
        saida: String,
    ) -> usize {
        let indice = self.proximo;
        self.proximo += 1;

        let entrada = Entrada {
            indice,
            ferramenta: ferramenta.to_string(),
            detalhe: detalhe.to_string(),
            sucesso,
            duracao,
            saida,
        };
        self.bytes += entrada.bytes();
        self.entradas.push_back(entrada);

        // Descarta do mais velho ate caber nos dois limites. O `while` cobre o
        // caso de uma entrada sozinha estourar o teto de bytes — ela fica, por
        // ser a unica, e a proxima e que a expulsa. Guardar a mais recente
        // mesmo grande e o que o usuario espera depois de rodar um comando.
        while self.entradas.len() > MAX_ENTRADAS
            || (self.bytes > MAX_BYTES && self.entradas.len() > 1)
        {
            if let Some(velha) = self.entradas.pop_front() {
                self.bytes -= velha.bytes();
            }
        }

        indice
    }

    /// As entradas guardadas, da mais antiga para a mais nova.
    pub fn listar(&self) -> impl Iterator<Item = &Entrada> {
        self.entradas.iter()
    }

    pub fn vazio(&self) -> bool {
        self.entradas.is_empty()
    }

    /// Procura por indice, distinguindo expirada de inexistente.
    pub fn buscar(&self, indice: usize) -> Busca<'_> {
        if let Some(e) = self.entradas.iter().find(|e| e.indice == indice) {
            return Busca::Achou(e);
        }
        if indice < self.proximo {
            Busca::Expirada
        } else {
            Busca::Inexistente
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn log_com(n: usize) -> ToolLog {
        let mut log = ToolLog::new();
        for i in 0..n {
            log.registrar(
                "bash",
                &format!("cmd {i}"),
                true,
                Duration::ZERO,
                "ok".into(),
            );
        }
        log
    }

    #[test]
    fn guarda_e_devolve_pelo_indice() {
        let mut log = ToolLog::new();
        let i = log.registrar(
            "bash",
            "cargo test",
            true,
            Duration::from_millis(6300),
            "148 passed".into(),
        );
        match log.buscar(i) {
            Busca::Achou(e) => {
                assert_eq!(e.ferramenta, "bash");
                assert_eq!(e.saida, "148 passed");
            }
            outro => panic!("esperava achar: {outro:?}"),
        }
    }

    /// O indice e do usuario: ele le na tela e digita depois. Reaproveitar
    /// numero faria `/tool 3` mudar de significado sem aviso.
    #[test]
    fn indice_nunca_e_reusado() {
        let mut log = log_com(MAX_ENTRADAS + 5);
        let novo = log.registrar("bash", "mais um", true, Duration::ZERO, "ok".into());
        assert_eq!(novo, MAX_ENTRADAS + 5);
        // E os antigos nao voltaram a existir com outro dono.
        assert_eq!(log.listar().filter(|e| e.indice == 0).count(), 0);
    }

    /// Entrada descartada tem de dizer que expirou — mostrar outra em silencio
    /// seria pior do que nao mostrar nada.
    #[test]
    fn expirada_e_diferente_de_inexistente() {
        let log = log_com(MAX_ENTRADAS + 3);
        assert_eq!(log.buscar(0), Busca::Expirada, "0 saiu do registro");
        assert_eq!(log.buscar(9_999), Busca::Inexistente, "9999 nunca existiu");
    }

    #[test]
    fn respeita_o_teto_de_entradas() {
        let log = log_com(MAX_ENTRADAS * 2);
        assert_eq!(log.listar().count(), MAX_ENTRADAS);
    }

    /// O teto de bytes existe porque o de entradas nao segura memoria sozinho.
    #[test]
    fn respeita_o_teto_de_bytes() {
        let mut log = ToolLog::new();
        let gorda = "x".repeat(200 * 1024);
        for _ in 0..10 {
            log.registrar("bash", "gera", true, Duration::ZERO, gorda.clone());
        }
        assert!(
            log.bytes <= MAX_BYTES,
            "estourou o teto de bytes: {}",
            log.bytes
        );
        assert!(log.listar().count() < 10, "nada foi descartado");
    }

    /// Uma saida sozinha maior que o teto fica: e a que o usuario acabou de
    /// gerar, e descarta-la deixaria o `/tool` vazio logo apos o comando.
    #[test]
    fn entrada_unica_gigante_sobrevive() {
        let mut log = ToolLog::new();
        let i = log.registrar(
            "bash",
            "gera",
            true,
            Duration::ZERO,
            "x".repeat(MAX_BYTES * 2),
        );
        assert!(matches!(log.buscar(i), Busca::Achou(_)));
    }

    #[test]
    fn comeca_vazio() {
        let log = ToolLog::new();
        assert!(log.vazio());
        assert_eq!(log.buscar(0), Busca::Inexistente);
    }
}
