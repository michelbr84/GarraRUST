# `ollama launch garraia` — patch pronto para o upstream

Esta pasta contém a integração do GarraIA com o `ollama launch`, escrita
contra o `ollama/ollama` real e **validada compilando e rodando os testes
lá dentro**. Ela não vive neste repositório em tempo de execução — é um
patch esperando para virar PR no repositório do Ollama.

## Por que está aqui e não lá

O registro de integrações do Ollama é uma slice Go compilada dentro do
binário ([`cmd/launch/registry.go`](https://github.com/ollama/ollama/blob/main/cmd/launch/registry.go)):

```go
var integrationSpecs = []*IntegrationSpec{ /* … */ }
```

Não há manifesto, plugin, diretório escaneado nem namespace no ollama.com.
Grep por `manifest`, `plugin`, `ReadDir` e `integrationSpecs = append` no
pacote retorna zero. As 19 integrações existentes (Claude Code, Codex,
Hermes, OpenCode, …) entraram **por pull request**. Portanto
`ollama launch garraia` só passa a existir quando este patch for aceito lá.

## Conteúdo

| Arquivo | Vai para |
|---|---|
| `garraia.go` | `cmd/launch/garraia.go` |
| `garraia_test.go` | `cmd/launch/garraia_test.go` |
| `registry.go.patch` | `git apply` sobre `cmd/launch/registry.go` |

## Como submeter

```bash
gh repo fork ollama/ollama --clone   # ou fork pela UI e clone o seu
cd ollama
git checkout -b garraia-launch-integration

cp <GarraRUST>/contrib/ollama-launch/garraia.go       cmd/launch/
cp <GarraRUST>/contrib/ollama-launch/garraia_test.go  cmd/launch/
git apply <GarraRUST>/contrib/ollama-launch/registry.go.patch

gofmt -l cmd/launch/          # deve sair vazio
go vet ./cmd/launch/
go build ./...
go test ./cmd/launch/          # a suíte inteira, não só TestGarraia*

git commit -am "launch: add GarraIA integration"
gh pr create --repo ollama/ollama
```

## O que foi verificado

Contra o `ollama/ollama` na `main` (Go 1.26.0), nesta sessão:

- `gofmt -l cmd/` — limpo.
- `go vet ./cmd/launch/` — limpo.
- `go build ./...` — ok.
- `go test ./cmd/launch/` — **suíte inteira verde**, incluindo os 10 testes
  novos `TestGarraia*` e os testes pré-existentes de registro.

Uma armadilha que só apareceu rodando a suíte de verdade: `TestListIntegrationInfos`
tem dois subtestes com exigências opostas. `follows_launcher_order` compara a
lista visível **inteira** contra `launcherIntegrationOrder`, então toda
integração não-hidden precisa estar lá; já `prioritizes_primary_launcher_integrations`
trava o **prefixo** (`claude, chatgpt, hermes, openclaw, …`). Ou seja: a entrada
nova tem de ir no **fim** da lista. A primeira tentativa colocou `garraia` em
quarto lugar e quebrou o segundo subteste — decidir promover uma integração no
menu primário é escolha dos mantenedores do Ollama, não nossa.

## Desenho

Implementa `Runner` (`Run`, `String`) e `ManagedSingleModel` (`Paths`,
`Configure`, `CurrentModel`, `Onboard`), espelhando `cmd/launch/hermes.go`
— a integração mais próxima em forma (um modelo primário escolhido pelo
launcher, com o app mantendo a própria UX de troca de modelo depois).

| Contrato | GarraIA |
|---|---|
| Instalação não-interativa | `curl -fsSL https://garraia.org/install.sh \| sh -s -- --skip-setup` |
| Config | `$GARRAIA_CONFIG_DIR` → `~/.config/garraia/config.yml` → `~/.garraia/config.yml` (legada só se já existir) |
| Entrada escrita | `llm["ollama-launch"] = {provider: openai, model, api_key: ollama, base_url: <host>/v1}` + `agent.default_provider` |
| Binário | `garraia` ou `garra` no `PATH`; senão `~/.local/bin/garraia`, `/usr/local/bin/garraia` |
| Executar | `garraia chat` |

Decisões que valem registrar:

- **`provider: openai` + `/v1`, não `provider: ollama`.** É o mesmo contrato
  que o launcher já escreve para o Hermes. O endpoint ignora a chave, mas o
  cliente OpenAI exige uma não-vazia — daí o placeholder `ollama`.
- **A chave `ollama-launch` é do launcher.** Fica separada de uma entrada
  `ollama` escrita à mão pelo usuário, então as duas nunca disputam a mesma
  config.
- **`Configure` preserva o resto do `config.yml` byte a byte.** Roda
  desatendido; sobrescrever a config que o operador preencheu seria perda
  silenciosa de dado. Um `default_provider` anterior que ainda resolve é
  rebaixado para o início de `fallback_providers`, não descartado.
- **`CurrentModel` só reivindica o modelo** quando `default_provider` aponta
  para a chave do launcher **e** o `base_url` bate com o host atual do Ollama.
  Senão o usuário trocou de provider na mão e o launcher não pode agir sobre
  estado que não é dele.
- **`Configure` fecha o arquivo em `0600`**, porque `config.yml` pode carregar
  `llm.*.api_key`.
- **Windows fica como não suportado nesta v1** (`Supported()` devolve erro com
  o caminho manual): o GarraIA não publica instalador `.ps1` para web — só
  `scripts/build-installer.ps1`, que é build local de MSI.

## Do lado do GarraIA

O que a integração precisa já está na `main` deste repositório (plano
[`plans/0357-ollama-defaults-and-launch.md`](../../plans/0357-ollama-defaults-and-launch.md)):
`install.sh --skip-setup`, `garraia config set-model` e o padrão
`qwen3.8:latest`. Ver [`docs/integrations/ollama-launch.md`](../../docs/integrations/ollama-launch.md).

Enquanto o PR upstream não é aceito, o equivalente manual é:

```bash
curl -fsSL https://garraia.org/install.sh | sh -s -- --skip-setup
garraia config set-model --model qwen3.8:latest
garraia --model qwen3.8
```
