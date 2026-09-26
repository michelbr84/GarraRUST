//! O projeto ativo de uma sessao remota (#1379, #1424): selecionar, listar,
//! limpar, **persistir** e **restaurar** — e a politica de comandos de
//! barra por principal no WhatsApp.
//!
//! Uma sessao de canal (WhatsApp, Telegram) nasce com `working_dir = None`:
//! as file tools nao sabem de que projeto a pessoa fala, e `repo_search`
//! recusa. O `POST /api/sessions` ja vinculava sessao a projeto — mas so em
//! memoria, e so para a API. Aqui o vinculo passa a viver no `sessions.db`
//! (`sessions.project_id`, coluna da Fase 2.1) e volta na hidratacao: um
//! `garraia restart` nao apaga o que a pessoa escolheu.
//!
//! O que NAO muda de poder: selecionar projeto da um `working_dir`; o que se
//! pode fazer nele continua sendo o portao do turno (modo ∧ teto do
//! principal, ADR 0025). Um `chat` com projeto selecionado continua sem
//! ferramenta nenhuma. E o caminho do projeto e confinado pelas raizes de
//! projeto do operador (`project_root::allowed_roots`, `GARRAIA_PROJECT_ROOTS`)
//! **de novo** na selecao e na restauracao: um projeto que ficou fora das
//! raizes depois de criado nao volta.
//!
//! Comandos de barra no WhatsApp: ate aqui `/mode` e `/project` de um
//! remetente iam para o modelo como texto. Agora [`decidir_comando`] diz, por
//! principal, o que e roteado ao registro de comandos e o que e recusado com
//! motivo — `/mode` e do dono (o nivel dos demais vem da politica de acesso);
//! `/project` e de dono e usuario; `/help` de todo admitido; grupo, pareado e
//! desconhecido nao selecionam projeto.

use std::path::PathBuf;

use crate::bootstrap::whatsapp_linked_politica::Principal;
use crate::project_root::{self, ProjectPathError};
use crate::state::AppState;
use tracing::warn;

/// O projeto ativo, como sai para tela e log: nome e id, nunca o caminho.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjetoAtivo {
    pub id: String,
    pub nome: String,
}

impl ProjetoAtivo {
    /// `nome (id curto)`.
    pub fn rotulo(&self) -> String {
        format!("{} ({})", self.nome, &self.id[..self.id.len().min(8)])
    }
}

/// O que [`selecionar`] fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selecao {
    Selecionado(ProjetoAtivo),
    /// Ja era o ativo desta sessao: nada gravado.
    JaEra(ProjetoAtivo),
}

/// Por que nao selecionou. O `Display` e a frase para a pessoa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroDeSelecao {
    /// Sem `sessions.db` nao ha projeto persistido nem lista.
    SemBanco,
    NaoEncontrado,
    /// Mais de um projeto com esse nome: use o id.
    Ambiguo(Vec<String>),
    /// O caminho do projeto nao passa nas raizes de projeto do operador.
    CaminhoRecusado(ProjectPathError),
    Banco(String),
}

impl std::fmt::Display for ErroDeSelecao {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SemBanco => write!(
                f,
                "este Garra esta sem banco de sessoes: nao ha projetos para selecionar"
            ),
            Self::NaoEncontrado => write!(
                f,
                "nenhum projeto com esse nome ou id. `/project list` mostra os cadastrados"
            ),
            Self::Ambiguo(rotulos) => write!(
                f,
                "mais de um projeto com esse nome ({}): use o id",
                rotulos.join(", ")
            ),
            Self::CaminhoRecusado(_) => write!(
                f,
                "o caminho desse projeto nao esta nas raizes de projeto deste Garra \
                 (`GARRAIA_PROJECT_ROOTS`); o operador precisa revisar o cadastro"
            ),
            Self::Banco(e) => write!(f, "falha ao ler o banco de sessoes: {e}"),
        }
    }
}

/// Os projetos cadastrados (nome e id), do banco. Vazio sem banco.
pub async fn listar(state: &AppState) -> Vec<ProjetoAtivo> {
    let Some(store) = &state.session_store else {
        return Vec::new();
    };
    let guard = store.lock().await;
    match guard.list_projects(None) {
        Ok(projetos) => projetos
            .into_iter()
            .map(|p| ProjetoAtivo {
                id: p.id,
                nome: p.name,
            })
            .collect(),
        Err(e) => {
            warn!(erro = %e, "projetos: falha ao listar do sessions.db");
            Vec::new()
        }
    }
}

/// O projeto ativo desta sessao, da memoria (ja restaurado pela hidratacao).
pub fn ativo(state: &AppState, session_id: &str) -> Option<ProjetoAtivo> {
    let sessao = state.sessions.get(session_id)?;
    let id = sessao.project_id.clone()?;
    Some(ProjetoAtivo {
        id,
        nome: sessao.project_name.clone().unwrap_or_default(),
    })
}

/// Um projeto por id exato, senao por nome (sem distinguir maiusculas).
fn projeto_por_alvo(
    guard: &garraia_db::SessionStore,
    alvo: &str,
) -> Result<garraia_db::Project, ErroDeSelecao> {
    let alvo = alvo.trim();
    if alvo.is_empty() {
        return Err(ErroDeSelecao::NaoEncontrado);
    }
    if let Ok(Some(p)) = guard.get_project(alvo) {
        return Ok(p);
    }
    let todos = guard
        .list_projects(None)
        .map_err(|e| ErroDeSelecao::Banco(e.to_string()))?;
    let procurado = alvo.to_lowercase();
    let mut por_nome: Vec<garraia_db::Project> = todos
        .into_iter()
        .filter(|p| p.name.to_lowercase() == procurado)
        .collect();
    match por_nome.len() {
        0 => Err(ErroDeSelecao::NaoEncontrado),
        1 => Ok(por_nome.remove(0)),
        _ => Err(ErroDeSelecao::Ambiguo(
            por_nome
                .iter()
                .map(|p| {
                    ProjetoAtivo {
                        id: p.id.clone(),
                        nome: p.name.clone(),
                    }
                    .rotulo()
                })
                .collect(),
        )),
    }
}

/// Seleciona por id ou nome (sem distinguir maiusculas), confinando o
/// caminho pelas raizes de projeto do operador; grava no `sessions.db`.
pub async fn selecionar(
    state: &AppState,
    session_id: &str,
    alvo: &str,
) -> Result<Selecao, ErroDeSelecao> {
    selecionar_com_raizes(state, session_id, alvo, &project_root::allowed_roots()).await
}

/// Nucleo testavel de [`selecionar`], com as raizes injetadas.
pub async fn selecionar_com_raizes(
    state: &AppState,
    session_id: &str,
    alvo: &str,
    raizes: &[PathBuf],
) -> Result<Selecao, ErroDeSelecao> {
    let Some(store) = &state.session_store else {
        return Err(ErroDeSelecao::SemBanco);
    };
    let projeto = {
        let guard = store.lock().await;
        projeto_por_alvo(&guard, alvo)?
    };
    let caminho = project_root::confine_within(&projeto.path, raizes)
        .map_err(ErroDeSelecao::CaminhoRecusado)?;
    let ativo = ProjetoAtivo {
        id: projeto.id.clone(),
        nome: projeto.name.clone(),
    };
    if !state.sessions.contains_key(session_id) {
        state.create_session_with_id(session_id.to_string());
    }
    let ja_era = state
        .sessions
        .get(session_id)
        .is_some_and(|s| s.project_id.as_deref() == Some(projeto.id.as_str()));
    if ja_era {
        return Ok(Selecao::JaEra(ativo));
    }
    if let Some(mut sessao) = state.sessions.get_mut(session_id) {
        sessao.project_id = Some(projeto.id.clone());
        sessao.project_name = Some(projeto.name.clone());
        sessao.working_dir = Some(caminho.to_string_lossy().into_owned());
    }
    store
        .lock()
        .await
        .associate_session_to_project(session_id, &projeto.id)
        .map_err(|e| ErroDeSelecao::Banco(e.to_string()))?;
    Ok(Selecao::Selecionado(ativo))
}

/// Desmarca o projeto desta sessao (memoria e banco). `Ok(false)` quando
/// nao havia nenhum.
pub async fn limpar(state: &AppState, session_id: &str) -> Result<bool, ErroDeSelecao> {
    let havia_em_memoria = state
        .sessions
        .get_mut(session_id)
        .map(|mut s| {
            let havia = s.project_id.is_some();
            s.project_id = None;
            s.project_name = None;
            s.working_dir = None;
            havia
        })
        .unwrap_or(false);
    let Some(store) = &state.session_store else {
        return Ok(havia_em_memoria);
    };
    let guard = store.lock().await;
    let havia_no_banco = guard
        .session_project_id(session_id)
        .map_err(|e| ErroDeSelecao::Banco(e.to_string()))?
        .is_some();
    if havia_no_banco {
        guard
            .dissociate_session_from_project(session_id)
            .map_err(|e| ErroDeSelecao::Banco(e.to_string()))?;
    }
    Ok(havia_em_memoria || havia_no_banco)
}

/// Na hidratacao: se a sessao em memoria nao tem projeto e o banco tem um
/// vinculo, traz de volta — confinando o caminho de novo. Falha e silencio
/// com `warn!` (a sessao segue sem projeto; fail-closed).
pub async fn restaurar(state: &AppState, session_id: &str) {
    restaurar_com_raizes(state, session_id, &project_root::allowed_roots()).await;
}

/// Nucleo testavel de [`restaurar`].
pub async fn restaurar_com_raizes(state: &AppState, session_id: &str, raizes: &[PathBuf]) {
    if state
        .sessions
        .get(session_id)
        .is_some_and(|s| s.project_id.is_some())
    {
        return;
    }
    let Some(store) = &state.session_store else {
        return;
    };
    let projeto = {
        let guard = store.lock().await;
        let Ok(Some(id)) = guard.session_project_id(session_id) else {
            return;
        };
        match guard.get_project(&id) {
            Ok(Some(p)) => p,
            Ok(None) => {
                warn!(session = %session_id, "projeto da sessao nao existe mais no banco; nao restaurado");
                return;
            }
            Err(e) => {
                warn!(session = %session_id, erro = %e, "falha ao ler o projeto da sessao");
                return;
            }
        }
    };
    match project_root::confine_within(&projeto.path, raizes) {
        Ok(caminho) => {
            if let Some(mut sessao) = state.sessions.get_mut(session_id) {
                sessao.project_id = Some(projeto.id);
                sessao.project_name = Some(projeto.name);
                sessao.working_dir = Some(caminho.to_string_lossy().into_owned());
            }
        }
        Err(e) => warn!(
            session = %session_id,
            erro = %e,
            "projeto da sessao fora das raizes de projeto do operador; nao restaurado"
        ),
    }
}

// ---------------------------------------------------------------------------
// Comandos de barra por principal (WhatsApp)
// ---------------------------------------------------------------------------

/// Comandos que todo principal admitido pode usar.
pub const COMANDOS_DE_TODOS: &[&str] = &["help"];
/// Comandos de dono e usuario (nao de grupo, pareado ou desconhecido).
pub const COMANDOS_DE_USUARIO: &[&str] = &["project"];
/// Comandos so do dono: mexem no poder da sessao.
pub const COMANDOS_DO_DONO: &[&str] = &["mode", "goal"];

/// O que fazer com uma mensagem que comeca com `/` num turno do WhatsApp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisaoDeComando {
    /// Nao e comando registrado: segue para o modelo como texto.
    NaoEComando,
    /// Roteia ao registro de comandos (a resposta volta ao chat).
    Roteia,
    /// Comando registrado que este principal nao pode usar: a frase volta ao
    /// chat, e nada vai ao modelo.
    Negado(String),
}

/// Pura. `registrado` diz se o nome existe no registro de comandos.
pub fn decidir_comando(
    principal: Principal,
    texto: &str,
    registrado: impl Fn(&str) -> bool,
) -> DecisaoDeComando {
    let Some(resto) = texto.trim_start().strip_prefix('/') else {
        return DecisaoDeComando::NaoEComando;
    };
    let nome = resto.split_whitespace().next().unwrap_or("");
    if nome.is_empty() || !registrado(nome) {
        return DecisaoDeComando::NaoEComando;
    }
    if !principal.admitido() {
        return DecisaoDeComando::Negado("Esta conversa nao esta admitida.".to_string());
    }
    let e_dono = principal == Principal::Dono;
    let e_usuario = matches!(principal, Principal::Usuario(_));
    if COMANDOS_DE_TODOS.contains(&nome) {
        return DecisaoDeComando::Roteia;
    }
    if COMANDOS_DE_USUARIO.contains(&nome) {
        return if e_dono || e_usuario {
            DecisaoDeComando::Roteia
        } else {
            DecisaoDeComando::Negado(format!(
                "`/{nome}` e de dono e de usuario autorizado; {} nao seleciona projeto nesta conversa.",
                principal.as_str()
            ))
        };
    }
    if COMANDOS_DO_DONO.contains(&nome) {
        return if e_dono {
            DecisaoDeComando::Roteia
        } else {
            DecisaoDeComando::Negado(format!(
                "`/{nome}` e do dono deste Garra. O nivel desta conversa vem da politica de acesso \
                 (`garraia whatsapp access`), nao de um comando."
            ))
        };
    }
    DecisaoDeComando::Negado(format!("`/{nome}` nao esta disponivel por este canal."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::whatsapp_linked_politica::Alcance;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;
    use garraia_db::SessionStore;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn estado_com_banco(dir: &std::path::Path) -> (AppState, Arc<Mutex<SessionStore>>) {
        let store = Arc::new(Mutex::new(
            SessionStore::open(&dir.join("sessions.db")).expect("abrir store"),
        ));
        let mut st = AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        );
        st.set_session_store(Arc::clone(&store));
        (st, store)
    }

    async fn cria_projeto(
        store: &Arc<Mutex<SessionStore>>,
        nome: &str,
        path: &std::path::Path,
    ) -> String {
        store
            .lock()
            .await
            .create_project(nome, &path.to_string_lossy(), None, None, None)
            .expect("cria projeto")
            .id
    }

    #[tokio::test]
    async fn seleciona_por_nome_ou_id_grava_no_banco_e_da_working_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projetos");
        std::fs::create_dir_all(raiz.join("garra")).expect("mkdir");
        let (st, store) = estado_com_banco(dir.path());
        let id = cria_projeto(&store, "Garra", &raiz.join("garra")).await;
        let sid = "whatsapp-linked-5511999998888@s.whatsapp.net";
        st.hydrate_session_history(sid, Some("whatsapp_linked"), Some("5511999998888"))
            .await;

        let sel = selecionar_com_raizes(&st, sid, "garra", std::slice::from_ref(&raiz))
            .await
            .expect("seleciona por nome");
        assert!(
            matches!(sel, Selecao::Selecionado(ref p) if p.id == id && p.nome == "Garra"),
            "{sel:?}"
        );
        let sessao = st.sessions.get(sid).expect("sessao");
        assert_eq!(sessao.project_id.as_deref(), Some(id.as_str()));
        assert_eq!(sessao.project_name.as_deref(), Some("Garra"));
        let esperado = std::fs::canonicalize(raiz.join("garra")).expect("canon");
        assert_eq!(
            sessao.working_dir.as_deref(),
            Some(esperado.to_string_lossy().as_ref())
        );
        drop(sessao);
        assert_eq!(
            store.lock().await.session_project_id(sid).expect("le"),
            Some(id.clone()),
            "persistido em sessions.project_id"
        );
        assert_eq!(
            ativo(&st, sid),
            Some(ProjetoAtivo {
                id: id.clone(),
                nome: "Garra".into()
            })
        );
        // De novo, pelo id: ja era.
        let de_novo = selecionar_com_raizes(&st, sid, &id, std::slice::from_ref(&raiz))
            .await
            .expect("seleciona por id");
        assert!(matches!(de_novo, Selecao::JaEra(_)), "{de_novo:?}");
        // O ExecContext do turno leva o working_dir.
        let exec = st
            .exec_context_for_msg(sid, Some("5511999998888"), Some("oi"))
            .await;
        assert_eq!(
            exec.working_dir.as_deref(),
            Some(esperado.to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn restaura_apos_restart_e_limpar_apaga_nos_dois_lugares() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projetos");
        std::fs::create_dir_all(raiz.join("site")).expect("mkdir");
        let sid = "telegram-42";
        let id;
        {
            let (st, store) = estado_com_banco(dir.path());
            id = cria_projeto(&store, "Site", &raiz.join("site")).await;
            st.hydrate_session_history(sid, Some("telegram"), Some("42"))
                .await;
            selecionar_com_raizes(&st, sid, "site", std::slice::from_ref(&raiz))
                .await
                .expect("seleciona");
        }
        // "Restart": estado novo, mesmo banco. A hidratacao restaura.
        let (st, store) = estado_com_banco(dir.path());
        st.hydrate_session_history(sid, Some("telegram"), Some("42"))
            .await;
        restaurar_com_raizes(&st, sid, std::slice::from_ref(&raiz)).await;
        let sessao = st.sessions.get(sid).expect("sessao");
        assert_eq!(
            sessao.project_id.as_deref(),
            Some(id.as_str()),
            "voltou do banco"
        );
        assert!(sessao.working_dir.is_some());
        drop(sessao);
        // Limpar: memoria e banco.
        assert!(limpar(&st, sid).await.expect("limpa"));
        assert!(st.sessions.get(sid).expect("sessao").project_id.is_none());
        assert!(st.sessions.get(sid).expect("sessao").working_dir.is_none());
        assert_eq!(
            store.lock().await.session_project_id(sid).expect("le"),
            None
        );
        assert!(
            !limpar(&st, sid).await.expect("limpa de novo"),
            "idempotente"
        );
        // Restaurar depois de limpar: nada volta.
        restaurar_com_raizes(&st, sid, std::slice::from_ref(&raiz)).await;
        assert!(st.sessions.get(sid).expect("sessao").project_id.is_none());
    }

    #[tokio::test]
    async fn projeto_fora_das_raizes_nao_e_selecionado_nem_restaurado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projetos");
        let fora = dir.path().join("fora");
        std::fs::create_dir_all(&raiz).expect("mkdir");
        std::fs::create_dir_all(&fora).expect("mkdir");
        let (st, store) = estado_com_banco(dir.path());
        let id = cria_projeto(&store, "Fora", &fora).await;
        let sid = "web-1";
        st.hydrate_session_history(sid, Some("web"), None).await;
        let erro = selecionar_com_raizes(&st, sid, "fora", std::slice::from_ref(&raiz))
            .await
            .expect_err("fora das raizes");
        assert!(
            matches!(
                erro,
                ErroDeSelecao::CaminhoRecusado(ProjectPathError::OutsideAllowedRoots)
            ),
            "{erro:?}"
        );
        assert!(
            !erro.to_string().contains("/fora"),
            "a frase nao carrega caminho: {erro}"
        );
        assert!(st.sessions.get(sid).expect("sessao").project_id.is_none());
        // Um vinculo antigo no banco para um projeto que hoje esta fora nao volta.
        store
            .lock()
            .await
            .associate_session_to_project(sid, &id)
            .expect("vinculo antigo");
        restaurar_com_raizes(&st, sid, std::slice::from_ref(&raiz)).await;
        assert!(
            st.sessions.get(sid).expect("sessao").working_dir.is_none(),
            "fail-closed"
        );
        // Sem raiz nenhuma configurada, nada e selecionavel.
        let erro = selecionar_com_raizes(&st, sid, &id, &[])
            .await
            .expect_err("sem raizes");
        assert!(matches!(
            erro,
            ErroDeSelecao::CaminhoRecusado(ProjectPathError::NoRootsConfigured)
        ));
    }

    #[tokio::test]
    async fn nao_encontrado_ambiguo_e_lista_sem_caminho() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projetos");
        std::fs::create_dir_all(raiz.join("a")).expect("mkdir");
        std::fs::create_dir_all(raiz.join("b")).expect("mkdir");
        let (st, store) = estado_com_banco(dir.path());
        cria_projeto(&store, "Duplo", &raiz.join("a")).await;
        cria_projeto(&store, "duplo", &raiz.join("b")).await;
        let sid = "web-2";
        st.hydrate_session_history(sid, Some("web"), None).await;
        assert!(matches!(
            selecionar_com_raizes(&st, sid, "inexistente", std::slice::from_ref(&raiz)).await,
            Err(ErroDeSelecao::NaoEncontrado)
        ));
        match selecionar_com_raizes(&st, sid, "DUPLO", std::slice::from_ref(&raiz)).await {
            Err(ErroDeSelecao::Ambiguo(rotulos)) => assert_eq!(rotulos.len(), 2, "{rotulos:?}"),
            outro => panic!("esperava ambiguo: {outro:?}"),
        }
        let lista = listar(&st).await;
        assert_eq!(lista.len(), 2);
        let texto = format!("{lista:?}");
        assert!(!texto.contains("projetos/"), "lista sem caminho: {texto}");
        // Sem banco: lista vazia e selecao recusada.
        let sem_banco = AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        );
        assert!(listar(&sem_banco).await.is_empty());
        assert!(matches!(
            selecionar_com_raizes(&sem_banco, sid, "x", std::slice::from_ref(&raiz)).await,
            Err(ErroDeSelecao::SemBanco)
        ));
    }

    /// `/project` pelo registro de comandos, com a resposta que o chat ve:
    /// nome e id curto, nunca o caminho.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn comando_project_lista_seleciona_e_limpa_sem_caminho() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projetos");
        std::fs::create_dir_all(raiz.join("app")).expect("mkdir");
        let (st, store) = estado_com_banco(dir.path());
        let id = cria_projeto(&store, "App", &raiz.join("app")).await;
        let st = Arc::new(st);
        let sid = "whatsapp-linked-5511977776666@s.whatsapp.net";
        st.hydrate_session_history(sid, Some("whatsapp_linked"), Some("5511977776666"))
            .await;
        let mut registro = garraia_channels::CommandRegistry::new();
        crate::commands::register_commands(&mut registro);
        let roda = |texto: &str| {
            let cmd = registro.resolve(texto).expect("comando registrado");
            let ctx = garraia_channels::CommandContext {
                user_id: "5511977776666".into(),
                user_name: "dono".into(),
                chat_id: 0,
                full_text: texto.to_string(),
                args: texto
                    .split_whitespace()
                    .skip(1)
                    .map(str::to_string)
                    .collect(),
                user_role: garraia_channels::Role::User,
                state: Some(Arc::clone(&st) as Arc<dyn std::any::Any + Send + Sync>),
                session_id: Some(sid.to_string()),
            };
            garraia_channels::CommandRegistry::run(cmd.as_ref(), &ctx)
                .unwrap_or_else(|e| e.to_string())
        };
        // Sem raiz de projeto no ambiente do teste: o comando real usa
        // `allowed_roots()`; aqui apontamos a env para a raiz do teste.
        // (Processo-wide; este e o unico teste do arquivo que a usa.)
        unsafe {
            std::env::set_var(project_root::ROOTS_ENV, &raiz);
        }
        let lista = roda("/project list");
        assert!(lista.contains("App"), "{lista}");
        assert!(!lista.contains("projetos/"), "{lista}");
        let nada = roda("/project");
        assert!(
            nada.to_lowercase().contains("nenhum projeto ativo"),
            "{nada}"
        );
        let sel = roda("/project app");
        assert!(sel.contains("App"), "{sel}");
        assert!(!sel.contains("projetos/"), "{sel}");
        assert_eq!(ativo(&st, sid).map(|p| p.id), Some(id.clone()));
        let atual = roda("/project");
        assert!(atual.contains("App"), "{atual}");
        let limpo = roda("/project clear");
        assert!(limpo.to_lowercase().contains("desmarcado"), "{limpo}");
        assert!(ativo(&st, sid).is_none());
        let erro = roda("/project inexistente");
        assert!(erro.contains("nenhum projeto"), "{erro}");
        unsafe {
            std::env::remove_var(project_root::ROOTS_ENV);
        }
    }

    #[test]
    fn decidir_comando_por_principal() {
        let registrado = |n: &str| ["help", "mode", "goal", "project", "start"].contains(&n);
        let usuario = Principal::Usuario(Alcance::LEITURA);
        assert_eq!(
            decidir_comando(usuario, "oi, tudo bem?", registrado),
            DecisaoDeComando::NaoEComando
        );
        assert_eq!(
            decidir_comando(usuario, "/desconhecido x", registrado),
            DecisaoDeComando::NaoEComando,
            "nao registrado segue como texto"
        );
        assert_eq!(
            decidir_comando(usuario, "/help", registrado),
            DecisaoDeComando::Roteia
        );
        assert_eq!(
            decidir_comando(usuario, "/project list", registrado),
            DecisaoDeComando::Roteia
        );
        assert_eq!(
            decidir_comando(Principal::Dono, "/project app", registrado),
            DecisaoDeComando::Roteia
        );
        assert_eq!(
            decidir_comando(Principal::Dono, "/mode code", registrado),
            DecisaoDeComando::Roteia
        );
        match decidir_comando(usuario, "/mode code", registrado) {
            DecisaoDeComando::Negado(msg) => {
                assert!(msg.contains("dono"), "{msg}");
                assert!(msg.contains("politica de acesso"), "{msg}");
            }
            outro => panic!("usuario nao muda o modo: {outro:?}"),
        }
        for p in [
            Principal::Grupo(Alcance::LEITURA),
            Principal::Pareado,
            Principal::Desconhecido(Alcance::CHAT),
        ] {
            assert!(
                matches!(
                    decidir_comando(p, "/project app", registrado),
                    DecisaoDeComando::Negado(_)
                ),
                "{p:?}"
            );
            assert!(
                matches!(
                    decidir_comando(p, "/mode code", registrado),
                    DecisaoDeComando::Negado(_)
                ),
                "{p:?}"
            );
            assert_eq!(
                decidir_comando(p, "/help", registrado),
                DecisaoDeComando::Roteia,
                "{p:?}"
            );
        }
        // Registrado mas fora das tres listas (ex.: `/start`): recusado com motivo.
        assert!(matches!(
            decidir_comando(Principal::Dono, "/start", registrado),
            DecisaoDeComando::Negado(_)
        ));
        // Nao admitido nunca roteia.
        assert!(matches!(
            decidir_comando(Principal::Bloqueado, "/help", registrado),
            DecisaoDeComando::Negado(_)
        ));
    }
}
