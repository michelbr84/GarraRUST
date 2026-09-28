# Novidades da v0.4.7

> 🇬🇧 [English version](Whats-New-v0.4.7) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.7)

Release **curta e de correção**. Ela existe por um motivo só: um bug observado em
produção na v0.4.6 derrubava turnos inteiros do agente depois de todo o trabalho
já ter sido feito. Se você está na v0.4.6, é esta a atualização que te interessa.

Junto vão duas coisas que já estavam prontas e verdes na `main`: os **presets
nomeados de permissão** do WhatsApp pessoal e a atualização de duas famílias de
dependências.

---

## O lote paralelo deixa de furar o orçamento e de derrubar o turno (#1523)

**O que acontecia.** O modelo pode pedir várias ferramentas numa resposta só —
um "lote paralelo". O orçamento de execução, porém, só era conferido **antes de
cada chamada ao modelo**, nunca entre as ferramentas do mesmo lote. Um lote de
15 ferramentas rodava inteiro contra um teto de 10.

O efeito era pior do que "gastou mais do que devia". No modo `search` — que é o
piso do WhatsApp pessoal, com 10 chamadas por tarefa — o turno **caía** em
`execution budget exceeded` depois de tudo já ter executado, sem o modelo ver um
único resultado. Do lado do usuário, a conversa respondia "Tente de novo em
instantes" para um pedido que falharia exatamente igual na segunda vez.

**O que mudou.** O orçamento passa a ser conferido **por chamada**, dentro do
lote:

- a ferramenta que passaria do teto **não executa**, e o modelo recebe de volta
  a razão — ele sabe que bateu no limite, em vez de descobrir tarde;
- com o orçamento esgotado, o modelo ganha **uma volta final avisada** para
  responder com o que já coletou. Nada que ele pedir nessa volta roda.

Na prática: um pedido grande no WhatsApp agora termina com uma resposta baseada
no que deu tempo de apurar, em vez de terminar em erro.

## Presets nomeados de permissão (#1434)

Mais uma peça da [Access Policy v2](Seguranca-e-Operacao) da v0.4.6. Em vez de
combinar `level` e `write` na mão toda vez, você usa um nome:

```bash
garra whatsapp preset <numero> chat_only    # conversa, nenhuma ferramenta
garra whatsapp preset <numero> read         # só leitura
garra whatsapp preset <numero> developer    # full
garra whatsapp preset <numero> full_pod     # full
```

O mesmo nome serve para o default de quem não está na lista e para grupos:

```bash
garra whatsapp access default --preset read
garra whatsapp group <id> --preset chat_only
```

Três detalhes de desenho que importam:

- um preset é **apelido para uma combinação canônica**, não um campo novo no
  `config.yml`. Não existe rótulo guardado que possa divergir do que o portão
  realmente aplica — ele grava `level` e `write` pelo mesmo caminho único de
  mutação da política;
- `developer` e `full_pod` compilam para o **mesmo** `full`. Quem decide o poder
  real continua sendo o modo da sessão mais o `execution.profile`
  ([ADR 0024](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0024-perfis-de-execucao-isolated-pod.md));
  por isso os dois seguem **recusados** em `access.default` — o desconhecido não
  entra em `full`;
- `garra whatsapp access --json` e `GET /admin/api/whatsapp/access` passam a
  trazer um rótulo `preset` **calculado ao vivo** do `level`/`write`, com
  `custom` para o que nenhum preset cobre.

## Dependências: `utoipa` 6 e a família `opentelemetry` 0.33 (#1526, #1527)

As duas subiram **em bloco**, porque não sobem de outro jeito: `utoipa` e
`utoipa-swagger-ui` trocam o tipo `OpenApi` entre si, e as crates de
`opentelemetry` compartilham um facade — bumpar uma sozinha coloca duas versões
incompatíveis no mesmo grafo e o build quebra. Nenhuma mudança de código foi
necessária.

Para o problema não voltar, o `.github/dependabot.yml` ganhou grupos `utoipa` e
`opentelemetry`, no mesmo molde do grupo `wasmtime` que já existia.

---

## Como atualizar

```bash
garra update            # CLI; guarda a versão anterior e aceita `garra rollback`
```

Instalação nova segue pelo [instalador de sempre](Instalacao-e-Primeiros-Passos).

## A v0.4.6 continua no lugar

A Release v0.4.6, sua tag e seus assets **não foram tocados**. O runbook do
projeto é explícito: nunca reutilizar um tag já publicado, porque o
`garra update` verifica SHA-256 e clientes podem ter cacheado os binários
([`docs/releasing.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/releasing.md)
§Rollback). A correção chega por cima, nesta versão.

## O que fica para depois

- A página **global** de Agents & Permissions (#1433) e o **import/export** de
  políticas (#1435) seguem abertos, adiados por decisão de escopo — território
  de permissões, que não cabe numa release de correção.
- Instaladores do Windows e bundles de desktop continuam **não assinados**.
- O auto-updater do desktop Tauri segue desativado por desenho; o caminho de
  atualização suportado é o `garra update` da CLI.
