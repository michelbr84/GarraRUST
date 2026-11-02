- **Sintese de voz concorrente deixa de trocar o audio entre usuarios (#1754).**
  Os provedores Hibiki e LM Studio passavam o WAV por um temporario
  `garraia_tts_<pid>.wav`, o MESMO arquivo para toda chamada do processo: duas
  sinteses ao mesmo tempo escreviam e liam o mesmo caminho, e um usuario podia
  ouvir a resposta de outro (ou receber erro porque a outra chamada ja tinha
  apagado o arquivo). Agora os bytes vem direto da resposta HTTP, sem arquivo
  nenhum. Os clientes Whisper (STT) e Hibiki ganham timeout de 120 s, como o
  Chatterbox e o LM Studio ja tinham, e o resumo de resposta sem audio do LM
  Studio e cortado por caractere em vez de byte (o corte antigo dava panic com
  texto acentuado).
