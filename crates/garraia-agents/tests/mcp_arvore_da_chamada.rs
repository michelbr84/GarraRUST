//! #1913: cancelamento e timeout encerram a propria chamada sem afetar as
//! outras, e desconexao, queda e falha de subida encerram a arvore inteira do
//! servidor MCP.
//!
//! Dirigem um servidor stdio real (`tests/fixtures/fake_mcp_tree_server.py`)
//! que lanca processos de verdade, porque o que se afirma aqui — quem morre e
//! quem continua vivo — so existe com processos de verdade.
//!
//! So Linux: a prova de que um processo morreu le `/proc/<pid>/stat` (um
//! zumbi responde a `kill(pid, 0)` como se estivesse vivo), e a contencao de
//! quem abre sessao nova depende de `/proc/<pid>/environ` + `pidfd`.
#![cfg(all(feature = "mcp", target_os = "linux"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use garraia_agents::McpManager;
use garraia_agents::tools::{Tool, ToolContext};

/// Teto de cada teste: uma regressao reprova a suite em vez de pendurar o CI.
const TETO_DO_TESTE: Duration = Duration::from_secs(90);

struct Servidor {
    nome: &'static str,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

impl Servidor {
    fn novo(nome: &'static str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        Self {
            nome,
            log: dir.path().join("eventos.log"),
            _dir: dir,
        }
    }

    fn args(&self, extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            format!(
                "{}/tests/fixtures/fake_mcp_tree_server.py",
                env!("CARGO_MANIFEST_DIR")
            ),
            "--log".to_string(),
            self.log.display().to_string(),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        args
    }

    async fn conectar(&self, manager: &Arc<McpManager>, extra: &[&str]) {
        manager
            .connect(
                self.nome,
                "python3",
                &self.args(extra),
                &HashMap::new(),
                10,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await
            .expect("o servidor de fixture deveria conectar");
    }

    /// A tool `trabalho` deste servidor, com o timeout pedido.
    async fn trabalho(&self, manager: &Arc<McpManager>, timeout: Duration) -> Arc<dyn Tool> {
        self.tool(manager, "trabalho", timeout).await
    }

    async fn tool(
        &self,
        manager: &Arc<McpManager>,
        nome: &str,
        timeout: Duration,
    ) -> Arc<dyn Tool> {
        let alvo = format!("{}__{nome}", self.nome);
        let tool = manager
            .take_tools(self.nome, timeout)
            .await
            .into_iter()
            .find(|t| t.name() == alvo)
            .unwrap_or_else(|| panic!("tool {alvo} ausente"));
        Arc::from(tool)
    }

    fn eventos(&self) -> Vec<Vec<String>> {
        eventos(&self.log)
    }

    /// PID de cada descendente de longa vida, por tipo. Com reconexao o log
    /// acumula geracoes; aqui vale a PRIMEIRA de cada tipo.
    fn descendentes(&self) -> HashMap<String, u32> {
        let mut mapa = HashMap::new();
        for e in self.eventos() {
            if e[0] == "descendente" && e.len() >= 3 {
                mapa.entry(e[1].clone())
                    .or_insert_with(|| e[2].parse().expect("pid"));
            }
        }
        mapa
    }

    /// Espera o worker da chamada de `segundos` aparecer; devolve (id, pid).
    async fn worker(&self, segundos: u64) -> (String, u32) {
        let alvo = segundos.to_string();
        let achado = esperar(Duration::from_secs(15), || {
            self.eventos()
                .into_iter()
                .find(|e| e[0] == "inicio" && e.len() >= 4 && e[3] == alvo)
                .map(|e| (e[1].clone(), e[2].parse::<u32>().expect("pid")))
        })
        .await;
        achado.unwrap_or_else(|| panic!("worker de {segundos}s nao comecou"))
    }

    fn cancelados(&self) -> Vec<String> {
        self.eventos()
            .into_iter()
            .filter(|e| e[0] == "cancelado")
            .map(|e| e[1].clone())
            .collect()
    }
}

fn eventos(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.split_whitespace().map(str::to_string).collect())
        // Linha pela metade (a fixture ainda escrevendo) fica para a proxima
        // leitura: todo evento tem ao menos tipo e um campo.
        .filter(|e: &Vec<String>| e.len() >= 2)
        .collect()
}

/// Repete `olhar` ate ele devolver `Some` ou o prazo acabar.
async fn esperar<T>(prazo: Duration, mut olhar: impl FnMut() -> Option<T>) -> Option<T> {
    let comeco = Instant::now();
    loop {
        if let Some(valor) = olhar() {
            return Some(valor);
        }
        if comeco.elapsed() >= prazo {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// O processo `pid` esta vivo? Zumbi e PID inexistente contam como morto.
/// O nome do comando entra na conta como protecao contra reuso do PID: tudo
/// o que a fixture lanca e `sleep` ou `sh`.
fn vivo(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some((antes, depois)) = stat.rsplit_once(')') else {
        return false;
    };
    let comando = antes.split_once('(').map(|(_, c)| c).unwrap_or_default();
    let estado = depois.split_whitespace().next().unwrap_or_default();
    matches!(comando, "sleep" | "sh") && estado != "Z" && estado != "X"
}

async fn espera_morrer(pid: u32, prazo: Duration) -> bool {
    esperar(prazo, || (!vivo(pid)).then_some(()))
        .await
        .is_some()
}

fn ctx() -> ToolContext {
    ToolContext {
        session_id: "mcp-arvore-test".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: garraia_agents::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    }
}

async fn chamar(tool: Arc<dyn Tool>, segundos: u64) -> Result<String, String> {
    tool.execute(&ctx(), serde_json::json!({ "segundos": segundos }))
        .await
        .map(|saida| saida.content)
        .map_err(|e| e.to_string())
}

async fn com_teto(corpo: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(TETO_DO_TESTE, corpo)
        .await
        .expect("o teste nao pode pendurar");
}

/// Timeout de UMA chamada: o servidor recebe o cancelamento com o id dela,
/// o trabalho dela para, e a chamada vizinha no MESMO servidor e a de OUTRO
/// servidor terminam normalmente. O servidor compartilhado nao cai.
#[tokio::test]
async fn timeout_cancela_so_a_propria_chamada() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("arvore_a");
        let b = Servidor::novo("arvore_b");
        a.conectar(&manager, &[]).await;
        b.conectar(&manager, &[]).await;

        let curta = a.trabalho(&manager, Duration::from_secs(1)).await;
        let longa = a.trabalho(&manager, Duration::from_secs(30)).await;
        let de_b = b.trabalho(&manager, Duration::from_secs(30)).await;

        let (estourada, vizinha, outra) =
            tokio::join!(chamar(curta, 40), chamar(longa, 3), chamar(de_b, 2));

        let erro = estourada.expect_err("a chamada de 40s tem de estourar o timeout de 1s");
        assert!(erro.contains("tempo limite"), "erro inesperado: {erro}");
        assert_eq!(vizinha.as_deref(), Ok("pong"), "a vizinha foi afetada");
        assert_eq!(outra.as_deref(), Ok("pong"), "o outro servidor foi afetado");

        let (id, pid) = a.worker(40).await;
        assert!(
            espera_morrer(pid, Duration::from_secs(10)).await,
            "o trabalho da chamada estourada (pid {pid}) continuou rodando"
        );
        assert_eq!(a.cancelados(), vec![id], "cancelamento com id errado");
        assert!(b.cancelados().is_empty(), "o outro servidor foi cancelado");
        assert!(manager.is_connected("arvore_a").await);
        assert!(manager.is_connected("arvore_b").await);

        manager.disconnect_all().await;
    })
    .await;
}

/// Cancelamento explicito: quem chamou desistiu (o futuro da chamada foi
/// largado). Mesmo contrato do timeout.
#[tokio::test]
async fn chamada_abandonada_cancela_so_a_propria() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("abandono");
        a.conectar(&manager, &[]).await;
        let tool = a.trabalho(&manager, Duration::from_secs(60)).await;

        let abandonada = tokio::spawn(chamar(Arc::clone(&tool), 40));
        let vizinha = tokio::spawn(chamar(Arc::clone(&tool), 3));
        let (id, pid) = a.worker(40).await;
        assert!(
            vivo(pid),
            "o trabalho deveria estar rodando antes do abandono"
        );

        abandonada.abort();
        assert!(
            espera_morrer(pid, Duration::from_secs(10)).await,
            "o trabalho da chamada abandonada (pid {pid}) continuou rodando"
        );
        assert_eq!(
            vizinha.await.expect("join").as_deref(),
            Ok("pong"),
            "a vizinha foi afetada"
        );
        assert_eq!(a.cancelados(), vec![id]);
        assert!(manager.is_connected("abandono").await);

        manager.disconnect_all().await;
    })
    .await;
}

/// Servidor que ignora o cancelamento: o gateway NAO derruba o servidor
/// compartilhado por causa de uma chamada — o trabalho segue ate o servidor
/// sair, e ai morre com a arvore. E o limite da via cooperativa, fixado.
#[tokio::test]
async fn servidor_que_ignora_cancelamento_so_para_na_desconexao() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("surdo");
        a.conectar(&manager, &["--ignore-cancel"]).await;
        let curta = a.trabalho(&manager, Duration::from_secs(1)).await;

        chamar(curta, 40).await.expect_err("tem de estourar");
        let (id, pid) = a.worker(40).await;
        let cancelou = esperar(Duration::from_secs(10), || {
            a.cancelados().contains(&id).then_some(())
        })
        .await;
        assert!(cancelou.is_some(), "o cancelamento nao chegou ao servidor");
        assert!(
            vivo(pid),
            "o servidor ignora o cancelamento: o trabalho segue"
        );
        assert!(
            manager.is_connected("surdo").await,
            "o servidor compartilhado nao pode ser derrubado por uma chamada"
        );

        manager.disconnect("surdo").await;
        assert!(
            espera_morrer(pid, Duration::from_secs(10)).await,
            "o trabalho (pid {pid}) sobreviveu a desconexao do servidor"
        );
    })
    .await;
}

/// Desconexao: filho, neto e quem abriu sessao nova morrem; a arvore do
/// servidor vizinho fica intacta e ele continua atendendo.
#[tokio::test]
async fn desconexao_derruba_a_arvore_e_poupa_o_vizinho() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("sai");
        let b = Servidor::novo("fica");
        a.conectar(&manager, &["--descendentes", "filho,neto,sessao"])
            .await;
        b.conectar(&manager, &["--descendentes", "filho,neto,sessao"])
            .await;
        let de_a = a.descendentes();
        let de_b = b.descendentes();
        assert_eq!(de_a.len(), 3, "descendentes de A: {de_a:?}");
        assert_eq!(de_b.len(), 3, "descendentes de B: {de_b:?}");
        for (tipo, pid) in de_a.iter().chain(de_b.iter()) {
            assert!(vivo(*pid), "{tipo} (pid {pid}) deveria estar vivo");
        }

        manager.disconnect("sai").await;

        for (tipo, pid) in &de_a {
            assert!(
                espera_morrer(*pid, Duration::from_secs(10)).await,
                "{tipo} (pid {pid}) de A sobreviveu a desconexao"
            );
        }
        for (tipo, pid) in &de_b {
            assert!(vivo(*pid), "{tipo} (pid {pid}) de B foi morto junto");
        }
        let de_b_tool = b.trabalho(&manager, Duration::from_secs(30)).await;
        assert_eq!(chamar(de_b_tool, 1).await.as_deref(), Ok("pong"));

        manager.disconnect_all().await;
        for (tipo, pid) in &de_b {
            assert!(
                espera_morrer(*pid, Duration::from_secs(10)).await,
                "{tipo} (pid {pid}) de B sobreviveu ao encerramento"
            );
        }
    })
    .await;
}

/// Queda: o lider morre sozinho deixando descendentes — inclusive um que
/// segura o stdout dele (sem EOF no transporte) e um em sessao nova. O grupo
/// cai sem ninguem pedir; o da sessao nova cai quando a conexao morta e
/// trocada pelo health monitor.
#[tokio::test]
async fn queda_do_lider_derruba_a_arvore() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("cai");
        a.conectar(&manager, &["--descendentes", "filho,neto,sessao,pipe"])
            .await;
        let antes = a.descendentes();
        assert_eq!(antes.len(), 4, "descendentes: {antes:?}");

        let morrer = a.tool(&manager, "morrer", Duration::from_secs(20)).await;
        morrer
            .execute(&ctx(), serde_json::json!({}))
            .await
            .expect_err("o servidor morre no meio da chamada");

        for tipo in ["filho", "neto", "pipe"] {
            let pid = antes[tipo];
            assert!(
                espera_morrer(pid, Duration::from_secs(10)).await,
                "{tipo} (pid {pid}) sobreviveu a queda do lider"
            );
        }

        let sessao = antes["sessao"];
        let comeco = Instant::now();
        while vivo(sessao) {
            assert!(
                comeco.elapsed() < Duration::from_secs(30),
                "sessao (pid {sessao}) sobreviveu a troca da conexao morta"
            );
            manager.health_tick().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        manager.disconnect_all().await;
    })
    .await;
}

/// Servidor que ignora EOF e SIGTERM: o encerramento e limitado e leva a
/// arvore junto.
#[tokio::test]
async fn servidor_teimoso_cai_com_a_arvore() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("teimoso");
        a.conectar(&manager, &["--teimoso", "--descendentes", "filho,neto"])
            .await;
        let antes = a.descendentes();
        assert_eq!(antes.len(), 2, "descendentes: {antes:?}");

        tokio::time::timeout(Duration::from_secs(30), manager.disconnect("teimoso"))
            .await
            .expect("a desconexao de um servidor teimoso tem de ser limitada");

        for (tipo, pid) in &antes {
            assert!(
                espera_morrer(*pid, Duration::from_secs(10)).await,
                "{tipo} (pid {pid}) sobreviveu"
            );
        }
    })
    .await;
}

/// Falha de subida: o filho lancou descendentes e nunca respondeu ao
/// handshake. O `connect` falha e nao deixa arvore para tras.
#[tokio::test]
async fn falha_na_subida_nao_deixa_arvore() {
    com_teto(async {
        let manager = Arc::new(McpManager::new());
        let a = Servidor::novo("mudo");
        let resultado = manager
            .connect(
                a.nome,
                "python3",
                &a.args(&["--mudo", "--descendentes", "filho,neto,sessao"]),
                &HashMap::new(),
                2,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await;
        assert!(resultado.is_err(), "um servidor mudo nao pode conectar");

        let antes = a.descendentes();
        assert_eq!(antes.len(), 3, "descendentes: {antes:?}");
        for (tipo, pid) in &antes {
            assert!(
                espera_morrer(*pid, Duration::from_secs(10)).await,
                "{tipo} (pid {pid}) sobreviveu a falha de subida"
            );
        }
    })
    .await;
}
