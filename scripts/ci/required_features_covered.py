#!/usr/bin/env python3
"""Todo alvo `[[test]]` com `required-features` tem que ser executado por algum job do CI (#1242).

O defeito que este guard fecha:

    [[test]]
    name = "mcp_boot_pending_allowlist"
    required-features = ["mcp-http"]

O `cargo test --workspace` do job `test` roda com as features default. Um
alvo cujas `required-features` nao estao satisfeitas e **pulado em silencio**
-- sem uma linha no log, sem "0 tests", sem aviso. O teste compila no clippy
com a feature ligada, nunca executa em lugar nenhum, e o repo fica com um
call site compilado e nao exercitado. O proprio `ci.yml` ja documenta essa
armadilha em tres lugares (`auth-integration`, `mcp`, `signal`), sempre
depois de ela ter custado vermelho nao lido.

Regra verificada, por alvo `(pacote, nome, required-features)`: existe em
`.github/workflows/*.yml` alguma invocacao de `cargo test` que

  1. alcance o pacote (`-p <pacote>`; `--workspace` sozinho nao serve porque
     nao liga feature nenhuma),
  2. ligue TODAS as `required-features` (`--features ...` ou `--all-features`), e
  3. alcance alvos de teste de integracao -- sem `--test <outro>`, e sem
     restricao a outro tipo de alvo (`--lib`, `--bins`, `--doc`, `--examples`).

O guard e deliberadamente textual: ele le as linhas do workflow como elas
serao executadas. Nao resolve `${{ }}`, matriz nem variavel de shell -- uma
invocacao montada dinamicamente nao conta como cobertura, e isso e proposital.

Uso:
    python3 scripts/ci/required_features_covered.py

Exit 0 quando todo alvo esta coberto; exit 1 com uma linha por alvo orfao.
"""

from __future__ import annotations

import glob
import re
import shlex
import sys
import tomllib

WORKFLOW_GLOB = ".github/workflows/*.yml"
MANIFEST_GLOB = "crates/*/Cargo.toml"

# Restringem a invocacao a alvos que NAO sao testes de integracao.
TARGET_NARROWING = {"--lib", "--bins", "--bin", "--doc", "--examples", "--example", "--benches"}


def declared_targets() -> list[tuple[str, str, list[str]]]:
    """(pacote, nome do alvo, required-features) de todo `[[test]]` gated."""
    targets: list[tuple[str, str, list[str]]] = []
    for manifest in sorted(glob.glob(MANIFEST_GLOB)):
        with open(manifest, "rb") as fh:
            data = tomllib.load(fh)
        package = data.get("package", {}).get("name")
        if not package:
            continue
        for target in data.get("test", []):
            features = target.get("required-features") or []
            if features:
                targets.append((package, target.get("name", "?"), sorted(features)))
    return targets


def cargo_test_invocations() -> list[list[str]]:
    """Linhas de `cargo test` dos workflows, ja tokenizadas."""
    invocations: list[list[str]] = []
    for workflow in sorted(glob.glob(WORKFLOW_GLOB)):
        with open(workflow, encoding="utf-8") as fh:
            for line in fh:
                stripped = line.strip()
                # Comentarios do YAML citam `cargo test` o tempo todo para
                # explicar justamente este buraco; eles nao executam nada.
                if stripped.startswith("#") or "cargo test" not in stripped:
                    continue
                stripped = stripped.removeprefix("run:").strip()
                try:
                    tokens = shlex.split(stripped, comments=False)
                except ValueError:
                    continue
                if "cargo" in tokens and "test" in tokens:
                    invocations.append(tokens)
    return invocations


def enabled_features(tokens: list[str]) -> set[str] | None:
    """Features ligadas por esta invocacao. `None` significa "todas"."""
    if "--all-features" in tokens:
        return None
    features: set[str] = set()
    for index, token in enumerate(tokens):
        value = None
        if token == "--features" and index + 1 < len(tokens):
            value = tokens[index + 1]
        elif token.startswith("--features="):
            value = token.split("=", 1)[1]
        if value:
            features.update(part for part in re.split(r"[,\s]+", value) if part)
    return features


def covers(tokens: list[str], package: str, target: str, required: list[str]) -> bool:
    if "-p" not in tokens and "--package" not in tokens:
        return False
    packages = {
        tokens[index + 1]
        for index, token in enumerate(tokens)
        if token in ("-p", "--package") and index + 1 < len(tokens)
    }
    if package not in packages:
        return False

    features = enabled_features(tokens)
    if features is not None and not set(required).issubset(features):
        return False

    if any(token in TARGET_NARROWING for token in tokens):
        return False

    named = {
        tokens[index + 1]
        for index, token in enumerate(tokens)
        if token == "--test" and index + 1 < len(tokens)
    }
    return not named or target in named


def main() -> int:
    invocations = cargo_test_invocations()
    orphans = [
        (package, target, required)
        for package, target, required in declared_targets()
        if not any(covers(tokens, package, target, required) for tokens in invocations)
    ]

    if orphans:
        print("Alvos de teste com `required-features` que nenhum job do CI executa:\n")
        for package, target, required in orphans:
            print(f"  {package} :: {target}  (required-features = {required})")
        print(
            "\nCada um destes compila e e PULADO EM SILENCIO pelo `cargo test --workspace`.\n"
            "Acrescente a um job do CI uma linha como:\n"
            f"    cargo test -p {orphans[0][0]} --features {','.join(orphans[0][2])} "
            f"--test {orphans[0][1]}\n"
        )
        return 1

    print(f"ok: {len(declared_targets())} alvo(s) com `required-features`, todos executados no CI")
    return 0


if __name__ == "__main__":
    sys.exit(main())
