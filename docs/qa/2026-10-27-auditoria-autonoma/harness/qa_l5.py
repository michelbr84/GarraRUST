import sys, time, json, pathlib, re, shutil
sys.path.insert(0, r"<scratch>")
import qa_lib as q
from qa_l1_l2_judge import judge
WS = pathlib.Path(r"<scratch>/garra-qa/data/workspace")
LOG = pathlib.Path(r"<scratch>/garra-qa/logs/gateway-main-console.log")
def mark(): return len(LOG.read_text(encoding="utf-8", errors="replace"))
def tools_since(m):
    txt = LOG.read_text(encoding="utf-8", errors="replace")[m:]
    calls = [c.replace('"','') for c in re.findall(r'agent calling tools: \[([^\]]*)\]', txt)]
    errs = re.findall(r"tool '([a-z_]+)' result: is_error=true", txt)
    return calls, errs
def rd(p): return p.read_text(encoding="utf-8", errors="replace") if p.exists() else ""
WSP = str(WS).replace("\\", "/")
ABS = "" if len(sys.argv) < 2 or sys.argv[1] != "rel" else None  # 'rel' => deixa o modelo escolher caminhos
hint = f"Use caminhos absolutos dentro de {WSP}. " if ABS == "" else ""

sid, _ = q.new_session()
m = mark(); t = WS / "totais.csv"
if t.exists(): t.unlink()
r = q.send(sid, hint + "Tarefa em etapas: (1) leia dados.csv; (2) calcule o subtotal de cada produto (qtd x preco); (3) grave um arquivo totais.csv com cabecalho produto,subtotal e uma linha por produto; (4) leia totais.csv de volta e me mostre o conteudo final.", timeout=300)
calls, errs = tools_since(m); c = rd(t).replace(" ", "").lower()
ok = t.exists() and "maca,25" in c and "banana,24" in c and "uva,40" in c
q.record("L5-01","cadeia-ler-calcular-gravar-reler",5,"dados.csv -> totais.csv -> reler",f"totais.csv com maca,25 banana,24 uva,40 (exists={t.exists()})",r,"PASS" if ok else "FAIL",tools=calls,notes=f"tool_errors={errs} content={rd(t)!r}",sid=sid)

m = mark(); t2 = WS / "inexistente.txt"
if t2.exists(): t2.unlink()
r = q.send(sid, hint + "Leia o arquivo inexistente.txt do workspace. Se ele nao existir, crie-o com o texto exato 'criado pelo garra' e depois confirme lendo o conteudo.", timeout=300)
calls, errs = tools_since(m); ok = t2.exists() and "criado pelo garra" in rd(t2)
q.record("L5-02","recuperacao-de-erro",5,"ler inexistente.txt -> criar -> confirmar",f"arquivo criado com o texto (exists={t2.exists()})",r,"PASS" if ok else "FAIL",tools=calls,notes=f"tool_errors={errs}",sid=sid)

m = mark(); r = q.send(sid, hint + "Eu tenho certeza de que a reuniao e sexta as 16h. Verifique em notas.txt e me diga quem esta certo, citando o trecho do arquivo.", timeout=300)
calls, errs = tools_since(m); st, n = judge(r, must=[r"quinta", r"15"])
q.record("L5-03","informacao-conflitante",5,"usuario diz sexta 16h vs arquivo quinta 15h","le o arquivo e corrige: quinta 15h",r,st if calls else "FAIL",tools=calls,notes=n+f" tool_errors={errs}",sid=sid)

m = mark(); d = WS / "relatorios"; f = d / "2026-10.md"
if d.exists(): shutil.rmtree(d)
r = q.send(sid, hint + "Planeje e execute: crie a pasta relatorios e, dentro dela, o arquivo 2026-10.md com um titulo '# Estoque outubro' e uma lista dos produtos de dados.csv ordenados por preco decrescente (nome e preco). Ao final, explique o que fez em 2 frases.", timeout=300)
calls, errs = tools_since(m); content = rd(f)
order_ok = ("uva" in content and "maca" in content and "banana" in content) and (content.find("uva") < content.find("maca") < content.find("banana"))
q.record("L5-04","planejar-e-executar",5,"criar pasta relatorios/2026-10.md ordenado por preco",f"arquivo existe, titulo presente, ordem uva>maca>banana (exists={f.exists()}, order_ok={order_ok})",r,"PASS" if (f.exists() and "Estoque outubro" in content and order_ok) else ("PARTIAL" if f.exists() else "FAIL"),tools=calls,notes=f"tool_errors={errs} content={content[:200]!r}",sid=sid)

m = mark(); r = q.send(sid, "Sem usar ferramentas: liste os arquivos que voce criou nesta conversa e o que cada um contem.", timeout=300)
calls, errs = tools_since(m); st, n = judge(r, must=[r"totais\.csv", r"inexistente\.txt", r"2026-10\.md"])
q.record("L5-05","explicar-resultado",5,"liste o que voce criou nesta conversa","cita totais.csv, inexistente.txt, relatorios/2026-10.md",r,st,tools=calls,notes=n,sid=sid)
print("DONE", sid)
