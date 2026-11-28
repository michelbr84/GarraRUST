- **Tarefas recorrentes e automacoes de hardware usam `croner` 5 com backend
  `jiff` (#1938, ADR 0028 item 3).** O `croner` saiu do backend `chrono`
  default, e o `chrono` deixou de ser dependencia do `garraia-hardware`; o
  `chrono-tz` deixou de ser dependencia do `garraia-db` (segue via
  `garraia-workspace`/`rrule`). Os advisories RUSTSEC-2026-0543/0555 nao saem
  com isso: `teloxide-core`, `rmcp` e `rrule` ainda puxam `chrono`. O fuso de
  uma tarefa recorrente e resolvido no banco IANA embutido do `jiff`
  (`tzdb-bundle-always`), nunca no `tzdata` do host, como o `chrono-tz` fazia:
  `US/Eastern` continua valendo em host sem `tzdata-legacy`. Mudancas de
  comportamento: o nome do fuso passa a ser aceito sem diferenciar maiusculas
  e e gravado na grafia canonica (`america/new_york` vira
  `America/New_York`); `Etc/Unknown` e recusado; e o parser do cron ficou
  estrito: nome de dia da semana fora do campo de dia da semana, nome de mes
  fora do campo de mes e `L` sozinho no dia da semana passam a ser erro na
  criacao da tarefa ou da automacao (antes `0 0 * * jan` rodava toda
  segunda-feira), e `SUN-SUN` passa a valer so domingo (antes rodava todo
  dia).
