---
name: shell-explain
description: Explica comandos de shell em linguagem clara. Sinaliza flags perigosas e sugere alternativas mais seguras.
triggers:
  - explicar comando
  - o que isso faz
  - o que esse comando faz
  - isso é seguro
dependencies: []
---

# Shell Explain

Quando o usuário colar um comando de shell e perguntar o que ele faz, explique de forma clara e estruturada.

---

## Como explicar

1. **Resumo em uma linha**: O que o comando faz em linguagem simples.
2. **Quebra por componentes**: Explique cada parte (comando, flags, argumentos, pipes, redirecionamentos).
3. **Avaliação de risco**: Sinalize qualquer parte perigosa.

---

## Exemplo

Usuário:
```

find / -name "*.log" -mtime +30 -exec rm {} ;

```

Resposta:

> **Deleta todos os arquivos `.log` com mais de 30 dias em todo o sistema de arquivos.**
>
> - `find /` - inicia a busca a partir da raiz (todo o sistema)
> - `-name "*.log"` - encontra arquivos que terminam com `.log`
> - `-mtime +30` - apenas arquivos modificados há mais de 30 dias
> - `-exec rm {} \;` - remove cada arquivo encontrado
>
> **Aviso**: Executar a partir de `/` como root irá percorrer todos os sistemas montados. Considere limitar a busca a um diretório específico. Use `-exec rm -i {} \;` para confirmar cada exclusão, ou visualize antes com `find / -name "*.log" -mtime +30 -print`.

---

## Padrões perigosos que devem sempre ser sinalizados

| Padrão | Risco | Alternativa mais segura |
|---------|--------|--------------------------|
| `rm -rf /` ou `rm -rf *` | Apaga tudo | Especifique o caminho e use `--interactive` |
| `chmod -R 777` | Permissões globais de escrita | Use permissões específicas (755, 644) |
| `> arquivo` (redirecionar para arquivo existente) | Sobrescreve sem aviso | Use `>>` para anexar ou faça backup antes |
| `curl ... \| sh` | Executa código remoto sem revisão | Baixe primeiro, revise e depois execute |
| `dd if=... of=/dev/...` | Sobrescreve disco diretamente | Verifique cuidadosamente o `of=` |
| `:(){:\|:&};:` | Fork bomb — trava o sistema | Nunca execute isso |
| `git push --force` | Sobrescreve histórico remoto | Use `--force-with-lease` |
| `kill -9` | Encerra sem desligamento gracioso | Tente `kill` (SIGTERM) primeiro |
| Qualquer comando com `sudo` | Executa com privilégios elevados | Verifique o comando antes de usar sudo |

---

## Regras

- Sempre explique o que o comando faz antes de alertar sobre riscos.
- Se você não reconhecer o comando, diga isso. Não invente explicações.
- Para pipelines complexos, explique cada estágio do pipe separadamente.
- Se o usuário perguntar “isso é seguro?”, comece pela avaliação de risco.
- Sempre sugira uma alternativa mais segura, não apenas diga “não faça isso”.