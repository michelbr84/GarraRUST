# Runbook de Produção

Este documento cobre os procedimentos essenciais para colocar o GarraIA em produção:
expor o serviço com TLS, configurar canais (Telegram, etc.), rotacionar secrets e
monitorar o sistema.

---

## 1. Requisitos

| Item | Mínimo recomendado |
|------|--------------------|
| CPU  | 1 vCPU             |
| RAM  | 512 MB             |
| Disco| 2 GB               |
| OS   | Linux (Debian/Ubuntu) ou container Docker |
| Rust | 1.86+ (para builds locais) |

---

## 2. Expondo o serviço com TLS

O GarraIA escuta em HTTP (padrão porta 3888). Em produção, **nunca exponha HTTP puro
para a internet**. Use um dos métodos abaixo:

### 2.1 Cloudflare Tunnel (recomendado para VPS sem IP fixo)

Sem necessidade de abrir portas no firewall:

```bash
# 1. Instale o cloudflared
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 \
  -o /usr/local/bin/cloudflared && chmod +x /usr/local/bin/cloudflared

# 2. Autentique e crie o tunnel
cloudflared tunnel login
cloudflared tunnel create garraia

# 3. Configure o tunnel (~/.cloudflared/config.yml)
cat > ~/.cloudflared/config.yml << 'EOF'
tunnel: <TUNNEL_ID>
credentials-file: /root/.cloudflared/<TUNNEL_ID>.json

ingress:
  - hostname: garraia.seudominio.com
    service: http://localhost:3888
  - service: http_status:404
EOF

# 4. Aponte o DNS (CNAME) e inicie o tunnel
cloudflared tunnel route dns garraia garraia.seudominio.com
cloudflared tunnel run garraia

# Para rodar como serviço systemd:
cloudflared service install
systemctl start cloudflared
```

### 2.2 Nginx Reverse Proxy + Certbot

```nginx
# /etc/nginx/sites-available/garraia
server {
    listen 443 ssl;
    server_name garraia.seudominio.com;

    ssl_certificate     /etc/letsencrypt/live/garraia.seudominio.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/garraia.seudominio.com/privkey.pem;

    location / {
        proxy_pass         http://127.0.0.1:3888;
        proxy_http_version 1.1;
        proxy_set_header   Upgrade $http_upgrade;
        proxy_set_header   Connection "upgrade";
        proxy_set_header   Host $host;
        proxy_set_header   X-Real-IP $remote_addr;
        proxy_read_timeout 300s;   # necessário para SSE / streaming
    }
}
```

```bash
# Certificado gratuito via Let's Encrypt
certbot --nginx -d garraia.seudominio.com
```

### 2.3 Docker com Traefik (compose)

```yaml
# docker-compose.yml — adicione labels ao serviço garraia:
labels:
  - "traefik.enable=true"
  - "traefik.http.routers.garraia.rule=Host(`garraia.seudominio.com`)"
  - "traefik.http.routers.garraia.entrypoints=websecure"
  - "traefik.http.routers.garraia.tls.certresolver=letsencrypt"
```

---

## 3. Configurar Telegram em produção

### 3.1 Webhook (recomendado com domínio HTTPS)

O GarraIA usa **polling** por padrão (sem necessidade de webhook). Para ambientes com
muita carga, o webhook é mais eficiente:

```bash
# Configure TELEGRAM_BOT_TOKEN no .env
TELEGRAM_BOT_TOKEN=1234567890:ABCDEFGhijklmnop

# Registre o webhook no Telegram
curl -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/setWebhook" \
  -d "url=https://garraia.seudominio.com/webhooks/telegram"
```

> **Nota:** Certifique-se de que o endpoint `/webhooks/telegram` é acessível
> publicamente via HTTPS.

### 3.2 Polling (padrão, sem requisito de domínio público)

Nenhuma configuração adicional. O GarraIA inicia polling automaticamente quando
`TELEGRAM_BOT_TOKEN` está definido.

---

## 4. Rotação de Secrets

### 4.1 Chaves LLM (Anthropic, OpenAI, etc.)

```bash
# Via API admin (requer autenticação):
curl -X POST https://garraia.seudominio.com/admin/api/secrets/rotate \
  -H "Content-Type: application/json" \
  -H "X-CSRF-Token: <csrf>" \
  -b "admin_session=<cookie>" \
  -d '{"provider":"anthropic","key_name":"ANTHROPIC_API_KEY","new_value":"sk-ant-nova-chave"}'
```

Ou via painel admin: **Admin → Secrets → Rotate**.

### 4.2 Vault passphrase

O vault AES-256-GCM é derivado de `GARRAIA_VAULT_PASSPHRASE`. Para rotacionar:

```bash
# 1. Exporte todos os secrets atualmente
# 2. Pare o GarraIA
# 3. Delete ~/.garraia/credentials/vault.json
# 4. Defina a nova passphrase no .env
GARRAIA_VAULT_PASSPHRASE=nova-passphrase-segura

# 5. Reinicie — o vault será recriado vazio
# 6. Re-adicione os secrets via Admin → Secrets
```

> **Gere uma passphrase forte:**
> ```bash
> openssl rand -hex 32
> ```

### 4.3 Tokens de canal (Telegram, Discord, Slack)

1. Revogue o token no painel do provedor (BotFather, Discord Developer Portal, etc.)
2. Obtenha o novo token
3. Atualize no `.env`:

```bash
TELEGRAM_BOT_TOKEN=novo_token
```

4. Reinicie o GarraIA: `docker compose restart garraia` ou `systemctl restart garraia`

---

## 5. Variáveis de ambiente de produção

Crie um `.env` baseado em `.env.example`:

```bash
cp .env.example .env
# Edite .env com seus valores reais
```

Variáveis obrigatórias (pelo menos uma chave LLM):

```env
ANTHROPIC_API_KEY=sk-ant-...
GARRAIA_VAULT_PASSPHRASE=<openssl rand -hex 32>
GARRAIA_API_KEY=<bearer token para proteger a API>
```

Variáveis opcionais mas recomendadas em produção:

```env
GARRAIA_PORT=3888
GARRAIA_CONFIG_DIR=/etc/garraia    # em vez do default ~/.garraia
```

---

## 6. Iniciar com Docker Compose

```bash
# Build e start
docker compose up -d

# Verificar logs
docker compose logs -f garraia

# Verificar health
curl http://localhost:3888/health
# → ok

# Atualizar para nova versão
git pull
docker compose build
docker compose up -d
```

### Com Postgres (GAR-302 — quando disponível):

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
```

---

## 7. Monitoramento

### Health check

```bash
# HTTP simples
curl https://garraia.seudominio.com/health

# Status detalhado (inclui versão, providers, sessões ativas)
curl https://garraia.seudominio.com/api/status

# Métricas Prometheus
curl https://garraia.seudominio.com/metrics
```

### Logs

```bash
# Docker
docker compose logs -f --tail=100 garraia

# Systemd
journalctl -u garraia -f

# Nível de log (via variável de ambiente)
RUST_LOG=garraia=info,garraia_gateway=debug
```

### Alertas via Admin UI

Acesse `https://garraia.seudominio.com/admin` → **Alerts** para ver erros e avisos
do sistema em tempo real.

---

## 8. Backup e Recuperação

### Dados críticos a fazer backup

| Arquivo | Conteúdo |
|---------|----------|
| `~/.garraia/credentials/vault.json` | Secrets criptografados (AES-256-GCM) |
| `~/.garraia/data/garraia.db` | Sessões, histórico, memória (SQLite) |
| `~/.garraia/config.yml` | Configuração do sistema |
| `~/.garraia/mcp.json` | Configuração dos servidores MCP |
| `~/.garraia/mcp-templates.json` | Templates customizados de MCP |

### Script de backup

```bash
#!/usr/bin/env bash
set -euo pipefail
BACKUP_DIR="/backups/garraia/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$BACKUP_DIR"
cp -r ~/.garraia/credentials "$BACKUP_DIR/"
cp -r ~/.garraia/data        "$BACKUP_DIR/"
cp    ~/.garraia/config.yml  "$BACKUP_DIR/" 2>/dev/null || true
cp    ~/.garraia/mcp.json    "$BACKUP_DIR/" 2>/dev/null || true
echo "Backup salvo em $BACKUP_DIR"
```

---

## 9. Atualizações

```bash
# 1. Pull da nova versão
git pull origin main

# 2. Build
cargo build --release

# 3. Reiniciar o serviço
systemctl restart garraia
# ou
docker compose up -d --build garraia
```

> Verifique o [CHANGELOG](../../CHANGELOG.md) antes de atualizar para identificar
> breaking changes.
