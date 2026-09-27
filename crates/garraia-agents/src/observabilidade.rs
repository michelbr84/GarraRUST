//! #1438: observabilidade LOCAL de confiabilidade — ferramentas, servidores
//! MCP, ponte de canal e tendencia de armazenamento.
//!
//! ## O que e
//!
//! Contadores agregados em memoria, do processo, desde o boot. Nada aqui sai
//! da maquina: nao ha exportador, nem telemetria, nem phone-home. Quem le sao
//! as superficies do gateway — `GET /admin/api/reliability` (cookie + RBAC),
//! o `/metrics` autenticado (so totais com label de conjunto fechado) e, so em
//! totais, a linha `reliability.local` do `/api/diagnostics`.
//!
//! ## O que NUNCA entra
//!
//! - prompt, conteudo de mensagem, input ou saida de ferramenta, texto de
//!   erro — o registro recebe o DESFECHO classificado, nunca o texto;
//! - id de sessao ou de usuario: sessao NAO e dimensao (o breaker do #1417 e
//!   por sessao; isto aqui e da instalacao);
//! - caminho e segredo.
//!
//! As chaves sao: nome de ferramenta REGISTRADA (ou um dos baldes constantes
//! [`DESCONHECIDA`] e [`OUTRAS`]), nome de servidor MCP (config do operador),
//! id de canal `&'static str` (conjunto fechado, escolhido pelo codigo) e o
//! recurso de armazenamento ([`Armazenamento`], enum). Um nome que o MODELO
//! inventou nunca vira chave: quem chama diz se o nome e de uma ferramenta
//! registrada, e o que nao e cai em [`DESCONHECIDA`] — o modelo pode por
//! texto do prompt no nome da ferramenta que pede, e esse texto nao pode
//! chegar a um contador que o operador (ou um scraper) le.
//!
//! ## Memoria limitada
//!
//! Teto de ferramentas ([`MAX_FERRAMENTAS`], o excedente soma em
//! [`OUTRAS`]), de servidores MCP ([`MAX_SERVIDORES_MCP`]), de amostras por
//! serie ([`MAX_AMOSTRAS`]) e de tamanho de nome ([`MAX_NOME`]). A latencia
//! vai para baldes FIXOS ([`LIMITES_DE_LATENCIA_MS`]): p50/p95 sao
//! aproximados (o limite superior do balde, nunca acima do maximo observado)
//! sem guardar amostra nenhuma.
//!
//! ## Estado puro
//!
//! Sem relogio nem I/O: quem tem o relogio passa o `Instant` (o padrao de
//! `tools::breaker`). O lock recupera o envenenamento sem panico — contador
//! nao tem invariante que um panico no meio de uma soma possa quebrar.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::tools::ToolOutput;

mod snapshot;
pub use snapshot::{
    BucketSnapshot, ChannelSnapshot, LatencySnapshot, McpSnapshot, SampleSnapshot, Snapshot,
    StorageSnapshot, ToolSnapshot, ToolStats,
};

/// O balde das chamadas a um nome que NAO e de ferramenta registrada.
pub const DESCONHECIDA: &str = "(unknown)";

/// O balde das ferramentas que chegaram depois do teto de cardinalidade.
pub const OUTRAS: &str = "(other)";

/// Quantas ferramentas tem linha propria. O inventario real (nativas + MCP)
/// fica bem abaixo disso; o teto so existe porque servidor MCP e externo.
pub const MAX_FERRAMENTAS: usize = 256;

/// Quantos servidores MCP tem linha propria (config do operador).
pub const MAX_SERVIDORES_MCP: usize = 64;

/// Quantas amostras cada serie de armazenamento guarda: 288 = 24 h no
/// intervalo de 5 min do amostrador do gateway.
pub const MAX_AMOSTRAS: usize = 288;

/// Teto (em chars) de um nome guardado. Nome de ferramenta MCP vem do
/// `tools/list` do servidor, que e externo.
pub const MAX_NOME: usize = 128;

/// Limites superiores (inclusivos, em ms) dos baldes de latencia; o ultimo
/// balde, sem limite, recebe o resto (`+Inf`).
pub const LIMITES_DE_LATENCIA_MS: [u64; 11] = [
    10, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000,
];

const N_BALDES: usize = LIMITES_DE_LATENCIA_MS.len() + 1;

/// Como uma chamada de ferramenta terminou, do ponto de vista da
/// confiabilidade. `codigo()` e o vocabulario estavel das superficies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Desfecho {
    /// Executou e deu certo.
    Sucesso,
    /// Executou e falhou (inclui nome desconhecido e recusa do jail).
    Erro,
    /// Estourou o timeout do orcamento — separado de `Erro` de proposito.
    Timeout,
    /// O portao do turno (modo, whitelist, teto do principal) negou: e a
    /// rejeicao de politica das ferramentas.
    NegadaPelaPolitica,
    /// #1425: permitida, mas nao operacional agora (canal, raiz, MCP caido).
    Indisponivel,
    /// #1417: o breaker da sessao estava aberto; nao executou.
    RecusadaPeloBreaker,
    /// GAR-187: pediu confirmacao humana — nem sucesso nem falha.
    AguardandoConfirmacao,
}

impl Desfecho {
    /// Todos, na ordem em que as superficies os listam.
    pub const TODOS: [Desfecho; 7] = [
        Desfecho::Sucesso,
        Desfecho::Erro,
        Desfecho::Timeout,
        Desfecho::NegadaPelaPolitica,
        Desfecho::Indisponivel,
        Desfecho::RecusadaPeloBreaker,
        Desfecho::AguardandoConfirmacao,
    ];

    /// Legivel por maquina (JSON do admin e label do `/metrics`).
    pub fn codigo(self) -> &'static str {
        match self {
            Desfecho::Sucesso => "success",
            Desfecho::Erro => "error",
            Desfecho::Timeout => "timeout",
            Desfecho::NegadaPelaPolitica => "denied_by_policy",
            Desfecho::Indisponivel => "unavailable",
            Desfecho::RecusadaPeloBreaker => "breaker_refused",
            Desfecho::AguardandoConfirmacao => "awaiting_confirmation",
        }
    }

    fn indice(self) -> usize {
        match self {
            Desfecho::Sucesso => 0,
            Desfecho::Erro => 1,
            Desfecho::Timeout => 2,
            Desfecho::NegadaPelaPolitica => 3,
            Desfecho::Indisponivel => 4,
            Desfecho::RecusadaPeloBreaker => 5,
            Desfecho::AguardandoConfirmacao => 6,
        }
    }

    /// A ferramenta chegou a rodar? So estas entram na latencia: uma recusa
    /// de portao/breaker nao mede nada da ferramenta.
    pub fn executou(self) -> bool {
        matches!(
            self,
            Desfecho::Sucesso
                | Desfecho::Erro
                | Desfecho::Timeout
                | Desfecho::AguardandoConfirmacao
        )
    }

    /// Conta como falha para `last_failure_ago_s`.
    fn falhou(self) -> bool {
        matches!(self, Desfecho::Erro | Desfecho::Timeout)
    }

    /// A saida de uma ferramenta executada, classificada pela MESMA regra do
    /// breaker (`tools::breaker::classificar`): o prefixo de timeout do
    /// despacho e timeout, pedido de confirmacao e neutro, o resto do
    /// `is_error` e erro. O texto da saida nao sai daqui.
    pub fn da_saida(saida: &ToolOutput) -> Desfecho {
        use crate::tools::breaker::{Classe, Veredito, classificar};
        match classificar(saida) {
            Veredito::Sucesso => Desfecho::Sucesso,
            Veredito::Neutro => Desfecho::AguardandoConfirmacao,
            Veredito::Falha(Classe::Transitoria) => Desfecho::Timeout,
            Veredito::Falha(_) => Desfecho::Erro,
        }
    }
}

/// O recurso de uma serie de tendencia de armazenamento (conjunto fechado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Armazenamento {
    /// Entradas da memoria do agente (com e sem vetor).
    MemoriaEntradas,
    /// Linhas do ledger `agent_runs`.
    RunsNoLedger,
}

impl Armazenamento {
    pub fn codigo(self) -> &'static str {
        match self {
            Armazenamento::MemoriaEntradas => "memory_entries",
            Armazenamento::RunsNoLedger => "agent_runs",
        }
    }
}

/// Uma chamada de ferramenta, como o despacho a relata.
#[derive(Debug, Clone, Copy)]
pub struct Chamada<'a> {
    /// O nome como o modelo pediu. So vira chave se `conhecida`.
    pub nome: &'a str,
    /// `true` quando `nome` e de ferramenta registrada (ou intrinseca).
    pub conhecida: bool,
    pub desfecho: Desfecho,
    /// Quanto a execucao levou; ignorada quando o desfecho nao executou.
    pub duracao: Duration,
    /// Esta saida ABRIU o breaker da sessao (fechado -> aberto).
    pub abriu_breaker: bool,
}

/// Latencia em baldes fixos: memoria constante, qualquer que seja o volume.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histograma {
    baldes: [u64; N_BALDES],
    contagem: u64,
    soma_ms: u64,
    max_ms: u64,
}

impl Histograma {
    pub fn registrar(&mut self, duracao: Duration) {
        let ms = u64::try_from(duracao.as_millis()).unwrap_or(u64::MAX);
        let balde = LIMITES_DE_LATENCIA_MS
            .iter()
            .position(|&limite| ms <= limite)
            .unwrap_or(LIMITES_DE_LATENCIA_MS.len());
        self.baldes[balde] = self.baldes[balde].saturating_add(1);
        self.contagem = self.contagem.saturating_add(1);
        self.soma_ms = self.soma_ms.saturating_add(ms);
        self.max_ms = self.max_ms.max(ms);
    }

    /// Soma outro histograma (mesmos baldes): o agregado e exato.
    pub fn somar(&mut self, outro: &Histograma) {
        for (a, b) in self.baldes.iter_mut().zip(outro.baldes.iter()) {
            *a = a.saturating_add(*b);
        }
        self.contagem = self.contagem.saturating_add(outro.contagem);
        self.soma_ms = self.soma_ms.saturating_add(outro.soma_ms);
        self.max_ms = self.max_ms.max(outro.max_ms);
    }

    /// Quantil aproximado: o limite superior do balde onde o acumulado
    /// alcanca `q`, nunca acima do maximo observado. `None` sem amostra.
    pub fn quantil(&self, q: f64) -> Option<u64> {
        if self.contagem == 0 {
            return None;
        }
        let q = if q.is_nan() { 1.0 } else { q.clamp(0.0, 1.0) };
        let alvo = ((q * self.contagem as f64).ceil() as u64).clamp(1, self.contagem);
        let mut acumulado = 0u64;
        for (i, n) in self.baldes.iter().enumerate() {
            acumulado = acumulado.saturating_add(*n);
            if acumulado >= alvo {
                return Some(match LIMITES_DE_LATENCIA_MS.get(i) {
                    Some(limite) => (*limite).min(self.max_ms),
                    None => self.max_ms,
                });
            }
        }
        Some(self.max_ms)
    }

    pub fn contagem(&self) -> u64 {
        self.contagem
    }

    fn snapshot(&self) -> LatencySnapshot {
        let tem = self.contagem > 0;
        LatencySnapshot {
            count: self.contagem,
            avg: tem.then(|| self.soma_ms / self.contagem),
            p50: self.quantil(0.5),
            p95: self.quantil(0.95),
            max: tem.then_some(self.max_ms),
            buckets: self
                .baldes
                .iter()
                .enumerate()
                .map(|(i, n)| BucketSnapshot {
                    le: LIMITES_DE_LATENCIA_MS.get(i).copied(),
                    count: *n,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PorFerramenta {
    desfechos: [u64; 7],
    aberturas_do_breaker: u64,
    latencia: Histograma,
    ultima_falha: Option<Instant>,
}

impl PorFerramenta {
    fn registrar(&mut self, c: &Chamada<'_>, agora: Instant) {
        let i = c.desfecho.indice();
        self.desfechos[i] = self.desfechos[i].saturating_add(1);
        if c.desfecho.executou() {
            self.latencia.registrar(c.duracao);
        }
        if c.desfecho.falhou() {
            self.ultima_falha = mais_recente(self.ultima_falha, Some(agora));
        }
        if c.abriu_breaker {
            self.aberturas_do_breaker = self.aberturas_do_breaker.saturating_add(1);
        }
    }

    fn somar(&mut self, outro: &PorFerramenta) {
        for (a, b) in self.desfechos.iter_mut().zip(outro.desfechos.iter()) {
            *a = a.saturating_add(*b);
        }
        self.aberturas_do_breaker = self
            .aberturas_do_breaker
            .saturating_add(outro.aberturas_do_breaker);
        self.latencia.somar(&outro.latencia);
        self.ultima_falha = mais_recente(self.ultima_falha, outro.ultima_falha);
    }

    fn stats(&self, agora: Instant) -> ToolStats {
        let n = |d: Desfecho| self.desfechos[d.indice()];
        let com_veredito = n(Desfecho::Sucesso)
            .saturating_add(n(Desfecho::Erro))
            .saturating_add(n(Desfecho::Timeout));
        let taxa = |x: u64| (com_veredito > 0).then(|| arredondar(x as f64 / com_veredito as f64));
        ToolStats {
            calls: self
                .desfechos
                .iter()
                .fold(0u64, |soma, n| soma.saturating_add(*n)),
            outcomes: Desfecho::TODOS
                .iter()
                .map(|d| (d.codigo(), n(*d)))
                .collect(),
            breaker_opened: self.aberturas_do_breaker,
            success_rate: taxa(n(Desfecho::Sucesso)),
            error_rate: taxa(n(Desfecho::Erro)),
            timeout_rate: taxa(n(Desfecho::Timeout)),
            latency_ms: self.latencia.snapshot(),
            last_failure_ago_s: self.ultima_falha.map(|t| segundos_desde(t, agora)),
        }
    }
}

fn mais_recente(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Quatro casas: taxa e para ler, nao para somar.
fn arredondar(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn segundos_desde(t: Instant, agora: Instant) -> u64 {
    agora.saturating_duration_since(t).as_secs()
}

fn truncar(nome: &str) -> String {
    nome.chars().take(MAX_NOME).collect()
}

fn e_balde(chave: &str) -> bool {
    chave == DESCONHECIDA || chave == OUTRAS
}

/// No balde [`OUTRAS`] (servidor alem do teto) a queda por transicao e
/// aproximada: servidores diferentes dividem o mesmo `queda_em_aberto`.
#[derive(Debug, Clone, Default)]
struct PorServidor {
    quedas: u64,
    queda_em_aberto: bool,
    reconexoes: u64,
    reconexoes_ok: u64,
    reconexoes_falhas: u64,
    ultima_reconexao: Option<Instant>,
}

impl PorServidor {
    fn snapshot(&self, server: &str, agora: Instant) -> McpSnapshot {
        McpSnapshot {
            server: server.to_string(),
            drops: self.quedas,
            reconnect_attempts: self.reconexoes,
            reconnects_ok: self.reconexoes_ok,
            reconnects_failed: self.reconexoes_falhas,
            last_reconnect_ago_s: self.ultima_reconexao.map(|t| segundos_desde(t, agora)),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PorCanal {
    conectado: bool,
    ja_conectou: bool,
    conexoes: u64,
    reconexoes: u64,
    quedas: u64,
    tentativas_sem_conexao: u64,
    ultima_mudanca: Option<Instant>,
}

impl PorCanal {
    fn registrar(&mut self, conectado: bool, agora: Instant) {
        match (conectado, self.conectado) {
            (true, false) => {
                self.conexoes = self.conexoes.saturating_add(1);
                if self.ja_conectou {
                    self.reconexoes = self.reconexoes.saturating_add(1);
                }
                self.ja_conectou = true;
                self.conectado = true;
                self.ultima_mudanca = Some(agora);
            }
            // O supervisor repete o aviso: nao e outra conexao.
            (true, true) => {}
            (false, true) => {
                self.quedas = self.quedas.saturating_add(1);
                self.conectado = false;
                self.ultima_mudanca = Some(agora);
            }
            // Caida e caida de novo: uma tentativa que nao chegou a conectar.
            (false, false) => {
                self.tentativas_sem_conexao = self.tentativas_sem_conexao.saturating_add(1);
            }
        }
    }

    fn snapshot(&self, channel: &'static str, agora: Instant) -> ChannelSnapshot {
        ChannelSnapshot {
            channel,
            connected: self.conectado,
            connections: self.conexoes,
            reconnects: self.reconexoes,
            drops: self.quedas,
            failed_attempts: self.tentativas_sem_conexao,
            last_change_ago_s: self.ultima_mudanca.map(|t| segundos_desde(t, agora)),
        }
    }
}

fn serie_snapshot(
    recurso: Armazenamento,
    serie: &VecDeque<(Instant, u64)>,
    agora: Instant,
) -> StorageSnapshot {
    let como_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    StorageSnapshot {
        resource: recurso.codigo(),
        current: serie.back().map(|(_, v)| *v),
        delta: match (serie.front(), serie.back()) {
            (Some((_, antiga)), Some((_, nova))) => {
                Some(como_i64(*nova).saturating_sub(como_i64(*antiga)))
            }
            _ => None,
        },
        window_s: serie
            .front()
            .map(|(t, _)| segundos_desde(*t, agora))
            .unwrap_or(0),
        samples: serie
            .iter()
            .map(|(t, v)| SampleSnapshot {
                ago_s: segundos_desde(*t, agora),
                value: *v,
            })
            .collect(),
    }
}

#[derive(Debug, Default)]
struct Estado {
    ferramentas: HashMap<String, PorFerramenta>,
    mcp: HashMap<String, PorServidor>,
    canais: HashMap<&'static str, PorCanal>,
    armazenamento: BTreeMap<Armazenamento, VecDeque<(Instant, u64)>>,
}

impl Estado {
    /// A chave de uma chamada: nome desconhecido vai para [`DESCONHECIDA`]
    /// (nunca o texto que o modelo mandou); nome novo alem do teto, para
    /// [`OUTRAS`]. Os dois baldes nao gastam o teto.
    fn chave_da_ferramenta(&self, nome: &str, conhecida: bool, teto: usize) -> String {
        if !conhecida {
            return DESCONHECIDA.to_string();
        }
        let nome = truncar(nome);
        if self.ferramentas.contains_key(&nome) {
            return nome;
        }
        let proprias = self.ferramentas.keys().filter(|k| !e_balde(k)).count();
        if proprias >= teto {
            OUTRAS.to_string()
        } else {
            nome
        }
    }

    fn servidor(&mut self, servidor: &str) -> &mut PorServidor {
        let nome = truncar(servidor);
        let chave = if self.mcp.contains_key(&nome)
            || self.mcp.keys().filter(|k| k.as_str() != OUTRAS).count() < MAX_SERVIDORES_MCP
        {
            nome
        } else {
            OUTRAS.to_string()
        };
        self.mcp.entry(chave).or_default()
    }
}

/// O registro. Mora no `AgentRuntime` (atras de um `Arc`), e o gateway o
/// entrega ao `McpManager` e ao supervisor da ponte de canal.
#[derive(Debug)]
pub struct Observabilidade {
    inner: Mutex<Estado>,
    max_ferramentas: usize,
}

impl Default for Observabilidade {
    fn default() -> Self {
        Self::new()
    }
}

impl Observabilidade {
    pub fn new() -> Self {
        Self::com_teto_de_ferramentas(MAX_FERRAMENTAS)
    }

    /// Teto zero vira um.
    pub fn com_teto_de_ferramentas(teto: usize) -> Self {
        Self {
            inner: Mutex::new(Estado::default()),
            max_ferramentas: teto.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Estado> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Uma chamada de ferramenta que passou pelo despacho.
    pub fn registrar(&self, chamada: Chamada<'_>, agora: Instant) {
        let mut estado = self.lock();
        let chave =
            estado.chave_da_ferramenta(chamada.nome, chamada.conhecida, self.max_ferramentas);
        estado
            .ferramentas
            .entry(chave)
            .or_default()
            .registrar(&chamada, agora);
    }

    /// O health monitor do MCP viu o transporte de `servidor` vivo ou morto.
    /// Conta uma QUEDA por transicao vivo -> morto, nao por tick: um
    /// transporte morto esperando o backoff e visto de novo a cada 30s.
    pub fn registrar_transporte_mcp(&self, servidor: &str, vivo: bool) {
        let mut estado = self.lock();
        let s = estado.servidor(servidor);
        if vivo {
            s.queda_em_aberto = false;
        } else if !s.queda_em_aberto {
            s.quedas = s.quedas.saturating_add(1);
            s.queda_em_aberto = true;
        }
    }

    /// Uma tentativa de reconexao automatica de `servidor` terminou.
    pub fn registrar_reconexao_mcp(&self, servidor: &str, ok: bool, agora: Instant) {
        let mut estado = self.lock();
        let s = estado.servidor(servidor);
        s.reconexoes = s.reconexoes.saturating_add(1);
        if ok {
            s.reconexoes_ok = s.reconexoes_ok.saturating_add(1);
            s.queda_em_aberto = false;
        } else {
            s.reconexoes_falhas = s.reconexoes_falhas.saturating_add(1);
        }
        s.ultima_reconexao = mais_recente(s.ultima_reconexao, Some(agora));
    }

    /// A ponte de um canal conectou (`true`) ou caiu (`false`). `canal` e
    /// `&'static str` de proposito: o conjunto e fechado pelo codigo, nunca
    /// um valor de config ou de request.
    pub fn registrar_conexao_do_canal(&self, canal: &'static str, conectado: bool, agora: Instant) {
        self.lock()
            .canais
            .entry(canal)
            .or_default()
            .registrar(conectado, agora);
    }

    /// Uma amostra de tamanho de armazenamento. A mais antiga sai depois de
    /// [`MAX_AMOSTRAS`].
    pub fn registrar_amostra(&self, recurso: Armazenamento, valor: u64, agora: Instant) {
        let mut estado = self.lock();
        let serie = estado.armazenamento.entry(recurso).or_default();
        serie.push_back((agora, valor));
        while serie.len() > MAX_AMOSTRAS {
            serie.pop_front();
        }
    }

    /// Tudo, agregado, relativo a `agora`. Copia e solta o lock.
    pub fn snapshot(&self, agora: Instant) -> Snapshot {
        let estado = self.lock();
        let mut tools: Vec<ToolSnapshot> = estado
            .ferramentas
            .iter()
            .map(|(nome, f)| ToolSnapshot {
                tool: nome.clone(),
                stats: f.stats(agora),
            })
            .collect();
        tools.sort_by(|a, b| a.tool.cmp(&b.tool));
        let mut total = PorFerramenta::default();
        for f in estado.ferramentas.values() {
            total.somar(f);
        }
        let mut mcp: Vec<McpSnapshot> = estado
            .mcp
            .iter()
            .map(|(nome, s)| s.snapshot(nome, agora))
            .collect();
        mcp.sort_by(|a, b| a.server.cmp(&b.server));
        let mut channels: Vec<ChannelSnapshot> = estado
            .canais
            .iter()
            .map(|(canal, c)| c.snapshot(canal, agora))
            .collect();
        channels.sort_by(|a, b| a.channel.cmp(b.channel));
        let storage = estado
            .armazenamento
            .iter()
            .map(|(recurso, serie)| serie_snapshot(*recurso, serie, agora))
            .collect();
        Snapshot {
            tools,
            tools_total: total.stats(agora),
            mcp,
            channels,
            storage,
        }
    }
}

#[cfg(test)]
mod tests;
