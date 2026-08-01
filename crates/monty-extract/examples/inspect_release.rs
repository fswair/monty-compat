use std::{env, error::Error};

use monty_compat_extract::{extract_release, resolve_release};

fn main() -> Result<(), Box<dyn Error>> {
    let release = env::args().nth(1).unwrap_or_else(|| "latest".to_owned());
    let metadata = resolve_release(&release)?;
    let graph = extract_release(&metadata)?;

    println!("release: {}", metadata.tag);
    println!("runtime version: {}", metadata.runtime_version);
    println!("builtins: {}", graph.builtin_functions.len());
    println!("modules: {}", graph.modules.len());
    println!("runtime types: {}", graph.type_attributes.len());

    if let Some(attributes) = graph.type_attributes.get("pathlib.Path") {
        println!("pathlib.Path attributes: {}", attributes.len());
        println!("pathlib.Path.is_dir: {}", attributes.contains("is_dir"));
    }
    Ok(())
}
