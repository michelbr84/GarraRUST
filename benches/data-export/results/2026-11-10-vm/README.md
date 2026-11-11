# data-export — 2026-11-10, host `vm`

Latencia de `GET /v1/me/export` (#1801). Receita, carga e o que entra na
medida: [`../../README.md`](../../README.md). Todos os numeros abaixo foram
copiados dos JSON em `raw/`; nada aqui e estimativa.

## Ambiente

| | |
| --- | --- |
| SHA medido | `2c6eff21011cdd93a599910bed33ed370343c5fe`, arvore limpa |
| Maquina | VM Linux 6.18 x86_64, Intel Xeon @ 2.10 GHz, 4 CPUs logicas, 16 GB de RAM |
| Toolchain | rustc 1.97.0, cargo 1.97.0 |
| Postgres | `pgvector/pgvector:pg16`, digest `sha256:0dd3149a…f8dc4f`, em testcontainer (Docker 29.9.2, overlayfs), configuracao padrao da imagem |
| Transporte | em processo (`tower::ServiceExt::oneshot`), sem rede nem TLS |
| Repeticoes | 1 fria + 30 quentes por cenario, sequenciais |

Detalhe completo por execucao em `environment-<perfil>-<N>msgs.txt`.

**Sobre os perfis de build.** `dev` e o que o CI compila
(`CARGO_PROFILE_DEV_DEBUG=0` so tira o debuginfo, nao muda a otimizacao).
`release` aqui e `opt-level = 3` **sem LTO e com 16 codegen units**
(`CARGO_PROFILE_RELEASE_LTO=off`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`),
para o build caber na maquina — nao e o perfil do binario publicado
(`lto = true`, `codegen-units = 1`), que nao foi medido.

## Resultados

Latencia ate o ultimo byte do corpo, em milissegundos. p50/p95 por posto mais
proximo sobre as 30 amostras quentes.

| Cenario | Perfil | Mensagens exportadas | Truncado | Corpo | Fria | min | p50 | p95 | max | media |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 10 000 mensagens | dev | 5 000 | sim | 2 043 956 B | 124.710 | 89.304 | 106.176 | 149.755 | 153.302 | 109.693 |
| 10 000 mensagens | release (sem LTO) | 5 000 | sim | 2 043 956 B | 40.073 | 19.628 | 24.682 | 37.067 | 46.689 | 26.543 |
| 5 000 mensagens | dev | 5 000 | nao | 2 051 146 B | 131.055 | 88.961 | 111.495 | 177.179 | 316.215 | 119.355 |
| 5 000 mensagens | release (sem LTO) | 5 000 | nao | 2 051 146 B | 37.415 | 17.169 | 19.600 | 228.642 | 230.580 | 34.590 |

Preparo da carga (seed + `ANALYZE`), fora da medida: 386 a 724 ms.

### Leitura

- **Referencia historica de 30 s** (`ROADMAP.md`: "Export LGPD ... em < 30s"):
  a pior amostra entre as 124 medidas foi **316.215 ms** (5 000 mensagens,
  dev) — cerca de 1 % da referencia. Nesta maquina e nesta carga, a
  referencia e atendida com folga de duas ordens de grandeza. Isso vale para
  o custo de servidor em processo; rede, TLS e um banco com outros tenants
  ativos nao foram medidos.
- **Amostras fora da curva.** No cenario de 5 000 mensagens em `release`,
  duas das 30 amostras quentes ficaram em ~229 ms contra p50 de 19.6 ms, e
  com 30 amostras o p95 por posto mais proximo e a 29ª — uma delas. No mesmo
  cenario em `dev` houve uma amostra de 316 ms. A causa nao foi investigada
  (a maquina e compartilhada e o Postgres roda com a configuracao padrao);
  os valores estao reportados como sairam.
- **O cenario de 10 mil mensagens mede um export truncado.** `export_me`
  limita cada secao a 5 000 linhas: o corpo traz 5 000 mensagens e
  `truncated_sections = ["messages"]`. A latencia de "exportar 10 mil
  mensagens" nao existe hoje neste endpoint — o que existe, e esta medido, e
  a de exportar as 5 000 que cabem no teto, com ou sem excedente no banco.
- **Quais 5 000.** O orcamento de mensagens e consumido grupo a grupo, na
  ordem de entrada da titular em cada um. Nesta carga o primeiro grupo tem
  7 500 mensagens e esgota o teto sozinho: as 2 500 do segundo grupo ficam
  inteiras de fora. O corte e declarado, mas nao e "as 5 000 mais recentes da
  conta". Registrado como observacao da medida; nao e alterado aqui.

### Conteudo e isolamento

Verificados pelo proprio teste em cada execucao (corpo da fria e da ultima
quente): contagem exata por secao (5 000 mensagens, 200 mencoes, 100
arquivos, 100 memorias, 100 tarefas), toda mensagem com a marca da titular e
`group_id` de um grupo dela, sem ids repetidos, corte declarado se e so se
houve, e nenhuma ocorrencia das marcas ou identificadores do outro tenant nem
das mensagens sem mencao da terceira no chat compartilhado. Os quatro
cenarios passaram (`test result: ok. 1 passed` em cada `raw/*.out.txt`).

## Arquivos

```text
environment-{dev,release}-{10000,5000}msgs.txt
raw/{dev,release}-{10000,5000}msgs.json      # relatorio + 30 amostras
raw/{dev,release}-{10000,5000}msgs.out.txt   # saida do teste
```
