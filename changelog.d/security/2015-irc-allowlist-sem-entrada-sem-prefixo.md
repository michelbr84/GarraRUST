- **IRC: um nick igual ao id de outro canal nao passa mais pela allowlist
  (GHSA-g6c8-mx6x-5g92).** A allowlist e uma so para todos os canais, e os ids
  dos outros canais ficam nela sem prefixo. O IRC aceitava uma entrada sem
  prefixo igual ao nick de quem mandou, e no IRC o nick e escolhido por quem
  conecta: quem entrasse no servidor com um nick igual ao id de alguem admitido
  pelo Slack (`U01ABCDEF` e um nick valido), inclusive o dono, falava com o
  agente como aquela pessoa, sem codigo de pareamento. Afetava da v0.4.1 a
  v0.5.2. Agora o IRC so aceita entradas `irc:<nick>`. Tambem saiu a migracao
  automatica das entradas sem prefixo para a forma `irc:`, que a #2015 tinha
  posto na `main` e que nenhuma versao lancou: ela estendia o mesmo acesso as
  outras grafias do nick. **Quem foi pareado pelo IRC antes desta versao precisa
  de um codigo novo de `/pair`**, gerado pelo dono em outro canal. Um dono que
  so usa o IRC troca `"<nick>"` por `"irc:<nick>"` no `owner` e em `users` do
  `allowlist.json`, com o gateway parado. Ver `docs/hardening-gateway.md`.
