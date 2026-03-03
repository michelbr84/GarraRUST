//! GAR-266: Benchmark suite — glob matching at 10k, 100k, 200k paths.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use garraia_glob::pattern::{GlobConfig, GlobMode, GlobPattern};

// ── Test data generators ──────────────────────────────────────────────────

/// Generate `n` realistic repo-like paths.
fn make_paths(n: usize) -> Vec<String> {
    let prefixes = ["src", "tests", "benches", "crates/foo/src", "crates/bar/src"];
    let suffixes = ["rs", "toml", "md", "json", "txt", "lock", "sh"];
    let mut paths = Vec::with_capacity(n);
    for i in 0..n {
        let prefix = prefixes[i % prefixes.len()];
        let suffix = suffixes[i % suffixes.len()];
        paths.push(format!("{prefix}/module_{i}/file_{i}.{suffix}"));
    }
    paths
}

fn compile(pat: &str, mode: GlobMode) -> GlobPattern {
    GlobPattern::new(pat, &GlobConfig { mode, ..GlobConfig::default() }).unwrap()
}

// ── Benchmarks ───────────────────────────────────────────────────────────

fn bench_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_star");
    for n in [10_000usize, 100_000, 200_000] {
        let paths = make_paths(n);
        let pat = compile("**/*.rs", GlobMode::Picomatch);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("picomatch", n), &paths, |b, paths| {
            b.iter(|| {
                let mut count = 0usize;
                for p in paths {
                    if pat.matches(black_box(p)) {
                        count += 1;
                    }
                }
                black_box(count)
            });
        });
    }
    group.finish();
}

fn bench_extglob_bang(c: &mut Criterion) {
    let mut group = c.benchmark_group("extglob_bang");
    for n in [10_000usize, 100_000, 200_000] {
        let paths = make_paths(n);
        let pat = compile("!(*.toml)", GlobMode::Picomatch);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("picomatch_safe", n), &paths, |b, paths| {
            b.iter(|| {
                let mut count = 0usize;
                for p in paths {
                    if pat.matches(black_box(p)) {
                        count += 1;
                    }
                }
                black_box(count)
            });
        });

        let pat_bash = GlobPattern::new(
            "!(*.toml)",
            &GlobConfig {
                mode: GlobMode::Bash,
                bash_greedy_negated_extglob: true,
                ..GlobConfig::default()
            },
        ).unwrap();
        group.bench_with_input(BenchmarkId::new("bash_greedy", n), &paths, |b, paths| {
            b.iter(|| {
                let mut count = 0usize;
                for p in paths {
                    if pat_bash.matches(black_box(p)) {
                        count += 1;
                    }
                }
                black_box(count)
            });
        });
    }
    group.finish();
}

fn bench_brace_expansion(c: &mut Criterion) {
    let mut group = c.benchmark_group("brace_expansion");
    for n in [10_000usize, 100_000] {
        let paths = make_paths(n);
        let pat = compile("**/*.{rs,toml,md}", GlobMode::Picomatch);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("picomatch", n), &paths, |b, paths| {
            b.iter(|| {
                let mut count = 0usize;
                for p in paths {
                    if pat.matches(black_box(p)) {
                        count += 1;
                    }
                }
                black_box(count)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_simple, bench_extglob_bang, bench_brace_expansion);
criterion_main!(benches);
