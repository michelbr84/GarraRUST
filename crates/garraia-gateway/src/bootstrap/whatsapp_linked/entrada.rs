//! #1822 (item 1, primeira fatia): a porta de entrada do canal.
//!
//! Ao reconectar, a ponte entrega de uma vez o acumulado offline (`append`
//! de ate 10 min, #1819). Ate aqui o `GatewaySink::deliver` disparava um
//! `tokio::spawn` por mensagem, sem teto global e sem olhar o `id`. Esta
//! fatia entrega as duas defesas baratas:
//!
//! - **Deduplicacao por `id`**: [`IdsVistos`] lembra os ultimos
//!   [`IDS_LEMBRADOS`] pares `(chat, id)`. A mesma mensagem reentregue (o
//!   `append` que repete um `notify` ja visto, ou a reentrega apos queda) nao
//!   vira segundo turno nem segunda resposta paga.
//! - **Teto global de turnos simultaneos**: [`MAX_TURNOS_SIMULTANEOS`] e o
//!   tamanho do semaforo do canal. A rajada da reconexao espera na fila em vez
//!   de abrir N chamadas ao provider ao mesmo tempo.
//!
//! A segunda fatia fecha a corrida de historico: [`FilasPorChat`] poe os
//! turnos de um MESMO chat em fila, na ordem de chegada. Duas mensagens do
//! mesmo chat nao rodam mais ao mesmo tempo na mesma sessao; chats diferentes
//! seguem em paralelo, ate o teto global.
//!
//! A ultima fatia junta o que se acumula: [`FilasPorChat::entrar_juntando`]
//! funde a mensagem nova ao ultimo lote da fila quando os dois se juntam, e
//! [`FilasPorChat::juntar_prefixo_em`] e a janela de juncao do trabalhador —
//! antes de comecar o turno, ele funde ao item na mao o prefixo da fila que se
//! junta a ele. O backlog da reconexao (o `append` de ate 10 min, marcado
//! `backlog` na ponte) vira 1 turno e 1 resposta por chat; mensagem ao vivo
//! nunca se junta. QUEM se junta e decisao de quem chama (aqui so ha a
//! mecanica); no canal WhatsApp e backlog do mesmo remetente + texto + sem
//! comando de barra, concatenado na ordem de chegada.
//!
//! Modulo puro: sem relogio, sem I/O, sem identidade alem do `chat_jid` e do
//! `id` opaco da mensagem. Estado em memoria; um restart zera, de proposito.

use std::collections::{HashMap, HashSet, VecDeque};

/// Quantos turnos do canal podem rodar ao mesmo tempo. Acima de uma conversa
/// humana em varios chats, abaixo de uma rajada de reconexao inteira.
pub const MAX_TURNOS_SIMULTANEOS: usize = 4;

/// Quantos pares `(chat, id)` a deduplicacao lembra. A janela do `append` e
/// de 10 min; 1024 mensagens cobrem com folga o acumulado de um canal pessoal.
pub const IDS_LEMBRADOS: usize = 1024;

/// Memoria limitada dos ids ja entregues, por chat. FIFO: passando do teto,
/// o mais antigo sai.
#[derive(Debug)]
pub struct IdsVistos {
    capacidade: usize,
    ordem: VecDeque<(String, String)>,
    vistos: HashSet<(String, String)>,
}

impl Default for IdsVistos {
    fn default() -> Self {
        Self::com_capacidade(IDS_LEMBRADOS)
    }
}

impl IdsVistos {
    /// Capacidade zero e tratada como um: a ultima mensagem sempre conta.
    pub fn com_capacidade(capacidade: usize) -> Self {
        let capacidade = capacidade.max(1);
        Self {
            capacidade,
            ordem: VecDeque::with_capacity(capacidade.min(IDS_LEMBRADOS)),
            vistos: HashSet::with_capacity(capacidade.min(IDS_LEMBRADOS)),
        }
    }

    /// `true` na primeira vez que o par `(chat, id)` chega; `false` na
    /// reentrega. `id` vazio nunca e deduplicado: sem identidade nao ha como
    /// provar que e a mesma mensagem, e descartar seria perder uma real.
    pub fn primeira_vez(&mut self, chat: &str, id: &str) -> bool {
        if id.is_empty() {
            return true;
        }
        let chave = (chat.to_string(), id.to_string());
        if self.vistos.contains(&chave) {
            return false;
        }
        if self.ordem.len() >= self.capacidade
            && let Some(velho) = self.ordem.pop_front()
        {
            self.vistos.remove(&velho);
        }
        self.vistos.insert(chave.clone());
        self.ordem.push_back(chave);
        true
    }

    /// Quantos pares estao lembrados agora.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.ordem.len()
    }

    /// `true` quando nada foi lembrado ainda.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.ordem.is_empty()
    }
}

/// Fila de espera por chat: um turno por chat de cada vez, em ordem de
/// chegada (#1822).
///
/// O contrato e de dois passos, os dois sob o mesmo lock de quem usa:
///
/// - [`entrar`](Self::entrar) devolve `Some(item)` quando o chat estava livre:
///   quem chamou vira o trabalhador do chat e roda o item. Devolve `None`
///   quando ja ha um trabalhador: o item ficou na fila dele.
/// - [`proximo`](Self::proximo), chamado pelo trabalhador ao fim de cada item,
///   devolve o seguinte ou, com a fila vazia, libera o chat e devolve `None`.
///
/// Um chat ocupa memoria so enquanto tem turno rodando ou esperando. Sem teto
/// proprio: cada item aqui ja ocupava uma task parada no semaforo antes desta
/// fila, e quem limita a rajada de um chat e a guarda de cadencia, que corta
/// sem chamar o provider.
#[derive(Debug)]
pub struct FilasPorChat<T> {
    filas: HashMap<String, VecDeque<T>>,
}

impl<T> Default for FilasPorChat<T> {
    fn default() -> Self {
        Self {
            filas: HashMap::new(),
        }
    }
}

impl<T> FilasPorChat<T> {
    /// `Some(item)`: o chat estava livre e agora e de quem chamou. `None`: o
    /// item entrou na fila do trabalhador que ja roda esse chat.
    pub fn entrar(&mut self, chat: &str, item: T) -> Option<T> {
        match self.filas.get_mut(chat) {
            Some(fila) => {
                fila.push_back(item);
                None
            }
            None => {
                self.filas.insert(chat.to_string(), VecDeque::new());
                Some(item)
            }
        }
    }

    /// O proximo item do chat, ou `None` — e nesse caso o chat fica livre e a
    /// proxima mensagem dele abre um trabalhador novo.
    pub fn proximo(&mut self, chat: &str) -> Option<T> {
        let seguinte = self.filas.get_mut(chat).and_then(VecDeque::pop_front);
        if seguinte.is_none() {
            self.filas.remove(chat);
        }
        seguinte
    }

    /// Como [`entrar`](Self::entrar), mas o que se junta vira um item so.
    ///
    /// Com o chat ocupado, a mensagem nova se junta ao ULTIMO item da fila
    /// quando `junta` aprova o par (`junta(ultimo, &nova)`); aprovado,
    /// `fundir(ultimo, nova)` absorve a mensagem e a fila nao cresce. Nao
    /// aprovado (ou fila vazia), a mensagem entra como item a mais, na ordem
    /// de chegada. Com o chat livre, devolve o item — quem chamou vira o
    /// trabalhador, e a juncao com o que chegar nas proximas fracoes de
    /// segundo e papel de [`juntar_prefixo_em`](Self::juntar_prefixo_em),
    /// chamado por ele antes de comecar.
    pub fn entrar_juntando(
        &mut self,
        chat: &str,
        item: T,
        junta: impl FnOnce(&T, &T) -> bool,
        fundir: impl FnOnce(&mut T, T),
    ) -> Option<T> {
        match self.filas.get_mut(chat) {
            Some(fila) => {
                if let Some(ultimo) = fila.back_mut()
                    && junta(ultimo, &item)
                {
                    fundir(ultimo, item);
                    return None;
                }
                fila.push_back(item);
                None
            }
            // Chat livre: o [`entrar`] de sempre — quem chamou vira trabalhador.
            None => self.entrar(chat, item),
        }
    }

    /// Junca ao `alvo` o maior prefixo da fila que `junta` aprova com ele.
    ///
    /// E a janela de juncao do trabalhador: antes de rodar o primeiro turno,
    /// o que chegou enquanto ele acordava entra no MESMO lote (via `fundir`,
    /// na ordem de chegada). A juncao PARA no primeiro item que `junta` recusa
    /// — a ordem dos turnos continua a de chegada, item a item.
    pub fn juntar_prefixo_em(
        &mut self,
        chat: &str,
        alvo: &mut T,
        junta: impl Fn(&T, &T) -> bool,
        fundir: impl Fn(&mut T, T),
    ) {
        let Some(fila) = self.filas.get_mut(chat) else {
            return;
        };
        while fila.front().is_some_and(|proximo| junta(alvo, proximo)) {
            let proximo = fila.pop_front().expect("front verificado acima");
            fundir(alvo, proximo);
        }
    }

    /// Quantos chats tem turno rodando ou esperando.
    #[cfg(test)]
    pub fn chats_ocupados(&self) -> usize {
        self.filas.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primeiro_item_do_chat_roda_e_os_seguintes_esperam_em_ordem() {
        let mut f = FilasPorChat::default();
        assert_eq!(f.entrar("chat-a", 1), Some(1), "chat livre: roda ja");
        assert_eq!(f.entrar("chat-a", 2), None, "chat ocupado: espera");
        assert_eq!(f.entrar("chat-a", 3), None);
        assert_eq!(f.proximo("chat-a"), Some(2), "ordem de chegada");
        assert_eq!(f.proximo("chat-a"), Some(3));
        assert_eq!(f.proximo("chat-a"), None);
    }

    #[test]
    fn fila_vazia_libera_o_chat_e_nao_guarda_memoria() {
        let mut f = FilasPorChat::default();
        assert_eq!(f.entrar("chat-a", 1), Some(1));
        assert_eq!(f.chats_ocupados(), 1);
        assert_eq!(f.proximo("chat-a"), None);
        assert_eq!(f.chats_ocupados(), 0, "chat ocioso nao ocupa memoria");
        assert_eq!(f.entrar("chat-a", 2), Some(2), "livre de novo");
    }

    #[test]
    fn chats_diferentes_nao_esperam_um_pelo_outro() {
        let mut f = FilasPorChat::default();
        assert_eq!(f.entrar("chat-a", 1), Some(1));
        assert_eq!(
            f.entrar("chat-b", 2),
            Some(2),
            "outro chat roda em paralelo"
        );
        assert_eq!(f.entrar("chat-a", 3), None);
        assert_eq!(f.proximo("chat-b"), None);
        assert_eq!(f.proximo("chat-a"), Some(3), "a fila de A segue intacta");
    }

    #[test]
    fn proximo_de_chat_desconhecido_e_none() {
        let mut f: FilasPorChat<u8> = FilasPorChat::default();
        assert_eq!(f.proximo("ninguem"), None);
        assert_eq!(f.chats_ocupados(), 0);
    }

    /// Juncao de teste: tudo se junta, menos o item marcado com `!`. Serve
    /// como `junta` para `FilasPorChat<String>`.
    /// `#[allow(clippy::ptr_arg)]`: o bound `Fn(&T, &T)` e higher-ranked, e so
    /// um item de funcao de assinatura concreta satisfaz ele — o `impl
    /// AsRef<str>` genérico monomorfiza numa vida unica e o rustc rejeita com
    /// "implementation of `FnOnce` is not general enough".
    #[allow(clippy::ptr_arg)]
    fn junta_menos_bang(a: &String, b: &String) -> bool {
        !a.starts_with('!') && !b.starts_with('!')
    }

    fn fundir_linha(a: &mut String, b: String) {
        a.push('\n');
        a.push_str(&b);
    }

    #[test]
    fn entrar_juntando_funde_ao_ultimo_item_do_chat_ocupado() {
        let mut f = FilasPorChat::default();
        assert_eq!(
            f.entrar_juntando("chat-a", "1".to_string(), junta_menos_bang, fundir_linha),
            Some("1".to_string()),
            "chat livre: o primeiro vira trabalhador"
        );
        assert_eq!(
            f.entrar_juntando("chat-a", "2".to_string(), junta_menos_bang, fundir_linha),
            None,
            "chat ocupado: espera"
        );
        assert_eq!(
            f.entrar_juntando("chat-a", "3".to_string(), junta_menos_bang, fundir_linha),
            None,
            "e o que ja espera e lote: a terceira funde nele"
        );
        assert_eq!(
            f.proximo("chat-a"),
            Some("2\n3".to_string()),
            "duas mensagens esperando viram um lote so"
        );
        assert_eq!(f.proximo("chat-a"), None);
    }

    #[test]
    fn entrar_juntando_empurra_separado_quando_nao_se_junta() {
        let mut f = FilasPorChat::default();
        assert_eq!(
            f.entrar_juntando("a", "!x".to_string(), junta_menos_bang, fundir_linha),
            Some("!x".to_string())
        );
        assert_eq!(
            f.entrar_juntando("a", "1".to_string(), junta_menos_bang, fundir_linha),
            None
        );
        assert_eq!(
            f.entrar_juntando("a", "2".to_string(), junta_menos_bang, fundir_linha),
            None,
            "o lote que espera junta entre si"
        );
        assert_eq!(
            f.entrar_juntando("a", "!y".to_string(), junta_menos_bang, fundir_linha),
            None,
            "o que nao se junta vira item proprio"
        );
        assert_eq!(f.proximo("a"), Some("1\n2".to_string()));
        assert_eq!(
            f.proximo("a"),
            Some("!y".to_string()),
            "na ordem de chegada, sem furar o item anterior"
        );
        assert_eq!(f.proximo("a"), None);
    }

    #[test]
    fn juntar_prefixo_em_para_no_primeiro_item_que_nao_se_junta() {
        let mut f = FilasPorChat::default();
        assert_eq!(
            f.entrar_juntando("a", "0".to_string(), junta_menos_bang, fundir_linha),
            Some("0".to_string())
        );
        for item in ["1", "!x", "2"] {
            assert_eq!(
                f.entrar_juntando("a", item.to_string(), junta_menos_bang, fundir_linha),
                None
            );
        }
        let mut alvo = "0".to_string();
        f.juntar_prefixo_em("a", &mut alvo, junta_menos_bang, fundir_linha);
        assert_eq!(alvo, "0\n1", "so o prefixo juntable entra no lote");
        assert_eq!(f.proximo("a"), Some("!x".to_string()));
        assert_eq!(
            f.proximo("a"),
            Some("2".to_string()),
            "o atrasado do item que nao junta segue separado"
        );
        assert_eq!(f.proximo("a"), None);
    }

    #[test]
    fn reentrega_do_mesmo_id_no_mesmo_chat_e_barrada() {
        let mut v = IdsVistos::default();
        assert!(v.primeira_vez("chat-a", "m1"));
        assert!(!v.primeira_vez("chat-a", "m1"), "reentrega vira 2o turno");
        assert!(v.primeira_vez("chat-a", "m2"));
    }

    #[test]
    fn mesmo_id_em_chats_diferentes_nao_colide() {
        let mut v = IdsVistos::default();
        assert!(v.primeira_vez("chat-a", "m1"));
        assert!(v.primeira_vez("chat-b", "m1"));
    }

    #[test]
    fn id_vazio_nunca_e_deduplicado() {
        let mut v = IdsVistos::default();
        assert!(v.primeira_vez("chat-a", ""));
        assert!(v.primeira_vez("chat-a", ""));
        assert!(v.is_empty(), "id vazio nao ocupa a memoria");
    }

    #[test]
    fn memoria_e_limitada_e_esquece_o_mais_antigo() {
        let mut v = IdsVistos::com_capacidade(2);
        assert!(v.primeira_vez("c", "1"));
        assert!(v.primeira_vez("c", "2"));
        assert!(v.primeira_vez("c", "3"));
        assert_eq!(v.len(), 2);
        assert!(v.primeira_vez("c", "1"), "o mais antigo saiu da memoria");
        assert!(!v.primeira_vez("c", "3"), "o recente continua lembrado");
    }

    #[test]
    fn capacidade_zero_vira_um() {
        let mut v = IdsVistos::com_capacidade(0);
        assert!(v.primeira_vez("c", "1"));
        assert!(!v.primeira_vez("c", "1"));
        assert_eq!(v.len(), 1);
    }
}
