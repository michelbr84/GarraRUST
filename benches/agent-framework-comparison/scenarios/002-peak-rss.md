# 002-peak-rss

| | |
|---|---|
| **Category** | performance |
| **Status** | active |

## Objective

Prove the minimum memory footprint of each framework's CLI by measuring
peak resident set size (RSS) during a `--help` invocation.

This is an honest *floor*, not "idle memory": measuring a running server's
idle RSS requires bringing the full gateway up with pinned configs for all
three frameworks and sampling over time — that is future scope. Until
then, no README claim may present this number as "idle RAM".

## Targets

Same pinning as [001-binary-size](001-binary-size.md).

## Expected command

```bash
./run.sh --garraia    # or --openclaw / --zeroclaw / --all
# underlying measurement:
/usr/bin/time -v <bin> --help 2> raw/<target>-time.log
```

## Expected result

`pass` when `Maximum resident set size` is captured for each target.
Records measurements; no threshold gate.

## Minimum evidence

- `raw/<target>-time.log` — full GNU time `-v` output (one per target)
- `environment.txt`
