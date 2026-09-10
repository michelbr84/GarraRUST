- **Seis paginas implementadas perdem a tag "Em breve" (#1116).** Providers,
  channels, sessions, settings, diagnostics e logs continuavam marcados como
  por vir na sidebar mesmo com o roteador ja despachando cada um para o seu
  loader real. As duas coisas que de fato nao existem - o sino de
  notificacoes e as abas CLI/Schema do painel direito - mantem a marca, mas
  o titulo agora cita a issue #1116 em vez do plan 0117a, que nao existe.
- **Falha de carregamento passa a dizer o codigo HTTP e o que fazer (#1116).**
  Um helper `renderFetchError` substitui os "Falha ao carregar X." espalhados
  pelos loaders. No caso 401 - o frequente, quando `gateway.api_key` esta
  ligada e o navegador nao tem chave salva - a mensagem aponta para o form
  "Gateway Authentication" do painel direito e o revela, em vez de mandar
  recarregar a pagina.
