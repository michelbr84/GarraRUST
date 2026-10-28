import re
def judge(r, must=None, must_not=None, maxlen=None, minlen=None):
    if r.get("status") != 200: return "FAIL", f"http {r.get('status')}: {r.get('error') or r.get('body')}"
    c = r["body"].get("content", "") if isinstance(r["body"], dict) else str(r["body"])
    notes = []
    if not str(c).strip(): return "FAIL", "resposta vazia"
    low = str(c).lower()
    for m in (must or []):
        if not re.search(m, low): notes.append(f"faltou /{m}/")
    for m in (must_not or []):
        if re.search(m, low): notes.append(f"indevido /{m}/")
    if maxlen and len(c) > maxlen: notes.append(f"longa ({len(c)}>{maxlen})")
    if minlen and len(c) < minlen: notes.append(f"curta ({len(c)}<{minlen})")
    return ("PASS" if not notes else "PARTIAL"), "; ".join(notes)
