use process_wrap::tokio::ChildWrapper;

use super::*;

fn dorminhoco() -> Command {
    let mut cmd = Command::new("sh");
    // O `sh` lanca um neto e espera por ele: lider + um descendente.
    cmd.args(["-c", "sleep 300 & wait"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// O lider nasce lider do proprio grupo — sem isso `kill(-pgid)` nao tem
/// alvo. **Mutacao**: tire o `process_group(0)` de `pre_spawn`.
#[tokio::test]
async fn o_lider_e_lider_do_proprio_grupo() {
    let mut filho = envolver(dorminhoco(), &novo_marcador())
        .0
        .spawn()
        .expect("spawn");
    let pid = filho.id().expect("pid") as i32;
    // SAFETY: `getpgid` so consulta.
    let pgid = unsafe { libc::getpgid(pid) };
    assert_eq!(pgid, pid, "o filho deveria liderar o proprio grupo");
    assert_ne!(
        pgid,
        unsafe { libc::getpgid(0) },
        "o filho nao pode ficar no grupo do gateway"
    );
    Box::into_pin(filho.kill()).await.expect("kill");
}

/// Protecao contra reuso de PID: depois de colhido o lider, o numero do
/// grupo nao e mais nosso e NENHUM sinal sai. **Mutacao**: tire a guarda
/// `colhido.is_some()` de `sinalizar_grupo` e o `kill` passa a ser
/// tentado (devolve `ESRCH` aqui, mas num PID reciclado mataria outro).
#[tokio::test]
async fn depois_de_colhido_nada_e_sinalizado() {
    let mut cmd = dorminhoco();
    cmd.process_group(0);
    let filho = cmd.spawn().expect("spawn");
    let pgid = filho.id().expect("pid") as i32;
    let mut filho = unix::FilhoDoGrupo::para_teste(Box::new(filho), pgid);

    assert!(
        filho.sinalizar_grupo(0).expect("sonda"),
        "com o lider vivo o grupo e alcancavel"
    );
    Box::into_pin(filho.kill()).await.expect("kill");
    assert!(
        !filho.sinalizar_grupo(libc::SIGKILL).expect("pos-colheita"),
        "depois de colhido o lider, nenhum sinal pode sair"
    );
    assert_eq!(
        unix::olhar_lider(pgid),
        unix::Lider::Indefinido,
        "o lider ja foi colhido: nao e mais filho"
    );
}

/// O `kill` derruba o grupo inteiro, nao so o lider. **Mutacao**: troque
/// `start_kill` pelo passthrough (`self.inner.start_kill()`) e o neto
/// `sleep` sobrevive.
#[tokio::test]
async fn kill_derruba_o_grupo_inteiro() {
    let mut filho = envolver(dorminhoco(), &novo_marcador())
        .0
        .spawn()
        .expect("spawn");
    let pgid = filho.id().expect("pid") as i32;
    // Da tempo de o `sh` lancar o neto.
    tokio::time::sleep(Duration::from_millis(200)).await;
    Box::into_pin(filho.kill()).await.expect("kill");

    let comeco = Instant::now();
    // SAFETY: sinal 0 so consulta.
    while unsafe { libc::kill(-pgid, 0) } == 0 {
        assert!(
            comeco.elapsed() < Duration::from_secs(5),
            "ainda ha processo no grupo {pgid} depois do kill"
        );
        tokio::time::sleep(INTERVALO).await;
    }
}

/// O processo existe e nao e zumbi (o zumbi responde a `kill(pid, 0)`).
#[cfg(target_os = "linux")]
fn vivo(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            let (_, resto) = stat.rsplit_once(')')?;
            Some(resto.split_whitespace().next()? != "Z")
        })
        .unwrap_or(false)
}

/// Lider que sai na hora deixando um neto no grupo. Devolve (lider, neto).
#[cfg(target_os = "linux")]
async fn lider_morto_com_neto() -> (tokio::process::Child, i32, u32) {
    use tokio::io::AsyncBufReadExt;

    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 300 >/dev/null 2>&1 & echo $!; exit 0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    let mut filho = cmd.spawn().expect("spawn");
    let pgid = filho.id().expect("pid") as i32;
    let mut linha = String::new();
    tokio::io::BufReader::new(filho.stdout.take().expect("stdout"))
        .read_line(&mut linha)
        .await
        .expect("pid do neto");
    let neto: u32 = linha.trim().parse().expect("pid do neto");
    let comeco = Instant::now();
    while unix::olhar_lider(pgid) != unix::Lider::Saiu {
        assert!(comeco.elapsed() < Duration::from_secs(5), "lider nao saiu");
        tokio::time::sleep(INTERVALO).await;
    }
    (filho, pgid, neto)
}

/// O vigia so sinaliza com a trava aberta: fechada (o lider foi, ou esta
/// sendo, colhido), ele sai sem mandar nada, mesmo com o lider zumbi a
/// vista. A prova e o NETO, por `/proc` — o grupo responde a `kill(-pgid,
/// 0)` so por causa do lider zumbi. **Mutacao**: tire o
/// `if *colhido { return true; }` de `vigiar` e o neto e morto.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn vigia_nao_sinaliza_com_a_colheita_fechada() {
    let (mut filho, pgid, neto) = lider_morto_com_neto().await;

    let fechada = Colheita(std::sync::Arc::new(std::sync::Mutex::new(true)));
    tokio::time::timeout(Duration::from_secs(5), unix::vigiar(pgid, fechada))
        .await
        .expect("com a trava fechada o vigia sai na primeira olhada");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        vivo(neto),
        "o vigia sinalizou o grupo com a colheita fechada"
    );

    // Com a trava aberta, o mesmo vigia derruba o grupo.
    tokio::time::timeout(
        Duration::from_secs(5),
        unix::vigiar(pgid, Colheita::default()),
    )
    .await
    .expect("vigia");
    filho.wait().await.expect("colher o lider");
    let comeco = Instant::now();
    while vivo(neto) {
        assert!(comeco.elapsed() < Duration::from_secs(5), "neto sobreviveu");
        tokio::time::sleep(INTERVALO).await;
    }
}

/// O relatorio do encerramento: motivo, conferencia feita, nada sobrando e
/// quem foi morto pelo marcador contado. **Mutacao**: devolva
/// `verificado: false` na passada do Linux, ou troque o motivo.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn encerrar_relata_motivo_e_dominio_vazio() {
    let marcador = novo_marcador();
    // Um processo em SESSAO nova carregando o marcador: fora de qualquer
    // grupo que o encerramento sinalize, so o marcador o alcanca.
    let mut cmd = Command::new("sleep");
    cmd.arg("300")
        .env(VAR_DOMINIO, &marcador)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: `setsid` e async-signal-safe e so afeta o filho.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut fugitivo = cmd.spawn().expect("spawn");
    let pid = fugitivo.id().expect("pid");
    let comeco = Instant::now();
    while !linux::tem_marcador(pid, &marcador) {
        assert!(comeco.elapsed() < Duration::from_secs(5), "exec nao veio");
        tokio::time::sleep(INTERVALO).await;
    }

    let dominio = Dominio::novo(marcador, None, Colheita::default());
    let relatorio = dominio.encerrar("teste", MotivoDoFim::Queda).await;

    assert_eq!(relatorio.motivo, MotivoDoFim::Queda);
    assert!(relatorio.verificado, "no Linux a conferencia acontece");
    assert_eq!(relatorio.remanescentes, Vec::<u32>::new());
    assert_eq!(relatorio.mortos_por_marcador, 1);
    assert!(relatorio.duracao < TETO_DA_CONFIRMACAO);
    assert!(relatorio.vida >= relatorio.duracao);
    // O fugitivo e filho direto deste teste e foi morto pelo marcador, entao
    // o proprio encerramento ja o colheu: o `wait` aqui so pode dar `ECHILD`.
    let _ = fugitivo.wait().await;
}

/// Um processo que demora a morrer e sinalizado em mais de uma passada, mas
/// conta uma vez so no relatorio — era o que fazia o teste acima falhar com
/// `mortos_por_marcador == 5` em runner carregado. **Mutacao**: troque o
/// corpo de `registrar_mortos` por `mortos.extend(da_passada)`.
#[test]
fn processo_sinalizado_em_varias_passadas_conta_uma_vez() {
    let mut mortos = Vec::new();
    registrar_mortos(&mut mortos, &[41]);
    registrar_mortos(&mut mortos, &[41, 42]);
    registrar_mortos(&mut mortos, &[42, 41]);
    registrar_mortos(&mut mortos, &[]);
    assert_eq!(mortos, vec![41, 42]);
}

/// O marcador casa por igualdade exata da entrada — um token que e
/// prefixo de outro nao pode casar com o processo errado.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn marcador_casa_so_por_igualdade() {
    let marcador = novo_marcador();
    let mut filho = envolver(dorminhoco(), &marcador).0.spawn().expect("spawn");
    let pid = filho.id().expect("pid");
    // O `environ` so reflete o marcador depois do `exec`.
    let comeco = Instant::now();
    while !linux::tem_marcador(pid, &marcador) {
        assert!(comeco.elapsed() < Duration::from_secs(5), "exec nao veio");
        tokio::time::sleep(INTERVALO).await;
    }
    assert!(!linux::tem_marcador(pid, &marcador[..marcador.len() - 1]));
    assert!(!linux::tem_marcador(pid, &format!("{marcador}0")));
    assert!(
        !linux::matar_fixado(pid, &novo_marcador()),
        "marcador de outro dominio nao pode matar este processo"
    );
    assert!(
        linux::com_marcador(&marcador)
            .expect("/proc")
            .contains(&pid)
    );
    assert!(!linux::tem_marcador(std::process::id(), &marcador));
    Box::into_pin(filho.kill()).await.expect("kill");
}
