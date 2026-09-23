- **Web Console mostra o perfil de execucao no header (#1410).** O perfil do
  ADR 0024 (`standard` | `isolated-pod`) so aparecia como uma linha dentro da
  pagina Diagnostics, e `isolated-pod` e justamente o perfil que da ao agente
  poder total dentro do pod — o operador precisava navegar para descobrir em
  qual dos dois o gateway esta. Agora um badge fixo no header le
  `/api/settings/effective` (auth-free, secret-free), mostra o valor e a
  origem (`default` | `file` | `env`) e, em `isolated-pod`, muda de tom e
  carrega no tooltip o mesmo aviso do boot: o pod e a fronteira de seguranca,
  nao o Garra, e como reverter.
