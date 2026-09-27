- **O modo `search` enxerga a leitura do MCP `filesystem` (#1384).** O piso de
  todo remetente do WhatsApp pessoal escondia TODA ferramenta do servidor
  `filesystem` — inclusive `read_text_file` e `list_directory` — porque a
  whitelist so listava as nativas, e o modelo dizia nao ter como ler arquivo
  com o servidor conectado. A `allowed` ganha uma terceira forma de entrada,
  `*/<operacao>` (a operacao exata, em qualquer servidor), e `search` passa a
  listar as dez operacoes somente-leitura do
  `@modelcontextprotocol/server-filesystem` (`read_file`, `read_text_file`,
  `read_media_file`, `read_multiple_files`, `list_directory`,
  `list_directory_with_sizes`, `directory_tree`, `search_files`,
  `get_file_info`, `list_allowed_directories`). Escrita (`write_file`,
  `edit_file`, `create_directory`, `move_file`) e qualquer nome desconhecido
  continuam fail-closed, e `denied` segue vencendo tudo. Os outros modos
  somente-leitura nao mudam nesta fatia.
