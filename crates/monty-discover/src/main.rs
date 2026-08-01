use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Parser;
use monty_compat_discover::{
    GeneratedProbeConfig, LINKED_MONTY_VERSION, MinimizationConfig, PythonWorker,
    run_baseline_probes, run_generated_discovery,
};
use monty_compat_extract::{extract_release, resolve_release};

const RELEASE_DEFAULT_SEEDS: u64 = 1_000;

#[derive(Debug, Parser)]
#[command(
    name = "monty-discover",
    about = "Run generated capability probes with Rust orchestration"
)]
struct Args {
    /// Build a complete manifest for `latest`, `0.0.19`, or `v0.0.19`.
    #[arg(long)]
    release: Option<String>,

    #[arg(long)]
    baseline: bool,

    #[arg(long, default_value = "python3")]
    python: String,

    #[arg(long, default_value_t = 0)]
    seed_start: u64,

    /// Generated sources to probe. Release pipelines default to 1,000.
    #[arg(long, default_value_t = 0)]
    seeds: u64,

    #[arg(long, default_value_t = 100)]
    node_limit: usize,

    #[arg(long, default_value_t = 5)]
    depth_limit: usize,

    #[arg(long)]
    no_minimize: bool,

    #[arg(long, default_value_t = 1_000)]
    minimizer_max_checks: u64,

    #[arg(long, default_value_t = 10_000)]
    worker_timeout_ms: u64,

    #[arg(long)]
    output: Option<PathBuf>,

    /// Reproducible ISO-8601 manifest timestamp; defaults to the oracle clock.
    #[arg(long)]
    generated_at: Option<String>,
}

fn main() {
    if std::env::args().any(|argument| argument == "--monty-worker") {
        if let Err(error) = monty_compat_discover::run_monty_worker_stdio() {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args.release.is_some() {
        return run_release_pipeline(&args);
    }
    if !args.baseline && args.seeds == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "enable --baseline, request --seeds, or use both",
        )
        .into());
    }
    let executable = std::env::current_exe()?;
    let config = GeneratedProbeConfig {
        seed_start: args.seed_start,
        seed_count: args.seeds,
        node_limit: args.node_limit,
        depth_limit: args.depth_limit,
        ..GeneratedProbeConfig::default()
    };
    let timeout = Duration::from_millis(args.worker_timeout_ms);
    let mut output = if args.baseline {
        serde_json::to_value(run_baseline_probes(
            &args.python,
            &executable,
            timeout,
            timeout,
        )?)?
    } else {
        serde_json::Value::Null
    };
    if args.seeds > 0 {
        let generated = run_generated_discovery(
            &config,
            &MinimizationConfig {
                enabled: !args.no_minimize,
                max_checks: args.minimizer_max_checks,
            },
            &args.python,
            &executable,
            timeout,
            timeout,
        )?;
        if args.baseline {
            let Some(object) = output.as_object_mut() else {
                return Err(std::io::Error::other("behavioral report is not a JSON object").into());
            };
            object.insert(
                "generated_corpus".to_owned(),
                serde_json::to_value(generated.generated_corpus)?,
            );
            object.insert(
                "minimized_failures".to_owned(),
                serde_json::to_value(generated.minimized_failures)?,
            );
            object.insert(
                "promotion_candidates".to_owned(),
                serde_json::to_value(generated.promotion_candidates)?,
            );
        } else {
            output = serde_json::to_value(generated)?;
        }
    }
    let json = serde_json::to_string_pretty(&output)? + "\n";
    if let Some(output) = args.output {
        write_atomic(&output, &json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}

fn run_release_pipeline(args: &Args) -> Result<(), Box<dyn Error>> {
    let requested = args.release.as_deref().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "release is missing")
    })?;
    let release = resolve_release(requested)?;
    if release.runtime_version != LINKED_MONTY_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "release {} resolves to Monty {}, but this binary links Monty {}; update the exact workspace dependency and rebuild before probing",
                release.tag, release.runtime_version, LINKED_MONTY_VERSION
            ),
        )
        .into());
    }

    let executable = std::env::current_exe()?;
    let timeout = Duration::from_millis(args.worker_timeout_ms);
    let mut environment_worker = PythonWorker::start(&args.python, timeout)?;
    let environment = environment_worker.environment_info()?;
    drop(environment_worker);
    let static_capabilities = extract_release(&release)?;
    let baseline = run_baseline_probes(&args.python, &executable, timeout, timeout)?;
    let config = GeneratedProbeConfig {
        seed_start: args.seed_start,
        seed_count: if args.seeds == 0 {
            RELEASE_DEFAULT_SEEDS
        } else {
            args.seeds
        },
        node_limit: args.node_limit,
        depth_limit: args.depth_limit,
        ..GeneratedProbeConfig::default()
    };
    let generated = run_generated_discovery(
        &config,
        &MinimizationConfig {
            enabled: !args.no_minimize,
            max_checks: args.minimizer_max_checks,
        },
        &args.python,
        &executable,
        timeout,
        timeout,
    )?;
    let mut behavioral = serde_json::to_value(baseline)?;
    let Some(behavioral) = behavioral.as_object_mut() else {
        return Err(std::io::Error::other("behavioral report is not a JSON object").into());
    };
    behavioral.insert(
        "generated_corpus".to_owned(),
        serde_json::to_value(generated.generated_corpus)?,
    );
    behavioral.insert(
        "minimized_failures".to_owned(),
        serde_json::to_value(generated.minimized_failures)?,
    );
    behavioral.insert(
        "promotion_candidates".to_owned(),
        serde_json::to_value(generated.promotion_candidates)?,
    );

    let manifest = serde_json::json!({
        "schema_version": 2,
        "generated_at": args.generated_at.as_ref().unwrap_or(&environment.generated_at),
        "target": {
            "repository": "pydantic/monty",
            "tag": release.tag,
            "runtime_distribution": "pydantic-monty",
            "runtime_version": release.runtime_version,
            "published_at": release.published_at,
            "release_url": release.release_url,
            "platform": environment.platform,
            "build_features": [],
        },
        "oracle": {
            "implementation": environment.implementation,
            "version": environment.python_version,
        },
        "static_capabilities": static_capabilities,
        "behavioral_capabilities": behavioral,
    });
    let json = serde_json::to_string_pretty(&manifest)? + "\n";
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("manifests/monty-v{LINKED_MONTY_VERSION}.json")));
    write_atomic(&output, &json)?;
    eprintln!(
        "wrote exact Monty {} manifest to {}",
        LINKED_MONTY_VERSION,
        output.display()
    );
    Ok(())
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output has no UTF-8 file name",
            )
        })?;
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if let Err(error) = (|| -> std::io::Result<()> {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })() {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}
