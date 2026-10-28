import json, subprocess, sys, pathlib
sys.stdout.reconfigure(encoding="utf-8")
ws = str(pathlib.Path(r"<scratch>/garra-qa/data/workspace").resolve())  # backslashes
print("allowed dir arg:", ws)
p = subprocess.Popen(["npx.cmd", "-y", "@modelcontextprotocol/server-filesystem@2026.8.31", ws], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
def rpc(obj):
    p.stdin.write(json.dumps(obj) + "\n"); p.stdin.flush()
    line = p.stdout.readline(); return json.loads(line) if line.strip() else None
rpc({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}})
p.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized"})+"\n"); p.stdin.flush()
r = rpc({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_allowed_directories","arguments":{}}})
print("server allowed:", json.dumps(r.get("result", r.get("error")), ensure_ascii=False)[:300])
variants = {"backslash": ws + "\notas.txt", "forward": ws.replace("\\", "/") + "/notas.txt", "verbatim": "\\?\\" + ws + "\notas.txt", "lower_drive": "c" + ws[1:] + "\notas.txt", "dir_backslash(list)": ws}
i = 2
for name, path in variants.items():
    i += 1
    tool = "list_directory" if "list" in name else "read_text_file"
    r = rpc({"jsonrpc":"2.0","id":i,"method":"tools/call","params":{"name":tool,"arguments":{"path":path}}})
    res = r.get("result", r.get("error")); txt = json.dumps(res, ensure_ascii=False)
    print(f"{name:20} isError={res.get('isError') if isinstance(res,dict) else '?'} -> {txt[:150]}")
p.kill()
