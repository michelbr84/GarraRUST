- **Encerramento de servidor MCP conta cada processo uma vez (#1913).** O
  relatorio do encerramento somava o mesmo processo a cada passada de
  conferencia em que ele ainda aparecia vivo depois do `SIGKILL`, entao
  `mortos_por_marcador` saia inflado sob carga (5 no lugar de 1) e o teste do
  relatorio falhava de forma intermitente no CI da `main`. A contagem passa a
  ser por PID.
