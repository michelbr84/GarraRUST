# GarraIA — LIA (Legitimate Interests Assessment) — `ip_inet` + `user_agent`

- **Status:** RASCUNHO técnico (2026-10-09) — **sujeito a revisão jurídica
  externa**. Atende à pendência da §6.1 do `dpia.md`.
- **Owner:** @michelbr84
- **Escopo:** tratamento de `sessions.ip_inet` e `sessions.user_agent` no
  gateway GarraIA, com finalidade de segurança, prevenção a fraude e
  operação do serviço.
- **Base legal invocada:** LGPD art. 7, IX / GDPR art. 6.1.f (legítimo
  interesse).

> **Este documento é um assessment técnico, não parecer jurídico.** A decisão
> de governança do maintainer é **condicionada** à sua existência e à revisão
> externa: sem revisão de advogado, a invocação de legítimo interesse para
> estes campos permanece frágil perante auditoria ANPD.

## 1. Purpose test — o interesse é legítimo?

**Sim, para as finalidades abaixo:**

1. **Segurança da conta:** detectar session hijack e credential stuffing —
   a mesma conta autenticando de IPs/UAs radicalmente distintos em janela
   curta é o sinal primário de comprometimento.
2. **Prevenção a fraude e abuso:** identificar automação maliciosa, brute
   force distribuído e uso do gateway para spam nos canais conectados.
3. **Operação e diagnóstico:** correlacionar erros de request com versão de
   cliente/ambiente ao depurar incidentes.

Essas finalidades protegem os próprios titulares (contas deles) e terceiros
(usários finais dos canais). O interesse é do operador **e** compatível com a
expectativa razoável de quem usa um serviço autenticado: um serviço de
contas espera logs técnicos de acesso.

**Fora do escopo deste LIA (e portanto não cobertos por ele):**

- publicidade, perfilamento comercial ou marketing;
- tracking cross-site ou analytics de comportamento;
- qualquer tratamento de dados sensíveis (LGPD art. 5, II).

Tratamentos fora destes fins exigem base legal própria — este documento não
os autoriza.

## 2. Necessity test — o tratamento é necessário?

| Campo | Por que é necessário | Alternativa descartada e por quê |
|---|---|---|
| `ip_inet` | Sinal mínimo para detectar origem de acesso anômala (país/região, brute force por IP, rede tor conhecida). Sem ele, detecção de session hijack perde o fator "de onde". | Fingerprinting de device (canvas, WebGL) seria **mais invasivo** que o próprio IP. Geo-enriquecimento externo acrescenta precisão que a segurança operacional não exige. |
| `user_agent` | Distingue cliente legítimo de automatização; sinal de versão vulnerável para mitigação direcionada. | UA completo é dispensável; poderíamos guardar só a família de cliente — aceitável como melhoria futura, mas o ganho de privacidade é pequeno e o custo de engenharia real. |

Conjunto IP+UA é **proporcional e mínimo** para as finalidades declaradas.
Não é usado para construir perfil de comportamento pessoal.

## 3. Balancing test — o interesse supera os direitos do titular?

**A favor do interesse:**

- Campos são **metadados de acesso**, não conteúdo de comunicação.
- Retenção limitada (§4) e acesso restrito (§5).
- O tratamento protege o próprio titular.

**A favor do titular:**

- IP pode revelar localização aproximada — dado pessoal com expectativa de
  privacidade real.
- Acumulação histórica de IPs forma padrão de vida.

**Conclusão do balancing:** o tratamento é justificado **com as salvaguardas
deste documento** — retenção de 90 dias (e não indefinida), uso restrito a
segurança/operação, vedação de reutilização para marketing ou perfilamento.
Sem essas salvaguardas, o balancing deixa de favorecer o interesse.

## 4. Retenção

| Evento | Retenção |
|---|---|
| Sessão ativa/fechada | `ip_inet` e `user_agent` mantidos por **90 dias** após o fechamento da sessão (limpeza default do produto) |
| Incidente de segurança em investigação | Suspensão da limpeza para as sessões envolvidas, apenas pelo período da investigação, com registro documentado |
| Exclusão/anonimização da conta | Campos revogados junto com as sessões |

90 dias é o intervalo em que session hijack e fraudes de conta ainda têm
valor de resposta; ultrapassa-lo sem caso concreto deixa de ser necessário.

## 5. Acesso autorizado e salvaguardas

- Acesso: funções operacionais do gateway que rodam sob o papel de app
  (RLS; `garraia_login` restrita a verificação de credencial — ADR 0005).
- Exposição via API: `GET /v1/me/audit` devolve eventos ao próprio titular;
  metadados de sessão aparecem na própria gestão de sessões do titular.
- Não há exportação destes campos para terceiros; não há venda.
- Segredos adjacentes (hashes) nunca são expostos (redação por `SecretString`
  e `RedactingWriter`).

## 6. Reavaliação e decisão de governança

- **Reavaliar:** a cada 12 meses, ou ao mudar retenção/escopo dos campos.
- **Decisão de governança do maintainer (2026-10-09):** o tratamento de
  `ip_inet`/`user_agent` para segurança/fraude/operação **pode seguir como
  legítimo interesse, condicionado a** este assessment existir, estar
  registrado e passar por revisão jurídica externa antes do GA. Sem a
  revisão, a base legal declarada permanece "pendente" no DPIA.
- **Hipótese legal separada:** registros de acesso à aplicação guardados por
  **obrigação legal** (Marco Civil art. 15 — 6 meses) seguem base legal
  própria (LGPD art. 7, II), não este LIA; o operador self-host é quem
  responde por essa guarda.

Refs: `docs/compliance/dpia.md` §2.3, §6.1 · LGPD <https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm> · EDPB guidelines on legitimate interest.
