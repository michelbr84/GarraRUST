//! Resolução do executável do GarraIA Desktop — **leitura, nunca execução**.
//!
//! O M1 do épico #1181 inverte o controle que existia até aqui. Hoje é o
//! desktop que chama a CLI (o `garraia` vai dentro do bundle como sidecar
//! Tauri); com `garra desktop` passa a existir também o caminho CLI→Desktop.
//!
//! Este módulo responde a única pergunta difícil dessa inversão: **onde está o
//! aplicativo?**. Ele mora no núcleo sem Tauri, e não na CLI, por dois motivos:
//!
//! 1. É aqui que o ADR 0021 colocou a resolução de caminhos, junto do resto da
//!    lógica que entra nos gates obrigatórios de CI — a casca Tauri não entra.
//! 2. A casca vai precisar da mesma resposta mais tarde (M2+), e uma segunda
//!    cópia da lista de diretórios de instalação divergiria da primeira no
//!    primeiro instalador novo.
//!
//! # A ordem de resolução, e por que ela é essa
//!
//! Caminho de instalação da plataforma → `PATH` → diretório do próprio
//! executável (instalação lado-a-lado), como fixado no ADR 0021.
//!
//! O caminho de instalação vem primeiro de propósito: é o único dos três que
//! um terceiro não consegue ocupar por acidente. Um `garraia-desktop` que
//! apareça na `PATH` do usuário pode ser qualquer coisa, e é por isso que ele é
//! a segunda opção, não a primeira.
//!
//! # O invariante que este módulo protege
//!
//! **A CLI nunca pode resolver para si mesma.** O `.deb` do desktop instala os
//! dois binários lado a lado (`/usr/bin/garraia-desktop` e `/usr/bin/garraia`,
//! com `Provides/Conflicts/Replaces: garraia`), então para quem instalou pelo
//! pacote a CLI *é* o sidecar de dentro do bundle — e um lançamento recursivo
//! ali seria um fork bomb com nome de feature. O nome procurado já é diferente
//! do da CLI, mas "já é diferente" não é garantia: [`Locator::new`] recebe o
//! caminho do próprio executável e recusa qualquer candidato igual a ele, e um
//! teste fixa esse comportamento.
//!
//! Como [`crate::detect`], aqui só se pergunta ao [`Filesystem`] se um caminho
//! existe. Não há escrita e não há `std::process` — resolver é leitura;
//! **lançar é decisão de quem chama**, e acontece na CLI, com o caminho
//! visível antes.

use crate::detect::{Filesystem, RealFs};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Nome do executável do aplicativo desktop.
///
/// É o `productName` do `tauri.conf.json`, que é o que dá nome ao binário em
/// todos os bundles (`.deb`, `.AppImage`, MSI, NSIS e o `.app` do macOS).
/// Mudar um sem o outro quebra a resolução em toda instalação existente.
pub const DESKTOP_BINARY: &str = "garraia-desktop";

/// Extensões de executável do Windows, na ordem em que o shell as tentaria.
const WINDOWS_EXTENSIONS: [&str; 2] = ["exe", "cmd"];

/// Onde o aplicativo foi encontrado.
///
/// Aparece no `garra desktop --status` porque a origem muda o diagnóstico:
/// "achei na `PATH`" e "achei onde o instalador põe" são situações diferentes
/// quando algo dá errado depois.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Diretório de instalação da plataforma.
    Install,
    /// Um diretório da `PATH`.
    Path,
    /// Ao lado do próprio executável da CLI.
    SideBySide,
}

impl Source {
    /// Descrição curta, para a saída humana.
    pub fn describe(self) -> &'static str {
        match self {
            Source::Install => "caminho de instalacao da plataforma",
            Source::Path => "PATH",
            Source::SideBySide => "ao lado da CLI",
        }
    }
}

/// O aplicativo desktop encontrado no sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopApp {
    pub path: PathBuf,
    pub source: Source,
}

/// Plataforma, para escolher a lista de diretórios de instalação.
///
/// É um parâmetro e não um `cfg!` embutido justamente para ser testável: a
/// lista do Windows precisa de teste mesmo quando o CI que roda o teste é
/// Linux, e um `cfg!` tornaria as outras duas listas código que ninguém
/// exercita até alguém instalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
    /// Qualquer outro alvo: sem aplicativo desktop empacotado.
    Other,
}

impl Platform {
    /// A plataforma em que este binário está rodando.
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Other
        }
    }
}

/// Diretórios que vêm do ambiente e entram na montagem da lista de instalação.
///
/// Injetados em vez de lidos aqui dentro pelo mesmo motivo do
/// [`crate::detect::Detector`]: teste não deveria depender de variável de
/// ambiente global. Todos são opcionais e ausência degrada para "esse
/// candidato não existe", nunca para panic.
#[derive(Debug, Clone, Default)]
pub struct EnvDirs {
    pub home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub program_files_x86: Option<PathBuf>,
}

impl EnvDirs {
    /// Lê do ambiente do processo. `HOME` (ou `USERPROFILE` no Windows) e as
    /// variáveis de diretório do Windows, todas opcionais.
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            home: var("HOME").or_else(|| var("USERPROFILE")),
            local_app_data: var("LOCALAPPDATA"),
            program_files: var("ProgramFiles"),
            program_files_x86: var("ProgramFiles(x86)"),
        }
    }
}

/// Diretórios onde cada instalador oficial deixa o executável do desktop.
///
/// Ordem estável e sem duplicatas — é a ordem em que a resolução tenta.
pub fn install_dirs(platform: Platform, env: &EnvDirs) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    };

    match platform {
        // `.deb`/`.rpm` caem em `/usr/bin`; build local e AppImage extraído
        // costumam ficar em `/usr/local/bin` ou no `~/.local/bin`.
        Platform::Linux => {
            push(PathBuf::from("/usr/bin"));
            push(PathBuf::from("/usr/local/bin"));
            if let Some(home) = &env.home {
                push(home.join(".local/bin"));
                push(home.join("Applications"));
            }
            push(PathBuf::from("/opt").join(DESKTOP_BINARY));
        }
        // O bundle do macOS é `<productName>.app`, e o executável mora em
        // `Contents/MacOS/<productName>`.
        Platform::MacOs => {
            let app = format!("{DESKTOP_BINARY}.app");
            push(
                PathBuf::from("/Applications")
                    .join(&app)
                    .join("Contents/MacOS"),
            );
            if let Some(home) = &env.home {
                push(home.join("Applications").join(&app).join("Contents/MacOS"));
                push(home.join(".local/bin"));
            }
            push(PathBuf::from("/usr/local/bin"));
        }
        // NSIS instala por usuário em `%LOCALAPPDATA%`; o MSI, por máquina em
        // `%ProgramFiles%`. As duas grafias de Program Files entram porque uma
        // instalação de 32 bits num Windows de 64 cai na `(x86)`.
        Platform::Windows => {
            if let Some(lad) = &env.local_app_data {
                push(lad.join("Programs").join(DESKTOP_BINARY));
                push(lad.join(DESKTOP_BINARY));
            }
            if let Some(pf) = &env.program_files {
                push(pf.join(DESKTOP_BINARY));
            }
            if let Some(pf86) = &env.program_files_x86 {
                push(pf86.join(DESKTOP_BINARY));
            }
        }
        // Termux, BSD, container headless: não há instalador de desktop, e
        // dizer isso é melhor do que procurar num lugar que não existe.
        Platform::Other => {}
    }

    dirs
}

/// Resolução do executável do desktop.
///
/// Tudo é injetado: os diretórios das três etapas e o caminho do próprio
/// executável. [`Locator::from_env`] é o atalho para o caso real.
pub struct Locator<F: Filesystem> {
    fs: F,
    install_dirs: Vec<PathBuf>,
    path_dirs: Vec<PathBuf>,
    side_by_side: Option<PathBuf>,
    self_exe: Option<PathBuf>,
}

impl<F: Filesystem> Locator<F> {
    pub fn new(
        fs: F,
        install_dirs: Vec<PathBuf>,
        path_dirs: Vec<PathBuf>,
        side_by_side: Option<PathBuf>,
        self_exe: Option<PathBuf>,
    ) -> Self {
        Self {
            fs,
            install_dirs,
            path_dirs,
            side_by_side,
            self_exe,
        }
    }

    /// O primeiro executável do desktop encontrado, na ordem do ADR 0021.
    ///
    /// `None` quer dizer "não está instalado onde eu sei procurar" — não é
    /// erro, é o caso normal em máquina sem desktop, e quem chama transforma
    /// isso numa instrução de instalação.
    pub fn locate(&self) -> Option<DesktopApp> {
        let stages: [(&[PathBuf], Source); 2] = [
            (&self.install_dirs, Source::Install),
            (&self.path_dirs, Source::Path),
        ];

        for (dirs, source) in stages {
            if let Some(path) = self.first_match(dirs) {
                return Some(DesktopApp { path, source });
            }
        }

        let side = self.side_by_side.clone()?;
        let path = self.first_match(std::slice::from_ref(&side))?;
        Some(DesktopApp {
            path,
            source: Source::SideBySide,
        })
    }

    fn first_match(&self, dirs: &[PathBuf]) -> Option<PathBuf> {
        for dir in dirs {
            for candidate in candidates_in(dir) {
                if self.fs.is_file(&candidate) && !self.is_self(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// O candidato é o executável que está rodando agora?
    ///
    /// Sem esta recusa, um empacotamento que instalasse a CLI com o nome do
    /// aplicativo faria `garra desktop` lançar `garra desktop` — recursão que
    /// só apareceria na máquina do usuário. Fail-closed: na dúvida (sem saber
    /// o próprio caminho) não há como ser igual, e a comparação é sobre os
    /// caminhos como foram montados, sem resolver symlink (resolver exigiria
    /// I/O que este módulo não faz).
    fn is_self(&self, candidate: &Path) -> bool {
        self.self_exe.as_deref() == Some(candidate)
    }
}

impl Locator<RealFs> {
    /// Locator sobre o ambiente real: plataforma, `PATH` e o próprio
    /// executável. Cada peça ausente vira "não achei por esse caminho".
    pub fn from_env() -> Self {
        let install = install_dirs(Platform::current(), &EnvDirs::from_env());
        let path_dirs = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let self_exe = std::env::current_exe().ok();
        let side_by_side = self_exe
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Self::new(RealFs, install, path_dirs, side_by_side, self_exe)
    }
}

/// Nomes de arquivo a tentar dentro de um diretório, em ordem.
fn candidates_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.join(DESKTOP_BINARY)];
    out.extend(
        WINDOWS_EXTENSIONS
            .iter()
            .map(|ext| dir.join(format!("{DESKTOP_BINARY}.{ext}"))),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct FakeFs {
        files: BTreeSet<PathBuf>,
    }

    impl FakeFs {
        fn with_file(mut self, p: &str) -> Self {
            self.files.insert(PathBuf::from(p));
            self
        }
    }

    impl Filesystem for FakeFs {
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
    }

    fn locator(fs: FakeFs) -> Locator<FakeFs> {
        Locator::new(
            fs,
            vec![PathBuf::from("/usr/bin")],
            vec![PathBuf::from("/home/u/bin")],
            Some(PathBuf::from("/opt/garra")),
            Some(PathBuf::from("/opt/garra/garra")),
        )
    }

    #[test]
    fn sem_aplicativo_instalado_nao_resolve_nada() {
        assert_eq!(locator(FakeFs::default()).locate(), None);
    }

    #[test]
    fn o_caminho_de_instalacao_vence_a_path() {
        // A `PATH` é do usuário e qualquer um pode plantar um nome nela; o
        // diretório do instalador é o que temos de mais confiável.
        let l = locator(
            FakeFs::default()
                .with_file("/usr/bin/garraia-desktop")
                .with_file("/home/u/bin/garraia-desktop"),
        );
        let found = l.locate().expect("resolve o aplicativo");
        assert_eq!(found.path, PathBuf::from("/usr/bin/garraia-desktop"));
        assert_eq!(found.source, Source::Install);
    }

    #[test]
    fn a_path_vence_a_instalacao_lado_a_lado() {
        let l = locator(
            FakeFs::default()
                .with_file("/home/u/bin/garraia-desktop")
                .with_file("/opt/garra/garraia-desktop"),
        );
        let found = l.locate().expect("resolve o aplicativo");
        assert_eq!(found.path, PathBuf::from("/home/u/bin/garraia-desktop"));
        assert_eq!(found.source, Source::Path);
    }

    #[test]
    fn instalacao_lado_a_lado_e_a_ultima_chance() {
        let l = locator(FakeFs::default().with_file("/opt/garra/garraia-desktop"));
        let found = l.locate().expect("resolve o aplicativo");
        assert_eq!(found.path, PathBuf::from("/opt/garra/garraia-desktop"));
        assert_eq!(found.source, Source::SideBySide);
    }

    #[test]
    fn acha_executavel_com_extensao_do_windows() {
        let l = locator(FakeFs::default().with_file("/usr/bin/garraia-desktop.exe"));
        let found = l.locate().expect("resolve o aplicativo");
        assert_eq!(found.path, PathBuf::from("/usr/bin/garraia-desktop.exe"));
    }

    /// O invariante do ADR 0021: `garra desktop` lança o aplicativo, **nunca a
    /// si mesmo**. No `.deb` os dois binários são irmãos no mesmo diretório, e
    /// é ali que uma recursão nasceria.
    #[test]
    fn nunca_resolve_para_o_proprio_executavel() {
        let l = Locator::new(
            FakeFs::default().with_file("/usr/bin/garraia-desktop"),
            vec![PathBuf::from("/usr/bin")],
            vec![PathBuf::from("/usr/bin")],
            Some(PathBuf::from("/usr/bin")),
            // A própria CLI, instalada com o nome do aplicativo.
            Some(PathBuf::from("/usr/bin/garraia-desktop")),
        );
        assert_eq!(
            l.locate(),
            None,
            "resolver para o proprio binario seria lancamento recursivo"
        );
    }

    #[test]
    fn a_cli_irma_no_mesmo_diretorio_nao_atrapalha() {
        // Caso real do `.deb`: `/usr/bin/garraia` (a CLI, que está rodando) e
        // `/usr/bin/garraia-desktop` (o aplicativo) lado a lado.
        let l = Locator::new(
            FakeFs::default()
                .with_file("/usr/bin/garraia")
                .with_file("/usr/bin/garraia-desktop"),
            vec![PathBuf::from("/usr/bin")],
            vec![],
            Some(PathBuf::from("/usr/bin")),
            Some(PathBuf::from("/usr/bin/garraia")),
        );
        let found = l.locate().expect("resolve o aplicativo irmao");
        assert_eq!(found.path, PathBuf::from("/usr/bin/garraia-desktop"));
    }

    #[test]
    fn diretorios_de_instalacao_do_linux() {
        let env = EnvDirs {
            home: Some(PathBuf::from("/home/u")),
            ..Default::default()
        };
        let dirs = install_dirs(Platform::Linux, &env);
        assert_eq!(dirs.first(), Some(&PathBuf::from("/usr/bin")));
        assert!(dirs.contains(&PathBuf::from("/home/u/.local/bin")));
    }

    #[test]
    fn diretorios_de_instalacao_do_macos_apontam_para_dentro_do_bundle() {
        let env = EnvDirs {
            home: Some(PathBuf::from("/Users/u")),
            ..Default::default()
        };
        let dirs = install_dirs(Platform::MacOs, &env);
        assert_eq!(
            dirs.first(),
            Some(&PathBuf::from(
                "/Applications/garraia-desktop.app/Contents/MacOS"
            )),
            "no macOS o executavel mora dentro do .app, nao ao lado dele"
        );
    }

    #[test]
    fn diretorios_de_instalacao_do_windows_cobrem_nsis_e_msi() {
        let env = EnvDirs {
            local_app_data: Some(PathBuf::from(r"C:\Users\u\AppData\Local")),
            program_files: Some(PathBuf::from(r"C:\Program Files")),
            program_files_x86: Some(PathBuf::from(r"C:\Program Files (x86)")),
            ..Default::default()
        };
        let dirs = install_dirs(Platform::Windows, &env);
        // As esperas sao montadas com `join`, e nao escritas com `\`, porque
        // este teste roda no runner Linux: la o separador que o `join` insere e
        // `/`, e um literal com barra invertida nunca casaria.
        let lad = PathBuf::from(r"C:\Users\u\AppData\Local");
        // NSIS (por usuario) antes do MSI (por maquina): o per-user e o default
        // do instalador, entao e o que tem mais chance de ser o mais recente.
        assert_eq!(
            dirs.first(),
            Some(&lad.join("Programs").join(DESKTOP_BINARY))
        );
        assert!(dirs.contains(&PathBuf::from(r"C:\Program Files").join(DESKTOP_BINARY)));
        assert!(dirs.contains(&PathBuf::from(r"C:\Program Files (x86)").join(DESKTOP_BINARY)));
    }

    #[test]
    fn ambiente_vazio_nao_inventa_diretorio() {
        let vazio = EnvDirs::default();
        assert!(install_dirs(Platform::Windows, &vazio).is_empty());
        assert!(install_dirs(Platform::Other, &vazio).is_empty());
        // No Linux os caminhos absolutos não dependem de `$HOME`.
        assert!(!install_dirs(Platform::Linux, &vazio).is_empty());
    }

    #[test]
    fn a_lista_de_instalacao_nao_repete_diretorio() {
        let env = EnvDirs {
            local_app_data: Some(PathBuf::from("C:/x")),
            program_files: Some(PathBuf::from("C:/x")),
            program_files_x86: Some(PathBuf::from("C:/x")),
            ..Default::default()
        };
        let dirs = install_dirs(Platform::Windows, &env);
        let unicos: BTreeSet<_> = dirs.iter().collect();
        assert_eq!(unicos.len(), dirs.len(), "{dirs:?}");
    }

    #[test]
    fn o_nome_do_binario_segue_o_product_name_do_bundle() {
        // Guarda contra "simplificar" para `garraia`: esse é o sidecar da CLI
        // dentro do bundle, e apontar para ele seria lançar a CLI.
        assert_eq!(DESKTOP_BINARY, "garraia-desktop");
        assert_ne!(DESKTOP_BINARY, "garraia");
    }

    #[test]
    fn a_origem_serializa_com_nomes_estaveis() {
        let app = DesktopApp {
            path: PathBuf::from("/usr/bin/garraia-desktop"),
            source: Source::SideBySide,
        };
        let json = serde_json::to_value(&app).expect("DesktopApp e serializavel");
        assert_eq!(json["source"], "side_by_side");
    }

    /// Mesma garantia estrutural do `detect.rs`: resolver é leitura. Lançar o
    /// aplicativo é decisão explícita de quem chama, na CLI, e com o caminho
    /// impresso antes — não algo que a resolução possa fazer sozinha.
    #[test]
    fn o_modulo_de_resolucao_nunca_executa_binario() {
        let fonte = include_str!("locate.rs");
        let ate_o_teste = fonte
            .split("fn o_modulo_de_resolucao_nunca_executa_binario")
            .next()
            .unwrap_or(fonte);
        let corpo: String = ate_o_teste
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for proibido in [
            "std::process",
            "Command::new",
            "process::Command",
            "Stdio",
            ".status()",
            ".output()",
            ".spawn()",
        ] {
            assert!(
                !corpo.contains(proibido),
                "locate.rs nao pode conter `{proibido}`: resolver e leitura"
            );
        }
    }
}
