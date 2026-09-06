# Memória semântica

O GarraIA guarda cada turno de conversa num banco local e o traz de volta
quando é relevante. Este documento descreve **o que existe hoje**, com o nome
real de cada comando, chave de configuração e arquivo.

> **Nota histórica.** Até 2026-09 esta página descrevia um sistema que nunca
> foi construído — `garra memory add/clear/export/disable`, um `facts.json`
> com array de fatos datados, chaves `auto_extract` e `max_facts`. Nada disso
> existiu. A reescrita (#963) foi feita conferindo cada afirmação contra o
> código; onde algo é parcial ou tem limite conhecido, está dito.

## O caminho de um turno

```text
turno (pergunta + resposta)
        │
        ├─ filtro de ruído ──── "oi", "ok", "obrigado" → grava SEM vetor
        │                        (a entrada existe, só não é semanticamente
        │                         buscável)
        │
        └─ com conteúdo ─────── embedding → grava COM vetor
                                            │
                                            ▼
                                    memory.db (SQLite)
                                    ├─ entries    (texto, tenant, modelo)
                                    └─ vetores    (sqlite-vec)
```

Na volta, o `recall` embute a pergunta, busca por similaridade e devolve o que
passa do limiar. Sem provedor de embeddings configurado, a busca cai para o
caminho **textual** — e diz qual dos dois rodou, em vez de fingir semântica.

## O que fica sem vetor, e por quê

O filtro de ingestão (#952) é **ligado por padrão**, ao contrário da retenção.
A diferença é o que está em jogo: a retenção **apaga** memória, então só roda
quando o operador pede; o filtro não apaga nada.

Uma entrada filtrada continua gravada, continua no histórico e continua
achável pela busca textual. O que ela não ganha é vetor. Desligar a chave e
rodar `garra memory reindex` devolve o vetor.

O que o filtro **não** faz: limpar o que já está no índice. Um vetor de "oi"
gravado antes desta versão continua lá — limpar o passado apaga dado, e isso é
decisão do operador, via `garra memory compact`.

## Fatos extraídos

Além do turno inteiro, um extrator baseado em LLM lê a mensagem do usuário e
tenta tirar dela fatos estruturados (tipo, chave, valor, confiança). Fatos com
confiança **≥ 0,80** são gravados como entradas próprias, no formato:

```text
[FACT] type=preferencia key=idioma value=portugues confidence=0.95
```

Duas coisas que valem saber:

- **Eles moram no mesmo `memory.db`**, e não num arquivo separado. Aparecem no
  `garra memory list` como qualquer outra entrada.
- **Eles não passam pelo filtro de ruído.** Um fato extraído é, por definição,
  algo que o extrator julgou ser sinal — então sempre recebe vetor.

## `fatos.json` é outra coisa

Existe um arquivo `~/.garraia/memoria/fatos.json`, e ele **não** é onde os
fatos extraídos ficam. É um perfil estático que você escreve à mão e que o
gateway injeta no prompt de sistema no boot:

```json
{
  "nome": "Michel",
  "sobre": "Desenvolvedor Rust, prefere respostas diretas e em português"
}
```

É um objeto JSON com chaves livres — não um array. Se estiver malformado, o
boot segue normalmente e registra um aviso, em vez de falhar.

## Configuração

```toml
[memory]
enabled = true
# Nome de uma entrada da seção [embeddings]. Sem isto, a memória grava
# e busca textualmente, sem semântica.
embedding_provider = "local"
shared_continuity = false

[memory.ingestion]
filter_noise = true          # padrão
min_chars = 12
extra_noise_phrases = []     # frases suas, além da lista embutida

[memory.retention]
enabled = false              # DESLIGADO por padrão: isto apaga memória
max_age_days = 90            # faixa aceita 1..=3650
interval_hours = 24          # faixa aceita 1..=720

[embeddings.local]
provider = "ollama"
model = "nomic-embed-text"
base_url = "http://localhost:11434"
dimensions = 768
```

`garra config check` valida as faixas e reporta o que está fora.

**A retenção é desligada por padrão de propósito.** Ela apaga entradas mais
velhas que `max_age_days`, e apagar dado do usuário sem ele ter pedido é o
tipo de padrão que não se escolhe por conveniência.

## Comandos

Todos abrem o **mesmo** `memory.db` que o gateway usa, e — quando precisam de
um — o mesmo provedor de embeddings. O que o `reindex` escreve é exatamente o
que o recall do agente lê de volta.

| Comando | O que faz |
|---|---|
| `garra memory stats` | Contagens mais o relatório de integridade do índice |
| `garra memory list` | As entradas mais recentes; `--no-embedding` mostra só as sem vetor |
| `garra memory search <q>` | A **mesma** busca que o agente usa; diz se rodou semântica ou textual |
| `garra memory reindex` | Re-embute as entradas gravadas sem vetor |
| `garra memory backup` | Snapshot consistente, com retenção de cópias |
| `garra memory pin <id>` | Marca uma entrada para a retenção nunca apagar |
| `garra memory ttl <id>` | Define ou limpa a expiração de uma entrada |
| `garra memory delete <id>` | Apaga uma entrada e o vetor dela |
| `garra memory compact` | Apaga tudo mais velho que N dias, com os vetores |

A maioria aceita `--json` para saída legível por máquina.

### O backup usa `VACUUM INTO`, não `cp`

O banco roda em modo WAL, então copiar o arquivo `.db` com `cp` pode produzir
uma cópia sem as transações que ainda estão no WAL — um backup que parece bom
e está incompleto. O `VACUUM INTO` do SQLite produz um snapshot consistente.

## Observabilidade

Com `GARRAIA_METRICS_ENABLED=true`, o `/metrics` expõe quatro métricas da
memória (#957):

| Métrica | Para quê |
|---|---|
| `garraia_memory_embed_latency_seconds{provider,operation}` | Quanto o provedor está demorando |
| `garraia_memory_embed_failures_total{provider,operation}` | **A que mais importa** — ver abaixo |
| `garraia_memory_recall_latency_seconds` | O recall inteiro, embedding + busca |
| `garraia_memory_ingested_total{outcome}` | Turnos por desfecho: `embedded`, `noise`, `no_provider`, `failed` |

A de falha é a que merece alerta. Falha de embedding era **silenciosa**: a
entrada ia para o banco sem vetor e ficava invisível para a busca semântica
para sempre. O #948 tirou isso do silêncio no log; a métrica tira do painel —
log conta o caso, métrica conta a tendência, e é a tendência que faz alguém
descobrir que o provedor caiu **antes** de o recall degradar.

`no_provider` e `failed` são desfechos separados porque são a diferença entre
"ninguém configurou" e "configurou e está quebrado", que é a primeira pergunta
de quem opera.

Detalhes e consultas PromQL em [telemetry.md](../telemetry.md).

## Isolamento

Cada entrada carrega um `tenant_id`, e o recall filtra por ele. Uma sessão
nunca vê a memória de outro tenant.

O recall também filtra pelo **modelo de embedding** que gerou o vetor: vetores
de modelos diferentes não são comparáveis, e misturá-los produz similaridade
sem sentido. Ao trocar de modelo, rode `garra memory reindex`.

## API HTTP

```http
GET    /api/memory/recent          # entradas recentes
GET    /api/memory/search?q=...    # busca
DELETE /api/memory                 # limpa
```

## O que ainda não existe

- **Não há `garra memory add`.** Entradas nascem de conversas ou da extração
  de fatos; não há inserção manual pela CLI.
- **O retriever do `garraia-learning` é um stub.** A busca semântica de
  *skills* (distinta da memória de conversa) espera a Fase 2.1 — ver ADR 0002.
- **Os gauges de tamanho do índice** (`garraia_memory_entries`,
  `garraia_memory_vector_index_size`) estão na #957 e ainda não foram
  entregues: precisam de um worker próprio, porque pendurá-los no worker de
  retenção os deixaria mortos para quem não liga a retenção — que é o padrão.
