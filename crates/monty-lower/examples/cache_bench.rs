use std::{error::Error, fmt::Write as _, hint::black_box, time::Instant};

use monty_compat::{CacheConfig, CapabilityIndex, Transpiler};

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");
const TARGET_BYTES: usize = 100 * 1024;

fn lowering_heavy_source() -> Result<String, std::fmt::Error> {
    let mut source = String::with_capacity(TARGET_BYTES + 256);
    let mut index = 0usize;
    while source.len() < TARGET_BYTES {
        write!(
            source,
            "value_{index} = {value}\nmatch value_{index}:\n    case 0:\n        result_{index} = 'zero'\n    case 1:\n        result_{index} = 'one'\n    case _:\n        result_{index} = 'other'\n",
            value = index % 3,
        )?;
        index += 1;
    }
    writeln!(source, "result_{}", index.saturating_sub(1))?;
    Ok(source)
}

fn supported_source() -> Result<String, std::fmt::Error> {
    let mut source = String::with_capacity(TARGET_BYTES + 128);
    let mut index = 0usize;
    while source.len() < TARGET_BYTES {
        write!(
            source,
            "value_{index} = {index}\nresult_{index} = value_{index} + 1\n"
        )?;
        index += 1;
    }
    writeln!(source, "result_{}", index.saturating_sub(1))?;
    Ok(source)
}

fn samples_ms(
    transpiler: &Transpiler,
    source: &str,
    iterations: usize,
) -> Result<Vec<f64>, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let output = transpiler.transpile(black_box(source))?;
        black_box(output);
        samples.push(start.elapsed().as_secs_f64() * 1_000.0);
    }
    Ok(samples)
}

fn percentile(mut samples: Vec<f64>, percentile: usize) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    let index = (samples.len() - 1).saturating_mul(percentile) / 100;
    samples.get(index).copied()
}

fn benchmark(
    label: &str,
    source: &str,
    capabilities: &CapabilityIndex,
) -> Result<(), Box<dyn Error>> {
    let uncached = Transpiler::with_cache_config(capabilities.clone(), CacheConfig::disabled());
    let cached =
        Transpiler::with_cache_config(capabilities.clone(), CacheConfig::new(8, 64 * 1024 * 1024));

    // Warm parser/allocator and instruction-cache effects before collecting
    // distribution samples. The cache remains disabled for this workload.
    let _ = samples_ms(&uncached, source, 5)?;
    let uncached_samples = samples_ms(&uncached, source, 200)?;
    let miss_samples = samples_ms(&cached, source, 1)?;
    let hit_samples = samples_ms(&cached, source, 20_000)?;
    let stats = cached.cache_stats();

    let uncached_p50 = percentile(uncached_samples.clone(), 50).unwrap_or_default();
    let uncached_median = uncached_p50;
    let uncached_p99 = percentile(uncached_samples, 99).unwrap_or_default();
    let miss = miss_samples.first().copied().unwrap_or_default();
    let hit_p50 = percentile(hit_samples.clone(), 50).unwrap_or_default();
    let hit_median = hit_p50;
    let hit_p99 = percentile(hit_samples, 99).unwrap_or_default();
    let speedup = if hit_median > 0.0 {
        uncached_median / hit_median
    } else {
        0.0
    };

    println!("workload={label}");
    println!("source_bytes={}", source.len());
    println!("uncached_samples=200");
    println!("uncached_p50_ms={uncached_p50:.6}");
    println!("uncached_median_ms={uncached_median:.6}");
    println!("uncached_p99_ms={uncached_p99:.6}");
    println!("cache_miss_ms={miss:.6}");
    println!("cache_hit_samples=20000");
    println!("cache_hit_p50_ms={hit_p50:.6}");
    println!("cache_hit_median_ms={hit_median:.6}");
    println!("cache_hit_p99_ms={hit_p99:.6}");
    println!("median_speedup={speedup:.2}x");
    println!(
        "cache_hits={} cache_misses={} entries={} bytes={}",
        stats.hits, stats.misses, stats.entries, stats.bytes
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    benchmark("supported_noop", &supported_source()?, &capabilities)?;
    benchmark(
        "lowering_heavy_match",
        &lowering_heavy_source()?,
        &capabilities,
    )?;
    Ok(())
}
