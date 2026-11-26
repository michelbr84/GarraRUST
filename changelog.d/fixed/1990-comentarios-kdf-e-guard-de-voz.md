- **Comentarios de seguranca do gateway voltam a descrever o codigo (#1990).**
  O doc de `migrate_admin_secrets_kdf` ainda dizia que o `kdf.json` e gravado
  por ultimo (os parametros entram na mesma transacao do re-key), e o guard de
  voz se dizia "mesma regra" do guard do Signal, sendo mais estrito. So
  comentarios; nenhum comportamento muda.
