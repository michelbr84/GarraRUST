"""Second round: re-run the scenarios that failed or were partial in round 1,
plus regression probes for each fix. Records to garra-qa/logs/scenarios_r2.jsonl."""
import sys, time, json, pathlib, re
BASE_DIR = r"<scratch>"
sys.path.insert(0, BASE_DIR)
import qa_lib as q
from qa_l1_l2_judge import judge
q.LOG = pathlib.Path(BASE_DIR) / "garra-qa" / "logs" / "scenarios_r2.jsonl"
WS = pathlib.Path(BASE_DIR) / "garra-qa" / "data" / "workspace"
LOG = pathlib.Path(BASE_DIR) / "garra-qa" / "logs" / "gateway-main-console.log"
def mark(): return len(LOG.read_text(encoding="utf-8", errors="replace"))
def tools_since(m):
    txt = LOG.read_text(encoding="utf-8", errors="replace")[m:]
    calls = [c.replace('"', '') for c in re.findall(r'agent calling tools: \[([^\]]*)\]', txt)]
    errs = re.findall(r"tool '([a-z_]+)' result: is_error=true", txt)
    return calls, errs
def content_of(r): return (r["body"] or {}).get("content", "") if isinstance(r["body"], dict) else ""
def rd(p): return p.read_text(encoding="utf-8", errors="replace") if p.exists() else ""

print("health:", q.get("/api/health")["body"])

# --- R2-A: /clear apaga o historico persistido (#1723) ---
sid, _ = q.new_session()
q.send(sid, "Minha fruta favorita e jabuticaba. Confirme em uma frase.")
r = q.send(sid, "/clear"); print("clear ->", r["status"], r["body"])
h = q.history(sid)["body"].get("messages", [])
r2 = q.send(sid, "Sem usar memoria de longo prazo, so pelo historico desta conversa: qual e a minha fruta favorita? Se nao souber, diga que nao sabe.")
c = content_of(r2).lower()
q.record("R2-A","clear-persistido",3,"/clear -> history -> pergunta",f"history vazio apos clear (len={len(h)}) e agente nao sabe",r2,"PASS" if (len(h)==0 and "jabuticaba" not in c) else "FAIL",notes=f"history_after_clear={len(h)}",sid=sid)

# --- R2-B: data atual no prompt (#1725) ---
sid, _ = q.new_session()
m = mark(); r = q.send(sid, "Sem usar nenhuma ferramenta: que dia e hoje (data completa) e que dia da semana e?")
calls, errs = tools_since(m)
hoje = time.strftime("%Y-%m-%d"); dia = time.strftime("%d")
c = content_of(r).lower()
ok = (hoje in c) or (f"{int(dia)} de outubro de 2026" in c) or ("28/10/2026" in c) or ("2026-10-28" in c)
q.record("R2-B","data-atual",2,"que dia e hoje?",f"data correta ({hoje}) sem ferramenta",r,"PASS" if ok else "FAIL",tools=calls,notes=f"tool_errors={errs}",sid=sid)

# --- R2-C: caminho relativo (#1724) + MCP filesystem (#1722) ---
sid, _ = q.new_session()
for f in ["resumo.txt", "resumo2.txt"]:
    p = WS / f
    if p.exists(): p.unlink()
m = mark(); r = q.send(sid, "Leia dados.csv e calcule o valor total do estoque (soma de qtd x preco de cada linha). Responda com o numero final.", timeout=300)
calls, errs = tools_since(m); st, n = judge(r, must=[r"\b89\b|89[,.]0"])
q.record("R2-C1","ler-csv-relativo",4,"leia dados.csv (relativo)","89,0 via tool sem recusa de jail",r,st if calls else "FAIL",tools=calls,notes=n+f" tool_errors={errs} ms={r['ms']}",sid=sid)

m = mark(); r = q.send(sid, "Crie um arquivo chamado resumo2.txt no workspace contendo exatamente a linha: TOTAL=89.0", timeout=300)
time.sleep(1); calls, errs = tools_since(m); p = WS / "resumo2.txt"
q.record("R2-C2","escrever-relativo",4,"crie resumo2.txt (relativo)",f"arquivo existe com o conteudo (exists={p.exists()}, content={rd(p).strip()!r}) no mesmo turno",r,"PASS" if (p.exists() and "TOTAL=89.0" in rd(p)) else "FAIL",tools=calls,notes=f"tool_errors={errs} ms={r['ms']}",sid=sid)

m = mark(); r = q.send(sid, "Usando as ferramentas do servidor MCP filesystem (filesystem__list_directory e filesystem__read_text_file), liste o diretorio permitido e leia notas.txt. Diga qual e a senha do cofre de testes.", timeout=300)
calls, errs = tools_since(m); st, n = judge(r, must=[r"abacaxi-42"])
mcp_used = any(c.startswith("filesystem__") for call in calls for c in call.split(", "))
mcp_err = [e for e in errs if e.startswith("filesystem")]
q.record("R2-C3","mcp-filesystem-windows",4,"use filesystem__list_directory/read_text_file","tools MCP com is_error=false e senha lida",r,"PASS" if (st=="PASS" and mcp_used and not mcp_err) else ("PARTIAL" if st=="PASS" else "FAIL"),tools=calls,notes=n+f" mcp_used={mcp_used} mcp_errors={mcp_err}",sid=sid)

# --- R2-D: erros 4xx do provider e content vazio (#1726) ---
sid, _ = q.new_session()
r = q.send(sid, "oi", model="openrouter/modelo-que-nao-existe-xyz")
c = json.dumps(r["body"], ensure_ascii=False)
q.record("R2-D1","modelo-invalido-400",6,"model inexistente","400 sem user_id",r,"PASS" if (r["status"]==400 and "user_" not in c) else "FAIL",sid=sid)
r = q.post("/v1/chat/completions", {"model":"garraia","messages":[{"role":"user","content":"PONG"}]}, timeout=120)
c = json.dumps(r["body"], ensure_ascii=False)
q.record("R2-D2","openai-compat-model-invalido-400",6,"/v1 model=garraia","400 com objeto de erro OpenAI, sem user_id",r,"PASS" if (r["status"]==400 and "invalid_request_error" in c and "user_" not in c) else "FAIL")
r = q.send(sid, "   ")
q.record("R2-D3","content-vazio-400",6,"content so espacos","400",r,"PASS" if r["status"]==400 else "FAIL",sid=sid)

# --- R2-E: regressao rapida dos basicos ---
sid, _ = q.new_session()
r = q.send(sid, "Quanto e 7 x 8? Responda so o numero."); st, n = judge(r, must=[r"\b56\b"])
q.record("R2-E1","basico-matematica",1,"7 x 8","56",r,st,notes=n,sid=sid)
r = q.send(sid, "Responda apenas com a palavra OK."); st, n = judge(r, must=[r"\bok\b"], maxlen=12)
q.record("R2-E2","basico-instrucao",1,"Responda apenas OK","OK",r,st,notes=n,sid=sid)
print("DONE")
