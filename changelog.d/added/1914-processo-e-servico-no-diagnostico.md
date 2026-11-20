- **`garra_diagnostics` diz qual processo responde e quem o supervisiona
  (#1914).** A entrada nova `processo.servico`, logo depois de
  `versao.executavel`, traz o PID, o inicio do gateway em UTC (`iniciado_em`,
  ISO 8601 com `Z`) e o supervisor lido do proprio cgroup: unit systemd com
  escopo `usuario` (`systemctl --user`) ou `sistema`, ou `nenhum` quando o
  processo saiu de um terminal / `start -d`. Um restart muda PID e inicio,
  que e o que se compara antes/depois de um update. cgroup ilegivel sai
  `nao_verificado` com motivo e estado `erro` — nunca "sem supervisor"; fora
  do Linux, `nao_suportado`. Nada de `systemctl`, nada de argumento, nenhum
  caminho de cgroup no relatorio. O parser de cgroup do `garra restart`
  mudou para `garraia_common::systemd` para os dois usarem o mesmo.
  Contagem de reinicios da unit (`NRestarts`) fica fora: so sai do systemd.
