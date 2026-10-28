import json, sys, pathlib, statistics
sys.stdout.reconfigure(encoding="utf-8")
p = pathlib.Path(sys.argv[1])
rows = [json.loads(l) for l in p.read_text(encoding="utf-8").splitlines() if l.strip()]
from collections import Counter, defaultdict
tot = Counter(r["status"] for r in rows)
print(f"{p.name}: {len(rows)} cenarios -> {dict(tot)}")
by_level = defaultdict(Counter)
for r in rows: by_level[r["level"]][r["status"]] += 1
for lv in sorted(by_level): print(f"  nivel {lv}: {dict(by_level[lv])}")
lat = [r["latency_ms"] for r in rows if r.get("latency_ms") and r["http"] == 200 and r["latency_ms"] > 0]
if lat:
    lat.sort()
    print(f"  latencia (n={len(lat)}): mediana={statistics.median(lat):.0f}ms p90={lat[int(len(lat)*0.9)-1]}ms max={max(lat)}ms media={statistics.mean(lat):.0f}ms")
tools = Counter()
tool_err = 0
for r in rows:
    for call in (r.get("tools") or []):
        for t in call.split(", "): tools[t.strip()] += 1
    if r.get("notes") and "tool_errors=[" in r["notes"] and "tool_errors=[]" not in r["notes"]: tool_err += 1
print(f"  tools chamadas: {dict(tools.most_common(12))}; cenarios com erro de tool: {tool_err}")
print("  nao-PASS:")
for r in rows:
    if r["status"] != "PASS": print(f"    {r['id']:7} {r['status']:7} {r['category']:28} {str(r.get('notes'))[:90]}")
