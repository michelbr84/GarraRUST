- **Security Gate deixa de cair no timeout depois dos testes passarem (#1332).**
  O check obrigatorio `Security Gate (BOLA & Tenant Isolation)` terminava
  `cancelled` com os testes ja verdes: o orcamento de 30 min era do JOB, e o
  `Post Cache` de `target/` (~8 min, porque a entrada era evictada do cache do
  repositorio antes de cada run e o restore sempre dava miss) cruzava o teto.
  Tres runs so em 2026-09-21, cada um custando um re-run manual. Agora o
  orcamento que o gate exige (25 min) fica no step dos testes, o teto do job
  (45) so guarda contra infra travada, e o job cacheia so o registry do cargo:
  o build continua frio, como ja era, e o save cai para segundos. Nenhum teste
  pulado, sem `continue-on-error`.
