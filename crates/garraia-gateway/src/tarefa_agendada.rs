//! A autoridade com que uma tarefa agendada roda (#1985).
//!
//! Regra: a tarefa roda com a autoridade de quem a criou, **gravada na
//! criacao** — agendar nao eleva privilegio e tambem nao reduz. O runtime
//! publica as camadas de teto do turno para as tools de agenda, que as gravam
//! em `scheduled_tasks.authority`; aqui o executor as le de volta.
//!
//! Tres casos, e nenhum deles e "portao aberto por omissao":
//!
//! - **gravada**: valem as camadas gravadas. A superficie que a sessao tem
//!   HOJE entra por cima, como camada que so tira, salvo nos dois casos em
//!   que ela nao diz nada de novo: o canal que resolve a identidade de quem
//!   fala (WhatsApp vinculado — ali quem decide e a politica vigente, abaixo)
//!   e a tarefa que o operador reautorizou de proposito;
//! - **ausente** (tarefa anterior a coluna, ou criada fora de um turno): vale
//!   a superficie da sessao, como antes — operador sem teto, remota com teto
//!   de leitura — ate o operador reautorizar (`garraia tasks authorize`);
//! - **invalida** (JSON que este binario nao entende): teto de leitura.
//!
//! Por cima de qualquer um dos tres vai a **politica vigente** do canal para
//! quem criou a tarefa. So o WhatsApp vinculado tem principal com politica
//! propria: rebaixar ou bloquear um contato vale tambem para as tarefas que
//! ele ja tinha agendado, e nem a reautorizacao do operador passa por cima.
//!
//! # Por que a superficie de hoje ainda conta
//!
//! A tarefa roda com o HISTORICO da sessao. Numa sessao compartilhada (o
//! console do operador e o Telegram na mesma conversa), quem entrou depois
//! pode escrever nesse historico antes de a tarefa disparar. Se a autoridade
//! gravada do operador valesse sozinha, um participante remoto conduziria uma
//! execucao com acesso total. Por isso, fora do WhatsApp vinculado, uma sessao
//! que deixou de ser so do operador derruba a tarefa para o teto de leitura —
//! e o aviso diz isso. No WhatsApp vinculado a sessao E a conversa de quem
//! criou a tarefa, e a identidade dele e reavaliada pela config a cada
//! execucao.
//!
//! E a tarefa diz quando um limite atrapalhou: se um teto barrar o que ela
//! pediu, ou se ela rodar com acesso reduzido, a RESPOSTA dela comeca pelo
//! aviso, com o motivo e o que fazer. A resposta vai para o historico da
//! sessao e para o ledger de runs sempre; chegar ao canal e com
//! [`crate::entrega_agendada`] (#2002), que registra o desfecho no ledger.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use garraia_agents::capacidades::capacidades_nativas;
use garraia_agents::exec_context::ExecContext;
use garraia_agents::modes::{AutoridadeDaTarefa, OrigemDaAutoridade, TetoDeCapacidades, ToolGate};
use garraia_agents::tools::turn_tools::BloqueioDeTeto;

use crate::bootstrap::whatsapp_linked_politica::{
    principal_do_turno, teto_da_superficie_do_principal, teto_do_principal,
};
use crate::bootstrap::{
    WHATSAPP_LINKED_CONFIG_KEY, WhatsAppLinkedSettings, whatsapp_linked_modo_do_piso,
    whatsapp_linked_perfil_do_turno, whatsapp_linked_settings,
};
use crate::state::AppState;

/// O prefixo de `session_id` do WhatsApp vinculado.
const PREFIXO_WHATSAPP_LINKED: &str = "whatsapp-linked-";

/// O comando que o aviso cita. `garraia`, e nao `garra`: o primeiro e o nome
/// do binario instalado; o segundo e so o alias do instalador e nao existe em
/// toda maquina (#1228).
pub(crate) const COMANDO_DE_REAUTORIZACAO: &str = "garraia tasks authorize";

/// As ferramentas que o aviso de "acesso reduzido" sonda: escrever arquivo e
/// executar comando sao o que uma tarefa perde quando cai no teto de leitura.
const SONDAS: [(&str, &str); 2] = [
    ("file_write", "escrita de arquivo"),
    ("bash", "execucao de comandos"),
];

/// O que a coluna `authority` da tarefa diz.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AutoridadeLida {
    /// Em `Box`: o documento carrega politicas inteiras, e as outras duas
    /// variantes nao carregam nada.
    Gravada(Box<AutoridadeDaTarefa>),
    /// `NULL`: tarefa anterior a coluna, ou criada fora de um turno.
    Ausente,
    /// Documento que este binario nao entende. Nunca vira "sem teto".
    Invalida,
}

pub(crate) fn ler(task: &garraia_db::ScheduledTask) -> AutoridadeLida {
    match task.authority.as_deref() {
        None => AutoridadeLida::Ausente,
        Some(doc) => match AutoridadeDaTarefa::de_json(doc) {
            Some(a) => AutoridadeLida::Gravada(Box::new(a)),
            None => AutoridadeLida::Invalida,
        },
    }
}

/// A politica de HOJE do canal para quem criou a tarefa.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Vigente {
    /// Camadas compostas por E com a autoridade da tarefa: so tiram.
    pub tetos: Vec<TetoDeCapacidades>,
    /// O piso de modo do canal, aplicado quando a sessao nao escolheu modo.
    pub piso: Option<String>,
    /// O canal resolve a identidade de quem criou a tarefa a cada execucao
    /// (WhatsApp vinculado): `tetos` ja e a fronteira de hoje, e a superficie
    /// generica da sessao — que so sabe dizer "remota" — nao acrescenta nada.
    pub canal_resolve_identidade: bool,
}

/// A politica vigente do WhatsApp vinculado para `remetente` em `chat`, pela
/// config viva: o MESMO motor do turno (`principal_do_turno`, `modo_do_piso`).
///
/// `pareado` vai `false`: o pareamento vive na memoria do portao do canal, e
/// quem so esta pareado tem alcance de leitura — nunca conseguiu agendar.
/// Quem nao e mais admitido (bloqueado, removido da config) cai no teto de
/// `chat`: a tarefa roda sem ferramenta nenhuma.
pub(crate) fn vigente_do_whatsapp(
    settings: &WhatsAppLinkedSettings,
    perfil: garraia_config::ExecutionProfile,
    remetente: &str,
    chat: &str,
) -> Vigente {
    let e_grupo = chat.ends_with("@g.us");
    let principal = principal_do_turno(settings, remetente, chat, e_grupo, false);
    let piso = whatsapp_linked_modo_do_piso(
        whatsapp_linked_perfil_do_turno(perfil, settings, remetente, e_grupo),
        settings,
    );
    Vigente {
        tetos: [
            teto_do_principal(&principal),
            teto_da_superficie_do_principal(&principal),
        ]
        .into_iter()
        .flatten()
        .collect(),
        piso: Some(piso),
        canal_resolve_identidade: true,
    }
}

/// A politica vigente para esta tarefa. Vazia fora do WhatsApp vinculado: os
/// outros canais nao tem principal com politica propria, e ali a autoridade
/// gravada (ou a superficie da sessao) ja e a fronteira inteira.
pub(crate) fn politica_vigente(state: &AppState, task: &garraia_db::ScheduledTask) -> Vigente {
    let e_whatsapp = task.session_id.starts_with(PREFIXO_WHATSAPP_LINKED)
        || task.channel_id == WHATSAPP_LINKED_CONFIG_KEY;
    if !e_whatsapp {
        return Vigente::default();
    }
    let chat = task
        .session_id
        .strip_prefix(PREFIXO_WHATSAPP_LINKED)
        .unwrap_or(&task.session_id);
    let settings = whatsapp_linked_settings(&state.current_config());
    let perfil = crate::bootstrap::politica_de_execucao(&state.config).perfil;
    vigente_do_whatsapp(&settings, perfil, &task.user_id, chat)
}

/// O contexto de execucao da tarefa. `base` e o que o construtor central deu
/// para a sessao HOJE (modo, diretorio, e as camadas de teto deduzidas da
/// superficie). Pura.
pub(crate) fn contexto(
    base: ExecContext,
    autoridade: &AutoridadeLida,
    vigente: &Vigente,
) -> ExecContext {
    let superficie_de_hoje = base.teto_da_superficie.clone();
    let mut exec = match autoridade {
        AutoridadeLida::Gravada(a) => {
            let mut exec = a.aplicar(base);
            // A sessao deixou de ser so do operador desde que a tarefa foi
            // criada: quem entrou escreve no historico que a tarefa le. Ver
            // "Por que a superficie de hoje ainda conta" no doc do modulo.
            if a.origem == OrigemDaAutoridade::Turno
                && !vigente.canal_resolve_identidade
                && let Some(teto) = superficie_de_hoje
            {
                exec.tetos_vigentes.push(teto);
            }
            exec
        }
        AutoridadeLida::Ausente => base,
        AutoridadeLida::Invalida => base.sem_credencial_de_operador(),
    };
    exec.tetos_vigentes.extend(vigente.tetos.iter().cloned());
    if let Some(piso) = vigente.piso.as_deref() {
        exec = crate::bootstrap::whatsapp_linked_piso_somente_leitura(exec, piso);
    }
    exec
}

/// As tarefas que ja receberam o aviso de "acesso reduzido" neste processo.
/// Uma recorrente que so le nao precisa ouvir o mesmo aviso a cada execucao;
/// uma vez por subida do gateway basta para ninguem descobrir tarde.
static AVISADAS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// `true` na primeira vez que `task_id` passa por aqui neste processo.
fn primeira_vez(task_id: &str) -> bool {
    match AVISADAS.lock() {
        Ok(mut vistas) => vistas.insert(task_id.to_string()),
        // Lock envenenado: avisar de novo e o lado seguro.
        Err(_) => true,
    }
}

/// O que o acesso reduzido tira, em palavras, e o nome do teto que tira.
fn perdas(portao: &ToolGate) -> Option<(Vec<&'static str>, String)> {
    let barradas: Vec<(&'static str, &str)> = SONDAS
        .iter()
        .filter_map(|(tool, rotulo)| {
            portao
                .teto_que_barra(tool, capacidades_nativas(tool))
                .map(|teto| (*rotulo, teto))
        })
        .collect();
    let teto = barradas.first()?.1.to_string();
    Some((
        barradas.into_iter().map(|(rotulo, _)| rotulo).collect(),
        teto,
    ))
}

/// O que fazer, conforme QUEM barrou.
fn remedio(task_id: &str, autoridade: &AutoridadeLida, teto: &str, vigente: &Vigente) -> String {
    if vigente.tetos.iter().any(|t| t.nome == teto) {
        return "Esse e o acesso atual de quem criou a tarefa neste canal; para ampliar, \
                o administrador ajusta a politica de acesso do canal."
            .to_string();
    }
    match autoridade {
        // O limite veio da propria autoridade gravada: e o que quem criou a
        // tarefa tinha.
        AutoridadeLida::Gravada(a)
            if a.teto
                .iter()
                .chain(a.teto_da_superficie.iter())
                .chain(a.tetos_vigentes.iter())
                .any(|t| t.nome == teto) =>
        {
            format!(
                "A tarefa foi criada com esse limite. Se ela deve ir alem, o operador da \
                 maquina pode reautoriza-la com `{COMANDO_DE_REAUTORIZACAO} {task_id}`."
            )
        }
        // Gravada sem esse limite: quem o trouxe foi a sessao de hoje.
        AutoridadeLida::Gravada(_) => format!(
            "A conversa desta tarefa passou a ter um canal remoto (ou deixou de provar o \
             operador) depois que ela foi criada, e quem escreve nela alcanca a tarefa. Se \
             ela deve seguir com o acesso original, o operador da maquina pode reautoriza-la \
             com `{COMANDO_DE_REAUTORIZACAO} {task_id}`."
        ),
        AutoridadeLida::Ausente | AutoridadeLida::Invalida => format!(
            "Ela nao tem autoridade gravada (foi criada antes desse registro). O operador da \
             maquina pode reautoriza-la com `{COMANDO_DE_REAUTORIZACAO} {task_id}`."
        ),
    }
}

/// O aviso que abre a resposta da tarefa, se houver o que avisar. Pura, salvo
/// o "uma vez por processo" do segundo caso (`ja_avisada`).
///
/// 1. O modelo pediu uma ferramenta e um TETO barrou: avisa sempre.
/// 2. A tarefa rodou com acesso reduzido — sem autoridade gravada valida sob
///    um teto que tira escrita ou execucao, ou com a politica vigente tirando
///    o que a autoridade gravada dava. Aqui o modelo nem ve a ferramenta, e
///    portanto nem a pede: sem este caso a tarefa simplesmente "nao faria".
pub(crate) fn aviso(
    task_id: &str,
    autoridade: &AutoridadeLida,
    vigente: &Vigente,
    exec: &ExecContext,
    payload: &str,
    bloqueios: &[BloqueioDeTeto],
    ja_avisada: bool,
) -> Option<String> {
    if let Some(primeiro) = bloqueios.first() {
        let ferramentas = bloqueios
            .iter()
            .map(|b| format!("`{}`", b.ferramenta))
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!(
            "Aviso: esta tarefa agendada tentou usar {ferramentas} e o limite de acesso `{}` \
             barrou. O que dependia disso NAO foi feito. {}",
            primeiro.teto,
            remedio(task_id, autoridade, &primeiro.teto, vigente)
        ));
    }
    if ja_avisada {
        return None;
    }
    let efetivo = ToolGate::para_o_turno(exec, payload);
    let (perdidas, teto) = perdas(&efetivo)?;
    // Com autoridade gravada, o limite que ELA carrega nao e "reducao": e o
    // que quem criou a tarefa tinha. So conta o que a politica vigente tirou.
    if let AutoridadeLida::Gravada(a) = autoridade {
        let gravado = ToolGate::sem_politica()
            .com_teto(a.teto.clone())
            .com_teto_de_superficie(a.teto_da_superficie.clone())
            .com_tetos_vigentes(a.tetos_vigentes.clone());
        if perdas(&gravado).is_some_and(|(p, _)| p == perdidas) {
            return None;
        }
    }
    Some(format!(
        "Aviso: esta tarefa agendada rodou com acesso reduzido (`{teto}`): sem {}. Se ela \
         dependia disso, NAO foi feito. {}",
        perdidas.join(" e sem "),
        remedio(task_id, autoridade, &teto, vigente)
    ))
}

/// [`aviso`] com o controle de "uma vez por processo" do caso 2.
pub(crate) fn aviso_da_execucao(
    task: &garraia_db::ScheduledTask,
    autoridade: &AutoridadeLida,
    vigente: &Vigente,
    exec: &ExecContext,
    bloqueios: &[BloqueioDeTeto],
) -> Option<String> {
    // Consulta sem consumir: so marca a tarefa quando o aviso do caso 2 sai.
    let ja_avisada = AVISADAS
        .lock()
        .map(|vistas| vistas.contains(&task.id))
        .unwrap_or(false);
    let texto = aviso(
        &task.id,
        autoridade,
        vigente,
        exec,
        &task.payload,
        bloqueios,
        ja_avisada,
    )?;
    if bloqueios.is_empty() {
        primeira_vez(&task.id);
    }
    Some(texto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::modes::{Nivel, politica_do_nivel};
    // `OrigemDaAutoridade` vem do `super::*`.

    const DONO: &str = "5511999990000";
    const USUARIO: &str = "5511888880000";
    const ESTRANHO: &str = "5511777770000";

    fn teto(nome: &str, nivel: Nivel, write: bool) -> TetoDeCapacidades {
        TetoDeCapacidades {
            nome: nome.to_string(),
            politica: politica_do_nivel(nivel, write),
        }
    }

    fn permite(exec: &ExecContext, tool: &str) -> bool {
        ToolGate::para_o_turno(exec, "qualquer texto")
            .permite_com_capacidades(tool, capacidades_nativas(tool))
    }

    fn tarefa(authority: Option<&str>) -> garraia_db::ScheduledTask {
        garraia_db::ScheduledTask {
            id: "tarefa-1".to_string(),
            session_id: "sess-1".to_string(),
            channel_id: "telegram".to_string(),
            user_id: "user-1".to_string(),
            execute_at: chrono::Utc::now(),
            payload: "atualiza o documento".to_string(),
            session_metadata: serde_json::json!({}),
            cron_expr: None,
            timezone: None,
            run_count: 0,
            max_runs: None,
            attempts: 0,
            authority: authority.map(str::to_string),
        }
    }

    fn do_turno(
        teto_principal: Option<TetoDeCapacidades>,
        superficie: Option<TetoDeCapacidades>,
    ) -> AutoridadeDaTarefa {
        AutoridadeDaTarefa {
            v: AutoridadeDaTarefa::VERSAO,
            origem: OrigemDaAutoridade::Turno,
            teto: teto_principal,
            teto_da_superficie: superficie,
            tetos_vigentes: Vec::new(),
        }
    }

    /// O contexto que o construtor central da hoje a uma sessao REMOTA.
    fn base_remota() -> ExecContext {
        ExecContext::sem_prova_de_operador()
    }

    /// O contexto que o construtor central da hoje a uma sessao do OPERADOR.
    fn base_do_operador() -> ExecContext {
        ExecContext::default().operador_local()
    }

    /// Politica vigente de um canal que resolve a identidade (WhatsApp), sem
    /// teto nenhum: o caso do dono.
    fn canal_com_identidade() -> Vigente {
        Vigente {
            tetos: Vec::new(),
            piso: None,
            canal_resolve_identidade: true,
        }
    }

    fn settings(valor: serde_json::Value) -> WhatsAppLinkedSettings {
        let mut config = garraia_config::AppConfig::default();
        config.channels.insert(
            WHATSAPP_LINKED_CONFIG_KEY.to_string(),
            garraia_config::ChannelConfig {
                channel_type: WHATSAPP_LINKED_CONFIG_KEY.to_string(),
                enabled: Some(true),
                settings: valor
                    .as_object()
                    .expect("objeto")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
        );
        whatsapp_linked_settings(&config)
    }

    // ---- leitura da coluna -------------------------------------------------

    #[test]
    fn coluna_vazia_e_ausente_e_lixo_e_invalida() {
        assert_eq!(ler(&tarefa(None)), AutoridadeLida::Ausente);
        assert_eq!(ler(&tarefa(Some("nao e json"))), AutoridadeLida::Invalida);
        assert_eq!(
            ler(&tarefa(Some(r#"{"v":9,"origem":"turno"}"#))),
            AutoridadeLida::Invalida,
            "versao desconhecida nunca vira `sem teto`"
        );
        let doc = AutoridadeDaTarefa::do_operador().para_json().expect("json");
        assert_eq!(
            ler(&tarefa(Some(&doc))),
            gravada_com(AutoridadeDaTarefa::do_operador())
        );
    }

    // ---- contexto ----------------------------------------------------------

    /// O caso que motivou a mudanca: o dono do WhatsApp vinculado podia
    /// escrever na conversa, e a sessao dele e sempre classificada como
    /// remota. A tarefa que ele agendou continua escrevendo.
    #[test]
    fn dono_do_whatsapp_nao_perde_a_escrita_na_tarefa_que_agendou() {
        let gravada = gravada_com(do_turno(None, None));
        let exec = contexto(base_remota(), &gravada, &canal_com_identidade());
        assert!(permite(&exec, "file_write"));
        assert!(permite(&exec, "bash"));
        assert_eq!(
            aviso(
                "t",
                &gravada,
                &canal_com_identidade(),
                &exec,
                "x",
                &[],
                false
            ),
            None
        );
    }

    /// Tarefa do operador em sessao que segue so do operador: nada muda.
    #[test]
    fn tarefa_do_operador_em_sessao_do_operador_roda_inteira() {
        let gravada = gravada_com(do_turno(None, None));
        let exec = contexto(base_do_operador(), &gravada, &Vigente::default());
        assert!(permite(&exec, "file_write"));
        assert!(permite(&exec, "bash"));
    }

    /// O furo da sessao compartilhada: o operador agendou, e depois a sessao
    /// ganhou um canal remoto. Quem escreve nela alcanca o historico que a
    /// tarefa le — entao a tarefa cai para leitura, e o aviso diz por que e
    /// como o operador a reautoriza.
    #[test]
    fn sessao_que_ganhou_canal_remoto_derruba_a_tarefa_do_operador() {
        let gravada = gravada_com(do_turno(None, None));
        let vigente = Vigente::default();
        let exec = contexto(base_remota(), &gravada, &vigente);
        assert!(!permite(&exec, "file_write"));
        assert!(!permite(&exec, "bash"));
        assert!(permite(&exec, "file_read"));

        let texto = aviso(
            "t-compartilhada",
            &gravada,
            &vigente,
            &exec,
            "x",
            &[],
            false,
        )
        .expect("a reducao avisa");
        assert!(texto.contains("passou a ter um canal remoto"), "{texto}");
        assert!(
            texto.contains("garraia tasks authorize t-compartilhada"),
            "{texto}"
        );
    }

    /// E o inverso: quem criou a tarefa sob um teto nao ganha nada porque a
    /// sessao hoje e do operador. Agendar nao eleva.
    #[test]
    fn autoridade_gravada_nao_e_ampliada_pela_superficie_de_hoje() {
        let gravada = gravada_com(do_turno(
            Some(teto("usuario (full, write off)", Nivel::Full, false)),
            None,
        ));
        let exec = contexto(base_do_operador(), &gravada, &Vigente::default());
        assert!(!permite(&exec, "file_write"), "criada sem escrita");
        assert!(permite(&exec, "bash"), "o resto do `full` segue valendo");
    }

    /// Tarefa antiga: vale a superficie da sessao, nos dois sentidos.
    #[test]
    fn sem_autoridade_gravada_vale_a_superficie_da_sessao() {
        let remota = contexto(base_remota(), &AutoridadeLida::Ausente, &Vigente::default());
        assert!(!permite(&remota, "file_write"));
        assert!(!permite(&remota, "bash"));
        assert!(permite(&remota, "file_read"));

        let do_operador = contexto(
            ExecContext::default().operador_local(),
            &AutoridadeLida::Ausente,
            &Vigente::default(),
        );
        assert!(
            permite(&do_operador, "file_write"),
            "operador segue sem teto"
        );
    }

    /// Documento ilegivel fecha, mesmo numa sessao do operador.
    #[test]
    fn autoridade_invalida_cai_no_teto_de_leitura() {
        let exec = contexto(
            ExecContext::default().operador_local(),
            &AutoridadeLida::Invalida,
            &Vigente::default(),
        );
        assert!(!permite(&exec, "file_write"));
        assert!(permite(&exec, "file_read"));
    }

    /// A reautorizacao do operador abre a tarefa antiga de sessao remota — e a
    /// politica vigente continua por cima dela.
    #[test]
    fn reautorizacao_abre_mas_nao_passa_da_politica_vigente() {
        let reautorizada = gravada_com(AutoridadeDaTarefa::do_operador());
        let exec = contexto(base_remota(), &reautorizada, &Vigente::default());
        assert!(permite(&exec, "file_write"));

        let rebaixado = Vigente {
            tetos: vec![teto("usuario (read)", Nivel::Read, false)],
            piso: None,
            canal_resolve_identidade: true,
        };
        let exec = contexto(base_remota(), &reautorizada, &rebaixado);
        assert!(!permite(&exec, "file_write"));
    }

    // ---- politica vigente do WhatsApp --------------------------------------

    /// O dono segue sem teto; um usuario rebaixado para leitura perde a
    /// escrita nas tarefas que ja tinha; quem saiu da config nao roda nada.
    #[test]
    fn politica_vigente_do_whatsapp_segue_a_config_de_hoje() {
        let s = settings(serde_json::json!({
            "owners": [DONO],
            "access": { "users": { USUARIO: { "level": "read" } } }
        }));
        let standard = garraia_config::ExecutionProfile::Standard;
        let jid = |n: &str| format!("{n}@s.whatsapp.net");
        let criada_com_full = gravada_com(do_turno(None, None));

        let dono = vigente_do_whatsapp(&s, standard, DONO, &jid(DONO));
        assert!(dono.tetos.is_empty(), "dono nao tem teto: {:?}", dono.tetos);

        let rebaixado = vigente_do_whatsapp(&s, standard, USUARIO, &jid(USUARIO));
        let exec = contexto(base_remota(), &criada_com_full, &rebaixado);
        assert!(!permite(&exec, "file_write"), "rebaixado para leitura");
        assert!(!permite(&exec, "bash"));

        let removido = vigente_do_whatsapp(&s, standard, ESTRANHO, &jid(ESTRANHO));
        let exec = contexto(base_remota(), &criada_com_full, &removido);
        assert!(
            !permite(&exec, "file_read"),
            "quem nao e mais admitido nao roda ferramenta nenhuma"
        );
    }

    /// O piso de modo do canal vale na tarefa como vale no turno: sem modo
    /// escolhido na sessao, o contexto ganha o modo do piso.
    #[test]
    fn piso_do_canal_entra_quando_a_sessao_nao_escolheu_modo() {
        let s = settings(serde_json::json!({ "owners": [DONO] }));
        let vigente = vigente_do_whatsapp(
            &s,
            garraia_config::ExecutionProfile::Standard,
            DONO,
            &format!("{DONO}@s.whatsapp.net"),
        );
        let piso = vigente.piso.clone().expect("o canal tem piso");
        let exec = contexto(base_remota(), &gravada_com(do_turno(None, None)), &vigente);
        assert_eq!(exec.agent_mode.as_deref(), Some(piso.as_str()));

        // Modo escolhido na sessao vence o piso.
        let escolhido =
            ExecContext::with_mode(Some("code".to_string())).sem_credencial_de_operador();
        let exec = contexto(escolhido, &gravada_com(do_turno(None, None)), &vigente);
        assert_eq!(exec.agent_mode.as_deref(), Some("code"));
    }

    // ---- aviso -------------------------------------------------------------

    fn gravada_com(a: AutoridadeDaTarefa) -> AutoridadeLida {
        AutoridadeLida::Gravada(Box::new(a))
    }

    fn bloqueio(ferramenta: &str, teto: &str) -> BloqueioDeTeto {
        BloqueioDeTeto {
            ferramenta: ferramenta.to_string(),
            teto: teto.to_string(),
        }
    }

    /// Tarefa sem restricao nenhuma nao avisa nada.
    #[test]
    fn tarefa_sem_restricao_nao_avisa() {
        let gravada = gravada_com(do_turno(None, None));
        let exec = contexto(base_do_operador(), &gravada, &Vigente::default());
        assert_eq!(
            aviso("t", &gravada, &Vigente::default(), &exec, "x", &[], false),
            None
        );
    }

    /// O teto barrou o que o modelo pediu: avisa SEMPRE, nomeia a ferramenta e
    /// o teto, e diz como reautorizar.
    #[test]
    fn bloqueio_explicito_avisa_sempre_com_ferramenta_teto_e_remedio() {
        let vigente = Vigente::default();
        let exec = contexto(base_remota(), &AutoridadeLida::Ausente, &vigente);
        let bloqueios = [bloqueio(
            "file_write",
            "sem prova de operador (fail-closed)",
        )];
        for ja_avisada in [false, true] {
            let texto = aviso(
                "tarefa-9",
                &AutoridadeLida::Ausente,
                &vigente,
                &exec,
                "x",
                &bloqueios,
                ja_avisada,
            )
            .expect("bloqueio explicito sempre avisa");
            assert!(texto.contains("`file_write`"), "{texto}");
            assert!(texto.contains("sem prova de operador"), "{texto}");
            assert!(texto.contains("NAO foi feito"), "{texto}");
            assert!(
                texto.contains("garraia tasks authorize tarefa-9"),
                "{texto}"
            );
        }
    }

    /// Tarefa antiga de sessao remota: o modelo nem ve `file_write`, entao nao
    /// ha bloqueio a coletar — e mesmo assim a primeira execucao avisa.
    #[test]
    fn tarefa_antiga_restrita_avisa_uma_vez_mesmo_sem_bloqueio() {
        let vigente = Vigente::default();
        let exec = contexto(base_remota(), &AutoridadeLida::Ausente, &vigente);
        let texto = aviso(
            "t-antiga",
            &AutoridadeLida::Ausente,
            &vigente,
            &exec,
            "x",
            &[],
            false,
        )
        .expect("acesso reduzido avisa");
        assert!(texto.contains("acesso reduzido"), "{texto}");
        assert!(texto.contains("escrita de arquivo"), "{texto}");
        assert!(texto.contains("execucao de comandos"), "{texto}");
        assert!(
            texto.contains("garraia tasks authorize t-antiga"),
            "{texto}"
        );

        assert_eq!(
            aviso(
                "t-antiga",
                &AutoridadeLida::Ausente,
                &vigente,
                &exec,
                "x",
                &[],
                true
            ),
            None,
            "ja avisada: a recorrente que so le nao ouve de novo"
        );
    }

    /// Tarefa antiga de sessao do OPERADOR nao e restrita: nao avisa.
    #[test]
    fn tarefa_antiga_do_operador_nao_avisa() {
        let vigente = Vigente::default();
        let exec = contexto(
            ExecContext::default().operador_local(),
            &AutoridadeLida::Ausente,
            &vigente,
        );
        assert_eq!(
            aviso(
                "t",
                &AutoridadeLida::Ausente,
                &vigente,
                &exec,
                "x",
                &[],
                false
            ),
            None
        );
    }

    /// O limite que a propria autoridade gravada carrega nao e "reducao": quem
    /// criou a tarefa sem escrita nao recebe aviso por nao escrever.
    #[test]
    fn limite_da_propria_autoridade_gravada_nao_e_reducao() {
        let gravada = gravada_com(do_turno(
            Some(teto("usuario (full, write off)", Nivel::Full, false)),
            None,
        ));
        let vigente = Vigente::default();
        let exec = contexto(base_do_operador(), &gravada, &vigente);
        assert_eq!(aviso("t", &gravada, &vigente, &exec, "x", &[], false), None);

        // E se o modelo pedir a ferramenta mesmo assim, o remedio diz que o
        // limite e o da propria criacao.
        let texto = aviso(
            "t",
            &gravada,
            &vigente,
            &exec,
            "x",
            &[bloqueio("file_write", "usuario (full, write off)")],
            false,
        )
        .expect("bloqueio explicito avisa");
        assert!(texto.contains("foi criada com esse limite"), "{texto}");
    }

    /// A politica vigente tirou o que a autoridade gravada dava: avisa, e o
    /// remedio aponta para a politica do canal — reautorizar nao resolveria.
    #[test]
    fn reducao_pela_politica_vigente_avisa_e_aponta_para_o_canal() {
        let gravada = gravada_com(do_turno(None, None));
        let vigente = Vigente {
            tetos: vec![teto("usuario (read)", Nivel::Read, false)],
            piso: None,
            canal_resolve_identidade: true,
        };
        let exec = contexto(base_remota(), &gravada, &vigente);
        let texto = aviso("t", &gravada, &vigente, &exec, "x", &[], false)
            .expect("a politica vigente reduziu");
        assert!(texto.contains("`usuario (read)`"), "{texto}");
        assert!(texto.contains("politica de acesso do canal"), "{texto}");
        assert!(
            !texto.contains(COMANDO_DE_REAUTORIZACAO),
            "reautorizar nao passa da politica vigente: {texto}"
        );
    }

    /// O "uma vez por processo" e por tarefa, e bloqueio explicito nao o gasta.
    #[test]
    fn aviso_de_acesso_reduzido_sai_uma_vez_por_tarefa() {
        let vigente = Vigente::default();
        let exec = contexto(base_remota(), &AutoridadeLida::Ausente, &vigente);
        let mut t = tarefa(None);
        t.id = format!("uma-vez-{}", uuid::Uuid::new_v4());

        let explicito = [bloqueio("bash", "sem prova de operador (fail-closed)")];
        assert!(
            aviso_da_execucao(&t, &AutoridadeLida::Ausente, &vigente, &exec, &explicito).is_some()
        );
        assert!(
            aviso_da_execucao(&t, &AutoridadeLida::Ausente, &vigente, &exec, &[]).is_some(),
            "o bloqueio explicito nao consumiu o aviso de acesso reduzido"
        );
        assert_eq!(
            aviso_da_execucao(&t, &AutoridadeLida::Ausente, &vigente, &exec, &[]),
            None,
            "segunda execucao sem bloqueio: ja avisada"
        );
        assert!(
            aviso_da_execucao(&t, &AutoridadeLida::Ausente, &vigente, &exec, &explicito).is_some(),
            "bloqueio explicito avisa sempre"
        );
    }
}
