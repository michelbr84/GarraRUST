- **`file_jail` recusa `..` com `/` em caminho verbatim e relativo drive/root (#1737).**
  No Windows, `x/../../y` sob `\\?\C:\...` virava um unico componente e escapava
  da guarda de `ParentDir` (contido so pelo `starts_with(root)`); `C:foo` e `\foo`
  eram "relativos" e `join` descartava a ancora. Agora a guarda tambem olha o
  texto (so no Windows) e `confine` recusa `Prefix`/`RootDir` em relativos.
  Anotado o quirk de `sem_prefixo_verbatim` (F6). Sem mudanca em POSIX.
