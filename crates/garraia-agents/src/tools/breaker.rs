//! #1417: circuit breaker por sessao para ferramentas que falham repetidamente.
//!
//! ## O defeito
//!
//! No dogfood da v0.4.5 o modelo repetiu `repo_search` varias vezes no mesmo
//! turno depois de uma falha que nao ia mudar (sem repositorio) e depois de
//! timeouts. Cada repeticao custava uma volta de LLM e, no caso do timeout,
//! mais 30s de espera — por uma resposta que ja era conhecida. O detector de
//! loop por assinatura (`ExecutionBudget`) nao pega isso: o modelo varia o
//! input a cada tentativa, e a assinatura muda.
//!
//! ## O desenho
//!
//! Um [`Breaker`] por sessao, com um registro por ferramenta. Toda saida de
//! ferramenta passa por [`classificar`]:
//!
//! - **Deterministica** — a frase unica do jail (`NO_ROOTS_MESSAGE`,
//!   `DENIAL_MESSAGE`) ou a recusa sem repositorio do `repo_search`: repetir
//!   nao muda nada dentro do turno, entao abre **ate o fim do turno**. O turno
//!   seguinte sonda de novo (o usuario pode ter selecionado um projeto).
//! - **Transitoria** — o timeout do despacho: abre por um cooldown que dobra a
//!   cada timeout seguido ([`COOLDOWN_BASE`] ate [`COOLDOWN_TETO`]) e que
//!   ATRAVESSA turnos — uma mensagem nova um segundo depois nao e motivo para
//!   esperar outros 30s.
//! - **Generica** — qualquer outro erro: conta, e so abre depois de
//!   [`REPETICOES_PARA_ABRIR`] erros IGUAIS no mesmo turno.
//!
//! Uma chamada bem-sucedida fecha o breaker daquela ferramenta e zera a serie
//! de timeouts. Um turno com `working_dir` diferente e outro contexto: limpa a
//! sessao inteira. Ferramenta **indisponivel** (#1425) nao chega aqui — o
//! despacho a recusa antes de executar, e essa consulta ja e barata e
//! deterministica por si.
//!
//! ## Estado puro
//!
//! Sem relogio nem I/O dentro: quem tem o relogio passa o `Instant` (o
//! padrao de `pending_approval.rs`, e do `spinner.rs` da CLI). Nada de
//! `sleep`. O runtime consulta [`Breakers::estado`] antes de executar e
//! alimenta [`Breakers::registrar`] com a saida depois; o `garra_status` le
//! [`Breakers::abertas`]. O que sai daqui para o modelo e para o relatorio e
//! texto constante do modulo — nunca a saida crua da ferramenta, nunca
//! caminho.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::ToolOutput;
use super::file_jail::{DENIAL_MESSAGE, NO_ROOTS_MESSAGE};
use super::repo_search_tool::SEM_REPOSITORIO;

/// O prefixo da saida de erro que o despacho monta quando a ferramenta
/// estoura o timeout do orcamento. Vive aqui porque e a classificacao que
/// depende dele: o runtime formata com ele, [`classificar`] o reconhece.
pub const TIMEOUT_PREFIXO: &str = "tool timeout: ";

/// O primeiro cooldown depois de um timeout. Dobra a cada timeout seguido.
pub const COOLDOWN_BASE: Duration = Duration::from_secs(15);

/// O teto do cooldown: a serie 15s, 30s, 60s, 120s, 120s...
pub const COOLDOWN_TETO: Duration = Duration::from_secs(120);

/// Quantos erros genericos IGUAIS, no mesmo turno, abrem o breaker.
pub const REPETICOES_PARA_ABRIR: u32 = 3;

/// Teto (em chars) da assinatura guardada de um erro generico. So serve
/// para contar iguais; nunca e exibida.
pub const ASSINATURA_MAX: usize = 200;

/// Quantas sessoes o registro guarda; as mais antigas saem (o mesmo teto
/// do `TurnStatsRegistry`).
pub const MAX_SESSOES: usize = 512;

/// Por que uma falha e deterministica: repetir a chamada neste turno, com
/// qualquer input, falha igual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deterministica {
    /// `NO_ROOTS_MESSAGE`: a sessao nao tem raiz nenhuma para as file tools.
    SemRaiz,
    /// `DENIAL_MESSAGE`: o caminho pedido esta fora das raizes.
    ForaDasRaizes,
    /// A recusa do `repo_search` sem repositorio ativo (#1380).
    SemRepositorio,
}

impl Deterministica {
    /// Legivel por maquina, no vocabulario do registro de capacidades.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::SemRaiz => "no_roots",
            Self::ForaDasRaizes => "outside_roots",
            Self::SemRepositorio => "no_repository",
        }
    }

    /// Legivel por humano; texto constante, sem caminho.
    pub fn descricao(self) -> &'static str {
        match self {
            Self::SemRaiz => {
                "a sessao nao tem raiz para as file tools (sem diretorio de trabalho nem \
                 `agent.file_roots`), e todo caminho e negado ate o usuario selecionar um projeto"
            }
            Self::ForaDasRaizes => {
                "o caminho pedido esta fora das raizes permitidas nesta sessao, e caminhos \
                 parecidos vao falhar igual"
            }
            Self::SemRepositorio => {
                "nao ha repositorio ativo para buscar nesta sessao (sem diretorio de trabalho, e \
                 o diretorio do processo nao e um repositorio)"
            }
        }
    }
}

/// Como uma falha foi classificada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classe {
    /// Abre ate o fim do turno.
    Deterministica(Deterministica),
    /// Timeout: abre por um cooldown crescente e limitado.
    Transitoria,
    /// Erro generico, com a assinatura (texto normalizado e truncado) que
    /// permite contar erros iguais. Conta; so abre apos
    /// [`REPETICOES_PARA_ABRIR`] iguais no turno.
    Generica(String),
}

/// O que [`classificar`] responde sobre uma saida de ferramenta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Veredito {
    /// Rodou sem erro: fecha o breaker da ferramenta.
    Sucesso,
    /// Falhou, e assim.
    Falha(Classe),
    /// Nem um nem outro: um pedido de confirmacao humana (GAR-187) sai com
    /// `is_error: true` e NAO e falha — pausar o turno nao pode abrir o
    /// breaker.
    Neutro,
}

/// Classifica a saida de uma ferramenta pelas frases que o proprio runtime
/// e as tools nativas produzem.
pub fn classificar(saida: &ToolOutput) -> Veredito {
    if saida.requires_confirmation {
        return Veredito::Neutro;
    }
    if !saida.is_error {
        return Veredito::Sucesso;
    }
    let texto = saida.content.as_str();
    let classe = if texto.contains(NO_ROOTS_MESSAGE) {
        Classe::Deterministica(Deterministica::SemRaiz)
    } else if texto.contains(DENIAL_MESSAGE) {
        Classe::Deterministica(Deterministica::ForaDasRaizes)
    } else if texto.contains(SEM_REPOSITORIO) {
        Classe::Deterministica(Deterministica::SemRepositorio)
    } else if texto.starts_with(TIMEOUT_PREFIXO) {
        Classe::Transitoria
    } else {
        Classe::Generica(assinatura(texto))
    };
    Veredito::Falha(classe)
}

fn assinatura(texto: &str) -> String {
    texto.trim().chars().take(ASSINATURA_MAX).collect()
}

/// Por que o breaker esta aberto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motivo {
    Deterministica(Deterministica),
    Timeouts { seguidos: u32 },
    Repetida { vezes: u32 },
}

impl Motivo {
    pub fn codigo(&self) -> &'static str {
        match self {
            Self::Deterministica(d) => d.codigo(),
            Self::Timeouts { .. } => "timeout",
            Self::Repetida { .. } => "repeated_error",
        }
    }

    pub fn descricao(&self) -> String {
        match self {
            Self::Deterministica(d) => d.descricao().to_string(),
            Self::Timeouts { seguidos } => {
                format!("excedeu o tempo limite {seguidos} vez(es) seguida(s)")
            }
            Self::Repetida { vezes } => {
                format!("falhou {vezes} vezes com o mesmo erro neste turno")
            }
        }
    }
}

/// Ate quando o breaker fica aberto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ate {
    /// Ate o proximo [`Breaker::abrir_turno`].
    FimDoTurno,
    /// Ate este instante (cooldown de timeout).
    Instante(Instant),
}

/// O estado do breaker de UMA ferramenta numa sessao.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    Fechado,
    Aberto { motivo: Motivo, ate: Ate },
}

impl Estado {
    pub fn esta_aberto(&self) -> bool {
        matches!(self, Self::Aberto { .. })
    }

    /// O texto que volta ao modelo no lugar da execucao; `None` quando
    /// fechado. Nomeia a ferramenta e o codigo, e pede para nao repetir.
    pub fn explicacao(&self, nome: &str, agora: Instant) -> Option<String> {
        let Self::Aberto { motivo, ate } = self else {
            return None;
        };
        let janela = match ate {
            Ate::FimDoTurno => "ate o fim deste turno".to_string(),
            Ate::Instante(t) => format!("por mais {}s", segundos_restantes(*t, agora)),
        };
        Some(format!(
            "A ferramenta `{nome}` esta temporariamente indisponivel nesta conversa \
             ({codigo}): {descricao}; em pausa {janela}. Nao repita a chamada neste turno; \
             diga ao usuario o motivo e siga sem ela.",
            codigo = motivo.codigo(),
            descricao = motivo.descricao(),
        ))
    }
}

fn segundos_restantes(ate: Instant, agora: Instant) -> u64 {
    ate.saturating_duration_since(agora).as_secs_f64().ceil() as u64
}

/// Uma ferramenta em pausa, como o `garra_status` a relata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ToolAberta {
    pub tool: String,
    /// [`Motivo::codigo`].
    pub codigo: &'static str,
    /// [`Motivo::descricao`].
    pub motivo: String,
}

/// O registro de uma ferramenta dentro de uma sessao.
#[derive(Debug, Default)]
struct Registro {
    /// Aberta ate o fim do turno (deterministica ou repetida). Zera em
    /// [`Breaker::abrir_turno`].
    no_turno: Option<Motivo>,
    /// Erros genericos deste turno, por assinatura.
    genericas_no_turno: HashMap<String, u32>,
    /// Timeouts seguidos; zera no sucesso e na troca de contexto.
    timeouts_seguidos: u32,
    /// Fim do cooldown vigente, se ha um.
    cooldown_ate: Option<Instant>,
}

/// O breaker de UMA sessao: um registro por ferramenta mais o contexto
/// (`working_dir` declarado) do ultimo turno.
#[derive(Debug, Default)]
pub struct Breaker {
    contexto: Option<String>,
    por_tool: HashMap<String, Registro>,
}

impl Breaker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Turno novo. `contexto` diferente do anterior (o `working_dir`
    /// declarado da sessao mudou) limpa tudo; o mesmo contexto zera so a
    /// parte que vale por turno — o cooldown de timeout continua.
    pub fn abrir_turno(&mut self, contexto: Option<&str>) {
        let contexto = normalizar_contexto(contexto);
        if contexto != self.contexto {
            self.contexto = contexto;
            self.por_tool.clear();
            return;
        }
        for r in self.por_tool.values_mut() {
            r.no_turno = None;
            r.genericas_no_turno.clear();
        }
        self.por_tool.retain(|_, r| r.timeouts_seguidos > 0);
    }

    /// Alimenta o breaker com a saida da ferramenta, ja classificada.
    pub fn registrar(&mut self, tool: &str, saida: &ToolOutput, agora: Instant) {
        match classificar(saida) {
            Veredito::Sucesso => self.registrar_sucesso(tool),
            Veredito::Falha(classe) => self.registrar_falha(tool, classe, agora),
            Veredito::Neutro => {}
        }
    }

    /// Uma chamada que deu certo fecha o breaker da ferramenta e zera a
    /// serie de timeouts.
    pub fn registrar_sucesso(&mut self, tool: &str) {
        self.por_tool.remove(tool);
    }

    pub fn registrar_falha(&mut self, tool: &str, classe: Classe, agora: Instant) {
        let r = self.por_tool.entry(tool.to_string()).or_default();
        match classe {
            Classe::Deterministica(d) => r.no_turno = Some(Motivo::Deterministica(d)),
            Classe::Transitoria => {
                r.timeouts_seguidos = r.timeouts_seguidos.saturating_add(1);
                r.cooldown_ate = Some(agora + cooldown(r.timeouts_seguidos));
            }
            Classe::Generica(assinatura) => {
                let n = r.genericas_no_turno.entry(assinatura).or_insert(0);
                *n = n.saturating_add(1);
                if *n >= REPETICOES_PARA_ABRIR {
                    r.no_turno = Some(Motivo::Repetida { vezes: *n });
                }
            }
        }
    }

    pub fn estado(&self, tool: &str, agora: Instant) -> Estado {
        let Some(r) = self.por_tool.get(tool) else {
            return Estado::Fechado;
        };
        if let Some(motivo) = &r.no_turno {
            return Estado::Aberto {
                motivo: motivo.clone(),
                ate: Ate::FimDoTurno,
            };
        }
        match r.cooldown_ate {
            Some(ate) if agora < ate => Estado::Aberto {
                motivo: Motivo::Timeouts {
                    seguidos: r.timeouts_seguidos,
                },
                ate: Ate::Instante(ate),
            },
            _ => Estado::Fechado,
        }
    }

    /// As ferramentas abertas AGORA, em ordem lexica.
    pub fn abertas(&self, agora: Instant) -> Vec<ToolAberta> {
        let mut abertas: Vec<ToolAberta> = self
            .por_tool
            .keys()
            .filter_map(|tool| match self.estado(tool, agora) {
                Estado::Aberto { motivo, .. } => Some(ToolAberta {
                    tool: tool.clone(),
                    codigo: motivo.codigo(),
                    motivo: motivo.descricao(),
                }),
                Estado::Fechado => None,
            })
            .collect();
        abertas.sort_by(|a, b| a.tool.cmp(&b.tool));
        abertas
    }
}

/// `COOLDOWN_BASE * 2^(seguidos-1)`, com teto.
fn cooldown(seguidos: u32) -> Duration {
    let expoente = seguidos.saturating_sub(1).min(16);
    COOLDOWN_BASE
        .saturating_mul(2u32.saturating_pow(expoente))
        .min(COOLDOWN_TETO)
}

fn normalizar_contexto(contexto: Option<&str>) -> Option<String> {
    contexto
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

#[derive(Default)]
struct Sessoes {
    por_sessao: HashMap<String, Breaker>,
    /// Ordem de chegada, para saber quem sai quando o teto estoura.
    ordem: VecDeque<String>,
}

impl Sessoes {
    fn entrada(&mut self, session_id: &str, capacidade: usize) -> &mut Breaker {
        if !self.por_sessao.contains_key(session_id) {
            self.ordem.push_back(session_id.to_string());
            while self.ordem.len() > capacidade {
                if let Some(antiga) = self.ordem.pop_front() {
                    self.por_sessao.remove(&antiga);
                }
            }
        }
        self.por_sessao.entry(session_id.to_string()).or_default()
    }
}

/// Um [`Breaker`] por sessao, atras de um lock, com teto de sessoes. E o que
/// mora no `AgentRuntime`.
pub struct Breakers {
    inner: Mutex<Sessoes>,
    capacidade: usize,
}

impl Default for Breakers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Breakers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breakers")
            .field("sessoes", &self.sessoes())
            .field("capacidade", &self.capacidade)
            .finish()
    }
}

impl Breakers {
    pub fn new() -> Self {
        Self::com_capacidade(MAX_SESSOES)
    }

    /// Capacidade zero vira um.
    pub fn com_capacidade(capacidade: usize) -> Self {
        Self {
            inner: Mutex::new(Sessoes::default()),
            capacidade: capacidade.max(1),
        }
    }

    /// O lock sem panico. Se uma thread morreu segurando o lock, o registro
    /// pode estar pela metade: descarta tudo (cada ferramenta e sondada de
    /// novo) em vez de confiar no que sobrou.
    fn lock(&self) -> std::sync::MutexGuard<'_, Sessoes> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = Sessoes::default();
                self.inner.clear_poison();
                guard
            }
        }
    }

    /// Quantas sessoes tem registro agora.
    pub fn sessoes(&self) -> usize {
        self.lock().por_sessao.len()
    }

    /// Ver [`Breaker::abrir_turno`].
    pub fn abrir_turno(&self, session_id: &str, contexto: Option<&str>) {
        let capacidade = self.capacidade;
        self.lock()
            .entrada(session_id, capacidade)
            .abrir_turno(contexto);
    }

    /// Ver [`Breaker::registrar`].
    pub fn registrar(&self, session_id: &str, tool: &str, saida: &ToolOutput, agora: Instant) {
        let capacidade = self.capacidade;
        self.lock()
            .entrada(session_id, capacidade)
            .registrar(tool, saida, agora);
    }

    /// Ver [`Breaker::registrar_falha`].
    pub fn registrar_falha(&self, session_id: &str, tool: &str, classe: Classe, agora: Instant) {
        let capacidade = self.capacidade;
        self.lock()
            .entrada(session_id, capacidade)
            .registrar_falha(tool, classe, agora);
    }

    /// Ver [`Breaker::estado`]. Sessao desconhecida e `Fechado`, e a leitura
    /// nao a cria.
    pub fn estado(&self, session_id: &str, tool: &str, agora: Instant) -> Estado {
        self.lock()
            .por_sessao
            .get(session_id)
            .map(|b| b.estado(tool, agora))
            .unwrap_or(Estado::Fechado)
    }

    /// Ver [`Breaker::abertas`].
    pub fn abertas(&self, session_id: &str, agora: Instant) -> Vec<ToolAberta> {
        self.lock()
            .por_sessao
            .get(session_id)
            .map(|b| b.abertas(agora))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
