- **Domínio dos caminhos de contato e compra corrigido para `garraia.org` (#1574).** O
  `docs/src/pricing.md` e o `CONTRIBUTING.md` apontavam para `garraia.cloud`, que
  **não resolve** (NXDOMAIN): link de compra, demo, política de privacidade e os
  e-mails `enterprise@` e `security@` estavam num domínio morto. O domínio real do
  projeto é `garraia.org` — resolve, `/pricing`, `/demo` e `/privacy` respondem 200,
  e o MX existe (`mailserver.purelymail.com`). O `SECURITY.md` já dizia
  `security@garraia.org`; o `CONTRIBUTING.md` contradizia. Correção estritamente
  factual, decidida pelo maintainer em 2026-10-07. Os termos comerciais anunciados na
  página de preços (SLA, SOC 2, janelas de suporte) **não foram tocados** — publicar
  ou retratar termos continua sendo decisão comercial, rastreada na #1574.
