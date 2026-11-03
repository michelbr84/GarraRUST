#!/usr/bin/env bash
# Smoke REAL dos AppImages Linux que vao ser publicados (#1818).
#
# Executa os arquivos como um usuario executaria — nada de mock: o AppImage da
# CLI e o do Desktop, numa HOME vazia, sem Node no PATH. O que nao da para
# provar aqui (pareamento do WhatsApp com celular, microfone) continua no
# roteiro manual de dogfood e NAO e simulado.
#
#   smoke-appimages-linux.sh <cli.AppImage|-> <versao> [desktop.AppImage]
#
# <versao> e a do Cargo (ex.: 0.5.2), a que os binarios imprimem. Sem o
# AppImage do desktop (build best-effort), so a CLI e provada; com `-` no
# lugar da CLI (o job de PR do desktop, que nao monta a CLI), so o desktop.
#
# Requer: Xvfb e dbus-launch (so com desktop), curl, python3, timeout.
# Os AppImages rodam em modo extract-and-run (o runner nao tem FUSE); o
# conteudo executado e o mesmo.
set -uo pipefail

cli="${1:?uso: $0 <cli.AppImage> <versao> [desktop.AppImage]}"
versao="${2:?versao esperada}"
desk="${3:-}"

falhas=0
ok() { echo "OK    $*"; }
ko() {
  echo "FALHA $*"
  falhas=$((falhas + 1))
}

export APPIMAGE_EXTRACT_AND_RUN=1
work="$(mktemp -d)"
export HOME="$work/home"
export XDG_CONFIG_HOME="$HOME/.config"
mkdir -p "$HOME"
# Sem Node no PATH: o Desktop tem de usar o Node privado dele.
PATH_SEM_NODE="/usr/sbin:/usr/bin:/sbin:/bin"
[ "$cli" != "-" ] && chmod +x "$cli"
[ -n "$desk" ] && chmod +x "$desk"

if [ "$cli" = "-" ]; then
  echo "== CLI: pulada (-)"
else
echo "== CLI: $cli"
v="$(env PATH="$PATH_SEM_NODE" timeout 60 "$cli" --version 2>&1 | tail -1)"
[ "$v" = "garra $versao" ] && ok "--version: $v" || ko "--version: '$v' (esperado 'garra $versao')"

# Sem argumentos, sem TTY, sem sessao grafica: o comportamento de sempre
# (chat le fim de arquivo e sai) — e tem de SAIR, nunca pendurar.
(
  unset DISPLAY WAYLAND_DISPLAY
  env PATH="$PATH_SEM_NODE" timeout 60 "$cli" </dev/null >"$work/cli-sem-args.out" 2>&1
)
rc=$?
[ "$rc" -eq 0 ] && ok "sem argumentos e sem TTY sai sozinho (exit 0)" || ko "sem argumentos e sem TTY: exit $rc"

# Duplo clique (#1817): sem TTY, COM sessao grafica. O AppRun abre um
# terminal com o proprio AppImage. Um terminal de mentira grava o que recebeu.
mkdir -p "$work/fakebin"
cat >"$work/fakebin/x-terminal-emulator" <<'EOS'
#!/bin/sh
printf '%s\n' "$@" >"$TERMINAL_LOG"
EOS
chmod +x "$work/fakebin/x-terminal-emulator"
env DISPLAY=:0 TERMINAL_LOG="$work/terminal.log" PATH="$work/fakebin:$PATH_SEM_NODE" \
  timeout 60 "$cli" </dev/null >/dev/null 2>&1
if grep -qF -- "-e" "$work/terminal.log" 2>/dev/null && grep -qF "$(basename "$cli")" "$work/terminal.log"; then
  ok "duplo clique abre um terminal com o AppImage"
else
  ko "duplo clique nao abriu terminal: $(cat "$work/terminal.log" 2>/dev/null || echo '<nada>')"
fi
# Com argumento, o AppRun repassa (nada de terminal).
rm -f "$work/terminal.log"
v2="$(env DISPLAY=:0 TERMINAL_LOG="$work/terminal.log" PATH="$work/fakebin:$PATH_SEM_NODE" \
  timeout 60 "$cli" --version </dev/null 2>&1 | tail -1)"
if [ "$v2" = "garra $versao" ] && [ ! -e "$work/terminal.log" ]; then
  ok "com argumento o AppRun repassa direto"
else
  ko "com argumento: '$v2', terminal aberto: $([ -e "$work/terminal.log" ] && echo sim || echo nao)"
fi
fi

if [ -z "$desk" ]; then
  echo "== Desktop: sem AppImage (build best-effort) — so a CLI foi provada"
else
  echo "== Desktop: $desk"
  if curl -fsS -m 2 http://127.0.0.1:3888/api/health >/dev/null 2>&1; then
    ko "a porta 3888 ja estava ocupada antes do teste — o smoke precisa dela livre"
  fi
  Xvfb :77 -screen 0 1280x800x24 >"$work/xvfb.log" 2>&1 &
  xvfb=$!
  sleep 1
  export DISPLAY=:77
  eval "$(dbus-launch --sh-syntax)"
  env PATH="$PATH_SEM_NODE" "$desk" >"$work/desk.out" 2>"$work/desk.err" &
  app=$!

  saude=""
  for _ in $(seq 1 90); do
    saude="$(curl -fsS -m 2 http://127.0.0.1:3888/api/health 2>/dev/null || true)"
    [ -n "$saude" ] && break
    sleep 1
  done
  vgw="$(printf '%s' "$saude" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("version",""))' 2>/dev/null)"
  [ "$vgw" = "$versao" ] && ok "o sidecar do app respondeu /api/health (versao $vgw)" \
    || ko "sidecar sem /api/health em 90 s (versao '$vgw'); stderr: $(tail -3 "$work/desk.err" | tr '\n' ' ')"

  side="$(pgrep -af 'usr/bin/garraia start --exit-with-parent' | head -1)"
  case "$side" in
    *appimage_extracted*) ok "o gateway e o sidecar de dentro do AppImage" ;;
    *) ko "processo do sidecar inesperado: '${side:-nenhum}'" ;;
  esac

  bin="$(printf '%s' "$side" | awk '{print $2}')"
  if [ -x "$bin" ]; then
    st="$(env -i HOME="$HOME" PATH="$PATH_SEM_NODE" "$bin" whatsapp self-test --json 2>/dev/null | tail -1)"
    printf '%s' "$st" | python3 -c '
import sys, json
d = json.load(sys.stdin)
assert d.get("self_test") == "ok", d
assert d.get("node_origin") == "privado", d
assert "@whiskeysockets/baileys" in d.get("deps", {}), d
' && ok "a ponte do WhatsApp carrega com o Node privado ($(printf '%s' "$st" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("node_version"))'))" \
      || ko "self-test da ponte: $st"
  else
    ko "nao achei o binario do sidecar para o self-test"
  fi

  marcador="$XDG_CONFIG_HOME/org.garraia.desktop/onboarding-shown.marker"
  for _ in $(seq 1 60); do
    [ -f "$marcador" ] && break
    sleep 1
  done
  [ -f "$marcador" ] && ok "a janela do assistente abriu e o JS dela rodou (marcador gravado)" \
    || ko "o assistente nao abriu em 60 s (sem $marcador)"

  # Fechar o app derruba o sidecar (nada de gateway orfao segurando a porta).
  pkill -TERM -x garraia-desktop 2>/dev/null || kill "$app" 2>/dev/null
  morto=0
  for _ in $(seq 1 30); do
    if ! pgrep -f 'usr/bin/garraia start --exit-with-parent' >/dev/null; then
      morto=1
      break
    fi
    sleep 1
  done
  [ "$morto" -eq 1 ] && ok "fechar o app derruba o sidecar" || ko "o sidecar sobreviveu ao app"
  kill "$xvfb" 2>/dev/null || true
  [ -n "${DBUS_SESSION_BUS_PID:-}" ] && kill "$DBUS_SESSION_BUS_PID" 2>/dev/null || true
fi

rm -rf "$work"
echo "== resumo: $falhas falha(s)"
[ "$falhas" -eq 0 ]
