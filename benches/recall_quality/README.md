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

### O `HOME` é isolado, sempre

O `garra memory add` escreve no banco de memória de quem chama. Sem isolamento,
o benchmark despejaria trinta frases inventadas na memória real do operador —
e depois as mediria junto com as dele, estragando o número e a memória na
mesma execução. O `run.sh` cria um `HOME` temporário e o apaga no fim.

## As métricas, e o que cada uma responde

| Métrica | Pergunta |
|---|---|
| **recall@k** | Dos documentos que deviam vir, quantos vieram nos `k` primeiros? |
| **precision@k** | Dos `k` primeiros, quantos deviam vir? |
| **MRR** | Em que posição apareceu o primeiro acerto? (1/posição) |
| **ruído@k** | Para consulta que **não devia casar com nada**: quanto veio mesmo assim? |

O **ruído@k é o que a issue pediu sem nomear.** Ela relata que "quem é Michel"
devolveu "oi" no top-K. Um benchmark que só mede acerto daria nota cheia a um
sistema que devolve tudo para tudo — por isso o grupo `ruido-puro` (`oi`,
`bom dia`, `obrigado pela ajuda`) tem ground truth **vazio**, e é pontuado
numa escala separada, onde **menor é melhor**. Misturá-lo na tabela de acerto
faria um número ruim parecer bom.

`recall@k` e `precision@k` são `n/a` para essas consultas, e não zero: sem
documento esperado, a fração não existe, e inventar um zero puxaria a média
para baixo por uma razão que não é qualidade.

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

Os únicos grupos acima de zero são `curta` (0.375) e `viagem` (0.333) — as
consultas de uma palavra, que é justamente o caso em que o `LIKE` funciona.

Artefato: [`results/2026-09-07-vm/`](results/2026-09-07-vm/).

**A comparação que falta** é a mesma execução com um provider de embeddings
configurado. Ela precisa de uma máquina com Ollama (ou uma chave de API), e
fica para quem tiver uma — o harness está pronto e o `raw.json` da textual já
está commitado como base.
