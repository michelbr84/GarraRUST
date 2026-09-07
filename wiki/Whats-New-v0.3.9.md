# What's New in v0.3.9

> 🇧🇷 [Versão em português](Novidades-v0.3.9) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.9)

The largest release so far: **48 issues and PRs**, from #933 to #1012, plus the
field-feedback batch that had been ready since Sept 5 (#920–#930, #966).

v0.3.9 was actually prepared on Sept 5 and never shipped — no tag was ever cut.
What was ready that day stayed, and everything since went in with it.

---

## What you'll notice

### The terminal stopped being a black box

Running a tool used to be invisible: you saw the activity indicator and then,
minutes later, a finished answer — with no idea whether the agent was reading a
file, running `cargo test`, or simply stuck.

```
● Bash cargo test
  └─ 148 passed · 6.3s · #7

× Bash └─ error: exit 101 · 4.2s · #8
```

On failure the name is repeated on the same line, deliberately: after long
output, the start line may have scrolled away. The `#7` is a pointer — `/tool 7`
prints that call's full output.

Alongside it: **Markdown rendered without stalling the stream** (text keeps
appearing as it arrives, with headings, lists and code blocks formatted), new
status surfaces (`/status`, `/tools`, `/logs`), and a `garra logs` command.

### `garra chat 2>/dev/null | cat` no longer carries ANSI escapes

Colour was unconditional on several paths — `/help`, `/context`, `/history`,
the goodbye line. Redirecting output dragged escape sequences along. The CLI now
says *what*, and the renderer decides *how*, based on whether a terminal is
actually there.

### Memory became inspectable

`garra memory` is no longer read-only:

```bash
garra memory add "The building is on Flower Street" --session project-x
garra memory reindex        # embed entries stored without a vector
garra memory backup         # consistent snapshot, with retention
garra memory pin <id>       # retention will never delete it
garra memory ttl <id> 30    # or expire it in 30 days (a number, not "30d")
garra memory stats          # counts + vector-index integrity report
```

### Execution modes became real

This is the biggest behavioural change — see below.

---

## One pattern runs through the batch: *a restriction that was declared and never enforced*

Four different defects, one shape. Each promised a property the code did not
deliver:

| Where | What it promised | What actually happened |
|---|---|---|
| Modes' `ToolPolicy` (#988) | `search`/`review`/`architect` are read-only | **never checked in the executor** — it only asked in the system prompt |
| Named agent's `tools:` (#965) | *"Restrict which tools this agent can use"* | never read anywhere |
| `X-User-Id` on `/v1/chat/completions` (#1012) | the caller's identity | a forgeable header decided what got recorded |
| `continuity_key(_user_id)` | a per-person memory bus | returned `bus:shared-global` to everyone |

**If you use modes**, this is the change that matters most: a mode advertised as
read-only now **blocks** `file_write` and `bash` in the executor, instead of
merely asking nicely in the prompt.

---

## Security

- **`/v1/chat/completions` no longer derives identity from the caller** (#1012).
  The route is auth-free by design, like all of `/api/*` — but auth-free means
  "requires no credential", not "accepts whatever identity the caller claims".
  An arbitrary `Bearer` became the `user_id`; without one, `X-User-Id` was used
  raw. Measured impact: **false attribution**, not cross-tenant reads — history
  loading has always been keyed by `session_id`. No client breaks.
- **Tenant isolation on the recall KNN path.** The vector index only knows
  distance; the candidate fetch applied none of the query's filters. With the
  index active, a recall scoped to a `tenant_id` could return another tenant's
  memory.
- **Tool output and model text no longer inject ANSI** into the terminal
  (#995, #996) — a file containing escape sequences can no longer repaint your
  screen.
- **Embedding-provider error bodies** can no longer reach the log raw: 401 and
  403 lose the body entirely, the rest are truncated with known token formats
  scrubbed. (OpenAI echoes your API key back at you when it's wrong.)

---

## Three documents that contradicted the binary

Fixed by checking them against it, not by re-reading them:

- `garraia-learning`'s `retriever`, which described itself as working while
  being a stub (#964);
- `docs/src/memory.md`, which denied the existence of a command that had just
  shipped (#963);
- `docs/memory.md` — the page **this wiki linked to** — which still described
  `facts.json` and the commands `clear`/`export`/`disable`, none of which ever
  existed.

---

## Upgrading

```bash
garra update          # existing installs
```

Or reinstall:

```bash
curl -fsSL https://garraia.org/install.sh | sh          # Linux / macOS
irm https://garraia.org/install.ps1 | iex               # Windows
```

The raw binaries (`garraia-<os>-<arch>`) are still published next to the
archives — `garra update` resolves assets by exact name and requires the
sibling `<asset>.sha256`.

---

## Also in this release

- **A Portuguese recall-quality benchmark** (#958): 40 ground-truth queries in
  13 groups that separate failure modes — paraphrase, synonym, single-word
  query, Spanish query against a Portuguese corpus. It is not a CI gate, and it
  should not become one.
- **ADR 0017** (TerminalRenderer) and **ADR 0018** (the orphan
  `garraia-embeddings` crate, still `Proposed`).
- **`changelog.d/`**: PRs no longer edit `CHANGELOG.md` directly — each leaves a
  fragment, so two parallel PRs stop colliding.
- Session-scoped `/goal`, and `/stats` reporting the turn that actually ran.

---

**Everything, item by item:** [CHANGELOG.md](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md)
