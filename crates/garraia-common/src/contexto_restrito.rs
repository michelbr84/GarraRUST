//! Diagnóstico de contexto de execução restrito no Android/Termux (#1591).
//!
//! No Termux (Bionic), o MESMO binário executa ou recebe `EACCES` → exit 126
//! dependendo de COMO o processo ancestral foi lançado: do shell do Termux
//! tudo funciona; de um ancestral lançado via `/system/bin/linker64 <bin>`
//! (launcher externo, automação), os filhos não conseguem `execve` de
//! binários em `/data` — a restrição W^X do Android 10+ (targetSdk ≥ 29)
//! veta exec de arquivo gravável pelo app, e o contexto SELinux herdado do
//! launcher é o que decide.
//!
//! Este módulo NÃO contorna a restrição (não dá — é política do kernel). O
//! trabalho dele é fail-honest: quando um spawn falha com EACCES ou um
//! filho sai com 126, classificar a causa e devolver uma mensagem que diga
//! onde o problema está (contexto do launcher) e o que resolve (lançar o
//! gateway pelo shell do Termux), em vez do genérico "Permission denied".
//!
//! Classificação pura (`classifica`) é testável sem `/proc`; o wrapper
//! `diagnostico()` lê o processo atual.

/// Indicador de ambiente Termux: prefixo padrão da instalação.
const PREFIXO_TERMUX: &str = "/data/data/com.termux/files/usr";
/// O executável de um processo lançado via linker externo do Android.
const LINKER: &str = "/apex/com.android.runtime/bin/linker64";

/// Classifica um contexto de processo como "restrito herdado do launcher".
///
/// `exe`: caminho real de `/proc/self/exe` (ou `None` se indisponível).
/// `selinux`: label de `/proc/self/attr/current` (ou `None`).
/// `termux_na_path`: `true` se o PATH do processo menciona o prefixo Termux
/// (sinal de que o processo ESPERA executar binários de `/data`).
///
/// Restrito = há Termux no PATH (o processo depende de binários de `/data`)
/// E o próprio executável veio do linker do sistema (fora de `/data`), que é
/// o ancestral que o repro de #1591 mostra produzir filhos EACCES.
pub fn classifica(
    exe: Option<&str>,
    selinux: Option<&str>,
    termux_na_path: bool,
) -> Option<String> {
    if !termux_na_path {
        return None;
    }
    let exe = exe?;
    let via_linker = exe.starts_with(LINKER);
    // Processos do shell Termux (bash, mksh de $PREFIX) também aparecem
    // com exe em /data e NÃO são restritos — o sinal de restrição é o exe
    // fora de /data (linker do sistema).
    let em_data = exe.starts_with("/data/");
    if em_data || !via_linker {
        return None;
    }
    Some(format!(
        "Contexto de execução restrito herdado do launcher (exit 126/EACCES em binários de \
         {PREFIXO_TERMUX}). O processo foi lançado via linker do sistema ({exe}, selinux \
         `{}`) e a restrição W^X do Android (targetSdk ≥ 29) veta `execve` de binários em \
         /data por filhos neste contexto. Correção: lance o gateway pelo shell do Termux \
         (ex.: `termux-wake-lock && garraia start`) — filhos de um shell de $PREFIX executam \
         normalmente (verificado no repro de #1591). O workaround `linker64` no PATH \
         (#1561) lança os binários, mas não dá aos FILHOS o direito de exec em /data.",
        selinux.unwrap_or("indefinido")
    ))
}

/// Diagnóstico do processo atual, ou `None` se o contexto não é o restrito.
///
/// Só leitura de `/proc`; nunca falha — qualquer indisponibilidade devolve
/// `None` (sem diagnóstico, o erro original segue intacto).
pub fn diagnostico() -> Option<String> {
    #[cfg(not(target_os = "android"))]
    {
        // A restrição é específica de Android; em qualquer outro SO não há
        // o que diagnosticar. Em Android (bionic) o caminho abaixo roda.
        None
    }
    #[cfg(target_os = "android")]
    {
        let exe = std::fs::read_link("/proc/self/exe")
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        let selinux = std::fs::read_to_string("/proc/self/attr/current")
            .ok()
            .map(|s| s.trim().to_string());
        let termux_na_path = std::env::var("PATH")
            .map(|p| p.contains(PREFIXO_TERMUX))
            .unwrap_or(false)
            || std::path::Path::new(PREFIXO_TERMUX).exists();
        classifica(exe.as_deref(), selinux.as_deref(), termux_na_path)
    }
}

/// Acrescenta o diagnóstico (se aplicável) a uma mensagem de erro.
pub fn enriquece(mensagem: &str) -> String {
    match diagnostico() {
        Some(d) => format!("{mensagem}\n\n[Diagnostics #1591] {d}"),
        None => mensagem.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_termux_nao_e_restrito() {
        // exe em /data (processo lançado DO shell Termux): executa filhos ok.
        let exe = format!("{PREFIXO_TERMUX}/bin/bash");
        assert!(classifica(Some(&exe), Some("u0_a123:..."), true).is_none());
    }

    #[test]
    fn linker_externo_com_termux_e_restrito() {
        let d = classifica(Some(LINKER), Some("u0_a123:s0:c512"), true);
        assert!(d.is_some());
        let d = d.unwrap();
        assert!(d.contains("restrito"));
        assert!(d.contains(PREFIXO_TERMUX));
        assert!(d.contains("shell do Termux"));
    }

    #[test]
    fn sem_termux_nao_diagnostica() {
        // Linux glibc: PATH sem Termux => nada a dizer.
        assert!(classifica(Some("/usr/bin/bash"), Some("unconfined"), false).is_none());
    }

    #[test]
    fn exe_desconhecido_nao_diagnostica() {
        assert!(classifica(None, Some("u0_a123"), true).is_none());
    }

    #[test]
    fn enriquece_sem_diagnostico_devolve_original() {
        // Fora de Android, `diagnostico()` é None e a mensagem não muda.
        assert_eq!(enriquece("falhou x"), "falhou x");
    }
}
