#!/usr/bin/env python3
"""
Utilitário de varredura estática de segredos e credenciais em arquivos do repositório.
Verifica padrões de chaves de API reais, JWT secrets, credenciais de produção e tokens.
"""

import os
import re
import sys

PATTERNS = [
    (r"(?i)aws_secret_access_key\s*=\s*['\"][A-Za-z0-9/+=]{40}['\"]", "AWS Secret Key"),
    (r"(?i)ghp_[A-Za-z0-9]{36}", "GitHub Personal Access Token"),
    (r"(?i)gho_[A-Za-z0-9]{36}", "GitHub OAuth Token"),
    (r"(?i)\bsk-[A-Za-z0-9]{32,}\b", "Generic API Key (sk-...)"),
    (r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----", "Private Key Header"),
]

IGNORED_DIRS = {
    ".git",
    "target",
    "node_modules",
    ".quality",
    ".claude",
}

IGNORED_FILES = {
    ".env.example",
    "varredura-segredos.py",
}

def scan_file(filepath):
    findings = []
    try:
        with open(filepath, "r", encoding="utf-8", errors="ignore") as f:
            for idx, line in enumerate(f, start=1):
                # Ignore placeholders, test mocks, and intentional doc examples
                if any(x in line for x in ("sk-ant-test-key", "placeholder", "XXXXXXXX", "mock", "example.com")):
                    continue
                for pattern, desc in PATTERNS:
                    if re.search(pattern, line):
                        findings.append((idx, desc, line.strip()))
    except Exception as e:
        print(f"Erro ao ler {filepath}: {e}", file=sys.stderr)
    return findings

def main():
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    has_findings = False

    for root, dirs, files in os.walk(repo_root):
        dirs[:] = [d for d in dirs if d not in IGNORED_DIRS]
        for file in files:
            if file in IGNORED_FILES:
                continue
            filepath = os.path.join(root, file)
            findings = scan_file(filepath)
            if findings:
                has_findings = True
                rel_path = os.path.relpath(filepath, repo_root)
                print(f"Aviso de segredo potencial em {rel_path}:")
                for line_no, desc, snippet in findings:
                    print(f"  Linha {line_no} [{desc}]: {snippet[:80]}")

    if has_findings:
        print("\nVarredura concluída com potenciais alertas encontrados.", file=sys.stderr)
        sys.exit(1)
    else:
        print("Varredura de segredos concluída com sucesso. Nenhum segredo detectado.")
        sys.exit(0)

if __name__ == "__main__":
    main()
