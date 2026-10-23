- **Fundacao de schema de billing do Garra Cloud: plan/subscription/invoice (#1652).**
  Migration 035 do `garraia-workspace` adiciona tres entidades. `plans` e um
  catalogo global (sem RLS, mesma classe de roles/permissions), semeado apenas
  com o plano `free` sem preco (price/currency NULL = preco nao fixado neste
  repositorio MIT). `subscriptions` e `invoices` sao por tenant sob FORCE RLS
  (isolamento por `group_id`, fail-closed via NULLIF, WITH CHECK identico ao
  USING). Dinheiro e sempre inteiro de centavos + ISO 4217; ids de provedor de
  pagamento sao opcionais e UNIQUE por provedor; nunca ha PII de cartao. A
  fatura e ledger imutavel (garraia_app so SELECT/INSERT). Todo grupo sem
  assinatura viva resolve para o plano Free pela funcao
  `garraia_effective_plan_id` (sem trigger nem backfill; respeita RLS), entao
  um tenant novo nasce Free sem intervir ninguem. So o schema: webhook
  Stripe/Pix (#1653), entitlements (#1654), metering (#1655) e a pagina de
  signup (#1656) ficam para as proximas issues. ADR 0026.
