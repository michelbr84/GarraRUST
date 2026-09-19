#!/usr/bin/env python3
"""Confere que cada entrada do ledger ainda aponta para o statement que descreve.

A ancora e o **conteudo**, nao a posicao: o `sink_snippet` e o identificador, o
checker procura por ele no `path` e **deriva** o numero de linha. O campo `line`
do ledger e derivado e informativo — este script o reescreve quando a linha
anda, junto com a coluna `File:line` do ledger markdown.

Por que assim (issue #1263). Ate 2026-09-19 a ancora era `(path, line)` e o
snippet era conferido **naquela** linha. Qualquer commit que inserisse ou
removesse linhas *acima* do sink derrubava o gate sem encostar no statement
suprimido — e a saida mais facil para ficar verde era editar o `sink_snippet`
para casar com o que estivesse na linha, ou seja, fraudar o registro. A entrada
#113 andou **duas vezes no mesmo PR** (#1252, `:769` -> `:826` -> `:867`) sem o
snippet mudar um byte. Um guard cuja saida mais comoda e a fraude depende de
disciplina em vez de mecanismo.

Dois casos reais que motivaram o script, antes disso:

* O alerta #113 passou tres meses apontando para o statement errado
  (`wizard/mod.rs:640`, o `eprintln!` do "vault open failed") enquanto o alerta
  vivo estava no `println!` de `:673`. Ninguem percebeu porque nada comparava a
  ancora com o codigo. Corrigido a mao em 2026-08-30.
* O PR #882 (Copilot Autofix do alerta 162) inseriu 18 linhas em
  `skins_handler.rs` e deslocou as cinco ancoras 84/104/111/134/141 que quatro
  entradas do ledger registram. O CI so reclamou do erro de compilacao que o
  mesmo commit trazia de brinde; a dessincronizacao do ledger teria passado
  despercebida se ele compilasse.

O que e conferido, por entrada:

1. **Sink vs codigo** — o `sink_snippet` tem que aparecer em `path`, comparado
   linha a linha com `.strip()`:
   * **1 ocorrencia** -> e ela; a linha derivada substitui o `line` do ledger.
   * **0 ocorrencias** -> erro (exit 6). O statement mudou ou sumiu, e esse e o
     unico caso em que o gate **deve** ficar vermelho: a supressao precisa de
     reauditoria humana.
   * **2+ ocorrencias** -> erro de configuracao do ledger (exit 5). O snippet
     nao identifica um statement so; a entrada precisa de `disambiguator`.
2. **Desambiguacao** — entrada com `disambiguator` filtra as ocorrencias. Hoje
   ha um `kind` so:
   `{"kind": "function", "value": "<nome da fn Rust envolvente>"}`.
   Sobrou 1 candidato -> ok. Sobrou 0 -> exit 6 (a fn que continha o sink nao
   contem mais: renomeada, ou o sink saiu dela). Sobrou 2+ -> exit 5.
3. **Ancora vs `.md`** — a coluna `File:line` da linha correspondente no ledger
   markdown tem que apontar para o **mesmo arquivo** do `.json` (arquivo
   diferente e desync de verdade, exit 6), e o `Rule` tem que casar com
   `rule_id`. O numero de linha da coluna e derivado como o do `.json`, e
   reescrito junto. O `--check-md` do `codeql-reapply-dismissals.sh` **nao**
   cobre isso: ele so compara o conjunto de numeros de alerta entre os dois
   arquivos.
4. **Citacoes dentro da justificativa** — `arquivo.rs:NNN` no texto da
   `justification` e resolvido (relativo ao repo, ou relativo ao diretorio da
   propria entrada) e a linha tem que existir. Foi so por falta disso que o
   drift do #113 sobreviveu: a justificativa apontava `cloud_secret` em `:630`
   (estava em `:824`) e o `eprintln!` do vault em `:643` (estava em `:837`),
   num documento de auditoria de seguranca. Citacao de arquivo ancorado no repo
   que nao existe mais, ou linha fora de faixa, e erro (exit 6). Citacao por
   nome de arquivo solto que nao resolve vira **aviso**, porque prosa nao e
   caminho e o script nao adivinha. Citacao `:NNN` sem arquivo fica fora de
   escopo de proposito: quase toda ocorrencia e historica ("o sink desceu de
   `:1380` para `:1653`") e validar historia por faixa e tripwire sem valor.

REGRA ANTI-FRAUDE, e a razao de o snippet ser versionado em vez de derivado:

    NUNCA edite `sink_snippet` para fazer este check passar. O snippet e o
    registro do que a justificativa revisada em PR descreve. Se o statement
    mudou, ha exatamente dois caminhos legitimos, os dois com diff revisado por
    humano: reapontar a entrada (snippet novo, conferindo que o sink e mesmo o
    mesmo) ou reauditar a supressao. Reescrever o snippet para casar com o que
    quer que esteja no arquivo hoje e a mesma fraude que editar
    `.quality/baseline.json` a mao — ver CLAUDE.md, secao AI Quality Ratchet.

Mover `line` **nao** e fraude, e por isso este script faz isso sozinho: e
posicao derivada, nao afirmacao de auditoria.

Uso:
    python3 scripts/security/check-ledger-anchors.py
    python3 scripts/security/check-ledger-anchors.py --ledger caminho/ledger.json
    python3 scripts/security/check-ledger-anchors.py --no-rewrite   # so reporta

Exit codes (a convencao 2/3/4/5 ja esta ocupada pelos scripts irmaos):
    0  todas as entradas casam
    5  falha de precondicao (ledger ausente/malformado, arquivo-fonte sumido,
       entrada sem `sink_snippet`, snippet ambiguo sem `disambiguator`)
    6  drift: alguma entrada nao aponta mais para statement algum (ou cita
       codigo que nao existe mais) e precisa de reauditoria
    7  janela de re-auditoria de 90 dias vencida
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
from pathlib import Path

DEFAULT_JSON = "docs/security/codeql-suppressions.json"
DEFAULT_MD = "docs/security/codeql-suppressions.md"

EXIT_OK = 0
EXIT_PRECONDITION = 5
EXIT_DRIFT = 6
EXIT_EXPIRED = 7

# Linha canonica do ledger markdown. Casa apenas as tres primeiras celulas, para
# nao tropecar num `|` dentro da justificativa (que e prosa livre com codigo):
#   | <a id="alert-162"></a>[#162](.../162) | `rule` | `path:line` | ...
MD_ROW = re.compile(
    r'^\|\s*<a id="alert-(?P<number>\d+)"></a>[^|]*\|(?P<rule>[^|]*)\|(?P<fileline>[^|]*)\|'
)

# Assinatura de funcao Rust, para o `disambiguator` de `kind: "function"`.
# Cobre `pub`, `pub(crate)`, `const`, `async`, `unsafe` e `extern "C"` em
# qualquer combinacao valida, que e o que aparece nos sinks deste ledger.
RUST_FN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
    r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)

# Citacao `arquivo:linha` dentro da justificativa. Exige extensao conhecida para
# nao casar `#1252` (PR), `613510d` (squash) nem `tests/x.rs::nome_do_teste`.
CITATION = re.compile(
    r"(?P<path>[A-Za-z0-9_][A-Za-z0-9_./-]*"
    r"\.(?:rs|py|sh|ps1|toml|ya?ml|md|dart|sql|json|ts|tsx|js|html))"
    r":(?P<line>\d{1,6})(?![\d:])"
)

# Prefixos que fazem uma citacao ser "ancorada no repo": se comeca por um
# destes, o caminho e relativo a raiz e a ausencia do arquivo e erro, nao aviso.
REPO_ANCHORED = (
    "crates/",
    "apps/",
    "scripts/",
    "docs/",
    "benches/",
    "deploy/",
    "contrib/",
    "bridge/",
    "tests/",
    ".github/",
)


def fail(msg: str, code: int = EXIT_PRECONDITION) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(code)


def check_expiration(
    audit_expiration: str | None, today: dt.date
) -> tuple[bool, str]:
    """Is the 90-day re-audit window still open?

    Returns `(ok, message)`. The date lives in `audit_expiration` at the root
    of the ledger and §3 regra 4 makes the re-audit mandatory every 90 days —
    but until 2026-08-30 **nothing read the field**: not a script, not a
    workflow, not a test. It sat expired for 29 days and no build noticed.
    A rule with no reader is a comment, so this is what turns it into a gate.

    Deliberately not auto-renewable: the date is the record that a human
    looked at each justification. Moving it without doing that work is the
    same fraud as hand-editing `.quality/baseline.json` to pass the ratchet.
    """
    if not audit_expiration:
        return False, "ledger sem `audit_expiration` na raiz"
    try:
        deadline = dt.date.fromisoformat(audit_expiration)
    except ValueError:
        return False, f"`audit_expiration` nao e uma data ISO valida: {audit_expiration!r}"
    if deadline < today:
        overdue = (today - deadline).days
        return False, (
            f"re-auditoria do ledger vencida em {audit_expiration} "
            f"(ha {overdue} dia(s))"
        )
    return True, f"re-auditoria do ledger valida ate {audit_expiration}"


def unbacktick(cell: str) -> str:
    """Normaliza uma celula de tabela markdown: tira espacos e crases."""
    return cell.strip().strip("`").strip()


def find_snippet_lines(lines: list[str], snippet: str) -> list[int]:
    """Numeros de linha (1-based) onde `snippet` aparece, comparado com strip().

    O coracao da ancoragem por conteudo: e isto que substitui "le a linha
    `line` e compara". Reindentar nao e drift, e mover o statement 40 linhas
    para baixo tambem nao.
    """
    return [i + 1 for i, raw in enumerate(lines) if raw.strip() == snippet]


def enclosing_function(lines: list[str], line_no: int) -> str | None:
    """Nome da `fn` Rust mais proxima acima de `line_no` (1-based), se houver.

    Ancora auxiliar do `disambiguator`: e content-based como o snippet, ao
    contrario de "a N-esima ocorrencia", que reaponta em silencio no dia em que
    alguem acrescenta um statement identico acima.
    """
    for i in range(min(line_no, len(lines)) - 1, -1, -1):
        m = RUST_FN.match(lines[i])
        if m:
            return m.group("name")
    return None


def apply_disambiguator(
    lines: list[str], candidates: list[int], disambiguator: object
) -> tuple[list[int], str | None]:
    """Filtra `candidates` pelo `disambiguator` da entrada.

    Devolve `(candidatos_restantes, erro_de_forma)`. `erro_de_forma` nao-nulo
    significa que o proprio campo esta malformado (precondicao), nao que o
    codigo mudou.
    """
    if not isinstance(disambiguator, dict):
        return candidates, "`disambiguator` precisa ser um objeto {kind, value}"
    kind = disambiguator.get("kind")
    value = disambiguator.get("value")
    if not isinstance(kind, str) or not isinstance(value, str) or not value:
        return candidates, "`disambiguator` precisa de `kind` e `value` string nao-vazios"
    if kind != "function":
        return candidates, (
            f"`disambiguator.kind` desconhecido: {kind!r} "
            f"(suportado hoje: \"function\")"
        )
    return [c for c in candidates if enclosing_function(lines, c) == value], None


def resolve_citation(
    cite: str, entry_path: str, root: Path
) -> tuple[Path | None, bool]:
    """Resolve `arquivo.rs` citado na justificativa.

    Devolve `(caminho_relativo_ou_None, ancorado_no_repo)`. Citacao ancorada no
    repo (comeca por `crates/`, `scripts/`, ...) e resolvida contra a raiz;
    nome solto (`session_store.rs`, `wizard/mod.rs`) e resolvido subindo o
    diretorio da propria entrada, que e o escopo natural de quem escreveu a
    justificativa.
    """
    anchored = cite.startswith(REPO_ANCHORED)
    if anchored:
        return (Path(cite) if (root / cite).is_file() else None), True

    directory = Path(entry_path).parent
    while True:
        candidate = directory / cite
        if (root / candidate).is_file():
            return candidate, False
        if str(directory) in (".", ""):
            return None, False
        directory = directory.parent


def check_citations(
    justification: object, entry_path: str, root: Path
) -> tuple[list[str], list[str]]:
    """Confere as citacoes `arquivo:linha` de uma justificativa.

    Devolve `(erros, avisos)`. Erro: arquivo ancorado no repo que nao existe
    mais, ou linha fora de faixa num arquivo que existe. Aviso: nome de arquivo
    solto que nao resolve — pode ser prosa, e o script nao adivinha.
    """
    errors: list[str] = []
    warnings: list[str] = []
    # `str()` e nao `isinstance`: justificativa em lista (nenhuma hoje) ainda
    # tem as citacoes varridas em vez de escapar em silencio.
    for m in CITATION.finditer(str(justification or "")):
        cite = m.group("path")
        cited_line = int(m.group("line"))
        resolved, anchored = resolve_citation(cite, entry_path, root)
        if resolved is None:
            msg = f"citacao `{cite}:{cited_line}` na justificativa nao resolve para arquivo algum"
            (errors if anchored else warnings).append(msg)
            continue
        total = len((root / resolved).read_text(encoding="utf-8").splitlines())
        if not 0 < cited_line <= total:
            errors.append(
                f"citacao `{cite}:{cited_line}` na justificativa esta fora de faixa "
                f"({resolved} tem {total} linhas)"
            )
    return errors, warnings


def parse_md(md_path: Path) -> tuple[dict[int, dict], list[str]]:
    """Mapeia numero do alerta -> celulas do ledger markdown, e devolve as linhas.

    Cada valor traz `rule`, `fileline`, o indice 0-based da linha no arquivo e o
    `span` da celula `File:line` no texto cru, para que a reescrita da posicao
    derivada nao precise reformatar a linha inteira.
    """
    raw_lines = md_path.read_text(encoding="utf-8").splitlines()
    rows: dict[int, dict] = {}
    for index, raw in enumerate(raw_lines):
        m = MD_ROW.match(raw)
        if not m:
            continue
        number = int(m.group("number"))
        if number in rows:
            fail(f"ledger .md tem duas linhas para o alerta #{number}")
        rows[number] = {
            "rule": unbacktick(m.group("rule")),
            "fileline": unbacktick(m.group("fileline")),
            "index": index,
            "span": m.span("fileline"),
        }
    return rows, raw_lines


def rewrite_md_cell(raw_line: str, span: tuple[int, int], new_fileline: str) -> str:
    """Troca o conteudo da celula `File:line` preservando o resto da linha."""
    start, end = span
    cell = raw_line[start:end]
    stripped = cell.strip()
    if stripped.startswith("`") and stripped.endswith("`"):
        new_cell = f" `{new_fileline}` "
    else:
        new_cell = f" {new_fileline} "
    return raw_line[:start] + new_cell + raw_line[end:]


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Confere as ancoras (path, sink_snippet) do ledger CodeQL."
    )
    ap.add_argument("--ledger", default=DEFAULT_JSON, help=f"default: {DEFAULT_JSON}")
    ap.add_argument("--md", default=DEFAULT_MD, help=f"default: {DEFAULT_MD}")
    ap.add_argument(
        "--root",
        default=".",
        help="raiz do repo, para resolver os `path` das entradas (default: .)",
    )
    ap.add_argument(
        "--no-rewrite",
        action="store_true",
        help=(
            "nao reescreve o `line` derivado no ledger — so reporta a "
            "divergencia (util em arvore read-only)"
        ),
    )
    args = ap.parse_args()

    ledger_path = Path(args.ledger)
    md_path = Path(args.md)
    root = Path(args.root)

    if not ledger_path.is_file():
        fail(f"ledger nao encontrado: {ledger_path}")
    if not md_path.is_file():
        fail(f"ledger markdown nao encontrado: {md_path}")

    try:
        doc = json.loads(ledger_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"ledger malformado ({ledger_path}): {exc}")

    entries = doc.get("entries")
    if not isinstance(entries, list):
        fail(f"ledger sem lista `entries`: {ledger_path}")

    md_rows, md_lines = parse_md(md_path)

    # Cache de arquivo -> linhas, para nao reler o mesmo fonte 11 vezes.
    sources: dict[str, list[str]] = {}
    drift: list[str] = []
    # Erros de configuracao do ledger acumulam em vez de abortar na primeira
    # entrada: quando 13 das 30 entradas precisam de `disambiguator` — que foi o
    # estado real na migracao da #1263 — abortar no primeiro obrigaria a 13
    # rodadas de CI para descobrir as 13.
    config_errors: list[str] = []
    warnings: list[str] = []
    moved: list[str] = []
    json_dirty = False
    md_dirty = False

    for entry in entries:
        number = entry.get("alert_number")
        rel = entry.get("path")
        snippet = entry.get("sink_snippet")

        # `line` NAO entra nesta lista: e derivado. Entrada sem ele e valida, e
        # o checker o escreve.
        if number is None or rel is None:
            fail(f"entrada incompleta (alert_number/path): {entry!r}")
        if snippet is None:
            config_errors.append(
                f"#{number}: entrada sem `sink_snippet`. O ledger precisa estar em "
                f"schema_version >= 1.1.0 — rode a migracao antes deste check."
            )
            continue
        if not isinstance(snippet, str) or not snippet.strip():
            config_errors.append(
                f"#{number}: `sink_snippet` vazio nao identifica statement algum"
            )
            continue

        if rel not in sources:
            src = root / rel
            if not src.is_file():
                fail(f"#{number}: arquivo do ledger nao existe: {src}")
            sources[rel] = src.read_text(encoding="utf-8").splitlines()
        lines = sources[rel]

        ledger_line = entry.get("line")
        rule_id = entry.get("rule_id", "?")
        disambiguator = entry.get("disambiguator")
        candidates = find_snippet_lines(lines, snippet)

        # 0 ocorrencias: o statement suprimido mudou ou sumiu. Unico caso em que
        # o gate deve ficar vermelho por causa do codigo.
        if not candidates:
            drift.append(
                f"#{number} ({rule_id}) {rel}\n"
                f"      sink_snippet nao aparece mais no arquivo — REAUDITORIA\n"
                f"      ledger : {snippet}\n"
                f"      ultima posicao conhecida: linha {ledger_line}"
            )
            continue

        # 2+ ocorrencias: o snippet nao identifica um statement so.
        if len(candidates) > 1:
            if disambiguator is None:
                config_errors.append(
                    f"#{number} ({rule_id}) {rel}: `sink_snippet` ambiguo — "
                    f"{len(candidates)} ocorrencias (linhas "
                    f"{', '.join(str(c) for c in candidates)}).\n"
                    f"      ledger : {snippet}\n"
                    f"      Desambigue a entrada com "
                    f'`"disambiguator": {{"kind": "function", "value": '
                    f'"<fn envolvente>"}}` (ou escolha um sink_snippet unico).'
                )
                continue
            filtered, shape_error = apply_disambiguator(lines, candidates, disambiguator)
            if shape_error:
                config_errors.append(f"#{number} ({rule_id}) {rel}: {shape_error}")
                continue
            if not filtered:
                drift.append(
                    f"#{number} ({rule_id}) {rel}\n"
                    f"      o `disambiguator` "
                    f"{disambiguator.get('kind')}={disambiguator.get('value')!r} nao "
                    f"casa com nenhuma das {len(candidates)} ocorrencias "
                    f"(linhas {', '.join(str(c) for c in candidates)}) — REAUDITORIA\n"
                    f"      ledger : {snippet}"
                )
                continue
            if len(filtered) > 1:
                config_errors.append(
                    f"#{number} ({rule_id}) {rel}: `disambiguator` "
                    f"{disambiguator.get('kind')}={disambiguator.get('value')!r} ainda "
                    f"deixa {len(filtered)} candidatos (linhas "
                    f"{', '.join(str(c) for c in filtered)}). Use uma ancora "
                    f"auxiliar mais especifica."
                )
                continue
            candidates = filtered
        elif disambiguator is not None:
            # Ocorrencia unica: a posicao nao e ambigua, entao o disambiguator
            # nao decide nada e um `fn` renomeado nao pode derrubar o gate. Mas
            # ele integra a justificativa ("`get_skill` guards Path(name)"), e
            # por isso a divergencia sai como aviso em vez de silencio.
            still_matches, shape_error = apply_disambiguator(
                lines, candidates, disambiguator
            )
            if shape_error:
                config_errors.append(f"#{number} ({rule_id}) {rel}: {shape_error}")
                continue
            if not still_matches:
                warnings.append(
                    f"#{number} `disambiguator` "
                    f"{disambiguator.get('kind')}={disambiguator.get('value')!r} nao "
                    f"casa com a ocorrencia unica na linha {candidates[0]} "
                    f"(fn envolvente: "
                    f"{enclosing_function(lines, candidates[0])!r}) — atualize a "
                    f"ancora auxiliar"
                )

        derived = candidates[0]

        # `line` e derivado: a divergencia e fato a registrar, nao falha.
        if ledger_line != derived:
            moved.append(
                f"#{number} ({rule_id}) {rel}: linha derivada {derived} "
                f"(antes {ledger_line})"
            )
            entry["line"] = derived
            json_dirty = True

        # Citacoes dentro da justificativa.
        cite_errors, cite_warnings = check_citations(
            entry.get("justification", ""), rel, root
        )
        for msg in cite_errors:
            drift.append(f"#{number} ({rule_id}) {rel}\n      {msg}")
        warnings.extend(f"#{number} {msg}" for msg in cite_warnings)

        # Espelho no .md: o arquivo tem que ser o mesmo; a linha e derivada.
        md_row = md_rows.get(number)
        if md_row is None:
            drift.append(
                f"#{number} ({rule_id}) {rel}:{derived}\n"
                f"      sem linha correspondente em {md_path}"
            )
            continue
        md_fileline = md_row["fileline"]
        md_file = md_fileline.rsplit(":", 1)[0] if ":" in md_fileline else md_fileline
        if md_file != rel:
            drift.append(
                f"#{number} .md aponta para outro arquivo\n"
                f"      .json : {rel}\n"
                f"      .md   : {md_row['fileline']}"
            )
            continue
        if md_row["rule"] != entry.get("rule_id"):
            drift.append(
                f"#{number} .md com rule_id divergente\n"
                f"      .json : {entry.get('rule_id')}\n"
                f"      .md   : {md_row['rule']}"
            )
            continue
        expected_fileline = f"{rel}:{derived}"
        if md_row["fileline"] != expected_fileline:
            moved.append(
                f"#{number} .md: File:line derivado {expected_fileline} "
                f"(antes {md_row['fileline']})"
            )
            md_lines[md_row["index"]] = rewrite_md_cell(
                md_lines[md_row["index"]], md_row["span"], expected_fileline
            )
            md_dirty = True

    if moved:
        for item in moved:
            print(f"  {item}")
        if args.no_rewrite:
            print(
                f"aviso: {len(moved)} posicao(oes) derivada(s) divergem do ledger; "
                f"--no-rewrite, nada foi escrito"
            )
        else:
            if json_dirty:
                ledger_path.write_text(
                    json.dumps(doc, indent=2, ensure_ascii=False) + "\n",
                    encoding="utf-8",
                )
                print(f"ok: {ledger_path} reescrito com as linhas derivadas")
            if md_dirty:
                md_path.write_text("\n".join(md_lines) + "\n", encoding="utf-8")
                print(f"ok: {md_path} reescrito com as linhas derivadas")

    for item in warnings:
        print(f"aviso: {item}", file=sys.stderr)

    # Precondicao antes de drift: um ledger que o checker nao consegue resolver
    # e o problema mais urgente, e reportar os dois juntos esconderia este.
    if config_errors:
        print(
            f"error: {len(config_errors)} entrada(s) do ledger nao sao "
            f"resolviveis por conteudo\n",
            file=sys.stderr,
        )
        for item in config_errors:
            print(f"  {item}\n", file=sys.stderr)
        print(
            "A ancora do ledger e o `sink_snippet`, nao o numero de linha: cada\n"
            "entrada precisa identificar UM statement. Ambiguidade se resolve com\n"
            "`disambiguator` (ancora auxiliar de conteudo), nunca escolhendo a\n"
            "ocorrencia pela posicao antiga.",
            file=sys.stderr,
        )
        return EXIT_PRECONDITION

    if drift:
        print(
            f"error: {len(drift)} entrada(s) do ledger nao apontam mais para o "
            f"statement que descrevem\n",
            file=sys.stderr,
        )
        for item in drift:
            print(f"  {item}\n", file=sys.stderr)
        print(
            "Reaudite a supressao (o alerta ainda procede? mudou de categoria?) ou\n"
            "reaponte a entrada com o sink_snippet do statement de hoje, conferindo\n"
            "que o sink e o mesmo. NUNCA edite sink_snippet so para o check passar —\n"
            "ver a docstring deste script.",
            file=sys.stderr,
        )
        return EXIT_DRIFT

    print(f"ok: {len(entries)} entradas do ledger conferem com o codigo e com o .md")

    # As ancoras batem; agora a janela de re-auditoria. Vem depois de proposito:
    # um ledger com ancora podre e o problema mais urgente, e reportar os dois
    # de uma vez esconderia o primeiro atras do segundo.
    fresh, message = check_expiration(doc.get("audit_expiration"), dt.date.today())
    if not fresh:
        print(f"error: {message}", file=sys.stderr)
        print(
            "\nRevise o merito de cada entrada (a justificativa ainda procede? o\n"
            "alerta ainda existe? mudou de categoria?) e so entao avance a data,\n"
            "no MESMO commit. Avancar `audit_expiration` sozinho transforma o\n"
            "ledger em teatro — e a mesma fraude que editar .quality/baseline.json\n"
            "a mao para passar o ratchet.",
            file=sys.stderr,
        )
        return EXIT_EXPIRED
    print(f"ok: {message}")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
