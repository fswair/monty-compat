use std::{env, error::Error, fs, hint::black_box, path::Path, time::Instant};

use monty_compat_extract::{extract_local, extract_zip};

const WARMUP_ITERATIONS: usize = 5;
const SAMPLE_ITERATIONS: usize = 200;

fn samples_ms(
    mut operation: impl FnMut() -> Result<(), Box<dyn Error>>,
    iterations: usize,
) -> Result<Vec<f64>, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        operation()?;
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

fn report(label: &str, samples: &[f64]) {
    let p50 = percentile(samples.to_vec(), 50).unwrap_or_default();
    let p99 = percentile(samples.to_vec(), 99).unwrap_or_default();
    println!("workload={label}");
    println!("samples={}", samples.len());
    println!("p50_ms={p50:.6}");
    println!("median_ms={p50:.6}");
    println!("p99_ms={p99:.6}");
}

fn benchmark_local(root: &Path) -> Result<(), Box<dyn Error>> {
    let mut extract = || {
        let graph = extract_local(black_box(root))?;
        black_box(graph);
        Ok(())
    };
    let _ = samples_ms(&mut extract, WARMUP_ITERATIONS)?;
    let samples = samples_ms(extract, SAMPLE_ITERATIONS)?;
    report("local_source_tree", &samples);
    Ok(())
}

fn benchmark_zip(path: &Path) -> Result<(), Box<dyn Error>> {
    // Disk I/O is deliberately outside the timed region. This workload measures
    // bounded in-memory ZIP validation, decompression, and source scanning.
    let archive = fs::read(path)?;
    println!("archive_bytes={}", archive.len());
    let mut extract = || {
        let graph = extract_zip(black_box(&archive))?;
        black_box(graph);
        Ok(())
    };
    let _ = samples_ms(&mut extract, WARMUP_ITERATIONS)?;
    let samples = samples_ms(extract, SAMPLE_ITERATIONS)?;
    report("in_memory_zip", &samples);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let root = arguments
        .next()
        .ok_or("usage: extraction_bench <monty-repository-root> <monty-source.zip>")?;
    let archive = arguments
        .next()
        .ok_or("usage: extraction_bench <monty-repository-root> <monty-source.zip>")?;
    if arguments.next().is_some() {
        return Err("expected exactly two paths".into());
    }
    benchmark_local(Path::new(&root))?;
    benchmark_zip(Path::new(&archive))?;
    Ok(())
}
