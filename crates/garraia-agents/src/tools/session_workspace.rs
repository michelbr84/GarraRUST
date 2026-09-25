//! O workspace padrao das file tools, **escopado por sessao** (#1449).
//!
//! ## Por que isto existe
//!
//! A #1378 deu as sessoes sem `working_dir` uma raiz default —
//! `<data_dir>/workspace` — para que `file_read`, `file_write` e `list_dir`
//! deixassem de negar tudo numa instalacao limpa. A primeira versao a colocou
//! como **raiz fixa do [`FileJail`]**, e ai estava o defeito que a auditoria
//! R4 (#1449) achou: raiz fixa e a mesma para toda sessao, todo canal e todo
//! principal. Um contato do WhatsApp escrevia ali e o turno de outro contato
//! lia — disclosure cross-principal — e o que um escreveu voltava no contexto
//! do outro sem passar pelo guard de injecao de entrada, que e um vetor
//! persistente de prompt-injection indireta. Nada disso existia antes da
//! #1378: ali toda chamada caia em [`super::file_jail::Denial::NoRoots`].
//!
//! ## A correcao
//!
//! O workspace padrao **nao** volta a ser raiz do jail. Cada sessao ganha um
//! subdiretorio proprio, `<data_dir>/workspace/<sessao>`, que entra como o
//! `session_dir` da chamada — o mesmo parametro por onde o `working_dir` de
//! uma sessao com projeto ja passava desde a #1244. Duas consequencias, e as
//! duas sao a propriedade de seguranca:
//!
//! 1. No caso "workspace padrao" o jail fica **sem raiz fixa**
//!    ([`FileJail::sessions_only`]), entao a unica raiz efetiva de uma chamada
//!    e o diretorio daquela sessao. A sessao A nao tem como alcancar o
//!    diretorio da sessao B porque o diretorio de B nunca e raiz na chamada de
//!    A — o isolamento sai da estrutura, nao de uma checagem a mais.
//! 2. Nao ha um segundo mecanismo de confinamento para auditar: quem decide
//!    continua sendo [`FileJail::confine`], com as mesmas recusas e a mesma
//!    mensagem unica.
//!
//! ## Por que o nome do subdiretorio e um hash, e nao o `session_id`
//!
//! Duas razoes independentes, e cada uma bastaria:
//!
//! - **O `session_id` e dado nao confiavel para fins de caminho.** Ele pode
//!   trazer `..`, separador de diretorio ou caractere de controle. Sanitizar
//!   por substituicao nao serve: `a/b` e `a_b` colapsariam no **mesmo**
//!   diretorio, que e exatamente a colisao cross-sessao que esta correcao
//!   existe para fechar. Um hash e injetivo na pratica, e o conjunto de
//!   caracteres que ele produz (`[0-9a-f]`) nao tem como escapar do pai.
//! - **O `session_id` pode ser PII.** No WhatsApp ele carrega identificador de
//!   contato; gravar isso como nome de diretorio deixaria a lista de quem
//!   falou com o gateway legivel no disco, e nos logs que citam o caminho.
//!
//! O tamanho (128 bits de SHA-256, 32 caracteres hex) e folgado contra
//! colisao e curto o suficiente para nao esbarrar em limite de caminho no
//! Windows.
//!
//! ## Qual e exatamente a fronteira: a SESSAO, nao o usuario
//!
//! O escopo e a chave de sessao que o canal ja usa para o historico da
//! conversa — no `whatsapp_linked`, `whatsapp-linked-<chat_jid>`, derivada
//! pelo servidor a partir do JID da conversa, nunca de conteudo que o
//! remetente escolhe. **Essa garantia vale pela forca da identidade de
//! sessao na superficie de entrada, nao e universal:** a rota compativel com
//! OpenAI (`crates/garraia-gateway/src/openai_api.rs`, header
//! `X-Session-Id`) aceita o id verbatim, sem token, entao quem alcanca esse
//! endpoint escolhe o proprio `session_id` — inclusive o de uma conversa de
//! outro canal. Isso nao e uma regressao de classe: um `session_id` forjado
//! ali ja herda o `ExecContext` da sessao alvo — `working_dir` e modo —,
//! entao o acesso a arquivo desta correcao so segue a mesma identidade que
//! o request ja carregava, nao abre nada que o forjador nao tivesse. A rota
//! e coberta pelo mesmo gate de `gateway.api_key` das demais rotas de
//! conversa quando configurado, e o bind nao-loopback recusa subir sem
//! credencial — o caso sem credencial nenhuma e loopback-only, nao
//! "qualquer um na rede". Dito isso, esta correcao alarga levemente o raio
//! de um `session_id` forjado: antes da #1378 uma sessao sem projeto
//! declarado nao tinha raiz nenhuma (`NoRoots`), e agora tem o workspace da
//! sessao que o forjador escolheu. Aceitavel — e a mesma identidade de
//! sempre, so que agora com algo para alcancar —, mas vale estar dito. O
//! isolamento por sessao e tao forte quanto a identidade de sessao naquela
//! rota especifica — nao mais forte. Duas consequencias na fronteira
//! server-derived (`whatsapp_linked` e as demais integracoes de canal), e
//! as duas sao deliberadas:
//!
//! - Conversas diferentes (contatos diferentes, canais diferentes) nunca se
//!   alcancam. E o que a #1449 pede.
//! - Numa conversa de **grupo** todos os membros compartilham a sessao, logo o
//!   diretorio. Isso nao vaza nada novo: quem esta no grupo ja le as mensagens
//!   e as respostas do agente naquele grupo, entao a fronteira do arquivo e a
//!   mesma fronteira que a conversa ja tinha. Escopar por remetente dentro do
//!   grupo daria ao agente um workspace que muda no meio do fio da conversa.
//!
//! Isto vale a pena dizer em voz alta porque o proximo leitor pode supor
//! isolamento por *pessoa*: nao e, e por desenho.
//!
//! ## Residual conhecido: os diretorios nao sao recolhidos
//!
//! Cada sessao que chega a invocar uma ferramenta deixa um diretorio, e nada
//! os apaga. A cardinalidade e a mesma das sessoes que o gateway ja guarda, e
//! um diretorio vazio custa um inode — mas num gateway de vida longa com
//! muitas conversas isso acumula. Nao ha GC aqui de proposito: apagar
//! diretorio de trabalho por idade e uma decisao de operacao (pode haver
//! trabalho dentro), e o lugar dela e uma issue propria, nao o caminho quente
//! de um turno.
//!
//! ## Fail-closed
//!
//! Nenhum ramo desta funcao inventa uma raiz mais larga. Se o pai nao e um
//! diretorio de verdade, se o subdiretorio da sessao esta ocupado por um
//! symlink, ou se a criacao falha, a resposta e `None` — e o turno cai no
//! fail-closed que a #1244 ja tinha: `file_read`, `file_write` e `list_dir`
//! recusam com a mensagem unica. Um `session_id` vazio tambem devolve `None`:
//! sem identidade de sessao nao ha o que escopar, e um diretorio "de todo
//! mundo" e o defeito de novo.
//!
//! ## Residual conhecido: TOCTOU no pai
//!
//! A checagem de que o pai e um diretorio real acontece antes do
//! `create_dir_all`. Entre um e outro, quem tem escrita no `data_dir` pode
//! trocar o pai por um symlink e o subdiretorio nasceria no alvo. E a mesma
//! janela — e a mesma pre-condicao, escrita dentro do diretorio do proprio
//! Garra — que o [`super::file_jail`] declara para o caminho do arquivo.
//! Fechar exigiria `openat2`/`mkdirat` com `RESOLVE_BENEATH`, sem equivalente
//! portatil nos tres sistemas que o projeto suporta.

use std::path::{Path, PathBuf};

use tracing::warn;

/// A raiz dentro da qual cada sessao ganha o seu proprio diretorio.
///
/// Construida **so** quando a fonte das raizes das file tools e o workspace
/// padrao: com `agent.file_roots` declarado a declaracao vence sozinha e nao
/// ha escopo por sessao nenhum, exatamente como antes da #1378.
///
/// A raiz chega ja canonicalizada por quem constroi (o boot do gateway), para
/// que o caminho que sai daqui possa ser comparado com o `data_dir` tambem
/// canonicalizado sem cair no fallback que imprimiria o caminho do host.
#[derive(Debug, Clone)]
pub struct SessionWorkspace {
    raiz: PathBuf,
}

/// Quantos bytes do digest entram no nome. 16 bytes = 128 bits = 32 hex.
const BYTES_DO_NOME: usize = 16;

impl SessionWorkspace {
    /// A raiz pai. O chamador ja verificou que ela e um diretorio de verdade e
    /// a canonicalizou; esta funcao nao toca o disco.
    pub fn nova(raiz: PathBuf) -> Self {
        Self { raiz }
    }

    /// O diretorio pai, o mesmo para todas as sessoes. E o que o
    /// `/api/diagnostics` mostra — nunca o de uma sessao.
    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// O nome do subdiretorio desta sessao: 32 caracteres hex, derivados do
    /// `session_id` por SHA-256. `None` quando o id e vazio (ou so espaco).
    ///
    /// Pura, deterministica e sem I/O: e ela que os testes usam para afirmar
    /// que duas sessoes distintas recebem diretorios distintos.
    pub fn nome_do_subdiretorio(session_id: &str) -> Option<String> {
        use sha2::{Digest, Sha256};
        // `trim()` so decide "vazio" — o digest e sobre os bytes CRUS do
        // `session_id`. Hashear o trimado quebraria a injetividade que este
        // modulo promete: dois ids que diferem so por espaco (inclusive um
        // NBSP invisivel) colidiriam no mesmo diretorio, e essa e exatamente
        // a colisao cross-sessao que a #1449 existe para fechar (achado da
        // revisao independente de seguranca: `session_key` do iMessage de
        // grupo e o `group_name`, texto livre renomeavel por qualquer
        // participante).
        if session_id.trim().is_empty() {
            return None;
        }
        let digest = Sha256::digest(session_id.as_bytes());
        Some(
            digest
                .iter()
                .take(BYTES_DO_NOME)
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    /// Onde o diretorio desta sessao **moraria**. Pura: nao cria nada e nao
    /// afirma que ele existe.
    pub fn caminho_da_sessao(&self, session_id: &str) -> Option<PathBuf> {
        Some(self.raiz.join(Self::nome_do_subdiretorio(session_id)?))
    }

    /// O diretorio desta sessao, criado se ainda nao existia.
    ///
    /// Preguicoso de proposito: o boot cria so o pai, e o subdiretorio nasce
    /// no primeiro turno que precisa dele. Assim uma instalacao nao ganha um
    /// diretorio por sessao historica na subida.
    ///
    /// Fail-closed em todos os desvios — ver o doc do modulo. Em unix o
    /// diretorio criado fecha em `0700`, a mesma disciplina que o pai recebe
    /// no boot: ele guarda o que o agente escreveu a pedido de um principal
    /// remoto.
    pub fn garantir_para_sessao(&self, session_id: &str) -> Option<PathBuf> {
        let caminho = self.caminho_da_sessao(session_id)?;

        // O pai foi verificado e canonicalizado no boot, mas o processo vive
        // por semanas: se alguem trocou `<data_dir>/workspace` por um symlink
        // depois disso, o `create_dir_all` abaixo atravessaria o link e o
        // diretorio da sessao — que e a UNICA raiz efetiva da chamada —
        // nasceria onde o link aponta. E a mesma recusa que o boot faz, no
        // momento do uso.
        if !diretorio_de_verdade(&self.raiz) {
            warn!(
                workspace = %self.raiz.display(),
                "workspace padrao das file tools nao e mais um diretorio de verdade: a sessao \
                 fica sem raiz em vez de herdar o alvo de um link (#1449)"
            );
            return None;
        }

        match std::fs::symlink_metadata(&caminho) {
            // Symlink no lugar do diretorio da sessao: mesma logica do pai.
            Ok(meta) if meta.file_type().is_symlink() => {
                warn!(
                    sessao = %nome_para_log(&caminho),
                    "diretorio da sessao no workspace padrao e um symlink: recusado para o jail \
                     nao herdar o alvo do link (#1449)"
                );
                return None;
            }
            Ok(meta) if !meta.is_dir() => {
                warn!(
                    sessao = %nome_para_log(&caminho),
                    "diretorio da sessao no workspace padrao existe e nao e diretorio: as file \
                     tools desta sessao negam tudo (#1449)"
                );
                return None;
            }
            // Ja existe e e diretorio: a permissao e a que ele recebeu quando
            // foi criado, e a subida nao a reescreve a cada turno.
            Ok(_) => return Some(caminho),
            Err(_) => {}
        }

        match std::fs::create_dir_all(&caminho) {
            Ok(()) => {
                clampar_permissao(&caminho);
                Some(caminho)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    sessao = %nome_para_log(&caminho),
                    "diretorio da sessao no workspace padrao nao pode ser criado: as file tools \
                     desta sessao negam tudo (#1449)"
                );
                None
            }
        }
    }
}

/// `true` so quando o caminho e um diretorio e **nao** e symlink.
/// `symlink_metadata` nao segue o link, que e o ponto.
fn diretorio_de_verdade(caminho: &Path) -> bool {
    match std::fs::symlink_metadata(caminho) {
        Ok(meta) => meta.is_dir() && !meta.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// So o nome do subdiretorio (o hash) para o log. O caminho completo diria
/// tambem o `data_dir` do host, e o `session_id` cru seria PII.
fn nome_para_log(caminho: &Path) -> String {
    caminho
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Fecha o diretorio recem-criado em `0700`. Fail-soft: nao conseguir nao
/// invalida a raiz, mas fica no log.
#[cfg(unix)]
fn clampar_permissao(caminho: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(0o700)) {
        warn!(
            error = %e,
            sessao = %nome_para_log(caminho),
            "permissao do diretorio da sessao nao pode ser fechada em 0700 (#1449)"
        );
    }
}

/// Sem equivalente portavel de `0700` fora de unix: o ACL do Windows herda do
/// pai, que ja e o `data_dir` do proprio usuario.
#[cfg(not(unix))]
fn clampar_permissao(_caminho: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn raiz() -> (tempfile::TempDir, SessionWorkspace) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let raiz = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        (tmp, SessionWorkspace::nova(raiz))
    }

    /// **O achado R4 da #1449, no nucleo puro.** Duas sessoes distintas nao
    /// podem receber o mesmo diretorio — se recebessem, uma leria o que a
    /// outra escreveu.
    #[test]
    fn sessoes_distintas_recebem_nomes_distintos() {
        let a = SessionWorkspace::nome_do_subdiretorio("whatsapp:+1555000111").expect("a");
        let b = SessionWorkspace::nome_do_subdiretorio("whatsapp:+1555000222").expect("b");
        assert_ne!(a, b, "duas sessoes colidiram no mesmo diretorio");
    }

    /// E a mesma sessao sempre recebe o mesmo — senao o que ela escreveu num
    /// turno desaparecia no turno seguinte.
    #[test]
    fn a_mesma_sessao_recebe_sempre_o_mesmo_nome() {
        let a = SessionWorkspace::nome_do_subdiretorio("sessao-1").expect("a");
        let b = SessionWorkspace::nome_do_subdiretorio("sessao-1").expect("b");
        assert_eq!(a, b);
    }

    /// O nome e hex puro: nada que possa sair do pai, e nada do `session_id`
    /// em claro (ele pode ser PII, e pode trazer `..` ou separador).
    #[test]
    fn o_nome_nao_carrega_caminho_nem_o_id_em_claro() {
        for id in [
            "../../etc",
            "a/b",
            "a\\b",
            "sessao com espaco",
            "whatsapp:+15550001111",
            "com\0nulo",
        ] {
            let nome = SessionWorkspace::nome_do_subdiretorio(id).expect("nome");
            assert_eq!(nome.len(), BYTES_DO_NOME * 2, "{nome}");
            assert!(
                nome.chars().all(|c| c.is_ascii_hexdigit()),
                "nome nao-hex para {id:?}: {nome}"
            );
            assert!(!nome.contains(id), "o id vazou para o nome: {nome}");
        }
    }

    /// `a/b` e `a_b` sao sessoes diferentes e continuam diferentes — a
    /// armadilha de sanitizar por substituicao em vez de hashear.
    #[test]
    fn ids_que_um_sanitizador_colapsaria_seguem_distintos() {
        let barra = SessionWorkspace::nome_do_subdiretorio("a/b").expect("barra");
        let sublinha = SessionWorkspace::nome_do_subdiretorio("a_b").expect("sublinha");
        assert_ne!(barra, sublinha);
    }

    /// Achado da revisao independente de seguranca (#1449): o digest tem de
    /// ser sobre os bytes crus do `session_id`, nao sobre `session_id.trim()`
    /// — senao dois ids que so diferem por espaco (inclusive um NBSP
    /// invisivel) colidem no mesmo diretorio. Caminho de exploracao real: a
    /// `session_key` de um grupo do iMessage e o `group_name`, texto livre
    /// que qualquer participante pode renomear para o nome do grupo da
    /// vitima mais um espaco.
    #[test]
    fn ids_que_diferem_so_por_espaco_seguem_distintos() {
        let base = SessionWorkspace::nome_do_subdiretorio("s").expect("base");
        for variante in ["s ", " s", "s\u{a0}", " s "] {
            let outro = SessionWorkspace::nome_do_subdiretorio(variante)
                .unwrap_or_else(|| panic!("{variante:?} nao deveria ser vazio"));
            assert_ne!(
                base, outro,
                "{variante:?} colidiu com \"s\" — hash caiu sobre o id trimado"
            );
        }
    }

    /// E o caminho montado nunca escapa do pai, nem com `..` no id.
    #[test]
    fn o_caminho_fica_sempre_debaixo_da_raiz() {
        let (_t, ws) = raiz();
        for id in ["../fora", "/absoluto", "a/b/c"] {
            let caminho = ws.caminho_da_sessao(id).expect("caminho");
            assert_eq!(
                caminho.parent(),
                Some(ws.raiz()),
                "o caminho saiu do pai: {}",
                caminho.display()
            );
        }
    }

    /// Sem identidade de sessao nao ha o que escopar: fail-closed em vez de um
    /// diretorio compartilhado por todo mundo — que e o defeito da #1449.
    #[test]
    fn session_id_vazio_nao_ganha_diretorio() {
        let (_t, ws) = raiz();
        assert!(SessionWorkspace::nome_do_subdiretorio("").is_none());
        assert!(SessionWorkspace::nome_do_subdiretorio("   ").is_none());
        assert!(ws.caminho_da_sessao("").is_none());
        assert!(ws.garantir_para_sessao("  ").is_none());
    }

    #[test]
    fn cria_o_diretorio_da_sessao_e_e_idempotente() {
        let (_t, ws) = raiz();
        let primeiro = ws.garantir_para_sessao("sessao-1").expect("cria");
        assert!(primeiro.is_dir());
        let segundo = ws.garantir_para_sessao("sessao-1").expect("reaproveita");
        assert_eq!(primeiro, segundo);
    }

    /// Duas sessoes, dois diretorios em disco. E o par do teste puro acima,
    /// agora com o filesystem de verdade.
    #[test]
    fn duas_sessoes_ganham_diretorios_diferentes_em_disco() {
        let (_t, ws) = raiz();
        let a = ws.garantir_para_sessao("sessao-a").expect("a");
        let b = ws.garantir_para_sessao("sessao-b").expect("b");
        assert_ne!(a, b);
        assert!(a.is_dir() && b.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn o_diretorio_da_sessao_nasce_fechado_em_0700() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, ws) = raiz();
        let dir = ws.garantir_para_sessao("sessao-1").expect("cria");
        let modo = std::fs::metadata(&dir)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(modo, 0o700, "criado com {modo:o}, esperado 700");
    }

    /// Symlink plantado no lugar do diretorio de UMA sessao e recusado — a
    /// mesma disciplina que o pai recebe no boot. Sem isto, a unica raiz
    /// efetiva da chamada viraria o alvo do link.
    #[cfg(unix)]
    #[test]
    fn symlink_no_lugar_do_diretorio_da_sessao_e_recusado() {
        let (tmp, ws) = raiz();
        let alvo = tmp.path().join("alvo-do-link");
        std::fs::create_dir_all(&alvo).expect("cria o alvo");
        let caminho = ws.caminho_da_sessao("sessao-1").expect("caminho");
        std::os::unix::fs::symlink(&alvo, &caminho).expect("planta o link");

        assert!(
            ws.garantir_para_sessao("sessao-1").is_none(),
            "symlink no lugar do diretorio da sessao tem de ser recusado"
        );
    }

    /// Arquivo (nao diretorio) no lugar tambem e recusado.
    #[test]
    fn arquivo_no_lugar_do_diretorio_da_sessao_e_recusado() {
        let (_t, ws) = raiz();
        let caminho = ws.caminho_da_sessao("sessao-1").expect("caminho");
        std::fs::write(&caminho, b"x").expect("write");
        assert!(ws.garantir_para_sessao("sessao-1").is_none());
    }

    /// E se o PAI deixou de ser diretorio de verdade, nenhuma sessao ganha
    /// raiz — o `create_dir_all` atravessaria o link e o diretorio da sessao
    /// nasceria fora do workspace.
    #[cfg(unix)]
    #[test]
    fn pai_trocado_por_symlink_derruba_todas_as_sessoes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let alvo = tmp.path().join("alvo-do-link");
        std::fs::create_dir_all(&alvo).expect("cria o alvo");
        let pai = tmp.path().join("workspace");
        std::os::unix::fs::symlink(&alvo, &pai).expect("planta o link");

        let ws = SessionWorkspace::nova(pai);
        assert!(ws.garantir_para_sessao("sessao-1").is_none());
        assert!(
            !alvo
                .join(SessionWorkspace::nome_do_subdiretorio("sessao-1").expect("nome"))
                .exists(),
            "o diretorio da sessao nasceu dentro do alvo do link"
        );
    }

    /// Pai que nao existe: fail-closed, sem materializar nada.
    #[test]
    fn pai_inexistente_nao_ganha_diretorio() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pai = tmp.path().join("nao-existe");
        let ws = SessionWorkspace::nova(pai.clone());
        assert!(ws.garantir_para_sessao("sessao-1").is_none());
        assert!(!pai.exists(), "o pai foi materializado pelo uso");
    }
}
