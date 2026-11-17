- **Desligamento do gateway deixa de esperar um canal para sempre (#1913).**
  O `disconnect_all` dos canais push era a unica chamada de limpeza sem prazo
  dentro de `shutdown_subsystems`: um `disconnect()` que nunca retornasse
  segurava o processo depois do `SIGTERM`, e o gateway so saia pelo `SIGKILL`
  da CLI — antes do cancelamento do WhatsApp vinculado e da limpeza final do
  processo. O encerramento dos canais agora tem teto de 5 s (TLS 10 s + MCP
  10 s + canais 5 s cabem no grace de 30 s da CLI), cobre tambem a espera pelo
  lock dos canais, avisa no log quando estoura e segue o desligamento.
