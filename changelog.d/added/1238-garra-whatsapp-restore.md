- **`garra whatsapp restore` devolve a sessao que ficou arquivada (#1238).**
  Quando um re-vinculo e interrompido de um jeito que nao da ao GarraIA a
  chance de desfaze-lo — `kill -9`, queda de energia —, a sessao boa fica em
  `session.enc.prev` sem `session.enc`. A funcao que a traz de volta ja
  existia e nenhuma superficie a expunha: o `status` via o arquivado e mandava
  APAGAR. Agora ele oferece a recuperacao primeiro, e o comando novo renomeia
  o arquivo de volta, aperta o modo para 0600 e religa o canal na config —
  nessa ordem, a mesma do `link`. Ele nunca passa por cima de uma sessao em
  uso: nesse caso diz o que ha e sai 69, sem apagar nada.
