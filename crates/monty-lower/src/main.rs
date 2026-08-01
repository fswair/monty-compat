use std::{
    error::Error,
    fs,
    io::{self, Read},
    path::PathBuf,
};

use clap::Parser;
use monty_compat::{CapabilityIndex, DiagnosticDisposition, lower_source};

#[derive(Debug, Parser)]
#[command(
    name = "monty-lower",
    about = "Lower Python for an exact Monty capability manifest"
)]
struct Args {
    /// Versioned discovery manifest produced by monty-compat-discover.
    #[arg(long)]
    manifest: PathBuf,

    /// Python input path; omit or pass '-' to read stdin.
    #[arg(long, default_value = "-")]
    input: String,

    /// Output path; omit or pass '-' to write lowered Python to stdout.
    #[arg(long, default_value = "-")]
    output: String,

    /// Optional JSON report path containing rule decisions.
    #[arg(long)]
    report: Option<PathBuf>,

    /// Exit with status 2 when a seam needs review or cannot be lowered safely.
    #[arg(long)]
    deny_needs_review: bool,
}

fn main() {
    match run() {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32, Box<dyn Error>> {
    let args = Args::parse();
    let capabilities = CapabilityIndex::from_path(&args.manifest)?;
    let source = read_input(&args.input)?;
    let lowered = lower_source(&source, &capabilities)?;

    if args.output == "-" {
        print!("{}", lowered.code);
    } else {
        fs::write(&args.output, &lowered.code)?;
    }
    if let Some(report) = args.report {
        fs::write(report, serde_json::to_string_pretty(&lowered)? + "\n")?;
    }

    let needs_review = lowered
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.disposition != DiagnosticDisposition::Applied);
    Ok(i32::from(args.deny_needs_review && needs_review) * 2)
}

fn read_input(path: &str) -> io::Result<String> {
    if path == "-" {
        let mut source = String::new();
        io::stdin().read_to_string(&mut source)?;
        Ok(source)
    } else {
        fs::read_to_string(path)
    }
}
