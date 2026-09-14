//! Codigos de pareamento dos canais (`/pair` -> "manda o codigo pro bot").
//!
//! O codigo tem 6 digitos (~20 bits) e vale 5 minutos; quem acerta entra na
//! allowlist **em disco** (`Allowlist::add`), entao um chute certo e acesso
//! persistente. Ate o #1190 o `claim()` nunca era exercido em producao (cada
//! canal tinha um `PairingManager` proprio, e o codigo do `/pair` morava em
//! outro) — quando a fiacao foi consertada, tres fragilidades pre-existentes
//! passaram a ser alcancaveis (#1191):
//!
//! 1. **Sem limite de tentativas.** Cada palpite e uma mensagem comum de um
//!    usuario nao autorizado; a unica barreira era o rate limit do transporte
//!    (Telegram, Discord...), que nao e controle nosso. Agora ha dois freios:
//!    por usuario (`max_failures_per_user` erros -> `lockout`, sem nem
//!    comparar) e global (`max_failures_total` erros desde o ultimo `/pair`
//!    **queimam** todo codigo pendente — o dono gera outro). Com 20 palpites
//!    comparados por ciclo num espaco de 10^6, a chance de acerto por `/pair`
//!    e 2e-5. O global fecha o palpite distribuido por muitas contas; o preco
//!    e um DoS do pareamento: num canal onde identidade custa (Telegram,
//!    WhatsApp, Signal) um `/pair` novo resolve enquanto os ofensores estao
//!    em `lockout`, mas num canal de identidade gratis (IRC sem NickServ,
//!    alts de Discord) um atacante persistente queima cada codigo novo — e o
//!    dono precisa **saber** disso, por isso a queima e reportada no proximo
//!    `/pair` ([`GenerateStatus::previous_burned`]).
//! 2. **Comparacao nao constant-time.** `==` em `String` sai no primeiro byte
//!    diferente. O canal atravessa rede e transporte de chat, entao o sinal
//!    de tempo e fraco — mas o resto do projeto compara credencial em tempo
//!    constante (`garraia-auth`), e a consistencia custa uma linha:
//!    `subtle::ConstantTimeEq`, sem sair cedo no primeiro codigo que casa.
//! 3. **`generate()` descartava o codigo pendente anterior em silencio.**
//!    Um segundo `/pair` invalidava o codigo que o dono acabou de mandar para
//!    outra pessoa. [`PairingManager::generate_with_status`] devolve se
//!    substituiu um codigo ainda nao resgatado, e o `/pair` avisa.
//!
//! O que **nao** muda aqui, de proposito: `claim()` continua ignorando o
//! `channel_id` de origem (a allowlist e global, uma por instalacao, e o
//! `/pair` gera com a chave literal `"telegram"` em qualquer canal). Decidido
//! pelo dono em 2026-09-14: pareamento **global por instalacao** — um codigo
//! vale em qualquer canal habilitado e o `channel_id` e informativo, nao um
//! escopo; 6 digitos e os limites fixos tambem ficam (threat model 5.11).

use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tracing::warn;

/// Freios do `claim()`. Os defaults sao os de producao (`AppState`).
#[derive(Clone, Copy, Debug)]
pub struct ClaimLimits {
    /// Erros seguidos de um mesmo `user_id` antes de ele entrar em `lockout`.
    pub max_failures_per_user: u32,
    /// Quanto tempo o `user_id` fica sem poder tentar depois de estourar.
    pub lockout: Duration,
    /// Erros (de qualquer usuario) desde o ultimo `generate()` que queimam
    /// todo codigo pendente.
    pub max_failures_total: u32,
}

impl Default for ClaimLimits {
    fn default() -> Self {
        Self {
            max_failures_per_user: 5,
            lockout: Duration::from_secs(15 * 60),
            max_failures_total: 20,
        }
    }
}

/// O que aconteceu com uma tentativa de resgate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// Codigo valido e ainda nao usado: devolve o `channel_id` que o gerou.
    Paired(String),
    /// Codigo errado, ja usado ou expirado.
    Invalid,
    /// O usuario estourou `max_failures_per_user`; nada foi comparado.
    LockedOut,
    /// Esta tentativa errada estourou `max_failures_total`: todo codigo
    /// pendente foi invalidado. O dono precisa gerar outro.
    Burned,
}

/// O que [`PairingManager::generate_with_status`] tem a dizer ao dono alem
/// do codigo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerateStatus {
    /// O codigo de 6 digitos.
    pub code: String,
    /// Havia um codigo do mesmo canal ainda valido e nao resgatado — ele
    /// acabou de deixar de valer.
    pub replaced_pending: bool,
    /// Desde o ultimo `/pair`, os codigos pendentes foram queimados por
    /// excesso de tentativas erradas: quantas foram. `Some` quer dizer que
    /// alguem esta chutando codigos contra o bot — e que o convidado que nao
    /// conseguiu entrar nao errou nada.
    pub previous_burned: Option<u32>,
}

/// Manages pairing codes for device and channel authentication.
pub struct PairingManager {
    codes: HashMap<String, PairingCode>,
    code_ttl: Duration,
    limits: ClaimLimits,
    /// Erros por `user_id`, com o instante do ultimo.
    failures: HashMap<String, Failures>,
    /// Erros de qualquer usuario desde o ultimo `generate()`.
    failed_since_generate: u32,
    /// Contagem que provocou a ultima queima, ate o proximo `generate()`
    /// reporta-la ao dono.
    burned_since_generate: Option<u32>,
}

struct PairingCode {
    code: String,
    created_at: Instant,
    claimed_by: Option<String>,
}

struct Failures {
    count: u32,
    last_at: Instant,
}

impl PairingManager {
    pub fn new(code_ttl: Duration) -> Self {
        Self::with_limits(code_ttl, ClaimLimits::default())
    }

    pub fn with_limits(code_ttl: Duration, limits: ClaimLimits) -> Self {
        Self {
            codes: HashMap::new(),
            code_ttl,
            limits,
            failures: HashMap::new(),
            failed_since_generate: 0,
            burned_since_generate: None,
        }
    }

    /// Generate a new 6-digit pairing code for a channel.
    ///
    /// Substitui em silencio um codigo pendente do mesmo canal — use
    /// [`Self::generate_with_status`] quando o chamador quiser avisar.
    pub fn generate(&mut self, channel_id: &str) -> String {
        self.generate_with_status(channel_id).code
    }

    /// [`Self::generate`] dizendo tambem se **substituiu um codigo ainda
    /// valido e nao resgatado** do mesmo canal, e se os codigos pendentes
    /// foram **queimados** desde o ultimo `/pair` — as duas coisas que o dono
    /// precisa saber e que a resposta silenciosa do canal nao conta.
    ///
    /// O contador global de erros so zera quando **nenhum outro** codigo
    /// pendente sobrevive: se um codigo de outro canal continua vivo, o
    /// atacante nao ganha um orcamento novo de palpites contra ele.
    pub fn generate_with_status(&mut self, channel_id: &str) -> GenerateStatus {
        self.cleanup_expired();
        let code: String = {
            let mut rng = rand::rng();
            (0..6)
                .map(|_| rng.random_range(0..=9).to_string())
                .collect()
        };

        let outros_pendentes = self
            .codes
            .iter()
            .any(|(id, pc)| id != channel_id && pc.claimed_by.is_none());
        let anterior = self.codes.insert(
            channel_id.to_string(),
            PairingCode {
                code: code.clone(),
                created_at: Instant::now(),
                claimed_by: None,
            },
        );
        if !outros_pendentes {
            self.failed_since_generate = 0;
        }
        GenerateStatus {
            code,
            replaced_pending: anterior.is_some_and(|pc| pc.claimed_by.is_none()),
            previous_burned: self.burned_since_generate.take(),
        }
    }

    /// Attempt to claim a pairing code. Returns the channel ID if valid.
    ///
    /// E [`Self::try_claim`] achatado em `Option`: os 11 bootstraps de canal
    /// so precisam saber se entrou. `LockedOut`/`Burned` viram `None` — o
    /// usuario ve o mesmo "unauthorized" de sempre, de proposito: a resposta
    /// nao diz se o codigo existia.
    pub fn claim(&mut self, code: &str, user_id: &str) -> Option<String> {
        match self.try_claim(code, user_id) {
            ClaimOutcome::Paired(channel) => Some(channel),
            ClaimOutcome::Invalid | ClaimOutcome::LockedOut | ClaimOutcome::Burned => None,
        }
    }

    /// Tenta resgatar `code` para `user_id`, com os freios de [`ClaimLimits`].
    pub fn try_claim(&mut self, code: &str, user_id: &str) -> ClaimOutcome {
        self.cleanup_expired();

        if self.esta_bloqueado(user_id) {
            return ClaimOutcome::LockedOut;
        }

        // Comparacao em tempo constante e sem sair cedo: percorre todos os
        // codigos pendentes acumulando o match, em vez de parar no primeiro.
        let mut alvo: Option<String> = None;
        for (channel_id, pc) in &self.codes {
            let casa: bool = pc.claimed_by.is_none()
                && pc.code.len() == code.len()
                && pc.code.as_bytes().ct_eq(code.as_bytes()).into();
            if casa && alvo.is_none() {
                alvo = Some(channel_id.clone());
            }
        }

        match alvo {
            Some(channel_id) => {
                if let Some(pc) = self.codes.get_mut(&channel_id) {
                    pc.claimed_by = Some(user_id.to_string());
                }
                self.failures.remove(user_id);
                ClaimOutcome::Paired(channel_id)
            }
            None => self.registra_falha(user_id),
        }
    }

    /// `true` enquanto o usuario estiver dentro do `lockout` depois de
    /// estourar `max_failures_per_user`. Nao renova o prazo a cada tentativa
    /// bloqueada — senao um atacante persistente nunca sairia e um usuario
    /// legitimo que errou tambem nao.
    fn esta_bloqueado(&self, user_id: &str) -> bool {
        self.failures.get(user_id).is_some_and(|f| {
            f.count >= self.limits.max_failures_per_user
                && f.last_at.elapsed() < self.limits.lockout
        })
    }

    fn registra_falha(&mut self, user_id: &str) -> ClaimOutcome {
        let agora = Instant::now();
        let f = self
            .failures
            .entry(user_id.to_string())
            .or_insert(Failures {
                count: 0,
                last_at: agora,
            });
        // Cinto e suspensorio: `cleanup_expired` ja removeu bloqueios
        // vencidos, mas se o relogio cruzou o limite entre o `retain` e este
        // ponto, a contagem recomeca em vez de estender o bloqueio antigo.
        if f.count >= self.limits.max_failures_per_user
            && f.last_at.elapsed() >= self.limits.lockout
        {
            f.count = 0;
        }
        f.count += 1;
        f.last_at = agora;

        self.failed_since_generate = self.failed_since_generate.saturating_add(1);
        if self.failed_since_generate >= self.limits.max_failures_total
            && self.codes.values().any(|pc| pc.claimed_by.is_none())
        {
            let pendentes = self
                .codes
                .values()
                .filter(|pc| pc.claimed_by.is_none())
                .count();
            self.codes.retain(|_, pc| pc.claimed_by.is_some());
            self.burned_since_generate = Some(self.failed_since_generate);
            // Sem user_id nem codigo no log: so o fato e a contagem.
            warn!(
                tentativas = self.failed_since_generate,
                pendentes,
                "pairing: codigos pendentes queimados por excesso de tentativas erradas; gere outro com /pair"
            );
            return ClaimOutcome::Burned;
        }
        ClaimOutcome::Invalid
    }

    fn cleanup_expired(&mut self) {
        self.codes
            .retain(|_, pc| pc.created_at.elapsed() < self.code_ttl);
        // Contadores velhos nao interessam: um erro de ontem nao pode somar
        // com um de hoje para bloquear alguem.
        let lockout = self.limits.lockout;
        self.failures.retain(|_, f| f.last_at.elapsed() < lockout);
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimLimits, ClaimOutcome, PairingManager};
    use std::thread::sleep;
    use std::time::Duration;

    fn limites(por_usuario: u32, lockout: Duration, total: u32) -> ClaimLimits {
        ClaimLimits {
            max_failures_per_user: por_usuario,
            lockout,
            max_failures_total: total,
        }
    }

    /// Um codigo errado com 6 digitos, garantidamente diferente de `code`.
    fn errado(code: &str) -> String {
        let primeiro = code.as_bytes()[0];
        let trocado = if primeiro == b'0' { b'1' } else { b'0' };
        let mut s = String::from(trocado as char);
        s.push_str(&code[1..]);
        s
    }

    #[test]
    fn generate_returns_six_digit_code() {
        let mut manager = PairingManager::new(Duration::from_secs(60));
        let code = manager.generate("channel-1");
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn claim_succeeds_once_for_valid_code() {
        let mut manager = PairingManager::new(Duration::from_secs(60));
        let code = manager.generate("channel-abc");

        let first = manager.claim(&code, "user-1");
        let second = manager.claim(&code, "user-2");

        assert_eq!(first.as_deref(), Some("channel-abc"));
        assert!(second.is_none());
    }

    #[test]
    fn expired_codes_cannot_be_claimed() {
        let mut manager = PairingManager::new(Duration::from_millis(5));
        let code = manager.generate("channel-expire");

        sleep(Duration::from_millis(15));
        let claim = manager.claim(&code, "user-1");
        assert!(claim.is_none());
    }

    // ── #1191 ─────────────────────────────────────────────────────────────

    #[test]
    fn usuario_e_bloqueado_depois_de_errar_demais_mesmo_com_o_codigo_certo() {
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(3, Duration::from_secs(60), 100),
        );
        let code = m.generate("telegram");
        let ruim = errado(&code);
        for _ in 0..3 {
            assert_eq!(m.try_claim(&ruim, "atacante"), ClaimOutcome::Invalid);
        }
        // A quarta tentativa nem compara: bloqueado, ainda que acerte.
        assert_eq!(m.try_claim(&code, "atacante"), ClaimOutcome::LockedOut);
        assert!(
            m.claim(&code, "atacante").is_none(),
            "claim() achata em None"
        );
        // O bloqueio e por usuario: outra pessoa com o codigo certo entra.
        assert_eq!(
            m.try_claim(&code, "convidado"),
            ClaimOutcome::Paired("telegram".into())
        );
    }

    #[test]
    fn bloqueio_expira_e_a_contagem_recomeca() {
        // Lockout de 300 ms e espera de 600 ms: folga para um runner de CI
        // carregado, mantendo o teste abaixo de um segundo.
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(2, Duration::from_millis(300), 100),
        );
        let code = m.generate("telegram");
        let ruim = errado(&code);
        assert_eq!(m.try_claim(&ruim, "u"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&ruim, "u"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&code, "u"), ClaimOutcome::LockedOut);
        sleep(Duration::from_millis(600));
        // Prazo vencido: volta a comparar. Um erro agora e Invalid (a
        // contagem recomecou, nao somou com as anteriores)...
        assert_eq!(m.try_claim(&ruim, "u"), ClaimOutcome::Invalid);
        // ...e o acerto entra.
        assert_eq!(
            m.try_claim(&code, "u"),
            ClaimOutcome::Paired("telegram".into())
        );
    }

    #[test]
    fn acerto_zera_as_falhas_do_usuario() {
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(3, Duration::from_secs(60), 100),
        );
        let code = m.generate("a");
        let ruim = errado(&code);
        m.try_claim(&ruim, "u");
        m.try_claim(&ruim, "u");
        assert_eq!(m.try_claim(&code, "u"), ClaimOutcome::Paired("a".into()));
        // Novo ciclo: dois erros nao bloqueiam, porque os anteriores zeraram.
        let code2 = m.generate("b");
        let ruim2 = errado(&code2);
        m.try_claim(&ruim2, "u");
        m.try_claim(&ruim2, "u");
        assert_eq!(m.try_claim(&code2, "u"), ClaimOutcome::Paired("b".into()));
    }

    #[test]
    fn excesso_de_erros_de_qualquer_usuario_queima_os_codigos_pendentes() {
        // O palpite distribuido: cada conta erra pouco, mas somam.
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(100, Duration::from_secs(60), 4),
        );
        let code = m.generate("telegram");
        let ruim = errado(&code);
        assert_eq!(m.try_claim(&ruim, "u1"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&ruim, "u2"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&ruim, "u3"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&ruim, "u4"), ClaimOutcome::Burned);
        // O codigo certo ja nao vale para ninguem.
        assert_eq!(m.try_claim(&code, "convidado"), ClaimOutcome::Invalid);
        // Um /pair novo abre um ciclo novo — e conta ao dono o que houve.
        let status = m.generate_with_status("telegram");
        assert!(
            !status.replaced_pending,
            "o pendente anterior ja tinha sido queimado"
        );
        assert_eq!(
            status.previous_burned,
            Some(4),
            "o dono precisa saber que houve queima, e com quantos erros"
        );
        assert_eq!(
            m.try_claim(&status.code, "convidado"),
            ClaimOutcome::Paired("telegram".into())
        );
        // O aviso e de uma vez so.
        assert_eq!(m.generate_with_status("telegram").previous_burned, None);
    }

    #[test]
    fn codigos_ja_resgatados_sobrevivem_a_queima() {
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(100, Duration::from_secs(60), 1),
        );
        let code = m.generate("telegram");
        assert_eq!(
            m.try_claim(&code, "u"),
            ClaimOutcome::Paired("telegram".into())
        );
        assert_eq!(
            m.try_claim(&errado(&code), "x"),
            ClaimOutcome::Invalid,
            "nada pendente para queimar"
        );
    }

    #[test]
    fn pair_em_outro_canal_nao_da_orcamento_novo_contra_o_codigo_vivo() {
        // Dois canais com codigo pendente; 3 erros contra o do Telegram.
        // Um /pair no Discord NAO zera o contador enquanto o do Telegram
        // segue vivo — senao o atacante ganharia 20 palpites novos a cada
        // /pair alheio.
        let mut m = PairingManager::with_limits(
            Duration::from_secs(60),
            limites(100, Duration::from_secs(60), 4),
        );
        let code = m.generate("telegram");
        let ruim = errado(&code);
        for u in ["u1", "u2", "u3"] {
            assert_eq!(m.try_claim(&ruim, u), ClaimOutcome::Invalid);
        }
        let _ = m.generate("discord");
        assert_eq!(
            m.try_claim(&ruim, "u4"),
            ClaimOutcome::Burned,
            "o contador tem de sobreviver ao /pair de outro canal"
        );
        // Quando nao sobra pendente nenhum, o /pair seguinte zera de verdade.
        let status = m.generate_with_status("telegram");
        assert_eq!(status.previous_burned, Some(4));
        for u in ["v1", "v2", "v3"] {
            assert_eq!(m.try_claim(&errado(&status.code), u), ClaimOutcome::Invalid);
        }
        assert_eq!(
            m.try_claim(&status.code, "convidado"),
            ClaimOutcome::Paired("telegram".into())
        );
    }

    #[test]
    fn generate_avisa_quando_substitui_um_codigo_pendente() {
        let mut m = PairingManager::new(Duration::from_secs(60));
        let primeiro = m.generate_with_status("telegram");
        assert!(!primeiro.replaced_pending, "primeiro codigo do canal");
        assert_eq!(primeiro.previous_burned, None);
        let segundo = m.generate_with_status("telegram");
        assert!(segundo.replaced_pending, "o anterior ainda estava pendente");
        m.claim(&segundo.code, "u");
        let terceiro = m.generate_with_status("telegram");
        assert!(
            !terceiro.replaced_pending,
            "o anterior ja tinha sido resgatado"
        );
        // Outro canal nao conta.
        assert!(!m.generate_with_status("discord").replaced_pending);
    }

    #[test]
    fn comprimento_diferente_nunca_casa() {
        let mut m = PairingManager::new(Duration::from_secs(60));
        let code = m.generate("telegram");
        assert_eq!(m.try_claim(&code[..5], "u"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim(&format!("{code}0"), "u"), ClaimOutcome::Invalid);
        assert_eq!(m.try_claim("", "u"), ClaimOutcome::Invalid);
        assert_eq!(
            m.try_claim(&code, "u"),
            ClaimOutcome::Paired("telegram".into())
        );
    }
}
