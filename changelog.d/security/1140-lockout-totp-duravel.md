- **O lockout de TOTP do painel admin virou duravel (#1140).** A contagem de
  codigos errados (5 em 15 minutos) vivia num `HashMap` dentro do processo:
  reiniciar o gateway, atingir outra instancia ou usar processos
  independentes zerava o contador e devolvia o brute force de um codigo de 6
  digitos a quem ja tinha a senha. Agora ela mora na tabela `totp_attempts`
  do `admin.db`, com limpeza da janela expirada na mesma transacao em que
  conta — duas instancias sobre o mesmo banco compartilham um so orcamento
  de tentativas. Contagem ilegivel virou recusa fechada (500) em vez de "nao
  esgotou", e uma tentativa avaliada que nao consegue ser registrada tambem
  recusa: um chute de graca era o mesmo buraco por outro caminho. O custo
  aceito e o inverso do anterior: o dono espera a janela de 15 minutos, sem
  atalho por restart.
