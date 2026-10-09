# Preços e Planos

> **Status: PLANEJADO — programa beta futuro, não é uma oferta disponível.**
>
> O único caminho **vigente** é o self-host comunitário (MIT), gratuito para
> sempre, sem limite artificial de canais/agentes/provedores. Os planos Cloud
> abaixo descrevem um **programa beta planejado** para um grupo controlado de
> clientes: hoje **não existem** sistema de cobrança, contas de cliente cloud,
> quotas aplicadas por produto, medição de uso nem infraestrutura gerenciada.
>
> **Antes de publicar qualquer um destes valores como oferta**, é necessário
> confirmar: (a) aplicação real das quotas pelo produto, (b) sustentabilidade
> de custo com uso real, (c) entidade cobradora e meios de pagamento
> habilitados (ver §Pagamentos) e (d) revisão jurídica dos termos
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)). As quotas e
> os preços aqui são **parâmetros iniciais** da proposta
> ([#1576](https://github.com/michelbr84/GarraRUST/issues/1576)), não pratica
> corrente.
>
> Nada aqui é promessa de disponibilidade, de conformidade, de SLA ou de
> suporte contratual.

---

## Os três contextos

| Contexto | O que é | Status |
|---|---|---|
| **GarraIA Community (self-host)** | Software MIT, instalado na máquina de quem quiser, uso pessoal **e comercial** sem limitação artificial | **Vigente hoje** |
| **GarraIA Cloud** | Instância gerenciada por nós, com quotas e cobrança — planos abaixo | **Planejado (beta)** |
| **Distribuição via site/comunidade** | `install.sh`/`install.ps1` e binários de release servidos pelo site e pelo GitHub | Vigente |

---

## Community — Self-host (vigente, gratuito, MIT)

Para uso pessoal, empresas, revenda embutida em serviços, o que for — o MIT
não impõe restrição de uso comercial e **não planejamos criar nenhuma via
preços**.

**Inclui, sem quota:**
- Canais ilimitados (Telegram, Discord, Slack, WhatsApp, iMessage, conforme
  capacidade da sua própria infraestrutura e das APIs de cada plataforma)
- Agentes/configurações ilimitados
- Provedores LLM ilimitados (BYOK — a chave é sua; no self-host a chave fica
  na sua máquina, conforme `docs/legal/privacy-policy-draft.md`)
- Memória persistente, MCP, plugins WASM, atualizações

**Suporte:** apenas comunidade (GitHub Discussions/Issues, Discord conforme
disponibilidade) — ver `SUPPORT.md`.

**Como começar:**
```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
garra init
```

---

## GarraIA Cloud (PLANEJADO — programa beta)

### Free Beta — programa limitado, não "6 meses grátis por cadastro"

- Participação em **grupo controlado** (vagas limitadas), com horizonte de
  operação de **até 6 meses** para o programa como um todo — isto **não** é
  "6 meses grátis por assinatura": o beta pode encerrar, mudar de forma ou
  virar oferta paga antes disso, com aviso prévio.
- LLM **somente BYOK**: o participante usa a própria chave de provedor.
- Suporte: apenas comunidade.
- Quotas: linha "Beta" na tabela abaixo.

### Pro — R$ 39/mês (proposto)

Para uso diário de uma pessoa ou equipe muito pequena que quer instância
gerenciada.

### Studio — R$ 149/mês (proposto)

Para famílias, times pequenos e operações com mais de um canal e mais de um
dispositivo.

**Uma empresa de porte pequeno pode usar o Pro** enquanto couber nas quotas —
ter CNPJ não obriga o Studio. O Studio existe para quem precisa das quotas
superiores, não como categoria fiscal.

**Em todos os planos Cloud (inclusive Free Beta), desde o primeiro dia:**
- Segurança e isolamento entre grupos de clientes
- Exportação e exclusão dos próprios dados (direitos do titular)
- Backups operacionais (o que diferencia planos é nível de restauração
  avançada, não a existência de backup)

---

## Quotas (parâmetros iniciais do programa beta — sujeitos a confirmação no produto)

| Recurso | Free Beta | Pro | Studio |
|---|---|---|---|
| Pessoas (membros do grupo) | 1 | 1 | 3 |
| Canais (conexões ativas) | 1 | 2 | 5 |
| Agentes (configurações ativas) | 1 | 3 | 10 |
| Dispositivos (máquinas/nós vinculados) | 1 | 2 | 5 |
| Armazenamento | 100 MB | 1 GB | 5 GB |
| LLM | Somente BYOK | BYOK **ou** franquia Garra (ver abaixo) | BYOK **ou** franquia Garra (ver abaixo) |
| Suporte | Comunidade | Prioritário | Superior |

### Definições das quotas

- **Canal = conta/conexão**, não "plataforma". Dois números de WhatsApp são
  2 canais; um Telegram + um Discord também são 2.
- **Agente = configuração ativa**, não processo dedicado permanente. O Cloud
  não garante processo de agente residente 24/7 por cliente; a definição
  operacional é de configuração pronta para uso dentro da quota.
- **Dispositivo = máquina/nós vinculado** ao grupo (ex.: node de hardware).
  Celulares e navegadores usados **só para abrir o painel** de administração
  **não** contam como dispositivo.

---

## LLM: BYOK e franquia (proposto)

- **BYOK (padrão):** a chave é do cliente; em qualquer plano o Cloud pode
  operar apenas com chave própria do cliente.
- **Franquia Garra (Pro/Studio, opcional):** o projeto custeia um volume
  limitado de uso. Hipóteses internas de engenharia financeira — **não são
  créditos comerciais** —: teto interno de custo de LLM de **~R$ 5/mês no
  Pro** e **~R$ 20/mês no Studio**. Antes de vender, isso precisa virar uma
  **franquia compreensível** para o cliente: tabela de consumo em unidades
  claras (ex.: mensagens/equivalente) + consulta de saldo no painel.
- **Esgotou a franquia:** o cliente escolhe — continuar com **BYOK** ou
  fazer **recarga pré-paga** explícita. **Não** há cobrança automática
  pós-pago por excedente.
- **Cobrança automática por excedente** só existe se o cliente autorizar
  de forma específica **e** definir um teto de gasto. Limites são aplicados
  **antes** de novas chamadas (fail-closed), não depois do estouro.

---

## Suporte por plano (proposto)

| Plano | Suporte |
|---|---|
| Community (self-host) | Comunitário (GitHub/Discord) |
| Free Beta | Comunitário |
| Pro | Prioritário (canal privado para clientes cloud — `[PENDENTE: confirmar operação do canal/caixa antes de anunciar]`) |
| Studio | Superior (mesmo canal prioritário, precedência maior) |

- Prioridade de Pro/Studio é **comercial** (ordem de atendimento), não SLA.
- **Incidentes de segurança e pedidos de titulares de dados são atendidos
  por gravidade, independentemente do plano** — não deixam de fila por ser
  cliente free.
- SLA: **sem percentual contratual** em nenhum plano (ver
  `docs/operations/sla-decision-brief.md`); o Cloud, quando existir, comunica
  sua disponibilidade de forma clara sem promessa numérica inicial. SLA
  numérico, se vier, só em contrato enterprise específico.
- Dados pessoais, credenciais e vulnerabilidades: **nunca** em canal público
  (`SUPPORT.md` / `SECURITY.md`).

---

## Teste, conversão e inatividade (proposto)

- Quando os planos pagos operarem: **trial de 30 dias**, sem cartão.
- Passar a pagar exige **aceite explícito** — nada de conversão silenciosa
  no fim do trial.
- Suspensão por inatividade (ex.: conta parada por meses): o efeito
  explicado ao cliente antes da suspensão (o que é retido, o que é apagado,
  como reativar). `[PENDENTE: decisão de implementação — regra de
  inatividade, prazo de retenção pós-suspensão e efeito sobre dados ainda
  não definidos no produto]`

---

## Pagamentos (proposto — pendências antes de anunciar)

- Moeda: **BRL**, cartão nacional + Pix, conforme a disponibilidade real do
  processador (Stripe, se adotado).
- `[PENDENTE: confirmar entidade cobradora, habilitação do processador,
  recorrência do Pix (Pix Automático vs. avulso) antes de anunciar
  qualquer forma de pagamento]`
- **Anual** só depois de ~**90 dias de operação medida** — sem plano anual
  de saída, para não prometer o que a operação ainda não provou.

---

## Nota sobre o modelo financeiro (registro — não é resultado)

Já circulou internamente a conta de breakeven de ~11 pagantes. Ela **só
bate** com as premissas de ~R$ 200 de custo fixo e ~R$ 18,54 de margem de
contribuição por pagante; **não** prova cobertura de uma despesa de ~US$
200/ano (certificado EV/OV do canal desktop, ver
`docs/business/cost-model-inputs.md`). Antes de concluir qualquer margem
final é preciso somar impostos, taxa de pagamento, clientes free, suporte e
backups. **O modelo completo não está neste repositório** — não há número
final publicável aqui, e nenhum será inventado.

---

## Perguntas frequentes

**O código-fonte continuará open-source?**

Sim. MIT, para sempre. Qualquer receita de cloud financia infraestrutura
gerenciada — nunca restringe o código.

**Posso usar comercialmente sem pagar?**

Sim, no self-host. MIT não limita uso comercial e não planejamos criar
limite artificial de canais/agentes/provedores na edição comunitária.

**Onde ficam minhas conversas?**

Self-host: na sua máquina. Cloud (quando existir): em nossa infraestrutura,
com retenção e direitos descritos em `docs/legal/privacy-policy-draft.md`
(rascunho, ainda não publicado como política).

**Existe SLA?**

Não como percentual contratual. Self-host roda na sua infra; o Cloud
comunicará disponibilidade sem promessa numérica inicial
(`docs/operations/sla-decision-brief.md`, `docs/legal/tos-draft.md` §6).

**Como falar com alguém sobre planos cloud?**

`[PENDENTE: canal de contato comercial antes de anunciar os planos]` — por
enquanto, GitHub Discussions do projeto.

Refs: `docs/legal/tos-draft.md`, `docs/legal/privacy-policy-draft.md`,
`docs/operations/support-policy.md`, `docs/operations/sla-decision-brief.md`,
`docs/business/cost-model-inputs.md`, issues #1565/#1574/#1575/#1576.
