# changelog.d — fragmentos de changelog

Cada PR escreve o que mudou num **arquivo próprio** aqui, em vez de editar o
`CHANGELOG.md` direto. Arquivos diferentes nunca conflitam — e era exatamente
isso que fazia dois PRs paralelos colidirem toda vez na seção `[Unreleased]`.

## Como escrever um fragmento

Crie um arquivo `.md` dentro da pasta da seção certa:

```text
changelog.d/fixed/972-toctou-indice-vetorial.md
changelog.d/added/950-cli-garra-memory.md
changelog.d/security/971-knn-isola-tenant.md
```

Seções válidas (as do [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)):
`added`, `changed`, `deprecated`, `removed`, `fixed`, `security`.

Convenção de nome: `<numero-da-issue-ou-PR>-<slug-curto>.md`. O número é só
para achar a origem depois; a ordem final é alfabética por nome de arquivo,
então ela é determinística.

O conteúdo é o(s) bullet(s) markdown que iriam para o `CHANGELOG.md`, já no
estilo da casa — primeira frase em negrito dizendo o que mudou, com o número
da issue, e o resto explicando o porquê:

```markdown
- **Deletar memoria deixa de criar vetores orfaos (#960).** `delete_session_memory`
  e `compact` apagavam as linhas mas nunca o indice: vetor e mapeamento ficavam
  para sempre.
```

Sem acento, como o resto do `CHANGELOG.md`.

## Como juntar

```bash
python3 scripts/changelog/assemble.py            # imprime o resultado, nao toca em nada
python3 scripts/changelog/assemble.py --check    # valida os fragmentos (bom antes de commitar)
python3 scripts/changelog/assemble.py --write    # insere no CHANGELOG.md e apaga os fragmentos
```

O `--write` é passo do **release** (ver `docs/releasing.md`), não de PR
individual. Fora do release, os fragmentos ficam acumulando aqui — é o estado
normal do repositório entre uma versão e outra.
