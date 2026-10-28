import sys, time, json
sys.path.insert(0, r"<scratch>")
import qa_lib as q
from qa_l1_l2_judge import judge

def mem_list():
    r = q.get("/api/memory/recent?limit=200"); return r

# baseline memory
m0 = mem_list(); print("MEMORY BEFORE:", m0["status"], json.dumps(m0["body"], ensure_ascii=False)[:400])

# L3-01 set facts in session A
sidA, _ = q.new_session()
r = q.send(sidA, "Guarde na memória: minha cor favorita é verde-musgo, eu moro em Curitiba e meu time é o Coritiba. Lembre-se disso para sempre, mesmo em outras conversas.")
st, n = judge(r); q.record("L3-01","memoria-set",3,"cor verde-musgo / Curitiba / Coritiba","confirma e persiste",r,st,notes=n,sid=sidA)

# L3-02 same session recall
r = q.send(sidA, "Qual é a minha cor favorita?")
st, n = judge(r, must=[r"verde"]); q.record("L3-02","memoria-recall-intra",3,"Qual é a minha cor favorita?","verde-musgo",r,st,notes=n,sid=sidA)

# give async extraction a moment, then inspect storage
time.sleep(8)
m1 = mem_list(); print("MEMORY AFTER:", m1["status"], json.dumps(m1["body"], ensure_ascii=False)[:1500])
stored = json.dumps(m1["body"], ensure_ascii=False).lower()
st = "PASS" if ("verde" in stored or "curitiba" in stored) else "FAIL"
q.record("L3-03","memoria-storage-real",3,"GET /api/memory após o turno","fatos extraídos presentes no memory.db (verde-musgo/Curitiba)",m1,st,notes="verificação do armazenamento real, não do que o modelo afirma",sid=sidA)

# L3-04 cross-session recall
sidB, _ = q.new_session()
r = q.send(sidB, "Oi! Você lembra em que cidade eu moro e qual é o meu time?")
st, n = judge(r, must=[r"curitiba", r"coritiba"]); q.record("L3-04","memoria-cross-session",3,"nova sessão: cidade e time","Curitiba e Coritiba via memória semântica",r,st,notes=n,sid=sidB)

# L3-05 isolation of histories
hA = q.history(sidA)["body"].get("messages", []); hB = q.history(sidB)["body"].get("messages", [])
leak = any("verde-musgo" in (m.get("content","")) for m in hB if m.get("role")=="user")
q.record("L3-05","isolamento-historico",3,"GET history A vs B",f"B não contém turnos de A (A={len(hA)} msgs, B={len(hB)} msgs)",{"status":200,"ms":0,"body":{"content":f"A={len(hA)} B={len(hB)} leak_user_turns={leak}"}},"FAIL" if leak else "PASS",sid=sidB)

# L3-06 /clear slash command then recall within session
r = q.send(sidA, "/clear")
print("CLEAR:", r["status"], r["body"])
r2 = q.send(sidA, "Sem usar a memória de longo prazo, apenas pelo histórico desta conversa: o que eu te disse antes nesta sessão?")
st, n = judge(r2); hA2 = q.history(sidA)["body"].get("messages", [])
q.record("L3-06","clear-sessao",3,"/clear + pergunta sobre histórico",f"histórico zerado (antes={len(hA)}, depois={len(hA2)}); resposta não cita turnos antigos",r2,"PASS" if len(hA2) <= 2 else "FAIL",notes=f"history_after_clear={len(hA2)}",sid=sidA)

# L3-07 explicit memory clear
rc = q._req("DELETE", f"/api/memory?session_id={sidA}")
print("MEMORY CLEAR:", rc["status"], rc["body"])
m2 = mem_list(); stored2 = json.dumps([m for m in (m2["body"].get("memories", []) if isinstance(m2["body"], dict) else []) if m.get("session_id") == sidA], ensure_ascii=False).lower()
sidC, _ = q.new_session()
r = q.send(sidC, "Você sabe em que cidade eu moro? Se não souber, diga que não sabe.")
st, n = judge(r, must_not=[r"curitiba"])
q.record("L3-07","memoria-clear",3,"DELETE /api/memory?session_id=A + nova sessão","memória vazia e agente não sabe a cidade",r,"PASS" if ("curitiba" not in stored2 and st=="PASS") else "FAIL",notes=f"clear_http={rc['status']} mem_after={stored2[:120]}",sid=sidC)
print("DONE sessions:", sidA, sidB, sidC)
