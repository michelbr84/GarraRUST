# `benches/recall_quality`

Benchmark de **qualidade** do recall semântico em português (#958).

O outro benchmark do repo ([`agent-framework-comparison`](../agent-framework-comparison/))
mede desempenho — tamanho de binário, RSS, cold start. Este mede outra coisa:
**se a memória devolve a lembrança certa**. Recall@k, precision@k e MRR sobre
um conjunto de consultas com ground truth.

> **Não é gate de CI, e não deve virar um.** A execução depende de um provider
> de embeddings (Ollama local ou uma API paga), e um número que varia com a
> máquina e com o modelo instalado não pode reprovar o PR de ninguém. O lugar
> dele é a comparação deliberada: trocar de modelo, mexer no recall, e ver o
> que aconteceu.

## Como rodar

```bash
cargo build --release --bin garra
cd benches/recall_quality
./run.sh
```

Sem provider de embeddings configurado, ele mede a **busca textual** — o
fallback — e diz isso no relatório. Para medir a semântica, configure um
provider antes (`~/.config/garraia/config.toml`, seção `embeddings`).

Variáveis: `GARRA_BIN` (caminho do binário) e `LIMITE` (top-K coletado,
padrão 10).

## O que ele faz, e por que dividido em dois

`run.sh` **coleta**: semeia o corpus, roda as 40 consultas pelo mesmo recall
que o agente usa, e grava o cru em `results/<data>-<host>/raw.json`.
`metrics.py` **pontua**: lê só o JSON.

A separação permite repontuar uma coleta antiga com uma métrica nova, sem
rodar os modelos de novo. E mantém a regra herdada do outro benchmark:
**número sem artefato bruto commitado não conta como claim válido.**

### A memória é isolada, sempre

O `garra memory add` escreve no banco de memória de quem chama. Sem isolamento,
o benchmark despejaria trinta frases inventadas na memória real do operador —
e depois as mediria junto com as dele, estragando o número e a memória na
mesma execução. O `run.sh` cria um diretório temporário e o apaga no fim.

**Trocar só o `HOME` não bastava**, e a primeira versão disto errava nisso. O
`ConfigLoader::default_config_dir()` consulta `GARRAIA_CONFIG_DIR` primeiro e
retorna na hora, e depois `XDG_CONFIG_HOME` — o `HOME` só entra em terceiro.
Num shell com qualquer uma das duas definida (config centralizada, CI), o
`HOME` isolado não valia nada. O `run.sh` define `GARRAIA_CONFIG_DIR`, que é a
primeira da cadeia: apontá-la para o sandbox corta as outras duas, o que é mais
forte que desdefini-las uma a uma.

## As métricas, e o que cada uma responde

| Métrica | Pergunta |
|---|---|
| **recall@k** | Dos documentos que deviam vir, quantos vieram nos `k` primeiros? |
| **precision@k** | Dos `k` primeiros, quantos deviam vir? |
| **MRR** | Em que posição apareceu o primeiro acerto? (1/posição) |
| **ruído@k** | Para consulta que **não devia casar com nada**: quanto veio mesmo assim? |

O **ruído@k é o que a issue pediu sem nomear.** Ela relata que "quem é Michel"
devolveu "oi" no top-K. Um benchmark que só mede acerto daria nota cheia a um
sistema que devolve tudo para tudo — por isso o grupo `ruido-puro` tem ground
truth **vazio**, e é pontuado numa escala separada, onde **menor é melhor**.
Misturá-lo na tabela de acerto faria um número ruim parecer bom.

`recall@k` e `precision@k` são `n/a` para essas consultas, e não zero: sem
documento esperado, a fração não existe, e inventar um zero puxaria a média
para baixo por uma razão que não é qualidade.

### As consultas de ruído não podem ser saudações

A primeira versão deste grupo usava `oi`, `bom dia` e `obrigado pela ajuda`, e
**estava errada** — o corpus contém `d25: "oi"` e `d29: "bom dia"` como
documentos. Perguntar `oi` tem resposta exata ali, então devolvê-la é acerto, e
a sonda estava punindo o comportamento certo. O grupo agora pergunta por
assuntos que o corpus não tem (`qual a receita do bolo de cenoura`), e há uma
verificação no próprio dataset de que nenhuma palavra dessas consultas aparece
no corpus.

As saudações continuam entre os documentos, que é onde elas importam: como
**distratoras** das consultas de verdade. É o grupo `identidade-ruido` que
mede o caso da issue — `quem e Michel` deve trazer `d01`/`d02`, e não `d25`.

### Uma ressalva sobre `precision@k`

O denominador aqui é `min(k, |retornados|)`, não `k`. É deliberado — dividir
por `k` puniria o sistema por uma memória com menos de `k` entradas, que não é
erro dele. Mas **não é a definição padrão da literatura de IR**, então estes
números não se comparam diretamente com os de outro benchmark.

## O corpus

`dataset.json`: 30 documentos e 40 consultas, em 13 grupos.

**É sintético, e isso está declarado no próprio arquivo.** Não substitui a
memória real de ninguém — a memória real do operador tem o vocabulário dele, e
nenhum corpus inventado reproduz isso. O que ele serve para fazer é **comparar
duas execuções**: dois modelos de embedding, ou a mesma configuração antes e
depois de uma mudança no recall.

Os grupos existem para separar tipos de falha, que é onde está o sinal:

| Grupo | O que testa |
|---|---|
| `identidade`, `projeto`, `condominio`, `carro`, `saude`, `viagem` | casamento direto por assunto |
| `parafrase` | a consulta não usa as palavras do documento |
| `parafrase-dificil` | nem o assunto é dito com as mesmas palavras |
| `sinonimo` | "enferrujado" para achar "ferrugem" |
| `curta` | uma palavra só, que é como as pessoas realmente perguntam |
| `cross-lingual` | consulta em espanhol, corpus em português — a issue reporta esse uso real |
| `identidade-ruido` | "quem é Michel", o caso exato que a issue reporta falhando |
| `ruido-puro` | **não deve casar com nada** |

A issue pede 50-100 pares e aqui há 40. A diferença é deliberada: o sinal está
nos grupos de borda, não na contagem, e um corpus sintético maior não fica mais
representativo — fica só maior. Acrescentar consultas é editar um JSON, e a
estrutura por grupo aguenta isso sem mudar o código.

## Linha de base medida (2026-09-07, busca textual)

Primeira execução, **sem provider de embeddings** — então ela mede o
fallback textual, não a semântica:

```
TODAS                 40 MRR 0.108  r@1 0.095  r@5 0.095  r@10 0.095
```

O que esse número diz não é "a memória é ruim": é que **o fallback textual é
quase inútil para pergunta em linguagem natural**, e que `recall@1` igual a
`recall@10` é a assinatura disso — a busca é `LIKE '%frase inteira%'`, então
ou a frase casa ou não casa, e olhar mais fundo na lista não ajuda. "endereço
do condomínio" não acha "O condomínio fica na rua das Flores" porque a frase
inteira não está lá.

Só três dos treze grupos saem do zero: `curta` (MRR 0.500), `viagem` (0.333) e
`condominio` (0.143). Nove ficam em zero absoluto — entre eles `parafrase`,
`sinonimo` e `cross-lingual` — e o décimo terceiro (`ruido-puro`) é `n/a` por
construção.

**As quatro consultas que acertaram têm uma coisa só em comum**, e não é o
tamanho: a string da consulta aparece **literal** no documento. `ferrugem` está
dentro de "esta com ferrugem na dobradica"; `reserva do hotel` está dentro de
"A reserva do hotel esta no nome do Michel". Não é "consulta curta funciona" —
`cor do carro` tem o mesmo tamanho de `reserva do hotel` e falha, porque o
documento diz "Corolla prata" e nunca escreve a palavra "cor".

E o ruído:

```
ruido@1: 0.000   ruido@3: 0.000   ruido@5: 0.000   ruido@10: 0.000
```

**Zero — e isso não é elogio.** O `LIKE` não devolve nada para as consultas de
ruído pela mesma razão que não devolve nada para as consultas de verdade: a
frase inteira não está em documento nenhum. Um sistema que não responde nada a
ninguém tira nota cheia em ruído. O número só passa a significar alguma coisa
quando estiver ao lado de um recall que funciona — que é exatamente a
comparação que falta.

Artefato: [`results/2026-09-07-vm/`](results/2026-09-07-vm/).

**A comparação que falta** é a mesma execução com um provider de embeddings
configurado. Ela precisa de uma máquina com Ollama (ou uma chave de API), e
fica para quem tiver uma — o harness está pronto e o `raw.json` da textual já
está commitado como base.
