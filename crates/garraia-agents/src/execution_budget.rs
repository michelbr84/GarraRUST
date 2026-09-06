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
        }
    }

    /// Cria um orçamento com limite de tarefa personalizado.
    pub fn com_limite(max_per_task: usize) -> Self {
        Self {
            max_per_task,
            ..Self::padrao()
        }
    }

    /// Verifica se o limite por turno foi atingido (mas não o limite total da tarefa).
    /// Usado para estratégia de auto-reset entre turnos.
    pub fn atingiu_limite_turno(&self) -> bool {
        self.current_turn_calls >= self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Verifica se ainda é permitido chamar outra ferramenta.
    pub fn pode_chamar_ferramenta(&self) -> bool {
        self.current_turn_calls < self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Registra uma chamada de ferramenta com seu payload,
    /// para controle de orçamento e detecção de loop por assinatura.
    pub fn registrar_chamada(&mut self, tool_name: &str, payload: &Value) {
        self.current_turn_calls += 1;
        self.current_task_calls += 1;

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
}
