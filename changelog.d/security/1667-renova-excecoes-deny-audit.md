- **Excecoes temporarias de advisories renovadas ate 2026-12-31 e ignores mortos removidos (#1667).**
  As quatro excecoes com prazo verificado pelo CI (`deny.toml` + espelho em `.cargo/audit.toml`)
  venceriam em 2026-11-18/20 e pintariam o job `cargo-deny` no meio do ciclo da v0.5.0:
  RUSTSEC-2026-0370/0371/0372 (zbus_names/zvariant/zvariant_derive, `unmaintained`, via
  tauri-plugin-notification -> notify-rust 4.18.2 -> zbus 5, so desktop) e RUSTSEC-2026-0381
  (webpki-roots 0.26.11, shim que reexporta 1.0.10 ja corrigido). Sem upstream disponivel
  (notify-rust mais novo segue em zbus 5), o prazo foi renovado com justificativa nos dois
  arquivos; os comentarios "Expires 2026-11-15" das demais entradas unmaintained (proc-macro-error2,
  gtk-rs, glib) acompanham. Removidos, de forma espelhada, os ignores de `rsa` (RUSTSEC-2023-0071)
  e `unic-*` (RUSTSEC-2025-0075/0080/0081/0098/0100): as crates sairam do `Cargo.lock` e os
  ignores so geravam `advisory-not-detected`. Nada passou a ser ignorado a mais.
