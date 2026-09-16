//! Confinamento das file tools a um conjunto de raizes (issue #1244).
//!
//! ## Por que isto existe
//!
//! `file_read`, `file_write` e `list_dir` recebiam o caminho cru do modelo,
//! expandiam `~` e aceitavam caminho absoluto sem confinar coisa alguma: o
//! parametro `allowed_directories` existia, mas os dois pontos de registro em
//! producao (`garraia-gateway/src/bootstrap/mod.rs` e
//! `garraia-cli/src/chat.rs`) passavam `None`. Um prompt vindo de um canal —
//! Telegram, Discord, WhatsApp — mandava o modelo ler `~/.ssh/id_rsa`,
//! `/etc/shadow` ou o proprio `config.yml` do gateway, que carrega chave de
//! LLM em claro quando o operador nao usa o cofre.
//!
//! ## A regra
//!
//! As **raizes efetivas** de uma chamada sao a uniao de:
//!
//! 1. as raizes que o operador declarou (`agent.file_roots` na config, ou
//!    [`ROOTS_ENV`]), e
//! 2. o `working_dir` da sessao, quando ha um.
//!
//! Conjunto vazio nao significa "tudo liberado": significa **negar tudo**.
//! Esse e o ponto do fail-closed — sem raiz conhecida nao ha como afirmar que
//! um caminho e seguro, e a ausencia de configuracao e exatamente o estado do
//! deploy que a #1244 descreve.
//!
//! No gateway isso faz a raiz default ser o diretorio da sessao e nada mais;
//! o `working_dir` de uma sessao ja passa por `project_root::confine` antes de
//! ser gravado, entao a uniao nao alarga a politica de quem esta na porta.
//! Na CLI a raiz default e o CWD do processo ([`FileJail::process_cwd`]): quem
//! roda `garra chat` e o proprio dono da maquina, no diretorio que escolheu.
//!
//! ## Resolver antes de comparar
//!
//! [`FileJail::confine`] canonicaliza (`std::fs::canonicalize`, que **segue
//! symlink** e achata `..`) e so entao compara, componente a componente, via
//! [`Path::starts_with`]. Comparar a string crua deixaria passar tanto
//! `raiz/../../etc` quanto um symlink `raiz/fuga -> /etc` — e o segundo e o
//! que um teste de `..` textual nunca pega.
//!
//! Para escrita o arquivo alvo ainda nao existe, entao `canonicalize` falha.
//! A funcao sobe ate o **ancestral existente mais proximo**, canonicaliza esse
//! e recola a cauda.
//!
//! O passo contraintuitivo esta aqui, e e o que a primeira versao desta
//! funcao errou: **`canonicalize` falhar nao quer dizer "nao existe", quer
//! dizer "nao resolve"**. Um symlink pendurado — aquele cujo alvo nao existe —
//! falha no `canonicalize` e existe para o `lstat`; e o `open(O_CREAT)` de uma
//! escrita **segue** esse link e cria o arquivo no alvo, fora da raiz. O vetor
//! concreto e um `raiz/evil -> ../../../home/u/.ssh/authorized_keys`
//! versionado num repositorio clonado: a cauda `evil` era recolada dentro da
//! raiz, o `starts_with` aprovava, e o byte caia fora. Por isso cada
//! componente que nao canonicaliza ainda passa por `symlink_metadata`:
//! existir para o `lstat` sem resolver e recusa (`Denial::Outside`), nao
//! "cauda inexistente".
//!
//! Custo aceito: um symlink pendurado apontando para **dentro** da raiz
//! tambem e recusado. Distinguir exigiria reimplementar a resolucao de
//! symlink a mao — alvo relativo, ciclo, teto de profundidade — e fail-closed
//! sai mais barato que uma segunda resolucao caseira.
//!
//! O caminho e normalizado (`components().collect()`) antes desse `lstat`:
//! com barra final (`raiz/evil/`) o `lstat` **segue** o symlink por POSIX, e
//! um pendurado voltaria a parecer inexistente. A normalizacao derruba a
//! barra final, o `//` e o `.` do meio — e nao toca em `..`, que ja foi
//! recusado antes.
//!
//! ## Mensagem de recusa
//!
//! [`Denial`] tem variantes para diagnostico interno, mas **uma unica
//! mensagem** volta ao modelo: nem o caminho, nem a raiz, nem a distincao
//! entre "nao existe" e "existe mas esta fora". Um endpoint que responde
//! diferente para as duas coisas e um oraculo de existencia de arquivo
//! (threat-model §5.5).
//!
//! ## Residual conhecido: TOCTOU
//!
//! A checagem resolve o caminho e devolve o resolvido; quem abre o arquivo
//! e a tool, num segundo passo. Entre um e outro, alguem com escrita dentro
//! da raiz pode trocar um componente por symlink para fora. Fechar isso exige
//! abrir por descritor (`openat2` com `RESOLVE_BENEATH` no Linux) e nao tem
//! equivalente portatil nos tres sistemas operacionais que o projeto suporta.
//! Fica registrado como residual em vez de reivindicado como resolvido.

use std::path::{Component, Path, PathBuf};

use tracing::warn;

/// Variavel de ambiente que **acrescenta** raizes as de `agent.file_roots`.
///
/// Formato de `PATH`: `GARRAIA_FILE_ROOTS=/srv/dados:/opt/notas`.
pub const ROOTS_ENV: &str = "GARRAIA_FILE_ROOTS";

/// Por que um caminho foi recusado. Diagnostico interno: as tres variantes
/// produzem a **mesma** mensagem para o modelo, de proposito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// Nenhuma raiz efetiva — nem config, nem `working_dir` de sessao.
    NoRoots,
    /// Nem o caminho nem nenhum ancestral dele resolveu.
    Unresolvable,
    /// Resolveu, e caiu fora de todas as raizes.
    Outside,
}

/// A frase unica que volta ao modelo. Nao nomeia caminho, raiz nem causa.
pub const DENIAL_MESSAGE: &str = "acesso negado: o caminho esta fora das raizes \
permitidas para as file tools (confinamento do agente, issue #1244). Peca um \
caminho dentro do diretorio de trabalho da sessao.";

impl Denial {
    /// Sempre a mesma string. Ver [`DENIAL_MESSAGE`].
    pub fn message(self) -> &'static str {
        DENIAL_MESSAGE
    }
}

impl std::fmt::Display for Denial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(DENIAL_MESSAGE)
    }
}

impl From<Denial> for garraia_common::Error {
    fn from(d: Denial) -> Self {
        garraia_common::Error::Security(d.message().to_string())
    }
}

/// As raizes que o **operador** declarou, ja canonicalizadas.
///
/// O `working_dir` da sessao nao mora aqui: ele chega por chamada, em
/// [`FileJail::confine`], porque muda de sessao para sessao enquanto a tool
/// e registrada uma vez no boot.
#[derive(Debug, Clone, Default)]
pub struct FileJail {
    roots: Vec<PathBuf>,
}

impl FileJail {
    /// Nenhuma raiz de operador: so o `working_dir` da sessao autoriza algo.
    /// E o default do gateway.
    pub fn sessions_only() -> Self {
        Self { roots: Vec::new() }
    }

    /// Canonicaliza e guarda. Raiz que nao resolve e descartada com `warn!`:
    /// uma raiz inexistente nao pode autorizar nada.
    ///
    /// Uma raiz que **resolve para `/` ou para o `$HOME`** e guardada — a
    /// decisao e do operador — mas sai no log como aviso. Sem isso a unica
    /// configuracao que desliga o jail e tambem a mais silenciosa, e a pressao
    /// operacional de um jail apertado empurra exatamente para ela.
    pub fn from_roots<I, P>(roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let roots: Vec<PathBuf> = roots
            .into_iter()
            .filter_map(|root| canonicalizar_raiz(root.as_ref()))
            .collect();
        let home = process_home_dir();
        for root in &roots {
            if let Some(motivo) = motivo_de_raiz_perigosa(root, home.as_deref()) {
                warn!(
                    root = %root.display(),
                    "raiz de file tool perigosa: {motivo} — as file tools do agente alcancam \
                     tudo debaixo dela (issue #1244)"
                );
            }
        }
        Self { roots }
    }

    /// `agent.file_roots` da config mais [`ROOTS_ENV`]. Uso do gateway.
    pub fn from_config_roots<S: AsRef<str>>(configured: &[S]) -> Self {
        let mut raw: Vec<PathBuf> = configured
            .iter()
            .map(|s| s.as_ref().trim())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        if let Some(from_env) = std::env::var_os(ROOTS_ENV) {
            raw.extend(std::env::split_paths(&from_env));
        }
        Self::from_roots(raw)
    }

    /// As raizes da config **mais** o CWD do processo. Uso da CLI, onde quem
    /// roda o binario e o dono da maquina e o diretorio corrente e a escolha
    /// explicita dele. Nao serve ao gateway, que atende pedido de terceiro.
    pub fn from_config_roots_plus_cwd<S: AsRef<str>>(configured: &[S]) -> Self {
        let mut jail = Self::from_config_roots(configured);
        match std::env::current_dir() {
            // Sem passar pelo aviso de `from_roots`: o CWD nao e uma raiz
            // *declarada*, e quem roda `garra chat` no proprio home nao esta
            // desligando jail nenhum — esta no diretorio que escolheu.
            Ok(cwd) => jail.roots.extend(canonicalizar_raiz(&cwd)),
            Err(e) => {
                warn!(error = %e, "CWD indisponivel: file tools ficam so com as raizes da config")
            }
        }
        jail
    }

    /// So o CWD do processo.
    pub fn process_cwd() -> Self {
        let vazio: [&str; 0] = [];
        Self::from_config_roots_plus_cwd(&vazio)
    }

    /// Raizes declaradas pelo operador (sem o `working_dir` da sessao).
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// `true` quando o operador nao declarou nenhuma raiz — o que nao e o
    /// mesmo que "nega tudo": a sessao ainda pode trazer a dela.
    pub fn has_no_configured_roots(&self) -> bool {
        self.roots.is_empty()
    }

    /// As raizes que, **ja resolvidas**, desligam o jail na pratica: `/` e o
    /// `$HOME` do processo. O boot do gateway imprime cada uma.
    ///
    /// Resolver antes de comparar e o ponto: `$HOME/../$USER` e um symlink
    /// que aponta para o `$HOME` sao a mesma raiz perigosa escrita de outro
    /// jeito, e so o `canonicalize` achata `..` e segue link. (`/.` e `//.`
    /// ja caiam pelo `Path::parent`, que compara por componente — nao e
    /// merito daqui.) As raizes deste jail ja chegam canonicalizadas por
    /// [`FileJail::from_roots`].
    pub fn raizes_perigosas(&self) -> Vec<(&Path, &'static str)> {
        let home = process_home_dir();
        self.roots
            .iter()
            .filter_map(|root| {
                motivo_de_raiz_perigosa(root, home.as_deref())
                    .map(|motivo| (root.as_path(), motivo))
            })
            .collect()
    }

    /// Raizes efetivas desta chamada: as do operador mais o `working_dir` da
    /// sessao, quando ele existe e resolve.
    fn effective_roots(&self, session_dir: Option<&str>) -> Vec<PathBuf> {
        let mut roots = self.roots.clone();
        if let Some(wd) = session_dir.map(str::trim).filter(|w| !w.is_empty()) {
            match std::fs::canonicalize(wd) {
                Ok(resolved) => roots.push(resolved),
                Err(e) => warn!(
                    working_dir = %wd,
                    error = %e,
                    "working_dir da sessao ignorado como raiz: nao resolveu"
                ),
            }
        }
        roots
    }

    /// Resolve `path` e exige que o resultado caia sob uma das raizes
    /// efetivas. Devolve o **caminho resolvido** — e ele que a tool deve
    /// abrir, nao o original, senao a resolucao feita aqui nao vale nada.
    ///
    /// Um caminho que ainda nao existe e aceito quando o ancestral existente
    /// mais proximo esta dentro da raiz: e o caso da escrita, e tambem o que
    /// preserva a mensagem "nao encontrei" da #923 para leitura de arquivo
    /// ausente **dentro** da raiz (dizer que um arquivo da propria raiz nao
    /// existe nao vaza nada).
    pub fn confine(&self, path: &Path, session_dir: Option<&str>) -> Result<PathBuf, Denial> {
        let roots = self.effective_roots(session_dir);
        if roots.is_empty() {
            return Err(Denial::NoRoots);
        }
        let resolved = resolve_nearest_existing(path)?;
        if roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(Denial::Outside)
        }
    }
}

/// Canonicaliza uma raiz declarada. `None` (com `warn!`) quando ela nao
/// resolve: uma raiz inexistente nao pode autorizar nada.
fn canonicalizar_raiz(root: &Path) -> Option<PathBuf> {
    match std::fs::canonicalize(root) {
        Ok(resolved) => Some(resolved),
        Err(e) => {
            warn!(
                root = %root.display(),
                error = %e,
                "raiz de file tool ignorada: nao foi possivel resolver"
            );
            None
        }
    }
}

/// Por que uma raiz **ja canonicalizada** desliga o jail, ou `None`.
///
/// Recebe `resolvida` e `home` resolvidos pelo chamador — nucleo puro, para o
/// teste nao depender do `$HOME` da maquina que roda a suite.
fn motivo_de_raiz_perigosa(resolvida: &Path, home: Option<&Path>) -> Option<&'static str> {
    if resolvida.parent().is_none() {
        return Some("e a raiz do sistema (/)");
    }
    if home == Some(resolvida) {
        return Some("e o $HOME do processo (~/.ssh, ~/.aws, qualquer .env)");
    }
    None
}

/// O `$HOME` do processo, **canonicalizado**, para comparar com uma raiz que
/// tambem ja foi canonicalizada. Sem isso um home que e symlink (o
/// `/var` -> `/private/var` do macOS) nunca casaria.
fn process_home_dir() -> Option<PathBuf> {
    crate::tools::tool_context::process_home_dir().and_then(|home| std::fs::canonicalize(home).ok())
}

/// Canonicaliza `path`; se ele ainda nao existe, canonicaliza o ancestral
/// existente mais proximo e recola a cauda.
///
/// A cauda nao pode conter `..` — [`Component::ParentDir`] e recusado aqui
/// tambem, e nao so em `resolve_tool_path`, porque esta funcao e o ponto de
/// decisao e um `..` na cauda depois da canonicalizacao do ancestral
/// reescreveria o caminho depois da checagem.
///
/// Essa guarda **nao** e so defesa em profundidade: sem ela, um `..` depois
/// de um componente inexistente nao vira `Outside`, vira `Unresolvable` — a
/// subida chega a um caminho terminado em `..`, cujo `file_name()` e `None`.
/// As duas recusam, mas so a guarda diz *por que*, e e o que
/// `dotdot_depois_de_componente_inexistente_e_outside` fixa.
fn resolve_nearest_existing(path: &Path) -> Result<PathBuf, Denial> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Denial::Outside);
    }

    // Normaliza antes de qualquer `lstat`. Com barra final (`raiz/evil/`) o
    // `lstat` **segue** o symlink por POSIX, e um pendurado voltaria a parecer
    // inexistente — furando a checagem logo abaixo. `components()` derruba a
    // barra final, o `//` e o `.` do meio, e preserva `..` (ja recusado).
    let normalizado: PathBuf = path.components().collect();
    let path: &Path = &normalizado;

    match std::fs::canonicalize(path) {
        Ok(resolved) => return Ok(resolved),
        // Nao resolve, mas existe para o `lstat`: symlink pendurado ou ciclo.
        // O `open(O_CREAT)` segue esse link e escreve no alvo — que e o que a
        // recolagem da cauda nao ve. Ver o doc do modulo.
        Err(_) if pendurado(path) => return Err(Denial::Outside),
        Err(_) => {}
    }

    let mut cauda: Vec<std::ffi::OsString> = Vec::new();
    let mut atual = path;
    loop {
        let Some(nome) = atual.file_name() else {
            return Err(Denial::Unresolvable);
        };
        cauda.push(nome.to_os_string());
        let Some(pai) = atual.parent() else {
            return Err(Denial::Unresolvable);
        };
        // Caminho relativo que acabou os componentes: o pai vira `""`, que nao
        // canonicaliza. Ancora no CWD do processo — que e onde um caminho
        // relativo ja resolvia — e deixa a checagem de raiz decidir. No
        // gateway o CWD nao e raiz, entao isto nega; na CLI ele e, entao
        // `file_read Cargo.toml` continua funcionando.
        let pai = if pai.as_os_str().is_empty() {
            Path::new(".")
        } else {
            pai
        };
        match std::fs::canonicalize(pai) {
            Ok(base) => {
                let mut resolvido = base;
                for parte in cauda.iter().rev() {
                    resolvido.push(parte);
                }
                return Ok(resolvido);
            }
            // Mesmo furo, um nivel acima: `raiz/saida -> /fora/inexistente` e
            // um ancestral que nao resolve e existe.
            Err(_) if pendurado(pai) => return Err(Denial::Outside),
            Err(_) => {}
        }
        atual = pai;
    }
}

/// `true` quando o componente existe para o `lstat` mas nao canonicaliza —
/// symlink pendurado, ciclo de symlink. Chamado **so** depois de um
/// `canonicalize` que ja falhou.
fn pendurado(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raiz() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        (tmp, root)
    }

    #[test]
    fn aceita_arquivo_dentro_da_raiz() {
        let (_t, root) = raiz();
        let alvo = root.join("notas.md");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&alvo, None).expect("deve aceitar"), alvo);
    }

    #[test]
    fn aceita_arquivo_inexistente_com_pai_dentro_da_raiz() {
        let (_t, root) = raiz();
        let alvo = root.join("sub/novo.txt");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&alvo, None).expect("deve aceitar"), alvo);
    }

    /// Escrita num diretorio que ainda nao existe: sobe ate a raiz, que
    /// existe, e recola a cauda inteira.
    #[test]
    fn aceita_cauda_de_varios_niveis_inexistentes() {
        let (_t, root) = raiz();
        let alvo = root.join("a/b/c.txt");
        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&alvo, None).expect("deve aceitar"), alvo);
    }

    #[test]
    fn recusa_caminho_absoluto_fora_da_raiz() {
        let (_t, root) = raiz();
        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(Path::new("/etc/passwd"), None),
            Err(Denial::Outside)
        );
    }

    /// O vetor classico: symlink dentro da raiz apontando para fora. So pega
    /// porque a comparacao acontece **depois** de `canonicalize`.
    #[cfg(unix)]
    #[test]
    fn recusa_symlink_que_aponta_para_fora() {
        let (_t, root) = raiz();
        let link = root.join("fuga");
        std::os::unix::fs::symlink("/etc", &link).expect("symlink");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(&link.join("passwd"), None),
            Err(Denial::Outside)
        );
        assert_eq!(jail.confine(&link, None), Err(Denial::Outside));
    }

    /// Symlink de **diretorio pai** para fora, com o alvo ainda inexistente:
    /// e o caso da escrita, e e por isso que a subida ate o ancestral
    /// existente canonicaliza em vez de so cortar componentes.
    #[cfg(unix)]
    #[test]
    fn recusa_escrita_atraves_de_pai_symlink_para_fora() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let link = root.join("saida");
        std::os::unix::fs::symlink(&fora, &link).expect("symlink");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(&link.join("novo.txt"), None),
            Err(Denial::Outside)
        );
    }

    #[test]
    fn recusa_dotdot() {
        let (_t, root) = raiz();
        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(&root.join("../fora.txt"), None),
            Err(Denial::Outside)
        );
    }

    /// Armadilha do prefixo de string: `/tmp/raiz-evil` nao esta sob
    /// `/tmp/raiz`. `Path::starts_with` compara componentes; `str` nao.
    #[test]
    fn irmao_com_prefixo_de_string_comum_esta_fora() {
        let (_t, root) = raiz();
        let irmao = root.parent().expect("parent").join(format!(
            "{}-evil",
            root.file_name().expect("nome").to_string_lossy()
        ));
        std::fs::create_dir_all(&irmao).expect("mkdir");
        let alvo = irmao.join("x.txt");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::from_roots([&root]);
        let got = jail.confine(&alvo, None);
        std::fs::remove_dir_all(&irmao).ok();
        assert_eq!(got, Err(Denial::Outside));
    }

    /// Fail-closed: sem raiz de config e sem sessao, nada passa — nem um
    /// arquivo que existe.
    #[test]
    fn sem_raiz_nenhuma_nega_tudo() {
        let (_t, root) = raiz();
        let alvo = root.join("ok.txt");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::sessions_only();
        assert_eq!(jail.confine(&alvo, None), Err(Denial::NoRoots));
    }

    /// O `working_dir` da sessao e raiz por si so — e o default do gateway.
    #[test]
    fn working_dir_da_sessao_autoriza() {
        let (_t, root) = raiz();
        let alvo = root.join("ok.txt");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::sessions_only();
        let wd = root.to_string_lossy().into_owned();
        assert_eq!(jail.confine(&alvo, Some(&wd)).expect("deve aceitar"), alvo);
        // ... e nao alarga para o irmao.
        assert_eq!(
            jail.confine(Path::new("/etc/passwd"), Some(&wd)),
            Err(Denial::Outside)
        );
    }

    /// `working_dir` que nao resolve nao vira raiz — caso contrario uma
    /// string qualquer na sessao seria uma raiz fantasma.
    #[test]
    fn working_dir_inexistente_nao_vira_raiz() {
        let (_t, root) = raiz();
        let alvo = root.join("ok.txt");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::sessions_only();
        assert_eq!(
            jail.confine(&alvo, Some("/nao/existe/em/lugar/nenhum")),
            Err(Denial::NoRoots)
        );
    }

    #[test]
    fn working_dir_vazio_conta_como_ausente() {
        let (_t, root) = raiz();
        let alvo = root.join("ok.txt");
        std::fs::write(&alvo, b"x").expect("write");
        let jail = FileJail::sessions_only();
        assert_eq!(jail.confine(&alvo, Some("   ")), Err(Denial::NoRoots));
    }

    /// A mensagem e a mesma nas tres recusas: nada de oraculo de existencia.
    #[test]
    fn as_tres_recusas_dizem_a_mesma_coisa() {
        assert_eq!(Denial::NoRoots.message(), Denial::Outside.message());
        assert_eq!(Denial::Outside.message(), Denial::Unresolvable.message());
    }

    /// E ela nao carrega caminho nem raiz.
    #[test]
    fn a_mensagem_nao_vaza_caminho() {
        let (_t, root) = raiz();
        let jail = FileJail::from_roots([&root]);
        let erro = jail
            .confine(Path::new("/etc/shadow"), None)
            .expect_err("deve recusar");
        let msg = erro.message();
        assert!(!msg.contains("/etc"), "{msg}");
        assert!(!msg.contains("shadow"), "{msg}");
        assert!(!msg.contains(&*root.to_string_lossy()), "{msg}");
    }

    #[test]
    fn raiz_que_nao_resolve_e_descartada() {
        let jail = FileJail::from_roots(["/nao/existe/mesmo"]);
        assert!(jail.roots().is_empty());
        assert!(jail.has_no_configured_roots());
    }

    // ─── #1244 rodada 2: symlink pendurado ────────────────────────────────

    /// O furo que a auditoria R4 achou: `canonicalize` falhar nao quer dizer
    /// "nao existe". Um symlink pendurado dentro da raiz falha no
    /// `canonicalize`, existe para o `lstat`, e o `open(O_CREAT)` da escrita
    /// segue o link e cria o arquivo **fora**. Com a cauda recolada dentro da
    /// raiz, o `starts_with` aprovava.
    ///
    /// Remova o `Err(_) if pendurado(path)` e este teste fica vermelho.
    #[cfg(unix)]
    #[test]
    fn recusa_symlink_pendurado_para_fora() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        // O alvo nao existe: e disso que vem o "pendurado".
        let alvo_fora = fora.join("authorized_keys");
        let link = root.join("evil");
        std::os::unix::fs::symlink(&alvo_fora, &link).expect("symlink");
        assert!(!alvo_fora.exists(), "precondicao: alvo nao existe");
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "precondicao: o link existe para o lstat"
        );

        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&link, None), Err(Denial::Outside));
    }

    /// Mesma coisa com barra final. `lstat("raiz/evil/")` **segue** o symlink
    /// por POSIX e devolve ENOENT, entao sem a normalizacao por
    /// `components()` o pendurado volta a parecer inexistente e passa.
    #[cfg(unix)]
    #[test]
    fn recusa_symlink_pendurado_com_barra_final() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let link = root.join("evil");
        std::os::unix::fs::symlink(fora.join("authorized_keys"), &link).expect("symlink");

        let com_barra = PathBuf::from(format!("{}/", link.to_str().expect("utf8")));
        assert!(
            std::fs::symlink_metadata(&com_barra).is_err(),
            "precondicao: com barra final o lstat segue o link e falha"
        );

        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&com_barra, None), Err(Denial::Outside));
    }

    /// A variante com o pendurado como **diretorio pai**: `raiz/saida` aponta
    /// para um diretorio que nao existe fora da raiz, e o pedido e por um
    /// arquivo debaixo dele. O furo estava um nivel acima da folha.
    #[cfg(unix)]
    #[test]
    fn recusa_pai_symlink_pendurado_para_fora() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let link = root.join("saida");
        std::os::unix::fs::symlink(fora.join("dir-inexistente"), &link).expect("symlink");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(&link.join("arquivo.txt"), None),
            Err(Denial::Outside)
        );
    }

    /// O preco do fail-closed, dito em teste para ninguem "consertar" sem
    /// perceber: pendurado apontando para **dentro** da raiz tambem e
    /// recusado. Distinguir exigiria resolver symlink a mao.
    #[cfg(unix)]
    #[test]
    fn pendurado_para_dentro_tambem_e_recusado() {
        let (_t, root) = raiz();
        let link = root.join("interno");
        std::os::unix::fs::symlink(root.join("ainda-nao-existe"), &link).expect("symlink");

        let jail = FileJail::from_roots([&root]);
        assert_eq!(jail.confine(&link, None), Err(Denial::Outside));
    }

    /// E a recusa continua sendo a mesma frase: o furo tinha reaberto o
    /// oraculo de existencia (alvo vivo dava `DENIAL_MESSAGE`, alvo morto
    /// escrevia e respondia `Ok`).
    #[cfg(unix)]
    #[test]
    fn pendurado_e_link_vivo_recusam_igual() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let vivo = root.join("vivo");
        let morto = root.join("morto");
        std::os::unix::fs::symlink(&fora, &vivo).expect("symlink");
        std::os::unix::fs::symlink(fora.join("nao-existe"), &morto).expect("symlink");

        let jail = FileJail::from_roots([&root]);
        let a = jail.confine(&vivo.join("x.txt"), None).expect_err("vivo");
        let b = jail.confine(&morto, None).expect_err("morto");
        assert_eq!(a.message(), b.message());
    }

    // ─── #1244 rodada 2: a guarda de `..` ─────────────────────────────────

    /// `recusa_dotdot` passa pelo caminho normal (o `canonicalize` do pai
    /// achata o `..` e cai fora), entao nao fixa a guarda: apagar
    /// `Component::ParentDir` de `resolve_nearest_existing` deixava a suite
    /// inteira verde. Aqui o pai **nao existe**, o `canonicalize` nao achata
    /// nada, a subida chega a um caminho terminado em `..` e `file_name()`
    /// devolve `None` — sem a guarda isto vira `Unresolvable`.
    #[test]
    fn dotdot_depois_de_componente_inexistente_e_outside() {
        let (_t, root) = raiz();
        let jail = FileJail::from_roots([&root]);
        assert_eq!(
            jail.confine(&root.join("naoexiste/../fora.txt"), None),
            Err(Denial::Outside)
        );
    }

    // ─── #1244 rodada 2: raiz que desliga o jail ──────────────────────────

    #[test]
    fn raiz_do_sistema_e_perigosa() {
        assert!(motivo_de_raiz_perigosa(Path::new("/"), None).is_some());
    }

    #[test]
    fn raiz_de_projeto_nao_e_perigosa() {
        let (_t, root) = raiz();
        assert!(motivo_de_raiz_perigosa(&root, Some(Path::new("/home/u"))).is_none());
    }

    #[test]
    fn home_e_perigoso_e_so_o_home() {
        let (_t, home) = raiz();
        let outro = home.join("projeto");
        std::fs::create_dir_all(&outro).expect("mkdir");
        assert!(motivo_de_raiz_perigosa(&home, Some(&home)).is_some());
        assert!(motivo_de_raiz_perigosa(&outro, Some(&home)).is_none());
        // Sem `$HOME` conhecido nao se chuta.
        assert!(motivo_de_raiz_perigosa(&home, None).is_none());
    }

    /// F5: a comparacao acontece **depois** do canonicalize. `$HOME/../$HOME`
    /// e `raiz/.` sao a mesma raiz escrita de outro jeito, e a comparacao
    /// textual (`path.parent().is_none()`) nao ve nenhuma das duas.
    #[test]
    fn raiz_perigosa_e_detectada_depois_de_resolver() {
        let (_t, home) = raiz();
        let nome = home.file_name().expect("nome").to_owned();
        let rodeio = home.join("..").join(&nome).join(".");

        let jail = FileJail::from_roots([&rodeio]);
        assert_eq!(jail.roots(), [home.clone()], "a raiz tinha de resolver");

        // `raizes_perigosas` usa o `$HOME` real do processo, que nao e este
        // tempdir; o nucleo puro e quem prova a deteccao.
        let resolvida = &jail.roots()[0];
        assert!(motivo_de_raiz_perigosa(resolvida, Some(&home)).is_some());
    }

    /// `/.` e guardado como `/` e acusado sem depender de nenhum `$HOME`.
    #[test]
    fn barra_ponto_e_acusada_como_raiz_do_sistema() {
        let jail = FileJail::from_roots(["/."]);
        assert_eq!(jail.roots(), [PathBuf::from("/")]);
        let perigosas = jail.raizes_perigosas();
        assert_eq!(perigosas.len(), 1, "{perigosas:?}");
        assert_eq!(perigosas[0].0, Path::new("/"));
    }

    #[test]
    fn jail_de_projeto_nao_tem_raiz_perigosa() {
        let (_t, root) = raiz();
        assert!(FileJail::from_roots([&root]).raizes_perigosas().is_empty());
    }

    #[test]
    fn varias_raizes_basta_cair_sob_uma() {
        let (_a, root_a) = raiz();
        let (_b, root_b) = raiz();
        let alvo = root_b.join("x.txt");
        std::fs::write(&alvo, b"x").expect("write");

        let jail = FileJail::from_roots([&root_a, &root_b]);
        assert_eq!(jail.confine(&alvo, None).expect("deve aceitar"), alvo);
    }
}
