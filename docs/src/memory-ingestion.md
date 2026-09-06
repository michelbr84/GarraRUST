# Filtro de ruído na ingestão da memória

> Fecha o [#952](https://github.com/michelbr84/GarraRUST/issues/952).

Nem todo turno de conversa merece um vetor. `"oi"`, `"ok"`, `"kkkk"` e
`"bom dia"` são gravados como qualquer outra mensagem, mas como **vetor** eles
só atrapalham: texto curto tem cosseno alto com quase tudo, então competem no
top-K com memória de verdade. Numa base de ~7.100 entradas, a consulta
*"quem é Michel"* trazia entradas `"oi"` entre os primeiros resultados.

O gateway agora decide, na ingestão, se vale gastar um vetor com aquele
conteúdo.

## O que o filtro faz — e o que ele não faz

**Faz:** decide se o conteúdo entra no índice vetorial.

**Não faz:**

- **Não apaga nada.** A entrada continua gravada, continua no histórico e
  continua achável pelo recall textual. O que ela não ganha é um vetor.
- **Não é retroativo.** Vetor de `"oi"` gravado antes desta versão continua no
  índice. Limpar o passado apaga dado, e isso é decisão explícita do operador
  — `garra memory compact` e a política de TTL existem para isso.
- **Não é irreversível.** Desligue o filtro, rode `garra memory reindex`, e os
  vetores voltam.

## Configuração

```yaml
memory:
  ingestion:
    filter_noise: true          # padrão
    min_chars: 4                # padrão
    extra_noise_phrases:        # somam à lista embutida
      - "salve família"
```

| Chave | Padrão | O que faz |
| --- | --- | --- |
| `filter_noise` | `true` | `false` restaura o comportamento anterior: todo turno não vazio recebe vetor. |
| `min_chars` | `4` | Piso de caracteres **úteis** (já sem acento, pontuação e emoji). `0` desliga só o piso; a lista de frases segue valendo. Faixa aceita: `0`–`40`. |
| `extra_noise_phrases` | `[]` | Frases que, sozinhas, são o conteúdo inteiro de uma entrada de ruído. **Somam** à lista embutida, nunca a substituem. |

`garra config check` valida a faixa de `min_chars`, avisa quando um piso alto
vai engolir conteúdo curto de verdade, e avisa quando você escreveu chaves que
não têm efeito porque `filter_noise` está `false`.

### Por que ligado por padrão

Ao contrário da política de retenção — que fica **desligada** por padrão
porque apaga memória —, este filtro não apaga nada. O pior caso é uma entrada
gravada sem vetor, e isso se desfaz com um `reindex`. Deixá-lo desligado por
padrão significaria que ninguém recebe a correção do bug.

### Por que o piso é baixo

Quatro caracteres, e não os dez ou doze que pegariam `"bom dia"` por
comprimento. A lista de frases faz o trabalho de precisão; um piso alto
derrubaria junto um fato curto de verdade, como `"moro em SP"` (10
caracteres). Errar embeddando ruído custa ranking; errar do outro lado custa
**memória que o agente deveria ter e não tem** — e o usuário não tem como
saber que perdeu.

## O que conta como ruído

Nesta ordem:

1. **Conteúdo que some na normalização.** `"?!"`, `"..."`, `"👍"` — só
   pontuação ou emoji, não há o que embeddar.
2. **Abaixo de `min_chars`** caracteres úteis.
3. **Frase inteira** na lista: saudações, confirmações, agradecimentos,
   despedidas (`oi`, `ok`, `bom dia`, `obrigado`, `valeu`, `beleza`,
   `entendi`, `thanks`, `got it`, …).
4. **Risada pura:** `kkkk`, `hahaha`, `rsrsrs`, `hehe`.

A regra de frase só casa quando a frase é o **conteúdo inteiro**:

| Conteúdo | Ganha vetor? |
| --- | --- |
| `obrigado` | não |
| `obrigado pela ajuda com o deploy do gateway` | **sim** |
| `bom dia` | não |
| `bom dia, preciso do relatório de vendas de março` | **sim** |
| `kkkk` | não |
| `kkkk pode ser, mas o build quebrou` | **sim** |

Acento e pontuação não mudam a decisão: `"tá"`, `"ta"` e `"Tá!"` são a mesma
coisa. Fato extraído (`[FACT] …`) nunca passa pelo filtro — já é sinal
filtrado por um LLM com limiar de confiança.

## Efeito no `garra memory`

Uma entrada pulada fica com `embedding IS NULL`, exatamente como uma entrada
cujo embedding **falhou**. Duas consequências práticas:

**O total de "sem vetor" em `garra memory stats` não vai mais a zero.** Parte
dele é proposital. O `reindex` separa as duas coisas:

```console
$ garra memory reindex --dry-run
12 entrada(s) sem vetor seriam reprocessadas (dry-run: nada foi gravado).
Outras 340 ficam sem vetor de proposito, por serem ruido para a busca
semantica (#952) — elas seguem contadas em `stats` como "sem vetor".
Ajuste ou desligue em `memory.ingestion`.
```

**O `reindex` usa a mesma política da ingestão.** Se usasse outra, ele
reembeddaria uma por uma exatamente as entradas que a ingestão acabou de
pular — pagando provider para desfazer o filtro. Os dois lados leem a mesma
seção `memory.ingestion` da mesma config.

## Discordo do filtro. Como volto ao comportamento anterior?

```yaml
memory:
  ingestion:
    filter_noise: false
```

Reinicie o gateway e rode `garra memory reindex`. Toda entrada que ficou sem
vetor por causa da política recebe um — inclusive as que foram puladas antes
de você mudar a chave.
