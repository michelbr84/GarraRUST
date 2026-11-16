//! #1912 (follow-up): um turno resolve o workspace efetivo UMA vez.
//!
//! A aprovacao do turno, o `ToolContext` de cada chamada e o registro da
//! pausa liam o diretorio efetivo cada um por conta propria. Entre uma
//! leitura e outra o disco pode mudar (a raiz do workspace padrao aparece,
//! o diretorio da sessao vira arquivo), e `garantir_para_sessao` passa de
//! `None` para `Some` ou o contrario — entao a ferramenta rodava com um
//! `working_dir` diferente daquele a que a aprovacao estava vinculada, e a
//! pausa era gravada para um destino que a ferramenta nao viu.
//!
//! Sem `sleep` nem corrida: a mudanca no disco acontece num gancho
//! determinístico — dentro da chamada ao provider (depois da aprovacao,
//! antes do despacho) ou dentro da propria ferramenta (depois de ela ver o
//! contexto, antes do registro da pausa).
//!
//! O que isto NAO prova: o snapshot e uma string de caminho, nao um handle
//! de diretorio. Ele tira a divergencia entre os pontos de decisao do
//! runtime; nao elimina TOCTOU adversarial no sistema de arquivos.
use super::aprovacao_entre_turnos::{CAMINHOS, Caminho, Cenario, PedeAlvos, e_pedido, escopo};
use super::*;
use crate::exec_context::ExecContext;
use crate::providers::{LlmRequest, LlmResponse, StreamEvent};
use crate::tools::approval::ApprovalFingerprint;
use futures::Stream;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;

type Gancho = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

fn gancho_vazio() -> Gancho {
    Arc::new(Mutex::new(None))
}

fn armar(gancho: &Gancho, f: impl FnOnce() + Send + 'static) {
    *gancho.lock().expect("lock") = Some(Box::new(f));
}

fn disparar(gancho: &Gancho) {
    let f = gancho.lock().expect("lock").take();
    if let Some(f) = f {
        f();
    }
}

/// O `PedeAlvos` de sempre, com um gancho que roda na PRIMEIRA chamada ao
/// provider depois de armado — streaming ou batch. Essa chamada vem
/// sempre depois da aprovacao do turno e antes de qualquer despacho.
struct ProviderComGancho {
    interno: Arc<PedeAlvos>,
    antes: Gancho,
}

#[async_trait]
impl LlmProvider for ProviderComGancho {
    fn provider_id(&self) -> &str {
        "pede_alvos_com_gancho"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        disparar(&self.antes);
        self.interno.complete(request).await
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        disparar(&self.antes);
        self.interno.stream_complete(request).await
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// O mesmo contrato da `ToolQuePedeConfirmacao`, anotando o `working_dir`
/// que recebeu em TODA chamada — a que pausa e a que roda. O gancho roda
/// dentro da chamada que pausa, depois de o contexto ter sido lido.
struct ToolQueAnotaCwd {
    rodou: Arc<Mutex<Vec<String>>>,
    cwds: Arc<Mutex<Vec<Option<String>>>>,
    ao_pausar: Gancho,
}

#[async_trait]
impl Tool for ToolQueAnotaCwd {
    fn name(&self) -> &str {
        "precisa_confirmar"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        c: &ToolContext,
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        self.cwds.lock().expect("lock").push(c.working_dir.clone());
        let alvo = ToolQuePedeConfirmacao::alvo(&i);
        if c.approval.covers("precisa_confirmar", &alvo) {
            self.rodou.lock().expect("lock").push(alvo.clone());
            return Ok(ToolOutput::success(format!("feito: {alvo}")));
        }
        disparar(&self.ao_pausar);
        let marcador = ApprovalFingerprint::of("precisa_confirmar", &alvo).marker();
        Ok(ToolOutput::confirmation_request(format!(
            "confirme a acao perigosa em {alvo} {marcador}"
        )))
    }
}

struct Montagem {
    c: Cenario,
    cwds: Arc<Mutex<Vec<Option<String>>>>,
    antes_do_provider: Gancho,
    ao_pausar: Gancho,
    /// A raiz do workspace padrao (pode ainda nao existir).
    raiz: PathBuf,
    /// Onde mora o diretorio da sessao `s1` dentro dela.
    dir_da_sessao: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Montagem {
    fn nova(caminho: Caminho, raiz_existe: bool) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        let raiz = base.join("workspace");
        if raiz_existe {
            std::fs::create_dir(&raiz).expect("raiz");
        }
        let dir_da_sessao = crate::tools::SessionWorkspace::nova(raiz.clone())
            .caminho_da_sessao("s1")
            .expect("sessao");

        let mut rt = AgentRuntime::new();
        rt.set_workspace_padrao(Some(crate::tools::SessionWorkspace::nova(raiz.clone())));
        let rodou = Arc::new(Mutex::new(Vec::new()));
        let cwds = Arc::new(Mutex::new(Vec::new()));
        let ao_pausar = gancho_vazio();
        rt.register_tool(Box::new(ToolQueAnotaCwd {
            rodou: Arc::clone(&rodou),
            cwds: Arc::clone(&cwds),
            ao_pausar: Arc::clone(&ao_pausar),
        }));
        let antes_do_provider = gancho_vazio();
        rt.register_provider(Arc::new(ProviderComGancho {
            interno: PedeAlvos::novo(&["x", "x", "x"], matches!(caminho, Caminho::Streaming)),
            antes: Arc::clone(&antes_do_provider),
        }));
        Self {
            c: Cenario {
                rt,
                rodou,
                caminho,
                historico: Vec::new(),
            },
            cwds,
            antes_do_provider,
            ao_pausar,
            raiz,
            dir_da_sessao,
            _tmp: tmp,
        }
    }

    fn ultimo_cwd(&self) -> Option<String> {
        self.cwds
            .lock()
            .expect("lock")
            .last()
            .cloned()
            .expect("a ferramenta foi chamada")
    }
}

fn exec() -> ExecContext {
    ExecContext {
        approval_scope: escopo("web", "s1", "conn-a"),
        ..ExecContext::default()
    }
}

fn texto(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// O diretorio da sessao vira um arquivo comum: `garantir_para_sessao`
/// passa a devolver `None` (`NaoEDiretorio`).
fn diretorio_vira_arquivo(dir: &Path) {
    std::fs::remove_dir(dir).expect("remove_dir");
    std::fs::write(dir, b"nao sou diretorio").expect("write");
}

fn arquivo_volta_a_ser_diretorio(dir: &Path) {
    std::fs::remove_file(dir).expect("remove_file");
    std::fs::create_dir(dir).expect("create_dir");
}

/// **Aprovacao x despacho.** O "sim" e resolvido contra o workspace do
/// turno; o disco muda durante a chamada ao provider; a ferramenta tem de
/// receber o MESMO `working_dir` a que a aprovacao foi vinculada.
///
/// Os dois sentidos: a raiz que aparece no meio do turno (aprovado sem
/// workspace nenhum, e o `None` tem de ficar — e o `None` grudento do
/// snapshot) e o diretorio que deixa de ser diretorio (aprovado para o
/// diretorio da sessao, e e ele que a ferramenta recebe).
#[tokio::test]
async fn ferramenta_aprovada_recebe_o_workspace_da_aprovacao() {
    let mut divergencias = Vec::new();
    for caminho in CAMINHOS {
        // Raiz ausente no pedido e na aprovacao; ela aparece no provider.
        let mut m = Montagem::nova(caminho, false);
        assert!(e_pedido(
            &m.c.turno_com_exec("s1", "apaga x", &[], &exec()).await
        ));
        assert_eq!(m.ultimo_cwd(), None, "{caminho:?}: pedido sem workspace");
        let raiz = m.raiz.clone();
        armar(&m.antes_do_provider, move || {
            std::fs::create_dir(&raiz).expect("a raiz aparece no meio do turno");
        });
        m.c.turno_com_exec("s1", "sim", &[], &exec()).await;
        assert_eq!(m.c.rodou(), vec!["x"], "{caminho:?}: o sim aprovou");
        if let Some(outro) = m.ultimo_cwd() {
            divergencias.push(format!(
                "{caminho:?}/raiz aparece: aprovado sem workspace, rodou em {outro}"
            ));
        }

        // Diretorio da sessao no pedido e na aprovacao; vira arquivo no provider.
        let mut m = Montagem::nova(caminho, true);
        let dir = texto(&m.dir_da_sessao);
        assert!(e_pedido(
            &m.c.turno_com_exec("s1", "apaga x", &[], &exec()).await
        ));
        assert_eq!(m.ultimo_cwd().as_deref(), Some(dir.as_str()), "{caminho:?}");
        let alvo = m.dir_da_sessao.clone();
        armar(&m.antes_do_provider, move || diretorio_vira_arquivo(&alvo));
        m.c.turno_com_exec("s1", "sim", &[], &exec()).await;
        assert_eq!(m.c.rodou(), vec!["x"], "{caminho:?}: o sim aprovou");
        let recebido = m.ultimo_cwd();
        if recebido.as_deref() != Some(dir.as_str()) {
            divergencias.push(format!(
                "{caminho:?}/diretorio vira arquivo: aprovado para {dir}, rodou com {recebido:?}"
            ));
        }
    }
    assert!(
        divergencias.is_empty(),
        "a ferramenta aprovada recebeu outro working_dir:\n{}",
        divergencias.join("\n")
    );
}

/// **Pausa x ferramenta.** A pendencia e gravada para o `working_dir` que
/// a ferramenta que pausou recebeu — nao para o que o disco diz depois que
/// ela devolveu o pedido.
///
/// Disco ainda mudado no turno seguinte: o workspace agora e outro
/// (`None`), e o "sim" nao pode aprovar. Disco restaurado: o workspace
/// voltou a ser o da pausa, e o "sim" aprova.
#[tokio::test]
async fn pausa_fica_vinculada_ao_workspace_que_a_ferramenta_viu() {
    let mut divergencias = Vec::new();
    for caminho in CAMINHOS {
        for restaura in [false, true] {
            let mut m = Montagem::nova(caminho, true);
            let dir = texto(&m.dir_da_sessao);
            let alvo = m.dir_da_sessao.clone();
            armar(&m.ao_pausar, move || diretorio_vira_arquivo(&alvo));
            assert!(e_pedido(
                &m.c.turno_com_exec("s1", "apaga x", &[], &exec()).await
            ));
            assert_eq!(
                m.ultimo_cwd().as_deref(),
                Some(dir.as_str()),
                "{caminho:?}: a ferramenta que pausou viu o diretorio da sessao"
            );
            if restaura {
                arquivo_volta_a_ser_diretorio(&m.dir_da_sessao);
            }
            let r = m.c.turno_com_exec("s1", "sim", &[], &exec()).await;
            if restaura {
                // Mesmo workspace da pausa: o sim aprova, e no mesmo destino.
                if m.c.rodou() != vec!["x"] {
                    divergencias.push(format!(
                        "{caminho:?}/restaurado: pausa vista em {dir}, sim no mesmo \
                         workspace nao aprovou: {r}"
                    ));
                } else if m.ultimo_cwd().as_deref() != Some(dir.as_str()) {
                    divergencias.push(format!(
                        "{caminho:?}/restaurado: rodou com {:?}, nao em {dir}",
                        m.ultimo_cwd()
                    ));
                }
            } else if !e_pedido(&r) || !m.c.rodou().is_empty() {
                // O workspace mudou desde a pausa: tem de perguntar de novo.
                divergencias.push(format!(
                    "{caminho:?}/ainda mudado: pausa vista em {dir} aprovada com \
                     working_dir {:?}: {r}",
                    m.ultimo_cwd()
                ));
            }
        }
    }
    assert!(
        divergencias.is_empty(),
        "a pausa ficou vinculada a outro workspace:\n{}",
        divergencias.join("\n")
    );
}
