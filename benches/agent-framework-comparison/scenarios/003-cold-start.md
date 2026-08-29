# 003-cold-start

| | |
|---|---|
| **Category** | performance |
| **Status** | active |

## Objective

Prove executable cold-start latency: time from process spawn to exit for
`<bin> --help`, measured with hyperfine (3 warmups, 20 runs, median and
p95 reported).

This measures interpreter/runtime startup cost (meaningful for a
Node.js-based CLI vs native binaries). It does **not** measure
time-to-ready of the HTTP gateway (first `200` on `/api/health`), which
is future scope; README claims must not conflate the two.

## Targets

Same pinning as [001-binary-size](001-binary-size.md).

## Expected command

```bash
./run.sh --garraia    # or --openclaw / --zeroclaw / --all
# underlying measurement:
hyperfine --warmup 3 --runs 20 --export-json raw/<target>-hyperfine.json '<bin> --help'
```

## Expected result

`pass` when hyperfine completes 20 runs per target and the JSON export
exists. Records measurements; no threshold gate.

## Minimum evidence

- `raw/<target>-hyperfine.json` — machine-readable, per-run timings
- `raw/<target>-hyperfine.log` — human-readable summary
- `environment.txt`
