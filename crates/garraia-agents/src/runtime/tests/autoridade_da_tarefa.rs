//! Autoridade das tarefas agendadas (#1985), pelo caminho de PRODUCAO do
//! runtime (provider roteirizado + ferramentas de mentira).
//!
//! A tarefa agendada roda com a autoridade de quem a criou, gravada na
//! criacao. O runtime tem duas pontas nisso:
//!
//! - publica as camadas de teto do turno em volta do `execute` das tools de
//!   agenda (e so delas), para a tool gravar junto com a tarefa;
//! - avisa quem abriu o coletor quando um TETO barra uma ferramenta, para a
//!   execucao agendada nao falhar em silencio.
//!
//! E a terceira camada do portao — a politica VIGENTE — so tira: barra o que a
//! autoridade gravada permitia, e nunca libera o que ela nega.

use super::fronteira_wrappers::{PedeUmaVez, runtime_com_tool_destrutiva};
use super::*;
use crate::modes::{
    AutoridadeDaTarefa, Nivel, OrigemDaAutoridade, TetoDeCapacidades, politica_do_nivel,
};
use crate::tools::turn_tools::{BloqueioDeTeto, autoridade_do_turno, coletando_bloqueios_de_teto};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::Ordering::SeqCst;

/// Guarda o que `autoridade_do_turno()` devolve dentro do `execute`.
type Vista = Arc<StdMutex<Option<Option<AutoridadeDaTarefa>>>>;

/// Tool de mentira que registra a autoridade que enxergou. O nome decide a
/// classe (`capacidades_nativas`): `schedule_heartbeat` e agenda, `bash` nao.
struct EspiaAutoridade {
    nome: &'static str,
    vista: Vista,
}

#[async_trait]
impl Tool for EspiaAutoridade {
    fn name(&self) -> &str {
        self.nome
    }
    fn description(&self) -> &str {
        "stub que registra a autoridade do turno"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        *self.vista.lock().expect("lock do teste") = Some(autoridade_do_turno());
        Ok(ToolOutput::success("ok"))
    }
}

fn runtime_que_espia(nome: &'static str) -> (AgentRuntime, Vista) {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(PedeUmaVez { alvo: nome }));
    let vista: Vista = Arc::new(StdMutex::new(None));
    rt.register_tool(Box::new(EspiaAutoridade {
        nome,
        vista: Arc::clone(&vista),
    }));
    (rt, vista)
}

fn teto(nome: &str, nivel: Nivel, write: bool) -> TetoDeCapacidades {
    TetoDeCapacidades {
        nome: nome.to_string(),
        politica: politica_do_nivel(nivel, write),
    }
}

/// A tool de agenda enxerga as DUAS camadas de teto do turno que a chamou —
/// e o que ela grava na tarefa. `full` sem escrita deixa agendar e nega
/// `file_write`: a tarefa criada por esse principal nao pode escrever depois.
#[tokio::test]
async fn tool_de_agenda_enxerga_as_camadas_de_teto_do_turno() {
    let (rt, vista) = runtime_que_espia("schedule_heartbeat");
    let mut exec = ExecContext::default().operador_local();
    exec.teto = Some(teto("usuario (full, write off)", Nivel::Full, false));

    rt.process_message_with_exec("sessao-autoridade-1", "agenda", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    let autoridade = vista
        .lock()
        .expect("lock do teste")
        .clone()
        .expect("a tool de agenda rodou")
        .expect("o runtime publicou a autoridade para a tool de agenda");
    assert_eq!(autoridade.v, AutoridadeDaTarefa::VERSAO);
    assert_eq!(autoridade.origem, OrigemDaAutoridade::Turno);
    assert_eq!(autoridade.teto, exec.teto);
    assert_eq!(autoridade.teto_da_superficie, None);
    assert!(autoridade.tetos_vigentes.is_empty());
}

/// O operador sem teto nenhum grava "sem teto" — explicitamente, com origem
/// `turno`. Nao e o mesmo que tarefa sem autoridade gravada.
#[tokio::test]
async fn operador_sem_teto_grava_autoridade_sem_teto() {
    let (rt, vista) = runtime_que_espia("schedule_heartbeat");
    let exec = ExecContext::default().operador_local();

    rt.process_message_with_exec("sessao-autoridade-2", "agenda", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    let autoridade = vista
        .lock()
        .expect("lock do teste")
        .clone()
        .expect("a tool de agenda rodou")
        .expect("publicada");
    assert_eq!(autoridade.teto, None);
    assert_eq!(autoridade.teto_da_superficie, None);
}

/// Tool que nao agenda nao recebe a autoridade: o escopo e so das tools com
/// a classe de agenda.
#[tokio::test]
async fn tool_que_nao_agenda_nao_recebe_a_autoridade() {
    let (rt, vista) = runtime_que_espia("bash");
    let exec = ExecContext::default().operador_local();

    rt.process_message_with_exec("sessao-autoridade-3", "roda", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(
        vista.lock().expect("lock do teste").clone(),
        Some(None),
        "`bash` rodou e nao enxergou autoridade nenhuma"
    );
}

/// Quando um TETO barra a ferramenta que o modelo pediu, quem abriu o coletor
/// fica sabendo qual ferramenta e qual teto. E o que alimenta o aviso da
/// tarefa agendada bloqueada.
#[tokio::test]
async fn bloqueio_de_teto_chega_ao_coletor() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let exec = ExecContext::sem_prova_de_operador();

    let (resposta, bloqueios) = coletando_bloqueios_de_teto(rt.process_heartbeat_with_exec(
        "sessao-autoridade-4",
        "executa",
        &[],
        None,
        None,
        &exec,
    ))
    .await;
    resposta.expect("o turno termina");

    assert_eq!(executou.load(SeqCst), 0);
    assert_eq!(
        bloqueios,
        vec![BloqueioDeTeto {
            ferramenta: "bash".to_string(),
            teto: "sem prova de operador (fail-closed)".to_string(),
        }]
    );
}

/// Recusa so de MODO nao e bloqueio de teto: ali trocar de modo resolve, e o
/// aviso de tarefa bloqueada nao e sobre isso. E ferramenta que roda nao
/// registra nada.
#[tokio::test]
async fn recusa_de_modo_e_execucao_normal_nao_viram_bloqueio() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let so_modo = ExecContext::with_mode(Some("ask".to_string())).operador_local();
    let (resposta, bloqueios) = coletando_bloqueios_de_teto(rt.process_heartbeat_with_exec(
        "sessao-autoridade-5",
        "executa",
        &[],
        None,
        None,
        &so_modo,
    ))
    .await;
    resposta.expect("o turno termina");
    assert_eq!(executou.load(SeqCst), 0, "o modo `ask` nega `bash`");
    assert!(
        bloqueios.is_empty(),
        "quem barrou foi o modo: {bloqueios:?}"
    );

    let livre = ExecContext::default().operador_local();
    let (resposta, bloqueios) = coletando_bloqueios_de_teto(rt.process_heartbeat_with_exec(
        "sessao-autoridade-6",
        "executa",
        &[],
        None,
        None,
        &livre,
    ))
    .await;
    resposta.expect("o turno termina");
    assert_eq!(executou.load(SeqCst), 1);
    assert!(bloqueios.is_empty());
}

/// A camada VIGENTE so tira: com a autoridade do operador gravada a tarefa
/// roda a ferramenta; com a politica de hoje rebaixada para leitura por cima,
/// a mesma tarefa nao roda — e o bloqueio cita a camada vigente.
#[tokio::test]
async fn camada_vigente_barra_o_que_a_autoridade_gravada_permitia() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let gravada = AutoridadeDaTarefa::do_operador();

    let exec = gravada.aplicar(ExecContext::sem_prova_de_operador());
    assert_eq!(
        exec.teto_da_superficie, None,
        "a autoridade gravada substitui a camada deduzida da sessao"
    );
    rt.process_heartbeat_with_exec("sessao-autoridade-7", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");
    assert_eq!(executou.load(SeqCst), 1, "autoridade do operador: roda");

    let mut rebaixada = gravada.aplicar(ExecContext::sem_prova_de_operador());
    rebaixada
        .tetos_vigentes
        .push(teto("usuario (read)", Nivel::Read, false));
    let (resposta, bloqueios) = coletando_bloqueios_de_teto(rt.process_heartbeat_with_exec(
        "sessao-autoridade-8",
        "executa",
        &[],
        None,
        None,
        &rebaixada,
    ))
    .await;
    resposta.expect("o turno termina");
    assert_eq!(
        executou.load(SeqCst),
        1,
        "a politica vigente barrou a segunda"
    );
    assert_eq!(
        bloqueios,
        vec![BloqueioDeTeto {
            ferramenta: "bash".to_string(),
            teto: "usuario (read)".to_string(),
        }]
    );
}

/// A camada vigente nunca AMPLIA: autoridade gravada de leitura com uma
/// politica vigente `full` por cima continua sem rodar a ferramenta.
#[tokio::test]
async fn camada_vigente_nao_amplia_a_autoridade_gravada() {
    let (rt, executou) = runtime_com_tool_destrutiva();
    let gravada = AutoridadeDaTarefa {
        v: AutoridadeDaTarefa::VERSAO,
        origem: OrigemDaAutoridade::Turno,
        teto: Some(teto("usuario (read)", Nivel::Read, false)),
        teto_da_superficie: None,
        tetos_vigentes: Vec::new(),
    };
    let mut exec = gravada.aplicar(ExecContext::default());
    exec.tetos_vigentes
        .push(teto("usuario (full)", Nivel::Full, true));

    rt.process_heartbeat_with_exec("sessao-autoridade-9", "executa", &[], None, None, &exec)
        .await
        .expect("o turno termina");

    assert_eq!(executou.load(SeqCst), 0);
}
