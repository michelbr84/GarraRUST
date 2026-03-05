param([string]$ApiKey = $env:LINEAR_API_KEY)

if ([string]::IsNullOrEmpty($ApiKey)) { Write-Host "LINEAR_API_KEY nao definida" -ForegroundColor Red; exit 1 }

$headers = @{ "Authorization" = $ApiKey; "Content-Type" = "application/json" }
$url = "https://api.linear.app/graphql"
$backlogStateId = "01cc9b83-b8bd-4427-a7db-3468d6f377f6"

function Invoke-Linear([string]$query) {
    $bodyObj = @{ query = $query }
    $bodyJson = $bodyObj | ConvertTo-Json -Depth 10 -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
    try {
        $resp = Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $bodyBytes -ContentType "application/json; charset=utf-8"
        return $resp
    } catch {
        Write-Host "HTTP error: $_" -ForegroundColor Red
        return $null
    }
}

$teamResp = Invoke-Linear "query { teams { nodes { id name key } } }"
$team = $teamResp.data.teams.nodes | Where-Object { $_.key -eq "GAR" }
if (-not $team) { Write-Host "Equipe GAR nao encontrada!" -ForegroundColor Red; exit 1 }
$teamId = $team.id
Write-Host "Team GAR: $teamId" -ForegroundColor Green

function Escape-GQL([string]$s) {
    return $s -replace '\\', '\\\\' -replace '"', '\"' -replace "`r`n", '\n' -replace "`n", '\n' -replace "`r", '\n'
}

function New-Issue([string]$title, [string]$desc, [int]$priority, [string]$parentId) {
    $t = Escape-GQL $title
    $d = Escape-GQL $desc
    $parentClause = if ($parentId) { "parentId: `"$parentId`"" } else { "" }
    $mutation = "mutation { issueCreate(input: { teamId: `"$teamId`" stateId: `"$backlogStateId`" priority: $priority title: `"$t`" description: `"$d`" $parentClause }) { issue { id identifier title } } }"
    $resp = Invoke-Linear $mutation
    if (-not $resp) { Write-Host "  FALHOU (sem resposta): $title" -ForegroundColor Red; return $null }
    if ($resp.errors) { Write-Host "  FALHOU ($($resp.errors[0].message)): $title" -ForegroundColor Red; return $null }
    $issue = $resp.data.issueCreate.issue
    if ($issue) { Write-Host "  [$($issue.identifier)] $title" -ForegroundColor Green }
    else { Write-Host "  FALHOU (sem issue): $title" -ForegroundColor Red }
    Start-Sleep -Milliseconds 400
    return $issue
}

# EPIC
Write-Host "`n== EPIC ==" -ForegroundColor Magenta
$epic = New-Issue "EPIC: Garra Desktop - Assistente Clippy-style com Tauri v2" "Implementar o assistente de desktop animado Garra (papagaio Clippy-style) usando Tauri v2. Overlay transparente sempre no topo, animado via WebSocket com GarraIA, entrada via hotkey Alt+G. Stack: Tauri v2 (Rust + WebView), sprite sheet, endpoint /ws/parrot." 2 $null
$epicId = if ($epic) { $epic.id } else { $null }

# Fase 1
Write-Host "`n== Fase 1: Overlay Inicial ==" -ForegroundColor Yellow
$f1 = New-Issue "GAR-Desktop Fase 1: Overlay Inicial" "Criar estrutura base Tauri v2 com janela transparente sempre-no-topo, sprite estatico e hotkey global Alt+G." 2 $epicId
$f1Id = if ($f1) { $f1.id } else { $null }

New-Issue "Criar app Tauri v2 (garraia-desktop)" "Executar cargo create-tauri-app garraia-desktop e integrar ao workspace Cargo.toml do GarraRUST. Verificar dependencias Tauri v2." 2 $f1Id | Out-Null
New-Issue "Configurar janela transparente sem decoracao" "No tauri.conf.json: decorations=false, transparent=true, always_on_top=true, skip_taskbar=true. Posicao canto inferior direito, tamanho 200x300." 2 $f1Id | Out-Null
New-Issue "Sprite estatico do papagaio (parrot.html e parrot.css)" "Criar ui/parrot.html com div#parrot usando CSS background-image para sprite placeholder. Fundo completamente transparente." 3 $f1Id | Out-Null
New-Issue "Hotkey global Alt+G com tauri-plugin-global-shortcut" "Integrar tauri-plugin-global-shortcut v2. Registrar Alt+G para alternar visibilidade. Handler em Rust chama window.show() ou window.hide()." 2 $f1Id | Out-Null
New-Issue "Click-through em areas transparentes do overlay" "Implementar set_ignore_cursor_events(true) para clicks no vazio passarem para janelas abaixo. Drag funciona apenas sobre a sprite via JS mousedown com startDragging()." 2 $f1Id | Out-Null
New-Issue "Criar overlay.rs com create_overlay e toggle_overlay" "Criar src-tauri/src/overlay.rs encapsulando criacao da janela e funcao toggle_overlay para uso no hotkey handler." 3 $f1Id | Out-Null

# Fase 2
Write-Host "`n== Fase 2: Animacao e Chat UI ==" -ForegroundColor Yellow
$f2 = New-Issue "GAR-Desktop Fase 2: Animacao e Interface de Chat" "Sprite sheet animado com estados idle/thinking/talking, bolha de fala e campo de entrada de texto no overlay." 2 $epicId
$f2Id = if ($f2) { $f2.id } else { $null }

New-Issue "Sprite sheet animado: idle, thinking e talking" "Sprite sheet 3 linhas: idle 4 frames 120ms, thinking 6 frames 100ms, talking 8 frames 90ms. Loop em parrot.js com setInterval trocando background-position." 2 $f2Id | Out-Null
New-Issue "Bolha de fala (speech bubble)" "Elemento HTML/CSS acima do papagaio. Texto truncado em 200 chars, auto-hide 8s, fade-in/out, scroll se longo." 2 $f2Id | Out-Null
New-Issue "Funcoes setState idle, thinking e talking" "Em parrot.js: setState(s) reinicia frame=0. Expor window.__garra = {setState, showBubble} para o WebSocket handler." 3 $f2Id | Out-Null
New-Issue "Campo de entrada de texto no overlay" "Input#query oculto que aparece ao Alt+G. Enter envia ao GarraIA e fecha. Escape cancela. Barra flutuante semitransparente." 2 $f2Id | Out-Null
New-Issue "Testes visuais em diferentes resolucoes e DPI" "Verificar 1080p 100% DPI, 1440p 125%, 4K 150%. Ajustar com logical_size vs physical_size no Tauri. Testar Windows 11." 3 $f2Id | Out-Null

# Fase 3
Write-Host "`n== Fase 3: Integracao GarraIA ==" -ForegroundColor Yellow
$f3 = New-Issue "GAR-Desktop Fase 3: Integracao com GarraIA via WebSocket" "Endpoint WS /ws/parrot no garraia-gateway, conexao bidirecional: usuario digita, GarraIA processa, animacao reage." 2 $epicId
$f3Id = if ($f3) { $f3.id } else { $null }

New-Issue "Endpoint WebSocket /ws/parrot no garraia-gateway" "Handler WS GET /ws/parrot. Protocolo: cliente envia {type:message,text,session_id}, server envia {type:thinking}, {type:response,text} ou {type:error,message}." 2 $f3Id | Out-Null
New-Issue "Fluxo completo: input para GarraIA para resposta animada" "Ao submeter: setState(thinking), envia JSON WS, ao receber response: setState(talking)+showBubble(text), apos 5s: setState(idle)." 2 $f3Id | Out-Null
New-Issue "Streaming de resposta no overlay" "Adaptar protocolo WS para chunks de texto progressivos na bolha enquanto estado talking persiste, se gateway suportar streaming." 3 $f3Id | Out-Null
New-Issue "Reconexao automatica com backoff exponencial" "No parrot.js: reconexao 1s, 2s, 4s ate 30s. Bolha offline ao server desligado. Timeout 30s sem resposta com reset idle." 2 $f3Id | Out-Null
New-Issue "garraia.rs - cliente HTTP/WS em Rust no desktop" "Criar src-tauri/src/garraia.rs com GarraiaClient e send_message. Alternativa via tauri::command expondo ao frontend por invoke()." 3 $f3Id | Out-Null
New-Issue "Testes de integracao: hotkey para animacao para resposta" "E2E manual: Alt+G abre input, pergunta muda para thinking, resposta chega com talking e bolha, retorna para idle. Documentar no PR." 3 $f3Id | Out-Null

# Fase 4
Write-Host "`n== Fase 4: Finalizacao ==" -ForegroundColor Yellow
$f4 = New-Issue "GAR-Desktop Fase 4: Finalizacao e Distribuicao" "Polimento UI, build cross-platform MSI/DMG/AppImage, autostart opcional, system tray e documentacao." 3 $epicId
$f4Id = if ($f4) { $f4.id } else { $null }

New-Issue "Posicionamento adaptativo por resolucao de tela" "Usar available_monitors() para calcular canto inferior direito independente da resolucao. Slide-in ao aparecer." 3 $f4Id | Out-Null
New-Issue "Build cross-platform: MSI, DMG e AppImage" "Configurar tauri.conf.json bundle. Windows MSI via WiX, macOS DMG com signing, Linux AppImage. Job build-desktop no CI." 3 $f4Id | Out-Null
New-Issue "System tray icon com menu de contexto" "Icone na bandeja com menu: Mostrar/Ocultar, Iniciar com Windows (toggle), Sair. Usar tauri-plugin-tray-icon." 3 $f4Id | Out-Null
New-Issue "Autostart opcional com tauri-plugin-autostart" "Integrar tauri-plugin-autostart. Toggle no tray menu. Persistir preferencia em ~/.garraia/desktop.json." 4 $f4Id | Out-Null
New-Issue "Documentacao README e guia de instalacao do Garra Desktop" "Criar crates/garraia-desktop/README.md: pre-requisitos, build, hotkey, configuracao de servidor remoto. Link em docs/src/SUMMARY.md." 4 $f4Id | Out-Null

Write-Host "`n=== Roadmap criado! ===" -ForegroundColor Green
if ($epic) { Write-Host "EPIC: $($epic.identifier)" -ForegroundColor Cyan }
