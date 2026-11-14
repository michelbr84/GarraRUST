//! Aprovacoes que valem para o resto da sessao (#1858 fatia 4).
//!
//! # O que e
//!
//! Com `agent.approval: session`, o primeiro pedido arriscado de cada
//! `(ferramenta, assunto)` continua pedindo o "sim" do humano, pelo fluxo de
//! sempre (GAR-187, registro do servidor do #1343). Quando o humano aprova, a
//! impressao digital daquele pedido fica guardada aqui, e o MESMO pedido nao
//! pergunta de novo no resto da sessao. E o meio-termo da #1858 entre o
//! `always` (todo pedido pergunta, sempre) e o `never-confirm` (nada
//! pergunta): tira a fadiga do "sim" repetido para o mesmo comando sem
//! aprovar de antemao o que o humano nunca viu.
//!
//! # O que NAO e
//!
//! - **Nao e padrao.** Aprovar `cargo test -p a` nao aprova `cargo test -p b`,
//!   nem `cargo test -p a && curl ...`: a regra e a igualdade da impressao
//!   digital, a mesma do [`ToolApproval::Granted`].
//! - **Nao atravessa sessao, canal nem remetente.** A chave e o trio do
//!   [`ApprovalScope`] — `(canal, sessao, remetente)`, derivado pelo
//!   servidor. O "sim" do dono num grupo nao aprova o pedido de outra pessoa
//!   do mesmo grupo, e uma sessao nova comeca sem nada lembrado.
//! - **Nao passa por cima de nada que decide antes do tier arriscado.** O
//!   hard-deny (`safety_gate`), o modo e o teto da superficie rodam antes de
//!   a ferramenta olhar a aprovacao; uma impressao digital lembrada nunca
//!   chega perto deles.
//! - **Nao cria canal.** Sem [`ApprovalScope`] (caminho sem identidade
//!   derivada pelo servidor) nada e lembrado, e a ferramenta sem canal de
//!   confirmacao continua fail-closed.
//! - **Nao persiste.** A impressao digital e um HMAC com chave aleatoria por
//!   processo ([`super::approval`]): reiniciar faz o humano confirmar de novo,
//!   a mesma regra das aprovacoes pendentes.
//!
//! # Limites
//!
//! - [`SESSION_APPROVAL_IDLE_TTL`]: sessao sem turno por esse tempo esquece o
//!   que aprovou. Conversa de canal (WhatsApp, Telegram) nunca "fecha", e a
//!   sessao de trabalho que o humano tinha em mente ao dizer "sim" acabou.
//! - [`SESSION_APPROVAL_MAX_POR_SESSAO`] pedidos por sessao; passando disso o
//!   mais antigo sai — o humano e perguntado de novo, que e o lado certo para
//!   errar.
//! - [`SESSION_APPROVAL_CAPACITY`] sessoes no processo; passando disso sai a
//!   que esta ha mais tempo sem turno.
//! - O assunto cru do pedido (o comando, que pode ter segredo) nunca entra
//!   aqui: so o HMAC. Log so leva o canal.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::approval::ToolApproval;
use super::pending_approval::ApprovalScope;

/// Por quanto tempo uma sessao sem turno guarda o que o humano aprovou.
pub const SESSION_APPROVAL_IDLE_TTL: Duration = Duration::from_secs(60 * 60);

/// Quantos pedidos aprovados uma sessao guarda.
pub const SESSION_APPROVAL_MAX_POR_SESSAO: usize = 64;

/// Quantas sessoes o processo acompanha ao mesmo tempo.
pub const SESSION_APPROVAL_CAPACITY: usize = 1024;

/// `(canal, sessao, remetente)` — o trio do [`ApprovalScope`].
type Chave = (String, String, String);

fn chave(scope: &ApprovalScope) -> Chave {
    (
        scope.channel().to_string(),
        scope.session_id().to_string(),
        scope.sender().to_string(),
    )
}

/// O que uma sessao ja aprovou, na ordem em que aprovou.
struct Lembradas {
    impressoes: Vec<String>,
    ultimo_turno: Instant,
}

/// As aprovacoes lembradas do processo, por `(canal, sessao, remetente)`.
///
/// `Debug` manual: a chave carrega o remetente (telefone no WhatsApp, id da
/// plataforma nos outros canais — PII). O dump mostra so quantas sessoes ha.
pub struct SessionApprovals {
    inner: Mutex<HashMap<Chave, Lembradas>>,
    idle_ttl: Duration,
    max_por_sessao: usize,
    capacity: usize,
}

impl std::fmt::Debug for SessionApprovals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sessoes = self
            .inner
            .lock()
            .map(|m| m.len())
            .unwrap_or_else(|e| e.into_inner().len());
        f.debug_struct("SessionApprovals")
            .field("sessoes", &sessoes)
            .field("idle_ttl", &self.idle_ttl)
            .field("max_por_sessao", &self.max_por_sessao)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl Default for SessionApprovals {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionApprovals {
    pub fn new() -> Self {
        Self::with_limits(
            SESSION_APPROVAL_IDLE_TTL,
            SESSION_APPROVAL_MAX_POR_SESSAO,
            SESSION_APPROVAL_CAPACITY,
        )
    }

    /// Para os testes: limites menores. Zero em qualquer limite de contagem
    /// vira um.
    pub fn with_limits(idle_ttl: Duration, max_por_sessao: usize, capacity: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            idle_ttl,
            max_por_sessao: max_por_sessao.max(1),
            capacity: capacity.max(1),
        }
    }

    /// O lock sem panico. Se uma thread morreu segurando o lock, o mapa pode
    /// estar pela metade: esquece tudo (cada humano e perguntado de novo) em
    /// vez de confiar no que sobrou — a mesma regra do
    /// [`super::pending_approval::PendingApprovals`].
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Chave, Lembradas>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                self.inner.clear_poison();
                guard
            }
        }
    }

    fn vencida(&self, l: &Lembradas, now: Instant) -> bool {
        now.saturating_duration_since(l.ultimo_turno) >= self.idle_ttl
    }

    /// A aprovacao que vale neste turno, para um runtime sob
    /// `agent.approval: session`.
    ///
    /// `deste_turno` e o que o registro de pendentes devolveu agora (o "sim"
    /// a um pedido pausado, ou nada). Um `Granted` passa a ser lembrado; o
    /// resultado cobre o que esta sessao ja aprovou mais o de agora. Sem nada
    /// lembrado, devolve `deste_turno` intacto — inclusive o `None`.
    pub fn compor(
        &self,
        scope: &ApprovalScope,
        deste_turno: ToolApproval,
        now: Instant,
    ) -> ToolApproval {
        let mut map = self.lock();
        let idle_ttl = self.idle_ttl;
        map.retain(|_, l| now.saturating_duration_since(l.ultimo_turno) < idle_ttl);
        let chave = chave(scope);

        if let ToolApproval::Granted(impressao) = &deste_turno {
            if !map.contains_key(&chave) && map.len() >= self.capacity {
                let mais_antiga = map
                    .iter()
                    .min_by_key(|(_, l)| l.ultimo_turno)
                    .map(|(k, _)| k.clone());
                if let Some(k) = mais_antiga {
                    map.remove(&k);
                }
            }
            let lembradas = map.entry(chave.clone()).or_insert_with(|| Lembradas {
                impressoes: Vec::new(),
                ultimo_turno: now,
            });
            if !lembradas.impressoes.iter().any(|i| i == impressao) {
                lembradas.impressoes.push(impressao.clone());
                if lembradas.impressoes.len() > self.max_por_sessao {
                    lembradas.impressoes.remove(0);
                }
                tracing::info!(
                    channel = %scope.channel(),
                    lembradas = lembradas.impressoes.len(),
                    "aprovacao lembrada para o resto da sessao (agent.approval: session)"
                );
            }
        }

        match map.get_mut(&chave) {
            Some(lembradas) if !lembradas.impressoes.is_empty() => {
                lembradas.ultimo_turno = now;
                ToolApproval::GrantedMany(lembradas.impressoes.clone())
            }
            _ => deste_turno,
        }
    }

    /// Quantos pedidos esta sessao tem lembrados agora (vencida = zero).
    pub fn lembradas(&self, scope: &ApprovalScope, now: Instant) -> usize {
        let map = self.lock();
        match map.get(&chave(scope)) {
            Some(l) if !self.vencida(l, now) => l.impressoes.len(),
            _ => 0,
        }
    }

    /// Quantas sessoes estao guardadas (vencidas inclusive, ate a proxima
    /// escrita varre-las).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::approval::ApprovalFingerprint;

    fn escopo(canal: &str, sessao: &str, remetente: &str) -> ApprovalScope {
        ApprovalScope::new(canal, sessao, remetente).expect("escopo valido")
    }

    fn sim(tool: &str, assunto: &str) -> ToolApproval {
        ToolApproval::granted(tool, assunto)
    }

    /// O "sim" de agora fica lembrado: o turno seguinte da MESMA sessao
    /// (sem novo "sim") ainda cobre aquele pedido — e so ele.
    #[test]
    fn sim_fica_lembrado_para_o_mesmo_pedido() {
        let s = SessionApprovals::new();
        let e = escopo("cli", "s1", "dono");
        let t0 = Instant::now();

        let agora = s.compor(&e, sim("bash", "cargo test"), t0);
        assert!(agora.covers("bash", "cargo test"));

        let depois = s.compor(&e, ToolApproval::None, t0 + Duration::from_secs(5));
        assert!(depois.covers("bash", "cargo test"));
        assert!(!depois.covers("bash", "cargo test -- --ignored"));
        assert!(!depois.covers("run_tests", "cargo test"));
        assert_eq!(s.lembradas(&e, t0 + Duration::from_secs(5)), 1);
    }

    /// Sem nada lembrado, `compor` nao inventa aprovacao: devolve o que veio.
    #[test]
    fn sem_nada_lembrado_devolve_o_turno_intacto() {
        let s = SessionApprovals::new();
        let e = escopo("cli", "s1", "dono");
        assert_eq!(
            s.compor(&e, ToolApproval::None, Instant::now()),
            ToolApproval::None
        );
        assert!(s.is_empty(), "turno sem sim nao cria entrada");
    }

    /// Dois pedidos aprovados na sessao: os dois ficam cobertos, e o mesmo
    /// "sim" repetido nao duplica a lembranca.
    #[test]
    fn varios_pedidos_somam_sem_duplicar() {
        let s = SessionApprovals::new();
        let e = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        s.compor(&e, sim("bash", "ls -la"), t0);
        s.compor(&e, sim("bash", "ls -la"), t0);
        let ap = s.compor(&e, sim("bash", "pwd"), t0);
        assert!(ap.covers("bash", "ls -la"));
        assert!(ap.covers("bash", "pwd"));
        assert_eq!(s.lembradas(&e, t0), 2);
    }

    /// A chave e o trio inteiro: outra sessao, outro canal e outro remetente
    /// (o grupo) nao herdam o que o dono aprovou.
    #[test]
    fn nao_atravessa_sessao_canal_nem_remetente() {
        let s = SessionApprovals::new();
        let t0 = Instant::now();
        s.compor(&escopo("telegram", "g1", "dono"), sim("bash", "x"), t0);

        for outro in [
            escopo("telegram", "g2", "dono"),
            escopo("web", "g1", "dono"),
            escopo("telegram", "g1", "outra-pessoa"),
        ] {
            let ap = s.compor(&outro, ToolApproval::None, t0);
            assert!(!ap.covers("bash", "x"), "{outro:?} herdou a aprovacao");
            assert_eq!(s.lembradas(&outro, t0), 0);
        }
        // E o dono continua com a dele.
        assert_eq!(s.lembradas(&escopo("telegram", "g1", "dono"), t0), 1);
    }

    /// Sessao sem turno pelo TTL esquece tudo; com turno dentro do TTL, o
    /// relogio recomeca.
    #[test]
    fn sessao_ociosa_esquece_e_turno_renova() {
        let ttl = Duration::from_secs(60);
        let s = SessionApprovals::with_limits(ttl, 8, 8);
        let e = escopo("cli", "s1", "dono");
        let t0 = Instant::now();
        s.compor(&e, sim("bash", "x"), t0);

        // Turno aos 50 s: dentro do TTL, renova.
        assert!(
            s.compor(&e, ToolApproval::None, t0 + Duration::from_secs(50))
                .covers("bash", "x")
        );
        // Aos 100 s: so 50 s desde o ultimo turno — ainda vale.
        assert!(
            s.compor(&e, ToolApproval::None, t0 + Duration::from_secs(100))
                .covers("bash", "x")
        );
        // Aos 161 s: 61 s sem turno — esqueceu.
        let t_ocioso = t0 + Duration::from_secs(161);
        assert_eq!(s.lembradas(&e, t_ocioso), 0);
        assert_eq!(
            s.compor(&e, ToolApproval::None, t_ocioso),
            ToolApproval::None
        );
        assert!(s.is_empty(), "a varredura removeu a sessao vencida");
    }

    /// Teto por sessao: o pedido mais antigo sai e volta a perguntar.
    #[test]
    fn teto_por_sessao_tira_o_mais_antigo() {
        let s = SessionApprovals::with_limits(Duration::from_secs(600), 2, 8);
        let e = escopo("cli", "s1", "dono");
        let t0 = Instant::now();
        s.compor(&e, sim("bash", "a"), t0);
        s.compor(&e, sim("bash", "b"), t0);
        let ap = s.compor(&e, sim("bash", "c"), t0);
        assert!(!ap.covers("bash", "a"), "o mais antigo saiu");
        assert!(ap.covers("bash", "b"));
        assert!(ap.covers("bash", "c"));
        assert_eq!(s.lembradas(&e, t0), 2);
    }

    /// Capacidade de sessoes: entra a nova, sai a que esta ha mais tempo sem
    /// turno.
    #[test]
    fn capacidade_tira_a_sessao_mais_ociosa() {
        let s = SessionApprovals::with_limits(Duration::from_secs(600), 8, 2);
        let t0 = Instant::now();
        let a = escopo("cli", "a", "dono");
        let b = escopo("cli", "b", "dono");
        let c = escopo("cli", "c", "dono");
        s.compor(&a, sim("bash", "x"), t0);
        s.compor(&b, sim("bash", "x"), t0 + Duration::from_secs(1));
        // `a` teve turno depois de `b`: a mais ociosa passa a ser `b`.
        s.compor(&a, ToolApproval::None, t0 + Duration::from_secs(2));
        s.compor(&c, sim("bash", "x"), t0 + Duration::from_secs(3));
        assert_eq!(s.len(), 2);
        let t = t0 + Duration::from_secs(3);
        assert_eq!(s.lembradas(&a, t), 1);
        assert_eq!(s.lembradas(&b, t), 0, "a mais ociosa saiu");
        assert_eq!(s.lembradas(&c, t), 1);
    }

    /// O `Debug` nao mostra remetente nem sessao (PII) — so contagens.
    #[test]
    fn debug_nao_vaza_remetente() {
        let s = SessionApprovals::new();
        s.compor(
            &escopo("whatsapp", "whatsapp-5511999990000", "5511999990000"),
            sim("bash", "x"),
            Instant::now(),
        );
        let dump = format!("{s:?}");
        assert!(!dump.contains("5511999990000"), "{dump}");
        assert!(dump.contains("sessoes: 1"), "{dump}");
    }

    /// So o HMAC fica guardado: o assunto cru (o comando) nunca.
    #[test]
    fn guarda_so_a_impressao_digital() {
        let s = SessionApprovals::new();
        let e = escopo("cli", "s1", "dono");
        let comando = "echo conteudo-privado > /tmp/nota";
        let ap = s.compor(&e, sim("bash", comando), Instant::now());
        let ToolApproval::GrantedMany(impressoes) = ap else {
            panic!("esperava a aprovacao da sessao");
        };
        assert_eq!(
            impressoes,
            vec![
                ApprovalFingerprint::of("bash", comando)
                    .as_str()
                    .to_string()
            ]
        );
        assert!(impressoes.iter().all(|i| !i.contains("conteudo-privado")));
    }
}
