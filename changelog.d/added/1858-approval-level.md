- **Nível de aprovação `agent.approval` e `--yolo` no CLI (#1858, segunda
  fatia).** O knob declara o que o runtime faz com o tier arriscado de
  comando: `always` (default — comportamento atual) e `never-confirm`,
  que dispensa o **pedido** de confirmação onde existe canal. O `garra chat`
  ganha `--yolo` / `--dangerously-skip-permissions` como atalho da sessão:
  opt-in explícito, vence o config, nunca persiste e nunca é default. O
  hard-deny (`rm -rf /`, fork bomb, exfiltração) e o fail-closed de canal
  ausente continuam intocados em todos os níveis. Auditoria: sob
  `never-confirm` todo comando executado sai com registro de auditoria —
  `WARN` no log (`bash`, `run_tests`, `device_execute`) e linha JSONL em
  `<data_dir>/audit/never-confirm.jsonl` para o `bash`, com comando redigido,
  origem e timestamp. `/status` mostra o nível ativo. Documentado em
  `docs/hardening-gateway.md` §2.
