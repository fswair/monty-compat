use std::error::Error;

use monty_compat::{CapabilityIndex, DiagnosticDisposition, Transpiler};

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");
const SOURCE: &str = "value = 2\nmatch value:\n    case 2:\n        result = 'two'\n    case _:\n        result = 'other'\nresult\n";

fn main() -> Result<(), Box<dyn Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let transpiler = Transpiler::new(capabilities);
    let output = transpiler.transpile(SOURCE)?;

    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.disposition != DiagnosticDisposition::Applied)
    {
        return Err("source contains a seam that cannot be lowered safely".into());
    }

    println!("target: {}", output.target_tag);
    println!("changed: {}", output.changed);
    println!("diagnostics: {}", output.diagnostics.len());
    println!("\n{}", output.code);
    Ok(())
}
