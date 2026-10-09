# GarraIA — Política de Suporte — PROPOSTA

> **Status: PROPOSTA — aguarda ratificação do maintainer.** Este documento
> expande o `SUPPORT.md` (canais). **Nada aqui é promessa contratual**: tempos
> de resposta são alvos de comunidade, não SLA.

## Canais (vigentes)

| Preciso de… | Onde |
|---|---|
| Documentação | Wiki · [garraia.org/docs](https://garraia.org/docs/introducao) · `docs/` |
| Dúvida / ideia | GitHub Discussions · Discord |
| Bug / feature | Issue com template |
| Segurança | `SECURITY.md` (privado; nunca issue pública) |
| Cliente Garra Cloud (beta) — atendimento prioritário | Canal privado do painel do cliente e/ou canal oficial do projeto — `[PENDENTE: confirmar operação do canal/caixa antes de anunciar]` |
| Pedido de titular de dados (LGPD/GDPR) | Canal privado próprio — `[PENDENTE: caixa/fluxo de pedidos de titulares a confirmar antes de anunciar]`; nunca issue pública |
| Contribuição | `CONTRIBUTING.md` |

**Regra de atendimento transversal:** incidentes de segurança e pedidos de
titulares de dados são tratados **por gravidade, independentemente do plano**
do solicitante — inclusive self-host/comunidade e Free Beta. Prioridade de
Pro/Studio (quando os planos cloud existirem) é **comercial** (ordem de
atendimento), e não muda essa regra nem cria SLA.

## Alvos de resposta (comunidade — não contractual)

| Tipo | Alvo |
|---|---|
| Vulnerabilidade de segurança (via canal privado) | Ack em até **72h**; status inicial em até **7 dias** |
| Bug com reprodução | Triagem em até **14 dias** |
| Questão/Discussion | Melhor esforço; resposta comunitária |
| Regressão de release | Prioridade alta; correção na release seguinte |

Esses alvos valem para a **comunidade ativa do projeto**. Eles **não criam
SLA**: suporte comunitário é best-effort por natureza. Oferta de suporte com
contrato/SLA é decisão comercial do maintainer (issue de negócio); quando
existir, será documento próprio. Para o futuro Garra Cloud (planejado), a
posição registrada é de **SLA de melhor esforço sem percentual contratual no
lançamento**, comunicando disponibilidade de forma clara; SLA numérico só em
contrato enterprise específico (ver `sla-decision-brief.md` e
`docs/legal/tos-draft.md` §6).

## O que é coberto

- O código publicado no repositório, nas versões atendidas pela
  `docs/operations/version-support-policy.md`.
- Instalação e configuração conforme a documentação.

## O que não é coberto (hoje)

- Disponibilidade de instalações self-host (infraestrutura é do operador).
- Integrações com terceiros fora do que a documentação descreve (provedores
  LLM, canais — regidos pelos termos de cada um).
- Suporte prioritário, onboarding ou SLA — inexistentes como oferta.

## Dados de operação hoje (verificáveis)

- Instalação de produção monitorada com `NRestarts=0` e health check ok
  (registro interno; é dado de **uma** máquina, não de uma rede de clientes).
- CI com gate de cobertura (piso 70% agregado), clippy, security gate BOLA e
  testes em 3 sistemas operacionais por PR.

`[PENDENTE: quando existir rede de clientes pagantes, revisar estes alvos com
dados reais de suporte]`
