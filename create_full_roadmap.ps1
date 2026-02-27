# PowerShell script para criar roadmap completo no Linear
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\create_full_roadmap.ps1

param(
    [string]$ApiKey = $env:LINEAR_API_KEY
)

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    Write-Host "Obtenha em: https://linear.app/settings/api" -ForegroundColor Yellow
    Write-Host "Exemplo: `$env:LINEAR_API_KEY='lin_api_xxx'; .\create_full_roadmap.ps1" -ForegroundColor Cyan
    exit 1
}

Write-Host "Criando roadmap completo no Linear..." -ForegroundColor Green

$headers = @{
    "Authorization" = $ApiKey
    "Content-Type" = "application/json"
}

# Buscar ID do projeto/equipe GAR
Write-Host "Buscando equipe GAR..." -ForegroundColor Cyan
$query = @{ query = "query { teams { nodes { id name key } } }" } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $query -ErrorAction Stop
$team = $response.data.teams.nodes | Where-Object { $_.key -eq "GAR" }

if (-not $team) {
    Write-Host "Equipe GAR nao encontrada!" -ForegroundColor Red
    exit 1
}

Write-Host "Equipe GAR encontrada: $($team.id)" -ForegroundColor Green

# Buscar estados disponiveis
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$states = $stateResponse.data.team.states.nodes

# Mapeamento de prioridade Linear: 1=Urgent, 2=High, 3=Medium, 4=Low
# Prioridades P0-P4 do roadmap:
# P0 = 1 (Urgent)
# P1 = 1 (Urgent)  
# P2 = 2 (High)
# P3 = 3 (Medium)
# P4 = 4 (Low)

$backlogState = $states | Where-Object { $_.name -eq "Backlog" } | Select-Object -First 1
if (-not $backlogState) {
    $backlogState = $states | Select-Object -First 1
}

Write-Host "Usando estado: $($backlogState.name) ($($backlogState.id))" -ForegroundColor Green

# Roadmap completo - todas as issues organizadas por Project/Epic
$roadmap = @(
    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 0 — "Board Hygiene & Source of Truth" (P0 | 0.5 dia)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    # EPIC 0.1 — Fechar o que está Done de verdade
    
    @{
        id = "GAR-201"
        title = "[P0] OpenRouter App Identity - headers HTTP-Referer + X-Title"
        desc = "## Status: JA IMPLEMENTADO

**Evidencia:** OpenRouter dashboard mostra App: GarraIA

**Headers enviados:**
- HTTP-Referer: https://garraia.org
- X-Title: GarraIA

**Testes:** cargo test -p garraia-agents (76 testes passaram)

**Acao:** Mover para Done e link do PR/commit.
**Arquivo:** crates/garraia-agents/src/openai.rs (linhas 97-100, 350-353, 407-410)
**Prioridade:** P0 (bloqueador repo)"
        priority = 1
    },
    
    @{
        id = "GAR-202"
        title = "[P0] Fix: CommandRegistry antes de criar canais Telegram"
        desc = "## Problema
CommandRegistry deve ser inicializado ANTES de criar canais Telegram para que comandos funcionem.

## Criterios de Aceite
- [ ] Comandos respondem no runtime
- [ ] Evidencia: output de comando no Telegram

**Prioridade:** P0"
        priority = 1
    },
    
    @{
        id = "GAR-203"
        title = "[P0] /model implementado e funcional"
        desc = "## Problema
Comando /model deve funcionar para trocar modelo.

## Criterios de Aceite
- [ ] /model openrouter/free funciona
- [ ] Output: Model set to: ...
- [ ] Evidencia: screenshot do Telegram

**Prioridade:** P0"
        priority = 1
    },
    
    # EPIC 0.2 — Corrigir o plans/ e roadmap
    
    @{
        id = "GAR-204"
        title = "[P3] Fix: roadmap versionado (plans/ ou docs/roadmaps/)"
        desc = "## Problema
.gitignore com plans/ pode impedir versionar roadmap.

## Criterios de Aceite
- [ ] Roadmap esta versionado (plans/ removido do .gitignore OU movido para docs/roadmaps/)
- [ ] Conteudo colado no Linear como source of truth

**Prioridade:** P3"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 1 — "Runtime Stability & Integrity" (P0 | 2-5 dias)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    # EPIC 1.1 — file_write Integrity (P0 bloqueador)
    
    @{
        id = "GAR-210"
        title = "[P0] Test: stress test do file_write no Windows (hash/tamanho)"
        desc = "## Problema
file_write pode corromper arquivos grandes, virando corruptor de repo.

## Criterios de Aceite
- [ ] 50+ ciclos de overwrite em arquivo grande sem divergencia de hash/tamanho
- [ ] Falha = erro explicito (nunca sucesso silencioso)
- [ ] Log claro: bytes escritos + verificacao pos-write

**Prioridade:** P0 (pode quebrar repo)
**Arquivo:** crates/garraia-agents/src/tools/file_write_tool.rs"
        priority = 1
    },
    
    @{
        id = "GAR-211"
        title = "[P0] Fix: escrita atomica + verificacao pos-write + backup opcional"
        desc = "## Depende de: GAR-210

## Criterios de Aceite
- [ ] Escreve em temp + replace seguro
- [ ] Rele e valida integridade
- [ ] Opcao de .bak (config)
- [ ] Testes automatizados (pelo menos unit/integration)

**Prioridade:** P0
**Arquivo:** crates/garraia-agents/src/tools/file_write_tool.rs"
        priority = 1
    },
    
    @{
        id = "GAR-212"
        title = "[P0] Guardrails: allowed_directories / path normalize no Windows"
        desc = "## Depende de: GAR-211

## Criterios de Aceite
- [ ] Normalizacao de path (case/\//)
- [ ] Mensagens acionaveis quando bloqueado
- [ ] Doc rapida: como liberar G:\Projetos\GarraRUST

**Prioridade:** P0"
        priority = 1
    },
    
    # EPIC 1.2 — /health real (providers e UX)
    
    @{
        id = "GAR-213"
        title = "[P0] Fix: OpenAI 401 (config/disable smart)"
        desc = "## Problema atual
- openai: 401
- openrouter: ok, mas pode 429
- ollama-local: nao conecta

## Criterios de Aceite
- [ ] Se nao tiver key: provider aparece como Not configured (nao down confuso)
- [ ] Se tiver key invalida: erro claro + instrucao

**Prioridade:** P0
**Arquivo:** crates/garraia-gateway/src/health.rs"
        priority = 1
    },
    
    @{
        id = "GAR-214"
        title = "[P0] Fix: endpoints/portas dos servicos locais (Ollama/Whisper/TTS)"
        desc = "## Depende de: GAR-213

## Criterios de Aceite
- [ ] ollama-local detecta se servico esta online e URL correta
- [ ] whisper-stt / chatterbox / hibiki: porta/endpoints alinhados com config real
- [ ] /health mostra como corrigir (ex: inicie servico X na porta Y)

**Prioridade:** P0
**Arquivo:** crates/garraia-gateway/src/health.rs"
        priority = 1
    },
    
    @{
        id = "GAR-215"
        title = "[P1] Melhoria: /health com severidade + acoes sugeridas"
        desc = "## Depende de: GAR-214

## Criterios de Aceite
- [ ] Status: OK / DEGRADED / DOWN
- [ ] Cada erro com proximo passo (1 linha)

**Prioridade:** P1"
        priority = 2
    },
    
    # EPIC 1.3 — Resiliencia OpenRouter (429)
    
    @{
        id = "GAR-216"
        title = "[P1] Fix: backoff/retry + fallback automatico (OpenRouter 429)"
        desc = "## Problema
OpenRouter pode retornar 429 (rate limit).

## Criterios de Aceite
- [ ] Em 429: retry com backoff curto
- [ ] Se persistir: fallback para outro modelo/provider configurado
- [ ] Mensagem pro usuario: modelo free rate-limited, usando fallback...
- [ ] Log: tentativa, motivo, fallback aplicado

**Prioridade:** P1
**Arquivo:** crates/garraia-agents/src/provider_resilience.rs"
        priority = 2
    },
    
    @{
        id = "GAR-217"
        title = "[P2] Melhorar /model (persistencia, validacao, listagem)"
        desc = "## Depende de: GAR-216

## Criterios de Aceite
- [ ] Persistir override por sessao/usuario
- [ ] Validar modelo
- [ ] Listar modelos (se aplicavel)
- [ ] Comportamento previsivel e documentado

**Prioridade:** P2"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 2 — "Voice Pipeline End-to-End" (P0.5 | 3-7 dias)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    # EPIC 2.1 — Bootstrap / Handler
    
    @{
        id = "GAR-220"
        title = "[P0] Fix: voice_handler None no bootstrap"
        desc = "## Problema
voice_handler pode ser None mesmo com voice habilitado.

## Criterios de Aceite
- [ ] Com voice habilitado, voice_handler e Some(...)
- [ ] Log: Voice enabled + URLs STT/TTS

**Prioridade:** P0.5
**Arquivo:** crates/garraia-gateway/src/bootstrap.rs"
        priority = 1
    },
    
    @{
        id = "GAR-221"
        title = "[P0] TelegramAdapter: detectar msg.voice() e entrar no pipeline"
        desc = "## Depende de: GAR-220

## Criterios de Aceite
- [ ] Log: voice message received
- [ ] Se voice off: resposta de texto explicando como habilitar

**Prioridade:** P0.5
**Arquivo:** crates/garraia-channels/src/telegram.rs"
        priority = 1
    },
    
    # EPIC 2.2 — STT/TTS e formatos de envio
    
    @{
        id = "GAR-222"
        title = "[P0.5] Whisper STT health + transcricao"
        desc = "## Criterios de Aceite
- [ ] Endpoint de health ok
- [ ] Erro claro se offline

**Prioridade:** P0.5
**Arquivo:** crates/garraia-gateway/src/voice_handler.rs"
        priority = 1
    },
    
    @{
        id = "GAR-223"
        title = "[P0.5] TTS (Chatterbox/Hibiki) health + geracao"
        desc = "## Criterios de Aceite
- [ ] Gera audio consistente
- [ ] Path/temp bem definido

**Prioridade:** P0.5"
        priority = 1
    },
    
    @{
        id = "GAR-224"
        title = "[P0.5] Envio Telegram correto (sendVoice vs sendDocument)"
        desc = "## Depende de: GAR-223

## Criterios de Aceite
- [ ] Define padrao (voz como bolha = OGG/Opus)
- [ ] Conversao se necessario
- [ ] Teste real no Telegram

**Prioridade:** P0.5"
        priority = 1
    },
    
    # EPIC 2.3 — Teste E2E + troubleshooting
    
    @{
        id = "GAR-225"
        title = "[P1] Script/Checklist de teste de voz (manual + logs)"
        desc = "## Criterios de Aceite
- [ ] Passo a passo reprodutivel
- [ ] Outputs esperados

**Prioridade:** P1"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 3 — "Security & Hardening (MCP + Tools)" (P1 | 1-2 semanas)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    # EPIC 3.1 — MCP Hardening
    
    @{
        id = "GAR-230"
        title = "[P1] Auth/allowlist para MCP"
        desc = "## Criterios de Aceite
- [ ] Requests sem auth/fora allowlist bloqueadas com log claro

**Prioridade:** P1 (risco de seguranca)
**Arquivo:** crates/garraia-agents/src/mcp/manager.rs"
        priority = 2
    },
    
    @{
        id = "GAR-231"
        title = "[P1] Validacao de payload (schema/limites)"
        desc = "## Depende de: GAR-230

## Criterios de Aceite
- [ ] Rejeita payloads grandes, invalidos, tool names desconhecidos

**Prioridade:** P1"
        priority = 2
    },
    
    @{
        id = "GAR-232"
        title = "[P1] Rate limiting basico e audit trail"
        desc = "## Depende de: GAR-231

## Criterios de Aceite
- [ ] Logs por tool + user/sessao + tempo

**Prioridade:** P1"
        priority = 2
    },
    
    # EPIC 3.2 — Tool permissions model
    
    @{
        id = "GAR-233"
        title = "[P1] Escopo por tool (file_write, exec, network)"
        desc = "## Criterios de Aceite
- [ ] Permissoes por ambiente (dev/prod)
- [ ] Deny-by-default para paths

**Prioridade:** P1"
        priority = 2
    },
    
    @{
        id = "GAR-234"
        title = "[P1] Secrets hygiene"
        desc = "## Criterios de Aceite
- [ ] Nunca logar tokens
- [ ] Mensagens de erro sem vazar credenciais

**Prioridade:** P1"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 4 — "Multi-Agent & Tool Escalation" (P2 | 2-3 semanas)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-240"
        title = "[P2] Definir fluxo minimo de escalonamento (agent -> tool-runner)"
        desc = "## Criterios de Aceite
- [ ] 1 caso de uso real funcionando com trace

**Prioridade:** P2 (features estruturais)
**Arquivo:** crates/garraia-agents/src/runtime.rs"
        priority = 2
    },
    
    @{
        id = "GAR-241"
        title = "[P2] Session/context compartilhado + limites"
        desc = "## Depende de: GAR-240

## Criterios de Aceite
- [ ] Contexto nao explode
- [ ] Logs rastreaveis
- [ ] Timeout/retries

**Prioridade:** P2"
        priority = 2
    },
    
    @{
        id = "GAR-242"
        title = "[P2] Testes integrados (ao menos 1)"
        desc = "## Depende de: GAR-241

## Criterios de Aceite
- [ ] Teste passa local/CI

**Prioridade:** P2"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 5 — "Docs & garraia.org" (P3 | continuo)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-250"
        title = "[P3] Docs: Providers (OpenAI/OpenRouter/Ollama/Locais)"
        desc = "## Criterios de Aceite
- [ ] Configuracao + troubleshooting (401/429/offline)

**Prioridade:** P3 (docs/polish/UX)"
        priority = 3
    },
    
    @{
        id = "GAR-251"
        title = "[P3] Docs: Voice setup completo + troubleshooting"
        desc = "## Criterios de Aceite
- [ ] Portas/endpoints/flags/config reais

**Prioridade:** P3"
        priority = 3
    },
    
    @{
        id = "GAR-252"
        title = "[P3] Docs: Comandos Telegram referencia oficial"
        desc = "## Criterios de Aceite
- [ ] /health, /mcp, /stats, /model, /help (com exemplos)

**Prioridade:** P3"
        priority = 3
    },
    
    @{
        id = "GAR-253"
        title = "[P3] Runbook: Diagnostico rapido"
        desc = "## Criterios de Aceite
- [ ] Checklist em 2 minutos (logs + health)

**Prioridade:** P3"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # PROJECT 6 — "Local LLM (LM Studio) Integration" (P4 | depois de estabilizar)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-260"
        title = "[P4] Provider LM Studio (OpenAI-compatible base_url)"
        desc = "## Criterios de Aceite
- [ ] Chamadas funcionando local e via tunnel
- [ ] Com token

**Prioridade:** P4 (expansoes)
**Arquivo:** crates/garraia-agents/src/openai.rs"
        priority = 4
    },
    
    @{
        id = "GAR-261"
        title = "[P4] Infra segura (tunnel + auth)"
        desc = "## Depende de: GAR-260

## Criterios de Aceite
- [ ] Nao expor direto do browser, somente backend

**Prioridade:** P4"
        priority = 4
    },
    
    @{
        id = "GAR-262"
        title = "[P4] Docs de deploy headless"
        desc = "## Depende de: GAR-261

## Criterios de Aceite
- [ ] Passo a passo Windows + validacao

**Prioridade:** P4"
        priority = 4
    }
)

# Criar issues
$created = 0
$failed = 0

Write-Host ""
Write-Host "=== Criando $($roadmap.Count) issues ===" -ForegroundColor Cyan

foreach ($item in $roadmap) {
    Write-Host "Criando $($item.id): $($item.title)..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $backlogState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 10
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueCreate.success) {
            Write-Host "  $($item.id) -> OK" -ForegroundColor Green
            $created++
        } else {
            Write-Host "  $($item.id) -> FALHA: $($resp | ConvertTo-Json -Compress)" -ForegroundColor Red
            $failed++
        }
    } catch {
        $errMsg = $_.Exception.Message
        if ($errMsg -like "*duplicate*") {
            Write-Host "  $($item.id) -> JA EXISTE (pulando)" -ForegroundColor Yellow
        } else {
            try {
                $errResp = [System.Text.Encoding]::UTF8.GetString($_.Exception.Response.GetResponseStream().ReadToEnd())
                Write-Host "  $($item.id) -> ERRO: $errMsg | Response: $errResp" -ForegroundColor Red
            } catch {
                Write-Host "  $($item.id) -> ERRO: $errMsg" -ForegroundColor Red
            }
        }
        $failed++
    }
}

Write-Host ""
Write-Host "=== Resultado ===" -ForegroundColor Green
Write-Host "Criadas: $created" -ForegroundColor Green
Write-Host "Falhas/Existentes: $failed" -ForegroundColor Yellow
Write-Host ""
Write-Host "Roadmap completo criado com sucesso!" -ForegroundColor Green
Write-Host ""
Write-Host "=== Ordem recomendada de execucao ===" -ForegroundColor Cyan
Write-Host "1. P0: file_write integrity (GAR-210, GAR-211, GAR-212)" -ForegroundColor White
Write-Host "2. P0: /health providers (GAR-213, GAR-214)" -ForegroundColor White
Write-Host "3. P0.5: voice E2E (GAR-220, GAR-221, GAR-222, GAR-223, GAR-224)" -ForegroundColor White
Write-Host "4. P1: OpenRouter 429 (GAR-216) + MCP security (GAR-230-234)" -ForegroundColor White
Write-Host "5. P2: tool escalation (GAR-240, GAR-241, GAR-242)" -ForegroundColor White
Write-Host "6. P3: docs (GAR-250, GAR-251, GAR-252, GAR-253)" -ForegroundColor White
Write-Host "7. P4: LM Studio (GAR-260, GAR-261, GAR-262)" -ForegroundColor White
