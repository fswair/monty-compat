use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let lockfile = manifest_dir.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lockfile.display());
    let contents = fs::read_to_string(&lockfile)?;
    let version = package_version(&contents, "monty").ok_or("Cargo.lock has no monty package")?;
    println!("cargo:rustc-env=MONTY_COMPAT_LINKED_MONTY_VERSION={version}");
    Ok(())
}

fn package_version<'a>(lockfile: &'a str, package_name: &str) -> Option<&'a str> {
    lockfile.split("[[package]]").find_map(|block| {
        let mut name = None;
        let mut version = None;
        for line in block.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("name = ") {
                name = quoted(value);
            } else if let Some(value) = line.strip_prefix("version = ") {
                version = quoted(value);
            }
        }
        (name == Some(package_name)).then_some(version).flatten()
    })
}

fn quoted(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

#[cfg(test)]
mod tests {
    use super::package_version;

    #[test]
    fn finds_an_exact_package_block() {
        let lock = r#"[[package]]
name = "other"
version = "9.0.0"

[[package]]
name = "monty"
version = "0.0.19"
"#;
        assert_eq!(package_version(lock, "monty"), Some("0.0.19"));
    }
}
