- **Excecao com prazo para RUSTSEC-2026-0543/0555 (`chrono` e `chrono-tz`
  unmaintained) (#1938).** Os dois advisories apareceram no `cargo-deny` em
  2026-11-17 e reprovavam toda PR sem que nenhuma dependencia tivesse mudado.
  Sao so `unmaintained` (todas as versoes, sem upgrade seguro): nao e
  vulnerabilidade nem unsound. As duas IDs entram em `deny.toml` e
  `.cargo/audit.toml` com `EXPIRA 2026-12-31`, verificado por
  `scripts/security/check_deny_exceptions_expiry.py`. A saida de verdade e
  migrar para `jiff` ou `time`, que pede ADR e plano proprios.
