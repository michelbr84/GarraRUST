- **A skill local `code-review` do harness vira `review-inline`, e o hook `PreToolUse` de Bash sai do `.claude/settings.json` (#1983).**
  `code-review` e o nome da skill embutida do Claude Code, a revisao com nivel
  de esforco que o `/ship` chama; a do projeto, um checklist curto, respondia no
  lugar dela. O `session-start.sh` deixa de gerar o nome e remove a copia que
  sobrou, e o `scripts/verify.sh` recusa a volta dele. O hook saiu porque, com
  ele versionado no projeto, o `/ship` recusa commit, push e a vigilia do PR:
  deixam de valer o bloqueio de `rm` na raiz ou no home, de force-push na main e
  de eco de segredo, e o `.claude/audit.log`. O script continua em
  `.claude/hooks/pre-tool-use.sh` para quem quiser liga-lo nas proprias
  configuracoes de usuario.
