#!/usr/bin/env python3
"""`npm` falso que falha cuspindo material de credencial no stderr.

Existe para um unico fim: provar que `npm_ci` passa a cauda do stderr pela
redacao ANTES de mostra-la. Sem ele, a redacao era testada so como funcao pura
e neutralizar o call site (`.map(redact_tail_line)` -> `.map(to_string)`)
deixava a suite inteira verde.

Ignora os argumentos de proposito: quem o chama e o `npm_ci` de verdade, com
`ci --no-fund --no-audit --progress=false`.
"""

import sys

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
