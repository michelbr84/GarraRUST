#!/usr/bin/env bash
# Coleta bruta do benchmark de recall (#958).
#
# Semeia uma memoria **isolada** com o corpus do `dataset.json`, roda cada
# consulta pelo mesmo recall que o agente usa, e escreve o resultado cru em
# `results/<data>-<host>/raw.json`. Quem pontua e o `metrics.py`.
#
# A separacao e de proposito: coletar exige o binario, os modelos e a rede;
# pontuar exige um JSON. Assim da para repontuar uma coleta antiga com uma
# metrica nova, sem rodar os modelos de novo — e a coleta fica commitada como
# artefato, que e a regra herdada do `agent-framework-comparison`: numero sem
# artefato bruto nao conta como claim.
set -euo pipefail

AQUI="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RAIZ="$(cd "$AQUI/../.." && pwd)"
GARRA="${GARRA_BIN:-$RAIZ/target/release/garra}"
LIMITE="${LIMITE:-10}"

if [[ ! -x "$GARRA" ]]; then
  echo "erro: binario nao encontrado em $GARRA" >&2
  echo "      construa com: cargo build --release --bin garra" >&2
  echo "      ou aponte GARRA_BIN para outro caminho" >&2
  exit 1
fi

# Memoria propria, sempre.
#
# O `garra memory add` escreve no banco de memoria de quem chama. Sem isto o
# benchmark despejaria trinta frases inventadas na memoria real do operador —
# e depois as mediria junto com as dele, o que estragaria o numero e a memoria
# na mesma execucao.
#
# Trocar so o `HOME` NAO basta, e a primeira versao disto errava nisso. A
# cadeia de `ConfigLoader::default_config_dir()` (garraia-config/src/loader.rs)
# consulta, em ordem:
#
#   1. `GARRAIA_CONFIG_DIR` — retorna na hora, sem nunca olhar o `HOME`
#   2. `dirs::config_dir()` — que le `XDG_CONFIG_HOME` quando definido
#   3. `dirs::home_dir()` — so aqui o `HOME` entra
#
# Ou seja: num shell com `GARRAIA_CONFIG_DIR` ou `XDG_CONFIG_HOME` definido
# (config centralizada, CI), o `HOME` isolado nao valia nada e a semeadura ia
# para a memoria real. Definir a variavel de maior precedencia e mais forte que
# desdefinir as outras: ela e a primeira da cadeia e corta as demais.
SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT
export GARRAIA_CONFIG_DIR="$SANDBOX/.garraia"
export HOME="$SANDBOX"
mkdir -p "$GARRAIA_CONFIG_DIR"

CARIMBO="$(date -u +%Y-%m-%d)"
SAIDA="$AQUI/results/$CARIMBO-$(hostname -s 2>/dev/null || echo host)"
mkdir -p "$SAIDA"

echo "== semeando (HOME isolado: $SANDBOX) =="
python3 - "$AQUI/dataset.json" > "$SANDBOX/docs.tsv" <<'PY'
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))

# O grupo `ruido-puro` so significa alguma coisa se as consultas dele
# realmente nao tiverem resposta no corpus. A primeira versao do dataset
# perguntava `oi` — e o corpus tem `d25: "oi"` como documento, entao
# devolve-lo era acerto e a sonda punia o comportamento certo. Esta checagem
# existe para que esse erro nao volte em silencio numa edicao futura do JSON.
corpus = " ".join(doc["texto"].lower() for doc in d["documentos"])
for c in d["consultas"]:
    if c["grupo"] != "ruido-puro":
        continue
    if c["esperados"]:
        sys.exit(f"erro: consulta de ruido {c['q']!r} tem `esperados` — o grupo exige vazio")
    for palavra in c["q"].lower().split():
        if len(palavra) > 4 and palavra in corpus:
            sys.exit(
                f"erro: consulta de ruido {c['q']!r} usa {palavra!r}, que aparece no corpus."
                " o grupo `ruido-puro` so mede ruido se nao houver resposta certa."
            )

for doc in d["documentos"]:
    print(f"{doc['id']}\t{doc['texto']}")
PY

while IFS=$'\t' read -r id texto; do
  # O id do dataset entra na sessao para o casamento com o ground truth nao
  # depender do texto: comparar por texto quebraria com qualquer normalizacao
  # que o store venha a fazer.
  "$GARRA" memory add "$texto" --session "bench:$id" --json > /dev/null
done < "$SANDBOX/docs.tsv"

SEMEADOS="$(wc -l < "$SANDBOX/docs.tsv" | tr -d ' ')"
echo "   $SEMEADOS documentos"

echo "== consultando =="
python3 - "$AQUI/dataset.json" "$GARRA" "$LIMITE" "$SAIDA/raw.json" <<'PY'
import json, subprocess, sys

dataset_path, garra, limite, saida = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
d = json.load(open(dataset_path, encoding="utf-8"))

# `bench:<id>` -> `<id>`: o recall devolve a sessao, e e por ela que o
# resultado casa com o ground truth.
def doc_de(entrada):
    s = entrada.get("session_id") or ""
    return s[len("bench:"):] if s.startswith("bench:") else None

consultas, modo, modelo = [], None, None
for c in d["consultas"]:
    out = subprocess.run(
        [garra, "memory", "search", c["q"], "--limit", str(limite), "--json"],
        capture_output=True, text=True, check=True,
    ).stdout
    r = json.loads(out)
    resultados = r.get("results", r if isinstance(r, list) else [])
    if modo is None:
        # A chave e `semantic` (bool) — verificada contra a saida real do
        # `garra memory search --json`, e nao adivinhada. Rotular o artefato
        # com "desconhecido" seria pior que nao rotular: o numero de um recall
        # textual e de um semantico nao se comparam, e o arquivo tem de dizer
        # qual dos dois ele mediu.
        modo = "semantica" if r.get("semantic") else "textual"
    if modelo is None:
        for e in resultados:
            if e.get("embedding_model"):
                modelo = e["embedding_model"]
                break
    consultas.append({
        "q": c["q"],
        "grupo": c["grupo"],
        "esperados": c["esperados"],
        "obtidos": [x for x in (doc_de(e) for e in resultados) if x],
    })

json.dump({
    "dataset_versao": d["versao"],
    "documentos": len(d["documentos"]),
    "limite": limite,
    "modo_de_busca": modo,
    "modelo": modelo or "(nenhum: busca textual)",
    "provider": None,
    "consultas": consultas,
}, open(saida, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
print(f"   {len(consultas)} consultas -> {saida}")
PY

echo
# O `tee` foi trocado por escrita-e-confere de proposito.
#
# A primeira coleta desta bancada gravou um `report.txt` truncado — parou no
# meio da tabela, sem a linha `TODAS` — porque o disco estava a 98%. O `tee`
# escreveu o que coube e o terminal mostrou o relatorio inteiro, entao a falha
# so aparecia em quem abrisse o arquivo depois. Um artefato pela metade e pior
# que artefato nenhum: o README aponta para ele como evidencia.
python3 "$AQUI/metrics.py" "$SAIDA/raw.json" > "$SAIDA/report.txt"
if ! grep -q '^TODAS' "$SAIDA/report.txt"; then
  echo "erro: $SAIDA/report.txt saiu incompleto (sem a linha TODAS)." >&2
  echo "      disco cheio e a causa provavel: df -h ." >&2
  echo "      o raw.json esta salvo; repontue com:" >&2
  echo "      python3 $AQUI/metrics.py $SAIDA/raw.json" >&2
  exit 1
fi
cat "$SAIDA/report.txt"
echo
echo "artefatos em $SAIDA"
