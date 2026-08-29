# 001-binary-size

| | |
|---|---|
| **Category** | performance |
| **Status** | active |

## Objective

Prove the size of the artifact each framework actually ships to a user's
machine — a native release binary for GarraIA and ZeroClaw, the installed
npm package for OpenClaw (Node.js has no single binary).

## Targets

| Target | Pinned by | Artifact measured |
|---|---|---|
| GarraIA | current checkout (`git rev-parse HEAD`) | `target/release/garra` (`cargo build --release -p garraia`) |
| OpenClaw | `OPENCLAW_REF` (npm dist-tag or version) | `du -sh` of the installed package in an isolated npm prefix |
| ZeroClaw | `ZEROCLAW_REF` (git tag/branch) | `target/release/zeroclaw` built from a pinned clone |

## Expected command

```bash
./run.sh --garraia    # or --openclaw / --zeroclaw / --all
```

## Expected result

`pass` when the artifact exists and its size is recorded. This scenario
records measurements; it does not gate on a threshold. Any number quoted
in the root `README.md` MUST match the latest committed
`results/<date>-<host>/` for this scenario.

## Minimum evidence

- `raw/garraia-binsize.log` — `ls -lh` output for the binary
- `raw/openclaw-binsize.log` — `du -sh` of the installed package
- `raw/zeroclaw-binsize.log` — `ls -lh` output for the binary
- `environment.txt` — host specs, toolchain versions, pinned refs

A number without its raw log in a committed results directory does not
count as a valid README claim.
