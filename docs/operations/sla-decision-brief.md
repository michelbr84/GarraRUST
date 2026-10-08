# GarraIA — SLA: decisão registrada e opções — NOTA DE ENGENHARIA

> **Contexto:** a issue de GA de operação exige "SLA definido **ou** decisão
> explícita de não oferecer SLA, e por quê". Este documento registra a decisão
> default de engenharia e apresenta as opções; a ratificação é do maintainer.

## Decisão default registrada (2026-10-07)

**O GarraIA self-host é distribuído sem SLA.** Justificativa:

1. **Arquitetura**: o produto roda na infraestrutura do operador. Disponibilidade
   depende de máquina, rede e provedores do operador — o projeto não controla
   esses fatores e não pode honestamente prometê-los.
2. **Preço**: distribuição comunitária/MIT não tem contrapartida contratual
   que sustente penalidade por descumprimento.
3. **Estatuto**: SLA sem rede de clientes pagantes é documento sem lastro —
   o dado de produção disponível (`NRestarts=0`, health ok) é de uma máquina,
   não de uma base de clientes.

Isso **não impede** SLA futuro: entra com oferta cloud operada ou suporte
contratual (ver `docs/operations/support-policy.md` e a issue de negócio).

## Opções para o maintainer (quando quiser definir)

| Opção | Para quem | O que exige |
|---|---|---|
| **A. Manter sem SLA** (default acima) | Self-host/comunidade | Nada além desta nota |
| **B. SLA de nuvem** | Oferta cloud futura | Infra monitorada multi-réplica, contrato, crédito automático |
| **C. SLA enterprise** | Contrato pago | Suporte com janela, escala, crédito; processo de atendimento |

Dados hoje disponíveis para modelar B/C: uptime observado de uma instalação
(1 máquina), CI verde por PR em 3 SO, health check contínuo. **Nenhum custo de
rede de clientes existe ainda** — não há como modelar B/C sem a decisão de
negócio da issue correspondente.

## O que NÃO está decidido aqui

Preço, crédito por descumprimento, janela de suporte paga — todos são decisões
comerciais do maintainer (issue de negócio). Este documento só registra a
posição técnica default e as opções.
