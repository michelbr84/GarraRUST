#!/usr/bin/env bash
# GAR-335/339: E2E tests for Garra Cloud Alpha mobile auth + chat endpoints.
#
# Usage:
#   BASE_URL=http://localhost:3888 bash tests/e2e_mobile_api.sh
#
# Requires: curl, jq

set -euo pipefail

BASE_URL="${BASE_URL:-http://localhost:3888}"
PASS=0
FAIL=0

ok()   { echo "  [PASS] $1"; ((PASS++)); }
fail() { echo "  [FAIL] $1"; ((FAIL++)); }

assert_status() {
    local label="$1" expected="$2" actual="$3"
    if [[ "$actual" == "$expected" ]]; then
        ok "$label (HTTP $actual)"
    else
        fail "$label — expected HTTP $expected, got $actual"
    fi
}

echo ""
echo "=== Garra Mobile API E2E ==="
echo "Target: $BASE_URL"
echo ""

# Unique email per run to avoid conflicts between runs
EMAIL="e2e_$(date +%s)@garraia.test"
PASSWORD="test-password-123"

# ── 1. Health check ──────────────────────────────────────────────────────────
echo "[1] Health check"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/health")
assert_status "GET /health" 200 "$STATUS"

# ── 2. Register ──────────────────────────────────────────────────────────────
echo "[2] Register new user"
REGISTER_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")
REGISTER_BODY=$(echo "$REGISTER_RESP" | head -1)
REGISTER_STATUS=$(echo "$REGISTER_RESP" | tail -1)
assert_status "POST /auth/register" 201 "$REGISTER_STATUS"
TOKEN=$(echo "$REGISTER_BODY" | jq -r '.token // empty')
USER_ID=$(echo "$REGISTER_BODY" | jq -r '.user_id // empty')
if [[ -n "$TOKEN" ]]; then
    ok "register: JWT token returned"
else
    fail "register: no JWT token in response"
fi

# ── 3. Register duplicate ────────────────────────────────────────────────────
echo "[3] Register duplicate email"
DUP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")
assert_status "POST /auth/register (duplicate)" 409 "$DUP_STATUS"

# ── 4. Login ─────────────────────────────────────────────────────────────────
echo "[4] Login"
LOGIN_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")
LOGIN_BODY=$(echo "$LOGIN_RESP" | head -1)
LOGIN_STATUS=$(echo "$LOGIN_RESP" | tail -1)
assert_status "POST /auth/login" 200 "$LOGIN_STATUS"
LOGIN_TOKEN=$(echo "$LOGIN_BODY" | jq -r '.token // empty')
if [[ -n "$LOGIN_TOKEN" ]]; then
    ok "login: JWT token returned"
    TOKEN="$LOGIN_TOKEN"  # Use login token going forward
else
    fail "login: no JWT token"
fi

# ── 5. Login with wrong password ─────────────────────────────────────────────
echo "[5] Login with wrong password"
BAD_LOGIN_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$EMAIL\",\"password\":\"wrong-password\"}")
assert_status "POST /auth/login (wrong password)" 401 "$BAD_LOGIN_STATUS"

# ── 6. GET /me ───────────────────────────────────────────────────────────────
echo "[6] GET /me"
ME_RESP=$(curl -s -w "\n%{http_code}" "$BASE_URL/me" \
    -H "Authorization: Bearer $TOKEN")
ME_BODY=$(echo "$ME_RESP" | head -1)
ME_STATUS=$(echo "$ME_RESP" | tail -1)
assert_status "GET /me" 200 "$ME_STATUS"
ME_EMAIL=$(echo "$ME_BODY" | jq -r '.email // empty')
if [[ "$ME_EMAIL" == "$EMAIL" ]]; then
    ok "/me: email matches"
else
    fail "/me: email mismatch (expected $EMAIL, got $ME_EMAIL)"
fi

# ── 7. GET /me without token ─────────────────────────────────────────────────
echo "[7] GET /me without token"
NO_TOKEN_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/me")
assert_status "GET /me (no token)" 401 "$NO_TOKEN_STATUS"

# ── 8. POST /chat ────────────────────────────────────────────────────────────
echo "[8] POST /chat"
CHAT_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/chat" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"message":"Olá! Quem é você?"}')
CHAT_BODY=$(echo "$CHAT_RESP" | head -1)
CHAT_STATUS=$(echo "$CHAT_RESP" | tail -1)
assert_status "POST /chat" 200 "$CHAT_STATUS"
REPLY=$(echo "$CHAT_BODY" | jq -r '.reply // empty')
if [[ -n "$REPLY" ]]; then
    ok "/chat: reply received (${#REPLY} chars)"
else
    fail "/chat: no reply in response"
fi

# ── 9. GET /chat/history ─────────────────────────────────────────────────────
echo "[9] GET /chat/history"
HIST_RESP=$(curl -s -w "\n%{http_code}" "$BASE_URL/chat/history" \
    -H "Authorization: Bearer $TOKEN")
HIST_BODY=$(echo "$HIST_RESP" | head -1)
HIST_STATUS=$(echo "$HIST_RESP" | tail -1)
assert_status "GET /chat/history" 200 "$HIST_STATUS"
MSG_COUNT=$(echo "$HIST_BODY" | jq '.messages | length')
if [[ "$MSG_COUNT" -ge 2 ]]; then
    ok "/chat/history: $MSG_COUNT messages found"
else
    fail "/chat/history: expected >= 2 messages, got $MSG_COUNT"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
