- **`repo_search` recusa na hora quando nao ha repositorio ativo (#1380).** Sem
  `working_dir` a busca herdava o diretorio do processo e varria a arvore inteira ate
  estourar o timeout de 15s, para no fim nao responder nada util — o caso da sessao
  remota sem projeto selecionado, em que o diretorio do processo e `/`, o `$HOME` ou o
  diretorio de dados. Agora a tool usa o `RepoDir::decidir` para separar os dois
  sentidos de "sem working_dir": o diretorio herdado que E (ou esta dentro de) um
  repositorio segue sendo buscado, como no `garra chat` local; sem `.git`/`.hg`/`.svn`/
  `.jj` nele ou acima dele, a resposta sai em milissegundos, dizendo onde a tool olhou
  e que basta selecionar um projeto. Sessao que escolheu um `working_dir` nao muda de
  comportamento, e num turno restrito a recusa nao cita o caminho do host. Mudanca de
  comportamento: uma arvore de codigo SEM metadado de VCS no diretorio do processo (por
  exemplo um `COPY` de container ou um tarball exportado) era buscavel e agora recebe a
  recusa — selecione o projeto (defina o `working_dir` da sessao) para buscar nela.
