use std::{error::Error, fs, path::PathBuf};

use clap::{ArgGroup, Parser};
use monty_compat_extract::{extract_local, extract_zip, to_json_pretty};

#[derive(Debug, Parser)]
#[command(
    name = "monty-extract",
    about = "Extract Monty's static capability graph from a source tree or ZIP archive",
    group(ArgGroup::new("source").required(true).args(["root", "archive"]))
)]
struct Args {
    /// Root of an exact Monty repository checkout.
    #[arg(long)]
    root: Option<PathBuf>,

    /// GitHub-style Monty source ZIP archive.
    #[arg(long)]
    archive: Option<PathBuf>,

    /// Optional output file; stdout is used when omitted.
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let graph = match (args.root, args.archive) {
        (Some(root), None) => extract_local(root)?,
        (None, Some(archive)) => extract_zip(&fs::read(archive)?)?,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "exactly one of --root or --archive is required",
            )
            .into());
        }
    };
    let json = to_json_pretty(&graph)?;
    if let Some(output) = args.output {
        fs::write(output, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}
