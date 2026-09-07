# 18. O que fazer com o crate `garraia-embeddings`

- **Status:** Proposed
- **Deciders:** @michelbr84 (decisão final) + Claude (levantamento, sessão autônoma 2026-09-07)
- **Date:** 2026-09-07
- **Tags:** fase-2, rag, embeddings, arquitetura, divida-tecnica
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: #949
  - ADR relacionada: [ADR 0002 — vector store (pgvector)](0002-vector-store.md),
    em especial a **Amendment 2026-09-06**, que registrou a divergência e
    deixou esta decisão explicitamente em aberto
  - Plan que criou o crate: `plans/0145-*` (scaffold da Fase 2.1, 2026-05-18)
  - Regra absoluta 8 do `CLAUDE.md`: decisão arquitetural irreversível pede ADR

---

## Context and Problem Statement

O workspace tem **dois sistemas que dizem ser embeddings**, e só um roda.

| | `garraia-agents::embeddings` | crate `garraia-embeddings` |
|---|---|---|
| Linhas | 966 | 961 |
| Consumidores | `runtime.rs`, `memory_reindex.rs`, `garraia-learning` | **nenhum** |
| Store | `garraia_db::vector_store` (sqlite-vec, `vec0`) | `VectorStore` trait sem implementação real |
| Dimensão | derivada do vetor que o provider devolve | `EMBEDDING_DIM = 768`, fixa |
| Providers | Ollama (com lote de 4), OpenAI | `DeterministicProvider` (sha2, para teste) |

Verificado nesta sessão: fora do próprio diretório, `garraia-embeddings`
aparece em exatamente **quatro** lugares — a entrada de membro e a dependência
de path no `Cargo.toml` raiz, e dois comentários em
`garraia-learning/src/retriever.rs` que o chamam de "crate órfão". Nenhum
`use`, nenhuma chamada.

### Uma precisão sobre a issue

A #949 diz que "o runtime real usa dimensão dinâmica (1024 no caso do
qwen3-embedding:0.6b via Ollama)". A parte que importa está certa e a
ilustração merece um ajuste: **1024 é uma configuração, não o valor do
sistema**. O `vector_store` cria uma tabela `vec_embeddings_{dims}` por
dimensão encontrada, e o modelo padrão do `OllamaEmbeddingProvider` é
`nomic-embed-text` — que tem 768.

Ou seja: `EMBEDDING_DIM = 768` **casa por acaso** com o padrão de hoje. Isso
piora o problema em vez de amenizá-lo, porque a constante parece certa
enquanto a afirmação que ela faz — "o sistema tem uma dimensão, e é esta" — é
falsa. Trocar 768 por 1024 seria consertar o número e manter o erro.

Isto é a mesma forma do problema que a #985 nomeou para os modos: enquanto
houver dois sistemas nomeando a mesma coisa, qualquer frase sobre "o sistema de
embeddings" é ambígua — qual dos dois? A diferença é que ali os três eram
alcançáveis, e aqui um é código morto que **contradiz** o vivo: quem lê
`EMBEDDING_DIM = 768` conclui que o GarraIA fixa 768 dimensões, e ele não fixa.

O crate não é acidente: o `CLAUDE.md` o registra como scaffold deliberado da
Fase 2.1, promovido a crate ativo em 2026-05-18 pelo plan 0145, por ADR 0002.
As duas coisas são verdade ao mesmo tempo — foi decidido de propósito, e hoje
engana.

Mitigação já em vigor (PR #993): o `lib.rs` do crate abre com um aviso de que
ele **não** é o caminho em uso. Isso reduz o dano e não resolve a pergunta.

## Decision Drivers

1. **Um leitor não deve precisar de contexto externo para não ser enganado.**
   O aviso funciona para quem abre `lib.rs`; não funciona para quem chega em
   `types.rs` por um `grep EMBEDDING_DIM`.
2. **A direção do ADR 0002 continua válida.** pgvector para a memória do
   workspace Postgres não está em questão aqui. O que está em questão é se o
   *código* escrito quatro meses antes da implementação ajuda ou atrapalha.
3. **Custo de manter.** O crate compila, roda 23 testes e entra em todo
   `cargo test --workspace`, `clippy` e no relatório de cobertura. É custo de
   CI e de leitura para zero função.
4. **O que se perde ao remover.** O `HybridQuery` tem uma ideia boa: ele recusa
   em *build time* uma consulta cujo `Scope` não bate com o `group_id`. Essa
   ideia não pode se perder junto com o arquivo.
5. **Reversibilidade.** Remover um crate sem consumidores é `git revert`. Não é
   uma porta de mão única — o que exige ADR aqui é a regra 8 ser sobre
   *decisão* arquitetural, não sobre dificuldade de desfazer.

## Considered Options

### A. Remover o crate

Tira do workspace, tira do `Cargo.toml`, e a ADR 0002 continua sendo o registro
da direção pgvector. Quando a memória do workspace existir, o crate nasce de
novo — a partir do ADR, e contra o sistema que existir naquele momento.

- **A favor:** acaba a ambiguidade; `EMBEDDING_DIM = 768` para de existir como
  afirmação falsa sobre o produto; some o custo de CI.
- **Contra:** perde-se código escrito e testado, incluindo o `HybridQuery`.
- **Mitigação do contra:** o desenho do `HybridQuery` (rejeição de
  cross-tenant na construção) fica registrado **neste ADR**, na seção
  "O que não pode se perder". Ideia documentada sobrevive melhor que código
  não usado — este crate é a prova.

### B. Manter e alinhar

Fazer `garraia-agents::embeddings` depender dos traits do crate, corrigir
`EMBEDDING_DIM` para dimensão dinâmica, e unificar os dois `EmbeddingProvider`.

- **A favor:** um sistema só, que é o estado desejável.
- **Contra:** é migração do caminho **vivo** — o que o runtime chama, o que o
  `memory_reindex` usa, o que o `garraia-learning` referencia — para uma
  abstração desenhada em torno do Postgres, antes de existir memória de
  workspace. Risco alto, benefício hoje: nenhum visível ao usuário. E congela a
  forma da abstração no momento em que se sabe **menos** sobre o caso de uso
  real do que se saberá quando a Fase 3 chegar.

### C. Status quo

Fica como está, com o aviso do `lib.rs`.

- **A favor:** custo zero agora.
- **Contra:** o aviso é uma nota de rodapé num crate que continua compilando,
  testando e aparecendo em toda listagem de cobertura como se fosse produto.
  Foi exatamente assim que o `agent_mode.rs` chegou a 76% de cobertura sendo
  inalcançável — teste que testa a si mesmo dá a aparência de saúde.

## Decision

**Recomendação: opção A — remover o crate.**

O argumento decisivo não é o custo de CI nem as 961 linhas: é que o crate faz
uma **afirmação falsa sobre o produto** (`EMBEDDING_DIM = 768`) num lugar onde
a busca por nome leva primeiro. A ADR 0002 já guarda a direção; o que o crate
acrescenta hoje é a chance de alguém acreditar nele.

A opção B é a certa **quando** a memória do workspace Postgres for construída —
e aí ela é o trabalho de construir, não uma migração preventiva.

> **Status `Proposed` de propósito.** A regra absoluta 8 pede o ADR antes da
> decisão, e a decisão é do dono. Este documento existe para que ela possa ser
> tomada com a evidência na mesa; nada foi removido. Aceitar este ADR é o
> gatilho para a remoção, e recusá-lo mantém o status quo com o aviso do
> `lib.rs`, que já está lá.

## O que não pode se perder

Se a opção A for aceita, estas duas ideias saem do código e ficam aqui:

1. **Cross-tenant rejeitado na construção, não na consulta.** O `HybridQuery`
   valida o par (`Scope`, `group_id`) no builder: uma busca de escopo `Group`
   sem `group_id`, ou de escopo `User` com `group_id` de outro grupo, não chega
   a existir como valor. É mais forte que validar na borda do handler, porque
   não há caminho que esqueça de validar. Vale reconstruir assim.
2. **Dimensão como invariante do tipo.** A intenção do `EmbeddingVector` era
   que um vetor de dimensão errada não fosse construível. A intenção é boa; o
   erro foi fixá-la numa constante. A versão certa carrega a dimensão no tipo
   ou a valida contra a do provider — nunca contra um número escrito à mão.

## Consequences

**Se aceito (A):**

- `crates/garraia-embeddings/` sai; `Cargo.toml` perde o membro e a dependência
  de path; o `CLAUDE.md` volta a listá-lo como planejado, não ativo.
- `garraia-learning/src/retriever.rs` perde os dois comentários que citam o
  crate órfão — o stub continua stub, mas para de se explicar por contraste com
  algo que não existe mais.
- A contagem de crates ativos cai de 22 para 21.
- Nada de comportamento muda: zero consumidores.

**Se recusado (C):**

- O aviso do `lib.rs` continua sendo a única defesa, e este ADR passa a ser o
  segundo lugar onde a divergência está registrada.

## Verificação

A afirmação central deste ADR — "nenhum consumidor" — é verificável e deve ser
reverificada antes de executar a remoção, porque ela pode deixar de ser verdade
a qualquer commit:

```bash
grep -rn "garraia.embeddings" --include=Cargo.toml --include='*.rs' . \
  | grep -v '^./crates/garraia-embeddings/'
```

Em 2026-09-07 isso devolve quatro linhas: duas do `Cargo.toml` raiz (membro do
workspace e dependência de path) e dois comentários em
`garraia-learning/src/retriever.rs`. Nenhum `use`, nenhuma chamada.
