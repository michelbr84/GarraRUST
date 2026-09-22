use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;

/// Tamanho da janela para detecção de loop.
/// Só dispara se as últimas N chamadas tiverem a MESMA assinatura (ferramenta + argumentos).
const JANELA_LOOP: usize = 3;

/// Assinatura de uma chamada de ferramenta: nome + hash dos argumentos.
/// Duas chamadas são consideradas "iguais" apenas se nome E argumentos forem idênticos.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssinaturaFerramenta {
    nome: String,
    hash_args: u64,
}

/// Calcula um hash determinístico dos argumentos da ferramenta (payload JSON).
fn calcular_hash_args(payload: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    hasher.finish()
}

/// O trecho do input repetido que entra no erro de loop (#1295).
///
/// **Nao** e o payload serializado. O erro vira `error_snippet` no ledger
/// de runs (`finish_agent_run`), cartao de erro na CLI e linha de log —
/// despejar o JSON inteiro confiando so no `redact_secrets` vazaria
/// exatamente o que o regex nao reconhece: `{"password": "hunter2"}` nao
/// tem prefixo nem forma de chave de API, e passaria inteiro para os tres.
/// A regra e a mesma do `summarize_tool_input` (#937), e pela mesma razao:
/// so o campo allow-listed da ferramenta, ja redigido e truncado; ferramenta
/// sem caso proprio nao mostra valor nenhum, ate alguem dizer qual campo
/// dela e o interessante.
///
/// Quando a ferramenta nao tem caso, o que ainda diz **o que** repetiu sem
/// despejar valor sao os nomes das chaves e o tamanho serializado:
/// `objeto com chaves [password, query] (41 bytes)` deixa claro que a mesma
/// consulta voltou tres vezes, e nao mostra a consulta. As chaves saem em
/// ordem lexica para a mensagem nao depender de `preserve_order` do
/// `serde_json`, e passam pelo mesmo `sanear` — chave e nome de campo do
/// schema, mas o JSON vem do modelo, e custa nada fechar o canto.
fn resumo_do_input(tool_name: &str, payload: &Value) -> String {
    let resumo = crate::turn_events::summarize_tool_input(tool_name, payload);
    if !resumo.is_empty() {
        return resumo;
    }
    let tamanho = payload.to_string().len();
    match payload {
        Value::Object(mapa) => {
            let mut chaves: Vec<&str> = mapa.keys().map(String::as_str).collect();
            chaves.sort_unstable();
            let lista = crate::turn_events::sanear(&chaves.join(", "));
            format!("objeto com chaves [{lista}] ({tamanho} bytes)")
        }
        outro => format!("{} ({tamanho} bytes)", tipo_json(outro)),
    }
}

/// Nome do tipo JSON, para o fallback de [`resumo_do_input`] quando o input
/// nem objeto e — o modelo pode mandar uma string ou um array como input.
fn tipo_json(valor: &Value) -> &'static str {
    match valor {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Orçamento de execução para controlar chamadas de ferramentas no runtime do agente.
/// Evita loops infinitos, mas permite tarefas legítimas de longa duração.
///
/// A detecção de loop utiliza abordagem baseada em **assinatura**:
/// - Uma "assinatura" = nome da ferramenta + hash dos argumentos
/// - Só bloqueia quando as últimas `JANELA_LOOP` chamadas têm a MESMA assinatura
/// - Argumentos diferentes para a mesma ferramenta (ex: `bash("ls")` e `bash("cat file")`)
///   NÃO são considerados loop
pub struct ExecutionBudget {
    /// Máximo de chamadas de ferramenta por turno (um turno da conversa)
    max_per_turn: usize,
    /// Máximo de chamadas de ferramenta por tarefa (execução completa)
    max_per_task: usize,
    /// Timeout de cada execução de ferramenta em segundos
    tool_timeout_secs: u64,
    /// Quantidade atual de chamadas neste turno
    current_turn_calls: usize,
    /// Quantidade atual de chamadas nesta tarefa
    current_task_calls: usize,
    /// Janela deslizante com assinaturas recentes para detecção de loop
    historico_assinaturas: VecDeque<AssinaturaFerramenta>,
    /// #1295 item 1: a tarefa ja recebeu o seu unico aviso de loop.
    ///
    /// Escopo de TAREFA, de proposito: so [`Self::resetar_tarefa`] o
    /// desliga. O [`Self::resetar_turno`] roda cada vez que o teto por turno
    /// e atingido, e se ele rearmasse o aviso o modelo ganharia um aviso novo
    /// a cada 10 chamadas — um jeito de cultivar avisos em vez de parar.
    aviso_de_loop_dado: bool,
    /// #1295 (revisao da onda A): a assinatura que recebeu o aviso.
    ///
    /// Sem ela, uma chamada diferente no meio (`X, X, X` avisado, `Y`, `X`)
    /// esvaziava a janela de tres e a chamada avisada voltava a rodar mais
    /// duas vezes antes do corte. Com ela, a proxima ocorrencia da mesma
    /// assinatura na tarefa aborta, em sequencia ou nao. Mesmo escopo do
    /// `aviso_de_loop_dado`: so [`Self::resetar_tarefa`] a limpa — nem o
    /// [`Self::resetar_turno`], que esvazia a janela no meio da tarefa.
    assinatura_avisada: Option<AssinaturaFerramenta>,
}

/// O que fazer com uma chamada que fechou a janela de loop (#1295 item 1).
///
/// Nos dois casos a chamada **nao** e executada — a deteccao roda antes da
/// execucao, como sempre. A diferenca e o que acontece com o turno.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VereditoDeLoop {
    /// Primeira deteccao da tarefa: o modelo recebe, no lugar do resultado,
    /// esta observacao corretiva, e o turno segue. Custa no maximo UMA volta
    /// a mais de LLM por tarefa.
    Avisar(String),
    /// Ja houve aviso nesta tarefa: o turno aborta com a mensagem do #1318.
    Abortar(String),
}

/// Timeout por execução de ferramenta, em segundos, quando nada é
/// configurado.
pub const TIMEOUT_PADRAO_SECS: u64 = 30;

/// Nome da variável que sobrescreve o timeout.
pub const ENV_TOOL_TIMEOUT: &str = "GARRA_TOOL_TIMEOUT_SECS";

/// Acima disto o valor quase certamente é engano — ver [`timeout_configurado`].
const TIMEOUT_SUSPEITO_SECS: u64 = 3600;

/// O timeout efetivo: a variável de ambiente quando válida, senão o padrão.
///
/// # Por que existe (#981)
///
/// 30s fixos matam ferramenta que chama LLM por dentro. Um MCP que faz
/// sumarização ou um `ask` aninhado passa disso legitimamente, e o agente
/// recebia `tool timeout` como se fosse falha da ferramenta — o erro apontava
/// para o lugar errado.
///
/// # Por que uma variável de ambiente, e não uma chave de config
///
/// O `garraia-agents` **não depende do `garraia-config`**, e criar essa
/// dependência só para um `u64` acoplaria o crate de agentes ao carregador de
/// configuração inteiro. O preço da env var é ela nascer invisível ao
/// `garra config check` — pago em separado: o `check` agora reporta o valor
/// efetivo, então o operador descobre sem ler código.
///
/// # Valor inválido avisa, em vez de sumir
///
/// `GARRA_TOOL_TIMEOUT_SECS=abc` ou `=0` volta ao padrão **e loga**. Cair no
/// padrão em silêncio faria alguém configurar, ver o comportamento antigo, e
/// não ter como saber por quê.
///
/// Valor absurdo (acima de 1h) é **aceito com aviso**, não rejeitado: quem
/// escreve `30000` provavelmente quis milissegundos, e um turno de 8 horas é o
/// tipo de coisa que se descobre tarde. Mas se a pessoa quer mesmo, o número é
/// dela.
pub fn timeout_configurado() -> u64 {
    let bruto = match std::env::var(ENV_TOOL_TIMEOUT) {
        Ok(v) => v,
        Err(_) => return TIMEOUT_PADRAO_SECS,
    };
    interpretar_timeout(bruto.trim())
}

/// A decisão em si, separada da leitura do ambiente para poder ser testada
/// sem mexer em estado global do processo.
fn interpretar_timeout(bruto: &str) -> u64 {
    match bruto.parse::<u64>() {
        Ok(0) => {
            tracing::warn!(
                "{ENV_TOOL_TIMEOUT}=0 nao faz sentido (toda ferramenta falharia \
                 imediatamente); usando o padrao de {TIMEOUT_PADRAO_SECS}s"
            );
            TIMEOUT_PADRAO_SECS
        }
        Ok(v) if v > TIMEOUT_SUSPEITO_SECS => {
            tracing::warn!(
                "{ENV_TOOL_TIMEOUT}={v} passa de uma hora por ferramenta. Se a \
                 intencao era milissegundos, o valor esta 1000x maior. Usando \
                 {v}s como pedido."
            );
            v
        }
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(
                "{ENV_TOOL_TIMEOUT}={bruto:?} nao e um numero de segundos; \
                 usando o padrao de {TIMEOUT_PADRAO_SECS}s"
            );
            TIMEOUT_PADRAO_SECS
        }
    }
}

impl ExecutionBudget {
    /// Cria um orçamento com valores padrão:
    /// - 10 chamadas por turno
    /// - 50 chamadas por tarefa
    /// - [`TIMEOUT_PADRAO_SECS`] de timeout por ferramenta, salvo override
    pub fn padrao() -> Self {
        Self {
            max_per_turn: 10,
            max_per_task: 50,
            tool_timeout_secs: timeout_configurado(),
            current_turn_calls: 0,
            current_task_calls: 0,
            historico_assinaturas: VecDeque::with_capacity(JANELA_LOOP),
            aviso_de_loop_dado: false,
            assinatura_avisada: None,
        }
    }

    /// Cria um orçamento com limite de tarefa personalizado.
    pub fn com_limite(max_per_task: usize) -> Self {
        Self {
            max_per_task,
            ..Self::padrao()
        }
    }

    /// Aplica os limites do modo escolhido (#979).
    ///
    /// `ModeLimits` existia desde que os modos foram desenhados, e o runtime
    /// nunca o leu: todo turno rodava com `padrao()`, entao um modo que se
    /// declarava mais curto ou mais longo nao era nem uma coisa nem outra. Os
    /// dois campos que tem correspondencia direta sao mapeados; `max_turns` e
    /// do escopo de conversa, nao de orcamento de ferramenta, e nao entra aqui.
    ///
    /// # O modo baixa o teto, nunca levanta
    ///
    /// Modo e um seletor do **usuario**: `/mode` e um comando, e o
    /// `POST /api/mode/select` e aberto. Deixar o modo levantar o teto faria o
    /// custo de API e o tempo de parede de um turno serem funcao do que o
    /// usuario digitou, sem o operador ter dito nada. Dos nove perfis, um so
    /// pede mais que o padrao — o `orchestrator`, com 100 chamadas e 60s, que
    /// levaria o pior caso de ~25 para ~100 minutos.
    ///
    /// Entao o teto do padrao (que ja e configuravel pelo operador, via
    /// `GARRAIA_TOOL_TIMEOUT_SECS` no caso do timeout) e o limite superior, e o
    /// modo so encurta a partir dali. Quem **quer** um orcamento maior passa
    /// `max_tool_calls` explicito no runtime, que continua vencendo tudo — e
    /// esse e um knob de quem sobe o processo, nao de quem manda mensagem.
    ///
    /// O `max_per_turn` acompanha o teto da tarefa quando o modo pede menos que
    /// os 10 do padrao — um modo que so permite 3 chamadas na tarefa inteira
    /// nao pode ter um teto de turno de 10, senao o limite mais apertado dos
    /// dois nunca seria o do modo.
    pub fn com_limites_do_modo(limits: &crate::modes::ModeLimits) -> Self {
        let padrao = Self::padrao();
        let max_per_task = (limits.max_tool_loops.max(1) as usize).min(padrao.max_per_task);
        Self {
            max_per_task,
            max_per_turn: padrao.max_per_turn.min(max_per_task),
            tool_timeout_secs: limits.timeout_secs.max(1).min(padrao.tool_timeout_secs),
            ..padrao
        }
    }

    /// Verifica se o limite por turno foi atingido (mas não o limite total da tarefa).
    /// Usado para estratégia de auto-reset entre turnos.
    pub fn atingiu_limite_turno(&self) -> bool {
        self.current_turn_calls >= self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Quantas ferramentas esta tarefa ja executou (#984).
    pub fn chamadas_na_tarefa(&self) -> usize {
        self.current_task_calls
    }

    /// Verifica se ainda é permitido chamar outra ferramenta.
    pub fn pode_chamar_ferramenta(&self) -> bool {
        self.current_turn_calls < self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Conta uma chamada contra o orcamento do turno e da tarefa **sem**
    /// entrar na janela de deteccao de loop (#1226, achado de revisao).
    ///
    /// Existe para o envelope `tool_program`: ele gasta orcamento como
    /// qualquer chamada, mas cada passo dele volta pelo despacho e registra a
    /// propria assinatura. Se o envelope tambem entrasse na janela, um modelo
    /// preso repetindo `tool_program{steps:[X]}` a cada volta deixaria a
    /// janela alternando `[tp, X, tp]` / `[X, tp, X]`, e o corte de
    /// [`JANELA_LOOP`] chamadas identicas nunca dispararia — sobraria so o
    /// teto da tarefa, ~25 repeticoes em vez de 3. Fora da janela, os passos
    /// ficam colados um no outro e a terceira repeticao de X corta como corta
    /// fora do programa.
    pub fn registrar_contagem(&mut self) {
        self.current_turn_calls += 1;
        self.current_task_calls += 1;
    }

    /// Registra uma chamada de ferramenta com seu payload,
    /// para controle de orçamento e detecção de loop por assinatura.
    pub fn registrar_chamada(&mut self, tool_name: &str, payload: &Value) {
        self.registrar_contagem();
        self.registrar_assinatura(tool_name, payload);
    }

    /// So a metade de assinatura de [`Self::registrar_chamada`]: entra na
    /// janela de loop sem contar contra o orcamento (#1226, revisao do
    /// #1337). Usado quando um `tool_program` ja contado pelo envelope nao
    /// despachou passo nenhum — o orcamento segue 1 + N.
    pub fn registrar_assinatura(&mut self, tool_name: &str, payload: &Value) {
        let assinatura = AssinaturaFerramenta {
            nome: tool_name.to_string(),
            hash_args: calcular_hash_args(payload),
        };

        if self.historico_assinaturas.len() == JANELA_LOOP {
            self.historico_assinaturas.pop_front();
        }

        self.historico_assinaturas.push_back(assinatura);
    }

    /// Detecta se uma ferramenta está sendo chamada em loop.
    ///
    /// Retorna `true` apenas quando as últimas `JANELA_LOOP` chamadas
    /// possuem exatamente a MESMA assinatura (mesmo nome E mesmos argumentos).
    ///
    /// Exemplos:
    /// - bash("ls"), bash("cat f"), bash("pwd")  → false (argumentos diferentes)
    /// - bash("cargo check") x3                  → true  (loop real)
    /// - bash("ls"), file_read("x"), bash("ls")  → false (ferramentas diferentes no meio)
    pub fn detectar_loop_ferramenta(&self) -> bool {
        if self.historico_assinaturas.len() < JANELA_LOOP {
            return false;
        }

        let primeira = &self.historico_assinaturas[0];

        self.historico_assinaturas
            .iter()
            .all(|sig| sig.nome == primeira.nome && sig.hash_args == primeira.hash_args)
    }

    /// #1295: mensagem de erro diagnosticável para o loop detectado — nome
    /// da ferramenta, contagem da janela e o trecho do input repetido que
    /// [`resumo_do_input`] deixa sair (campo allow-listed da ferramenta, ou
    /// so a forma do objeto quando ela nao tem caso).
    ///
    /// Pré-condição: `detectar_loop_ferramenta()` acabou de devolver `true`
    /// para a chamada `input_atual`. A janela está cheia de assinaturas
    /// idênticas por construção, então o input da chamada atual **é** o
    /// input repetido — e o tamanho da janela é o número de chamadas iguais
    /// em sequência que disparou o corte.
    pub fn mensagem_de_loop(&self, tool_name: &str, input_atual: &Value) -> String {
        format!(
            "tool loop detected: {} ({} chamadas identicas em sequencia); input repetido: {}",
            tool_name,
            self.historico_assinaturas.len(),
            resumo_do_input(tool_name, input_atual),
        )
    }

    /// #1295 item 1: o veredito para a chamada que acabou de ser registrada.
    ///
    /// `None` quando a janela nao fechou em loop. Na primeira deteccao da
    /// tarefa devolve [`VereditoDeLoop::Avisar`] com a observacao corretiva
    /// e marca a tarefa; toda deteccao seguinte, da mesma assinatura ou de
    /// outro loop, devolve [`VereditoDeLoop::Abortar`] com a mensagem de
    /// [`Self::mensagem_de_loop`]. Depois do aviso, a assinatura avisada
    /// aborta na proxima ocorrencia mesmo sem fechar a janela (outra
    /// chamada no meio nao a libera). A chamada ja foi contada no orcamento
    /// (`registrar_chamada`), entao `max_per_turn`/`max_per_task` seguem
    /// valendo por cima.
    pub fn veredito_de_loop(
        &mut self,
        tool_name: &str,
        input_atual: &Value,
    ) -> Option<VereditoDeLoop> {
        if !self.detectar_loop_ferramenta() {
            // A janela nao fechou, mas a chamada avisada voltou: aborta.
            // A chamada atual ja foi registrada, entao ela e o fim da janela.
            let voltou_a_avisada = self
                .assinatura_avisada
                .as_ref()
                .is_some_and(|avisada| self.historico_assinaturas.back() == Some(avisada));
            if voltou_a_avisada {
                return Some(VereditoDeLoop::Abortar(format!(
                    "tool loop detected: {} (repetida depois do aviso de loop); \
                     input repetido: {}",
                    tool_name,
                    resumo_do_input(tool_name, input_atual),
                )));
            }
            return None;
        }
        let mensagem = self.mensagem_de_loop(tool_name, input_atual);
        if self.aviso_de_loop_dado {
            return Some(VereditoDeLoop::Abortar(mensagem));
        }
        self.aviso_de_loop_dado = true;
        self.assinatura_avisada = self.historico_assinaturas.back().cloned();
        Some(VereditoDeLoop::Avisar(format!(
            "{mensagem}. Esta chamada NAO foi executada: as ultimas {n} chamadas \
             foram identicas. Leia o resultado ou o erro anterior e mude de \
             abordagem; repetir a mesma chamada encerra o turno.",
            n = self.historico_assinaturas.len(),
        )))
    }

    /// Retorna a duração de timeout configurada para execução de ferramentas.
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tool_timeout_secs)
    }

    /// Reseta o orçamento para um novo turno (após resposta do assistente).
    pub fn resetar_turno(&mut self) {
        self.current_turn_calls = 0;
        self.historico_assinaturas.clear();
    }

    /// Reseta completamente o orçamento para uma nova tarefa (nova mensagem do usuário).
    pub fn resetar_tarefa(&mut self) {
        self.current_turn_calls = 0;
        self.current_task_calls = 0;
        self.historico_assinaturas.clear();
        self.aviso_de_loop_dado = false;
        self.assinatura_avisada = None;
    }

    /// Retorna o status atual do orçamento em formato textual.
    pub fn status(&self) -> String {
        format!(
            "turn={}/{} task={}/{}",
            self.current_turn_calls, self.max_per_turn, self.current_task_calls, self.max_per_task
        )
    }
}

#[cfg(test)]
mod tests {

    /// Os limites do modo alimentam o orcamento (#979).
    ///
    /// `ModeLimits` existia desde o desenho dos modos e o runtime nunca o leu:
    /// todo turno rodava com `padrao()`, entao um modo que se declarava mais
    /// curto nao era mais curto em lugar nenhum.
    #[test]
    fn limites_do_modo_viram_orcamento() {
        let limits = crate::modes::ModeLimits {
            max_tool_loops: 3,
            timeout_secs: 7,
            max_turns: 99,
        };
        let b = ExecutionBudget::com_limites_do_modo(&limits);

        assert_eq!(b.timeout().as_secs(), 7);

        // Tres chamadas cabem; a quarta nao.
        let mut b = b;
        for i in 0..3 {
            assert!(b.pode_chamar_ferramenta(), "chamada {i} deveria caber");
            b.registrar_chamada("t", &serde_json::json!({ "n": i }));
        }
        assert!(
            !b.pode_chamar_ferramenta(),
            "o teto do modo (3) precisa valer; com o max_per_turn de 10 do \
             padrao intacto, a quarta chamada passaria"
        );
    }

    /// Modo nao levanta o teto do operador (#979, achado de auditoria).
    ///
    /// `orchestrator` declara 100 chamadas e 60s — o dobro do padrao. Modo e um
    /// seletor do usuario; deixar `/mode orchestrator` quadruplicar o tempo de
    /// parede faria o custo do turno depender do que a pessoa digitou, sem o
    /// operador ter dito nada.
    #[test]
    fn modo_nao_levanta_o_teto_do_padrao() {
        let generoso = crate::modes::ModeLimits {
            max_tool_loops: 100,
            timeout_secs: 60,
            max_turns: 10,
        };
        let padrao = ExecutionBudget::padrao();
        let b = ExecutionBudget::com_limites_do_modo(&generoso);

        assert_eq!(b.max_per_task, padrao.max_per_task, "o teto e o do padrao");
        assert_eq!(
            b.timeout().as_secs(),
            padrao.timeout().as_secs(),
            "o timeout tambem nao sobe"
        );
    }

    /// Modo com teto zero nao pode virar orcamento inutilizavel.
    #[test]
    fn limite_zero_do_modo_vira_um() {
        let limits = crate::modes::ModeLimits {
            max_tool_loops: 0,
            timeout_secs: 0,
            max_turns: 1,
        };
        let b = ExecutionBudget::com_limites_do_modo(&limits);
        assert!(b.pode_chamar_ferramenta(), "zero viraria travamento total");
        assert_eq!(b.timeout().as_secs(), 1);
    }

    use super::{TIMEOUT_PADRAO_SECS, interpretar_timeout};

    /// O caso que motivou a issue: ferramenta que chama LLM por dentro passa
    /// de 30s legitimamente.
    #[test]
    fn valor_valido_e_respeitado() {
        assert_eq!(interpretar_timeout("180"), 180);
        assert_eq!(interpretar_timeout("  180  ".trim()), 180);
    }

    #[test]
    fn sem_override_usa_o_padrao() {
        // `timeout_configurado` sem a env var; nao mexe em estado global.
        assert_eq!(TIMEOUT_PADRAO_SECS, 30);
    }

    /// Valor invalido volta ao padrao **e avisa**. Cair no padrao em silencio
    /// faria alguem configurar, ver o comportamento antigo e nao ter como
    /// saber por que.
    #[test]
    fn valor_invalido_volta_ao_padrao() {
        assert_eq!(interpretar_timeout("abc"), TIMEOUT_PADRAO_SECS);
        assert_eq!(interpretar_timeout(""), TIMEOUT_PADRAO_SECS);
        assert_eq!(interpretar_timeout("-5"), TIMEOUT_PADRAO_SECS);
        assert_eq!(interpretar_timeout("1.5"), TIMEOUT_PADRAO_SECS);
    }

    /// Zero faria toda ferramenta falhar na hora — nao e configuracao, e
    /// engano.
    #[test]
    fn zero_volta_ao_padrao() {
        assert_eq!(interpretar_timeout("0"), TIMEOUT_PADRAO_SECS);
    }

    /// Valor absurdo e **aceito**, com aviso. Quem escreve 30000
    /// provavelmente quis milissegundos, mas se quer mesmo, o numero e dele.
    #[test]
    fn valor_absurdo_e_aceito_com_aviso() {
        assert_eq!(interpretar_timeout("30000"), 30000);
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn test_padrao_creation() {
        let budget = ExecutionBudget::padrao();
        assert!(budget.pode_chamar_ferramenta());
        assert_eq!(budget.max_per_turn, 10);
        assert_eq!(budget.max_per_task, 50);
    }

    #[test]
    fn test_registrar_chamada() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        assert_eq!(budget.current_turn_calls, 1);
        assert_eq!(budget.current_task_calls, 1);
    }

    /// #1226 (achado de revisao): `registrar_contagem` gasta orcamento mas
    /// nao entra na janela — tres X intercalados com contagens puras ainda
    /// sao tres X colados, e o detector dispara.
    #[test]
    fn registrar_assinatura_entra_na_janela_sem_gastar_orcamento() {
        // #1226 (revisao do #1337): o `tool_program` que nao despacha passo
        // nenhum entra na janela so pela assinatura — o envelope ja foi
        // contado, e o orcamento continua 1 + N.
        let mut b = ExecutionBudget::padrao();
        let x = serde_json::json!({ "steps": "mal formado" });
        for _ in 0..JANELA_LOOP {
            b.registrar_assinatura("tool_program", &x);
        }
        assert_eq!(b.chamadas_na_tarefa(), 0, "assinatura nao conta chamada");
        assert!(b.detectar_loop_ferramenta(), "mas a janela enche e corta");
    }

    #[test]
    fn registrar_contagem_gasta_orcamento_sem_entrar_na_janela() {
        let mut budget = ExecutionBudget::padrao();
        let x = json!({"command": "cargo check"});
        for _ in 0..3 {
            budget.registrar_contagem();
            budget.registrar_chamada("bash", &x);
        }
        assert_eq!(budget.current_turn_calls, 6, "o envelope conta no turno");
        assert_eq!(budget.current_task_calls, 6, "e na tarefa");
        assert!(
            budget.detectar_loop_ferramenta(),
            "a contagem pura nao pode separar as tres assinaturas iguais"
        );
    }

    #[test]
    fn test_no_loop_different_args() {
        let mut budget = ExecutionBudget::padrao();

        // 3 chamadas bash com argumentos DIFERENTES — não deve ser loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "cat file.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "pwd"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_same_args() {
        let mut budget = ExecutionBudget::padrao();

        // 3 chamadas bash com argumentos IDÊNTICOS — é loop
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_under_window() {
        let mut budget = ExecutionBudget::padrao();

        // Apenas 2 chamadas idênticas — abaixo do limite da janela
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_mixed_tools() {
        let mut budget = ExecutionBudget::padrao();

        // Ferramentas diferentes intercaladas — não é loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("file_read", &json!({"path": "test.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_breaks_after_different_call() {
        let mut budget = ExecutionBudget::padrao();

        // Começa repetindo...
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Uma chamada diferente quebra o padrão
        budget.registrar_chamada("bash", &json!({"command": "cat Cargo.toml"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_reset_turno() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        budget.resetar_turno();

        assert_eq!(budget.current_turn_calls, 0);
        assert_eq!(budget.current_task_calls, 2); // Chamadas da tarefa NÃO são resetadas
        assert!(budget.historico_assinaturas.is_empty());
    }

    #[test]
    fn test_reset_tarefa() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        budget.resetar_tarefa();

        assert_eq!(budget.current_turn_calls, 0);
        assert_eq!(budget.current_task_calls, 0);
        assert!(budget.historico_assinaturas.is_empty());
    }

    #[test]
    fn test_exceeds_max_per_turn() {
        let mut budget = ExecutionBudget::padrao();

        // Padrão é 10 por turno
        for i in 0..10 {
            budget.registrar_chamada("bash", &json!({"command": format!("cmd_{}", i)}));
        }

        assert!(!budget.pode_chamar_ferramenta());
    }

    #[test]
    fn test_status_display() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        let status = budget.status();
        assert_eq!(status, "turn=1/10 task=1/50");
    }

    #[test]
    fn test_sliding_window_eviction() {
        let mut budget = ExecutionBudget::padrao();

        // Preenche a janela com chamadas idênticas
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Terceira chamada diferente — remove a mais antiga, janela fica mista
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());

        // Agora preenche novamente com o novo comando
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        // Janela agora é [ls, ls, ls] — loop detectado
        assert!(budget.detectar_loop_ferramenta());
    }

    // ─── #1295: o erro de loop tem de ser diagnosticavel ─────────────────

    /// A mensagem leva o que quem le precisa para corrigir: qual tool, quantas
    /// voltas identicas e O QUE estava repetindo — para ferramenta com caso
    /// no `summarize_tool_input`, o campo allow-listed dela (o `path` do
    /// `file_read`, que a #937 mostra inteiro de proposito).
    #[test]
    fn mensagem_de_loop_traz_nome_contagem_e_input() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"path": "/tmp/alvo-repetido"});
        for _ in 0..3 {
            budget.registrar_chamada("file_read", &input);
        }
        assert!(budget.detectar_loop_ferramenta(), "pre-condicao do teste");

        let msg = budget.mensagem_de_loop("file_read", &input);
        assert!(msg.starts_with("tool loop detected: file_read"), "{msg}");
        assert!(msg.contains("3 chamadas identicas em sequencia"), "{msg}");
        assert!(msg.contains("input repetido: /tmp/alvo-repetido"), "{msg}");
    }

    /// Achado de revisao: despejar o input inteiro confiando so no regex
    /// vazaria segredo sem formato. `{"password": "hunter2"}` nao tem
    /// prefixo de chave de API e passaria inteiro para o ledger de runs, o
    /// cartao da CLI e o log — onde na base o erro era so o nome da tool.
    /// Ferramenta sem caso proprio mostra a FORMA do input (chaves + tamanho)
    /// e nenhum valor.
    #[test]
    fn mensagem_de_loop_nao_despeja_input_de_tool_sem_resumo() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"password": "hunter2", "query": "select 1"});
        for _ in 0..3 {
            budget.registrar_chamada("db_query", &input);
        }

        let msg = budget.mensagem_de_loop("db_query", &input);
        assert!(!msg.contains("hunter2"), "senha sem formato vazou: {msg}");
        assert!(!msg.contains("select 1"), "valor de campo vazou: {msg}");
        assert!(
            msg.contains("objeto com chaves [password, query]"),
            "a forma do input tem de aparecer, para dizer O QUE repetiu: {msg}"
        );
        let tamanho = input.to_string().len();
        assert!(msg.contains(&format!("({tamanho} bytes)")), "{msg}");

        // Ferramenta conhecida, campo nao allow-listed: o `url` do
        // `web_fetch` sai, o `password` ao lado nao.
        let input = json!({"url": "https://x", "password": "hunter2"});
        let msg = budget.mensagem_de_loop("web_fetch", &input);
        assert!(msg.contains("https://x"), "{msg}");
        assert!(
            !msg.contains("hunter2"),
            "campo fora da allow-list vazou: {msg}"
        );
    }

    /// Input que nem objeto e (string, array) tambem nao sai: so o tipo e o
    /// tamanho.
    #[test]
    fn resumo_do_input_sem_objeto_mostra_so_tipo_e_tamanho() {
        assert_eq!(
            super::resumo_do_input("eco", &json!("segredo em texto puro")),
            "string (23 bytes)"
        );
        assert_eq!(
            super::resumo_do_input("eco", &json!(["a", "b"])),
            "array (9 bytes)"
        );
        assert_eq!(
            super::resumo_do_input("eco", &json!({})),
            "objeto com chaves [] (2 bytes)"
        );
    }

    /// Input grande nao vira dump: o campo allow-listed sai pelo `sanear` do
    /// `turn_events`, que trunca em 72 chars sem partir UTF-8 ao meio.
    #[test]
    fn resumo_do_input_trunca_o_campo_allow_listed() {
        let longo = json!({"command": "x".repeat(500)});
        let resumo = super::resumo_do_input("bash", &longo);
        assert!(resumo.ends_with('…'), "{resumo}");
        assert!(resumo.chars().count() <= 72, "{}", resumo.chars().count());

        let multibyte = json!({"command": "ção".repeat(200)});
        let resumo = super::resumo_do_input("bash", &multibyte);
        assert!(resumo.chars().count() <= 72, "{}", resumo.chars().count());
        assert!(resumo.starts_with("ção"), "{resumo}");
    }

    /// O erro vai para o log, o ledger e a CLI: segredo COM formato no campo
    /// allow-listed sai redigido, e a redacao vem antes do corte — o corte
    /// nao pode deixar um prefixo de chave passar pelo regex.
    #[test]
    fn mensagem_de_loop_redige_segredo_do_input() {
        let chave = format!("sk-ant-api03-{}", "a".repeat(40));
        let mut budget = ExecutionBudget::padrao();
        let input =
            json!({"command": format!("curl -H 'Authorization: Bearer {chave}' https://x")});
        for _ in 0..3 {
            budget.registrar_chamada("bash", &input);
        }

        let msg = budget.mensagem_de_loop("bash", &input);
        assert!(!msg.contains(&chave), "segredo sobreviveu: {msg}");
        assert!(msg.contains("[REDACTED]"), "{msg}");

        // Mesmo com a chave comecando antes do corte de 72 e terminando
        // depois: truncar primeiro deixaria 50 chars dela passarem crus.
        let input = json!({"command": format!("{} {chave}", "p".repeat(20))});
        let msg = budget.mensagem_de_loop("bash", &input);
        assert!(!msg.contains(&chave[..30]), "prefixo da chave vazou: {msg}");
        assert!(msg.contains("[REDACTED]"), "{msg}");
    }

    // ── #1295 item 1: um aviso corretivo, depois aborta ──────────────────

    fn tres_iguais(budget: &mut ExecutionBudget, input: &serde_json::Value) {
        for _ in 0..3 {
            budget.registrar_chamada("file_read", input);
        }
    }

    #[test]
    fn primeira_deteccao_avisa_com_tool_e_input_redigido() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"path": "/tmp/alvo"});
        budget.registrar_chamada("file_read", &input);
        assert_eq!(budget.veredito_de_loop("file_read", &input), None);
        budget.registrar_chamada("file_read", &input);
        assert_eq!(budget.veredito_de_loop("file_read", &input), None);
        budget.registrar_chamada("file_read", &input);
        let Some(super::VereditoDeLoop::Avisar(msg)) = budget.veredito_de_loop("file_read", &input)
        else {
            panic!("a primeira deteccao avisa");
        };
        assert!(msg.contains("file_read"), "{msg}");
        assert!(msg.contains("input repetido: /tmp/alvo"), "{msg}");
        assert!(msg.contains("NAO foi executada"), "{msg}");
        assert!(msg.contains("mude de abordagem"), "{msg}");
    }

    #[test]
    fn repeticao_depois_do_aviso_aborta() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"path": "/tmp/alvo"});
        tres_iguais(&mut budget, &input);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &input),
            Some(super::VereditoDeLoop::Avisar(_))
        ));
        budget.registrar_chamada("file_read", &input);
        let Some(super::VereditoDeLoop::Abortar(msg)) =
            budget.veredito_de_loop("file_read", &input)
        else {
            panic!("a repeticao depois do aviso aborta");
        };
        assert!(msg.starts_with("tool loop detected: file_read"), "{msg}");
        assert!(msg.contains("3 chamadas identicas"), "{msg}");
    }

    /// Um aviso por tarefa: um loop DIFERENTE depois do aviso tambem aborta.
    #[test]
    fn outro_loop_depois_do_aviso_aborta() {
        let mut budget = ExecutionBudget::padrao();
        let a = json!({"path": "/a"});
        tres_iguais(&mut budget, &a);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &a),
            Some(super::VereditoDeLoop::Avisar(_))
        ));
        let b = json!({"path": "/b"});
        for _ in 0..2 {
            budget.registrar_chamada("file_read", &b);
            assert_eq!(budget.veredito_de_loop("file_read", &b), None);
        }
        budget.registrar_chamada("file_read", &b);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &b),
            Some(super::VereditoDeLoop::Abortar(_))
        ));
    }

    /// Revisao da onda A: uma chamada diferente no meio nao libera a
    /// chamada avisada. `X, X, X` (aviso), `Y`, `X` aborta no segundo `X`
    /// depois do aviso, sem esperar a janela fechar de novo.
    #[test]
    fn chamada_avisada_aborta_mesmo_com_outra_chamada_no_meio() {
        let mut budget = ExecutionBudget::padrao();
        let x = json!({"path": "/x"});
        let y = json!({"path": "/y"});
        tres_iguais(&mut budget, &x);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &x),
            Some(super::VereditoDeLoop::Avisar(_))
        ));
        budget.registrar_chamada("file_read", &y);
        assert_eq!(budget.veredito_de_loop("file_read", &y), None);
        budget.registrar_chamada("file_read", &x);
        let Some(super::VereditoDeLoop::Abortar(msg)) = budget.veredito_de_loop("file_read", &x)
        else {
            panic!("a chamada avisada aborta mesmo com outra no meio");
        };
        assert!(msg.starts_with("tool loop detected: file_read"), "{msg}");
        assert!(msg.contains("depois do aviso"), "{msg}");
        assert!(msg.contains("input repetido: /x"), "{msg}");
    }

    /// Nem o reset do teto por turno libera a chamada avisada; so a tarefa
    /// nova. A mesma ferramenta com OUTRO input segue livre.
    #[test]
    fn assinatura_avisada_sobrevive_ao_reset_de_turno_e_so_ela_aborta() {
        let mut budget = ExecutionBudget::padrao();
        let x = json!({"path": "/x"});
        tres_iguais(&mut budget, &x);
        let _ = budget.veredito_de_loop("file_read", &x);
        budget.resetar_turno();
        budget.registrar_chamada("file_read", &json!({"path": "/outro"}));
        assert_eq!(
            budget.veredito_de_loop("file_read", &json!({"path": "/outro"})),
            None
        );
        budget.registrar_chamada("file_read", &x);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &x),
            Some(super::VereditoDeLoop::Abortar(_))
        ));

        budget.resetar_tarefa();
        budget.registrar_chamada("file_read", &x);
        assert_eq!(budget.veredito_de_loop("file_read", &x), None);
    }

    /// O reset do teto por turno NAO rearma o aviso; nova tarefa rearma.
    #[test]
    fn resetar_turno_nao_rearma_o_aviso_resetar_tarefa_rearma() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"path": "/tmp/alvo"});
        tres_iguais(&mut budget, &input);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &input),
            Some(super::VereditoDeLoop::Avisar(_))
        ));
        budget.resetar_turno();
        tres_iguais(&mut budget, &input);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &input),
            Some(super::VereditoDeLoop::Abortar(_))
        ));

        budget.resetar_tarefa();
        tres_iguais(&mut budget, &input);
        assert!(matches!(
            budget.veredito_de_loop("file_read", &input),
            Some(super::VereditoDeLoop::Avisar(_))
        ));
    }

    /// A chamada avisada conta no orcamento: nao e uma volta de graca.
    #[test]
    fn chamada_avisada_conta_no_orcamento() {
        let mut budget = ExecutionBudget::padrao();
        let input = json!({"path": "/tmp/alvo"});
        tres_iguais(&mut budget, &input);
        let _ = budget.veredito_de_loop("file_read", &input);
        assert_eq!(budget.chamadas_na_tarefa(), 3);
    }

    /// O aviso tambem nao despeja segredo nem input cru.
    #[test]
    fn aviso_redige_segredo_e_nao_despeja_input_sem_resumo() {
        let chave = format!("sk-ant-api03-{}", "a".repeat(40));
        let mut budget = ExecutionBudget::padrao();
        let input =
            json!({"command": format!("curl -H 'Authorization: Bearer {chave}' https://x")});
        for _ in 0..3 {
            budget.registrar_chamada("bash", &input);
        }
        let Some(super::VereditoDeLoop::Avisar(msg)) = budget.veredito_de_loop("bash", &input)
        else {
            panic!("avisa");
        };
        assert!(!msg.contains(&chave), "{msg}");

        let mut budget = ExecutionBudget::padrao();
        let input = json!({"password": "hunter2", "query": "x"});
        for _ in 0..3 {
            budget.registrar_chamada("sem_resumo", &input);
        }
        let Some(super::VereditoDeLoop::Avisar(msg)) =
            budget.veredito_de_loop("sem_resumo", &input)
        else {
            panic!("avisa");
        };
        assert!(!msg.contains("hunter2"), "{msg}");
    }
}
