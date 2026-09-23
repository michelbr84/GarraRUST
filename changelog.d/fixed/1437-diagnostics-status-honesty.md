- **Diagnostics separa "nunca configurado" de "quebrado" (#1437).** O
  `GET /api/diagnostics` tinha quatro estados (`ok`/`warning`/`error`/`skipped`)
  e um subsistema opcional que o operador nunca montou saia igual a um
  subsistema montado e falhando: uma instalacao local recem-feita acendia
  amarelo por TLS ausente, por `.env` inexistente e por um `GARRAIA_JWT_SECRET`
  que o proprio texto da linha chamava de opcional. Entram duas variantes
  **aditivas** — `disabled` (ha um interruptor e ele esta desligado: modo voz)
  e `not_configured` (subsistema opcional que esta instalacao nunca montou:
  TLS, `.env`, secret de JWT, tokens de Telegram/Discord, aparelho de WhatsApp
  vinculado). As quatro originais mantem nome e significado, e as tres neutras
  (`skipped`, `disabled`, `not_configured`) nunca tiram o agregado do relatorio
  de `ok`. O console mostra as neutras em cinza, com o estado por extenso ao
  lado do rotulo, e trata status desconhecido como neutro — nunca como erro.
