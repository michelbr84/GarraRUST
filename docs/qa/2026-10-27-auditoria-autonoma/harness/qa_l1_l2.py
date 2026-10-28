import sys, re
sys.path.insert(0, r"<scratch>")
import qa_lib as q

def judge(r, must=None, must_not=None, maxlen=None, minlen=None):
    """Objective auto-check; returns (status, note). Manual review refines later."""
    if r.get("status") != 200: return "FAIL", f"http {r.get('status')}: {r.get('error') or r.get('body')}"
    c = r["body"].get("content", "") if isinstance(r["body"], dict) else str(r["body"])
    notes = []
    if not c.strip(): return "FAIL", "resposta vazia"
    low = c.lower()
    for m in (must or []):
        if not re.search(m, low): notes.append(f"faltou /{m}/")
    for m in (must_not or []):
        if re.search(m, low): notes.append(f"indevido /{m}/")
    if maxlen and len(c) > maxlen: notes.append(f"longa ({len(c)}>{maxlen})")
    if minlen and len(c) < minlen: notes.append(f"curta ({len(c)}<{minlen})")
    return ("PASS" if not notes else "PARTIAL"), "; ".join(notes)

# ---------- Nível 1 — comunicação básica ----------
sid, _ = q.new_session()
r = q.send(sid, "Olá! Quem é você?")
st, n = judge(r, must=[r"garra"]); q.record("L1-01","identidade",1,"Olá! Quem é você?","apresenta-se como Garra, pt-BR",r,st,notes=n,sid=sid)

r = q.send(sid, "Quanto é 2 + 2?")
st, n = judge(r, must=[r"\b4\b|quatro"]); q.record("L1-02","matematica-simples",1,"Quanto é 2 + 2?","contém 4",r,st,notes=n,sid=sid)

r = q.send(sid, "Explique o que você consegue fazer.")
st, n = judge(r, minlen=80); q.record("L1-03","capacidades",1,"Explique o que você consegue fazer.","lista capacidades reais (sem inventar integrações)",r,st,notes=n,sid=sid)

r = q.send(sid, "Responda apenas com a palavra OK.")
st, n = judge(r, must=[r"\bok\b"], maxlen=12); q.record("L1-04","instrucao-curta",1,"Responda apenas com a palavra OK.","exatamente 'OK' (curtíssimo)",r,st,notes=n,sid=sid)

r = q.send(sid, "Agora responda em inglês: qual é a capital da França?")
st, n = judge(r, must=[r"paris"], must_not=[r"\bcapital da\b|\bé\b"]); q.record("L1-05","troca-idioma-en",1,"Agora responda em inglês: qual é a capital da França?","responde em inglês com Paris",r,st,notes=n,sid=sid)

r = q.send(sid, "Volte a responder em português brasileiro. Qual é a capital do Brasil?")
st, n = judge(r, must=[r"bras[ií]lia"], must_not=[r"\bthe capital\b|\bis\b"]); q.record("L1-06","troca-idioma-pt",1,"Volte a responder em português brasileiro. Qual é a capital do Brasil?","responde em pt-BR com Brasília",r,st,notes=n,sid=sid)

r = q.send(sid, "Escreva um parágrafo de pelo menos 5 frases sobre o Rio de Janeiro.")
st, n = judge(r, minlen=250); q.record("L1-07","resposta-longa",1,"Escreva um parágrafo de pelo menos 5 frases sobre o Rio de Janeiro.","parágrafo longo coerente",r,st,notes=n,sid=sid)

# ---------- Nível 2 — contexto e raciocínio ----------
sid2, _ = q.new_session()
r = q.send(sid2, "Um trem sai de A às 8h a 60 km/h. Outro sai de B às 9h a 90 km/h em direção a A. A distância entre A e B é 300 km. A que hora eles se encontram? Mostre o cálculo.")
st, n = judge(r, must=[r"10h?\s*36|10:36|10h36"]); q.record("L2-01","matematica-logica",2,"trens A/B encontro","10h36 (às 9h faltam 240 km; 240/150 = 1,6h)",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Meu nome é Rafael e eu tenho um cachorro chamado Thor. Lembre-se disso.")
st, n = judge(r); q.record("L2-02","contexto-set",2,"Meu nome é Rafael e eu tenho um cachorro chamado Thor.","confirma",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Qual é o nome do meu cachorro e qual é o meu nome?")
st, n = judge(r, must=[r"thor", r"rafael"]); q.record("L2-03","contexto-recall",2,"Qual é o nome do meu cachorro e qual é o meu nome?","Thor e Rafael",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Na verdade eu errei: meu cachorro se chama Zeus, não Thor. Qual é o nome dele agora?")
st, n = judge(r, must=[r"zeus"]); q.record("L2-04","correcao-contexto",2,"correção Thor→Zeus","aceita correção: Zeus",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Leia o texto: 'A loja abre às 9h de segunda a sexta, aos sábados abre às 10h e fecha às 14h, e não abre aos domingos.' Se eu chegar às 9h30 num sábado, a loja estará aberta? E num domingo às 11h?")
st, n = judge(r, must=[r"n[ãa]o"]); q.record("L2-05","interpretacao-texto",2,"loja horários sábado 9h30 / domingo 11h","sábado 9h30: fechada (abre 10h); domingo: fechada",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Se todos os gatos são mamíferos e alguns mamíferos voam, então alguns gatos voam? Responda sim ou não e justifique em uma frase.")
st, n = judge(r, must=[r"n[ãa]o"]); q.record("L2-06","logica-silogismo",2,"silogismo gatos/mamíferos voam","Não — conclusão inválida",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Qual foi o placar exato do jogo de futebol entre Flamengo e Vasco realizado ontem?")
st, n = judge(r, must=[r"n[ãa]o (tenho|consigo|sei|posso)|sem acesso|não tenho acesso|não possuo|verificar|tempo real|atualizad"]); q.record("L2-07","incerteza",2,"placar Flamengo x Vasco ontem","reconhece que não sabe / sem acesso a tempo real, sem inventar placar",r,st,notes=n,sid=sid2)

r = q.send(sid2, "Resuma em uma frase tudo o que conversamos até agora nesta conversa.")
st, n = judge(r, must=[r"trem|rafael|zeus|loja|gato"]); q.record("L2-08","resumo-conversa",2,"resuma a conversa","menciona trens, Rafael/Zeus, loja, gatos",r,st,notes=n,sid=sid2)

print("HISTORY len:", len(q.history(sid2)["body"].get("messages", [])))
