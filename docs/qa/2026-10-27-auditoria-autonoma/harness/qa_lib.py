"""Tiny QA client for the GarraIA gateway (stdlib only)."""
import json, sys, time, urllib.request, urllib.error, pathlib, datetime, os
sys.stdout.reconfigure(encoding="utf-8")
BASE = "http://127.0.0.1:3899"
LOG = pathlib.Path(os.environ.get("QA_LOG") or (pathlib.Path(__file__).parent / "garra-qa" / "logs" / "scenarios.jsonl"))

def _req(method, path, body=None, headers=None, timeout=180):
    data = None if body is None else json.dumps(body).encode()
    h = {"Content-Type": "application/json"}
    if headers: h.update(headers)
    r = urllib.request.Request(BASE + path, data=data, method=method, headers=h)
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", "replace")
            status = resp.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", "replace"); status = e.code
    except Exception as e:  # timeout / connection
        return {"status": None, "error": repr(e), "ms": int((time.perf_counter()-t0)*1000), "body": None}
    ms = int((time.perf_counter()-t0)*1000)
    try: body = json.loads(raw)
    except Exception: body = raw
    return {"status": status, "ms": ms, "body": body}

def new_session(mode=None):
    body = {}
    if mode: body["mode"] = mode
    r = _req("POST", "/api/sessions", body)
    sid = None
    if isinstance(r["body"], dict):
        sid = r["body"].get("session_id") or r["body"].get("id")
    return sid, r

def send(sid, content, model=None, agent_id=None, timeout=180):
    body = {"content": content}
    if model: body["model"] = model
    if agent_id: body["agent_id"] = agent_id
    return _req("POST", f"/api/sessions/{sid}/messages", body, timeout=timeout)

def history(sid):
    return _req("GET", f"/api/sessions/{sid}/history")

def get(path, **kw):
    return _req("GET", path, **kw)

def post(path, body=None, **kw):
    return _req("POST", path, body, **kw)

def record(scenario_id, category, level, inp, expected, r, status, severity=None, tools=None, notes=None, sid=None):
    content = None
    if isinstance(r.get("body"), dict):
        content = r["body"].get("content") or r["body"].get("error") or r["body"]
    else:
        content = r.get("body") or r.get("error")
    rec = {
        "id": scenario_id, "category": category, "level": level, "input": inp,
        "expected": expected, "observed": content, "http": r.get("status"), "latency_ms": r.get("ms"),
        "tools": tools, "status": status, "severity": severity, "notes": notes, "session": sid,
        "ts": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds").replace("+00:00","Z"),
    }
    LOG.parent.mkdir(parents=True, exist_ok=True)
    with LOG.open("a", encoding="utf-8") as f: f.write(json.dumps(rec, ensure_ascii=False) + "\n")
    obs = content if isinstance(content, str) else json.dumps(content, ensure_ascii=False)
    print(f"[{scenario_id}] {status} http={r.get('status')} {r.get('ms')}ms\n  >> {inp[:120]!r}\n  << {obs[:600]!r}\n")
    return rec
