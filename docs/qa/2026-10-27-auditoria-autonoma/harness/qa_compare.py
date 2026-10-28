import json, sys, pathlib
sys.stdout.reconfigure(encoding="utf-8")
def load(p):
    rows = {}
    for l in pathlib.Path(p).read_text(encoding="utf-8").splitlines():
        if not l.strip(): continue
        r = json.loads(l)
        if r["id"].startswith("L"): rows[r["id"]] = r  # last occurrence wins
    return rows
a = load(sys.argv[1]); b = load(sys.argv[2])
ids = sorted(set(a) | set(b), key=lambda s: (int(s[1]), s))
print("| ID | Antes | Depois | Lat. antes | Lat. depois | Nota |"); print("|---|---|---|---|---|---|")
changed = 0
for i in ids:
    ra, rb = a.get(i), b.get(i)
    sa = ra["status"] if ra else "—"; sb = rb["status"] if rb else "—"
    la = f"{ra['latency_ms']/1000:.1f} s" if ra and ra.get("latency_ms") else "—"
    lb = f"{rb['latency_ms']/1000:.1f} s" if rb and rb.get("latency_ms") else "—"
    mark = "" if sa == sb else " **mudou**"
    if sa != sb: changed += 1
    print(f"| {i} | {sa} | {sb} | {la} | {lb} |{mark} |")
from collections import Counter
print(); print("antes :", dict(Counter(r['status'] for r in a.values()))); print("depois:", dict(Counter(r['status'] for r in b.values()))); print("mudaram:", changed)
for i in ("L2-07","L3-04","L3-06","L4-04","L4-05","L6-01","L6-12","L6-14"):
    if i in b: print(f"\n[{i}] depois -> {b[i]['status']} :: {str(b[i]['observed'])[:260]!r}")
