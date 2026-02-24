---
name: git-assist
description: Auxilia em fluxos de trabalho com git — gera mensagens de commit a partir de diffs, explica conflitos, sugere comandos.
triggers:
  - git ajuda
  - mensagem de commit
  - conflito de merge
  - git diff
  - rebase
dependencies: []
---

# Git Assist

Ajude o usuário com fluxos de trabalho do git utilizando a ferramenta `bash` para executar comandos git.

---

## Gerar mensagens de commit

Quando o usuário pedir uma mensagem de commit:

1. Execute `bash` com `git diff --cached` para ver as alterações staged (ou `git diff` para alterações não staged).
2. Analise o que mudou — arquivos modificados, linhas adicionadas/removidas, natureza da alteração.
3. Escreva a mensagem de commit seguindo o formato convencional:

```

<tipo>(<escopo>): <descrição curta>

<corpo opcional explicando o motivo>
```

Tipos aceitos:
`feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`

Regras para a mensagem:

* Use modo imperativo ("adiciona feature" e não "adicionada feature")
* Limite de 72 caracteres na linha de assunto
* Explique o *porquê*, não o *o quê* (o diff já mostra o que mudou)

---

## Explicar conflitos de merge

Quando o usuário tiver um conflito:

1. Execute `git status` para identificar arquivos em conflito.
2. Use `file_read` no arquivo conflitado para visualizar os marcadores de conflito.
3. Explique o que cada lado alterou e por que houve conflito.
4. Sugira uma resolução (ou pergunte ao usuário qual versão ele prefere).

---

## Fluxos de trabalho comuns

### Ajuda interativa

Quando o usuário perguntar "como eu...":

* Verifique o estado atual com `git status` e `git log --oneline -5`
* Sugira comandos específicos para a situação atual
* Explique o que cada comando fará antes de executá-lo

---

### Desfazer erros

| Situação                                      | Comando                     |
| --------------------------------------------- | --------------------------- |
| Desfazer último commit (mantendo alterações)  | `git reset --soft HEAD~1`   |
| Descartar alterações não staged em um arquivo | `git checkout -- <arquivo>` |
| Remover arquivo do staging                    | `git reset HEAD <arquivo>`  |
| Desfazer commit já enviado (push)             | `git revert <sha>`          |
| Encontrar commit perdido                      | `git reflog`                |

---

### Gerenciamento de branches

| Tarefa                          | Comando                                 |
| ------------------------------- | --------------------------------------- |
| Criar e mudar para uma branch   | `git checkout -b <nome>`                |
| Ver todas as branches           | `git branch -a`                         |
| Deletar branch já mergeada      | `git branch -d <nome>`                  |
| Fazer rebase na main            | `git rebase main`                       |
| Unir (squash) últimos N commits | `git reset --soft HEAD~N && git commit` |

---

## Regras

* Sempre execute `git status` antes de sugerir comandos destrutivos.
* Nunca sugira `--force` sem explicar o risco e recomendar `--force-with-lease`.
* Nunca sugira `git reset --hard` sem alertar sobre risco de perda de dados.
* Se o repositório estiver com alterações pendentes (dirty), mencione isso antes de qualquer ação.
* Prefira mostrar ao usuário o que vai acontecer (`--dry-run`, `git diff`) antes de aplicar mudanças.