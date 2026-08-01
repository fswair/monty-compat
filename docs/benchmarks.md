# Benchmarks

This report records release-mode latency for the public lowering and extraction
paths. It is a reproducible local snapshot, not a cross-machine performance
guarantee.

## Contents

- [Environment](#environment)
- [Methodology](#methodology)
- [Transpilation results](#transpilation-results)
- [Extraction results](#extraction-results)
- [Interpretation](#interpretation)
- [Reproduction](#reproduction)

## Environment

| Property | Value |
|---|---|
| Date | 2026-08-01 |
| Machine | MacBook Air (`MacBookAir10,1`) |
| CPU | Apple M1 |
| Memory | 8 GB |
| Architecture | arm64 |
| macOS | 26.1 |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)` |
| Monty source | exact `v0.0.19` release |
| Build profile | Cargo `--release`, incremental compilation disabled |

## Methodology

- Durations use `std::time::Instant` around one public API call.
- Five warm-up calls run before distribution sampling.
- Percentiles use sorted nearest-rank samples at indexes derived from
  `(n - 1) * percentile / 100`.
- `p50` and median are mathematically identical; both columns are retained to
  match requested release-report terminology.
- Source inputs are wrapped in `std::hint::black_box`.
- Transpilation uses the committed `monty-v0.0.19.json` manifest.
- Extraction scans the same exact Monty 0.0.19 source in two forms.
- In-memory ZIP timing excludes the initial archive read from disk and includes
  validation, decompression, source loading, and scanning.
- Network release resolution/download is not measured because remote latency is
  not a stable property of the extractor.

## Transpilation results

Both generated workloads are approximately 100 KiB.

| Workload | Source bytes | Samples | p50 | Median | p99 |
|---|---:|---:|---:|---:|---:|
| Supported/no-op, cache disabled | 102,450 | 200 | 4.223750 ms | 4.223750 ms | 5.660708 ms |
| Supported/no-op, exact cache hit | 102,450 | 20,000 | 0.031667 ms | 0.031667 ms | 0.042417 ms |
| Match-heavy lowering, cache disabled | 102,443 | 200 | 30.957000 ms | 30.957000 ms | 32.101750 ms |
| Match-heavy lowering, exact cache hit | 102,443 | 20,000 | 0.031625 ms | 0.031625 ms | 0.042875 ms |

The first cache population calls measured `4.210708 ms` for the supported
workload and `30.831666 ms` for the match-heavy workload. Those are single
observations and therefore are not reported as p50/p99 distributions.

Median exact-hit speedups in this run:

| Workload | Cache-disabled median | Cache-hit median | Speedup |
|---|---:|---:|---:|
| Supported/no-op | 4.223750 ms | 0.031667 ms | 133.38× |
| Match-heavy | 30.957000 ms | 0.031625 ms | 978.88× |

An exact cache hit still hashes/looks up the complete source string and clones
an `Arc`; its cost is therefore not constant with respect to source size.

## Extraction results

The ZIP archive is 2,356,205 bytes. Both operations produce the complete static
capability graph.

| Workload | Samples | p50 | Median | p99 |
|---|---:|---:|---:|---:|
| Local Monty source tree | 200 | 113.419291 ms | 113.419291 ms | 132.644083 ms |
| In-memory GitHub-style ZIP | 200 | 119.257042 ms | 119.257042 ms | 136.816000 ms |

Local extraction includes filesystem reads. ZIP extraction reads the archive
once before sampling, then includes bounded in-memory ZIP validation,
decompression, UTF-8 decoding, and static scanning in every timed iteration.

## Interpretation

- Ordinary supported 100 KiB source lowers in roughly 4.2 ms at p50 on this
  machine.
- A rule-dense source costs more because match lowering performs multiple
  evidence-gated edit passes and reinserts/parses generated source.
- Exact-source caching reduces repeated calls to tens of microseconds.
- Static extraction is a release/discovery operation around 0.11–0.12 seconds,
  not a request-path operation.
- Python `transpiler(...)` embeds the manifest and performs no extraction.

OS scheduling, thermal state, allocator behavior, source composition, compiler
version, and target CPU affect these numbers. Compare results only when the
benchmark environment and workload definitions match.

## Reproduction

Transpilation and cache:

```bash
CARGO_INCREMENTAL=0 \
cargo +1.95.0 run --release --locked \
  -p monty-compat --example cache_bench
```

Extraction from an exact checkout and its GitHub-style ZIP:

```bash
CARGO_INCREMENTAL=0 \
cargo +1.95.0 run --release --locked \
  -p monty-compat-extract --example extraction_bench -- \
  /path/to/monty-0.0.19 \
  /path/to/monty-v0.0.19.zip
```

Benchmark source is versioned in:

- `crates/monty-lower/examples/cache_bench.rs`
- `crates/monty-extract/examples/extraction_bench.rs`
