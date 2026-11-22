# Novidades da v0.5.2-rc7

> 🇬🇧 English: see the release notes link below · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.2-rc7)

**Pré-lançamento (release candidate).** Não é servido por `/releases/latest`, não é oferecido pelo `garraia update` nem pelo atualizador do app desktop. Para instalar esta versão explicitamente:

```bash
# Linux/macOS
curl -fsSL https://github.com/michelbr84/GarraRUST/releases/download/v0.5.2-rc7/install.sh | sh -s -- --version v0.5.2-rc7
```

```powershell
# Windows
$env:GARRAIA_VERSION='v0.5.2-rc7'; irm https://github.com/michelbr84/GarraRUST/releases/download/v0.5.2-rc7/install.ps1 | iex
```

Sétimo candidato da série v0.5.2, sobre a rc6. O binário se identifica como `garra 0.5.2` (o `-rc7` vive só na tag). A imagem Docker sai só com a tag `0.5.2-rc7`.

## O que entra desde a rc6

- **Circuit breaker mais justo (#1909, #1915).** A pausa por diretório de sessão morto agora solta no mesmo turno em que o diretório volta. Chamada **cancelada** não vira mais evidência de falha do modelo, e quem decide a evidência de uma conclusão é o **resultado observado**, não a alegação do executor. Quando o console recusa uma ação, a mensagem passa a nomear a **camada** que barrou (e o `garraia doctor`/`status` mostram o SHA-256 do executável, o instante da observação, o processo/serviço e a revisão do build — #1914).
- **Gateway mais organizado (#1966).** O módulo `rest_v1/me` foi dividido por área (`inbox/`, `export/`, `api_keys/`, `invites/`, etc.) sem mudança de comportamento ou de rota — preparativo para as mudanças de API da série.
- **Segurança de dependências.** wasmtime atualizado para 50.0.2/50.0.3 (#1952, #1957), SQLite 3.54 (#1850) e limpeza de exceções RUSTSEC vencidas (#1961).

## Testando um RC

O RC baixa com a versão fixa (comandos acima). Se algo quebrar, abra uma issue com a saída do `garraia doctor` — falha achada vira a rc8 (tag nova; nunca reeditamos uma tag publicada).
