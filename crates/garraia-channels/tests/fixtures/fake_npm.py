#!/usr/bin/env python3
"""`npm` falso. Por padrao falha cuspindo material de credencial no stderr.

Existe para dois fins:

1. Provar que `npm_ci` passa a cauda do stderr pela redacao ANTES de
   mostra-la. Sem ele, a redacao era testada so como funcao pura e
   neutralizar o call site (`.map(redact_tail_line)` -> `.map(to_string)`)
   deixava a suite inteira verde. E o modo padrao, e ele nao toca o
   `node_modules`: e o `npm ci` que recusa logo de cara (lockfile fora de
   sincronia, registry fora do ar) e deixa a arvore ANTIGA onde estava.
2. Provar a reinstalacao do boot do gateway (W1 da v0.4.5): quem prepara a
   ponte roda o `npm` so quando os manifestos mudaram, e nunca sobe a ponte
   contra um `node_modules` que nao e dos manifestos atuais.

Ignora os argumentos de proposito: quem o chama e o `npm_ci` de verdade, com
`ci --no-fund --no-audit --progress=false`. O modo vem de um arquivo no
diretorio de trabalho (o diretorio da ponte), e nao de variavel de ambiente:
o `npm_ci` limpa o ambiente do filho (`env_clear` + allowlist), entao uma
variavel nunca chegaria aqui.

- Toda chamada acrescenta uma linha a `.fake-npm-calls` no diretorio de
  trabalho — e assim que o teste conta quantas vezes o `npm` rodou.
- Com `.fake-npm-ok` no diretorio de trabalho, imita o `npm ci` que da
  certo: apaga o `node_modules` e o recria, com uma copia do
  `package-lock.json` em `node_modules/.package-lock.json` (o "lockfile
  escondido" que o npm 7+ grava ali) e sai 0.
"""

import os
import shutil
import sys

cwd = os.getcwd()

with open(os.path.join(cwd, ".fake-npm-calls"), "a", encoding="utf-8") as calls:
    calls.write(" ".join(sys.argv[1:]) + "\n")

if os.path.exists(os.path.join(cwd, ".fake-npm-ok")):
    modules = os.path.join(cwd, "node_modules")
    shutil.rmtree(modules, ignore_errors=True)
    os.makedirs(os.path.join(modules, "@whiskeysockets", "baileys"))
    lock = os.path.join(cwd, "package-lock.json")
    if os.path.exists(lock):
        shutil.copyfile(lock, os.path.join(modules, ".package-lock.json"))
    sys.exit(0)

# Base64 padrao com a forma de uma chave do Baileys, com a barra que a regra
# anterior usava para quebrar a sequencia em pedacos curtos. O literal tem 45
# caracteres e decodifica 33 bytes -- e uma imitacao, nao uma chave de 32 B de
# verdade, e para o que se mede aqui (a sequencia longa chega a tela?) o que
# importa e so passar dos 40 caracteres de BASE64_RUN_MIN.
SECRET_B64 = "c2VjcmV0/Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0="

sys.stderr.write("npm ERR! code ERESOLVE\n")
sys.stderr.write(f"npm ERR! _auth={SECRET_B64}\n")
sys.stderr.write(
    "npm ERR! at /home/user/.local/share/garraia/bridge/node_modules/"
    "@whiskeysockets/baileys/package.json\n"
)
sys.stderr.flush()
sys.exit(1)
