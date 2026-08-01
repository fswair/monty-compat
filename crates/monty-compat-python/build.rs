use std::{env, error::Error, fmt::Write as _, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let manifests = manifest_dir.join("../../manifests");
    println!("cargo:rerun-if-changed={}", manifests.display());
    let mut releases = Vec::new();
    for entry in fs::read_dir(&manifests)? {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(version) = file_name
            .strip_prefix("monty-v")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let order = parse_version(version).ok_or("manifest file has an invalid numeric version")?;
        let contents = fs::read_to_string(&path)?;
        let manifest: serde_json::Value = serde_json::from_str(&contents)?;
        let runtime_version = manifest
            .pointer("/target/runtime_version")
            .and_then(serde_json::Value::as_str)
            .ok_or("manifest has no target.runtime_version string")?;
        if runtime_version != version {
            return Err(format!(
                "manifest {file_name} targets runtime {runtime_version:?}, expected {version:?}"
            )
            .into());
        }
        println!("cargo:rerun-if-changed={}", path.display());
        releases.push((order, version.to_owned(), file_name.to_owned()));
    }
    releases.sort_by(|left, right| left.0.cmp(&right.0));
    let verified = releases
        .last()
        .ok_or("no bundled Monty manifests found")?
        .1
        .clone();
    let mut generated = format!("pub const VERIFIED_RELEASE: &str = {verified:?};\n");
    generated.push_str("pub const RELEASE_MANIFESTS: &[(&str, &str)] = &[\n");
    for (_, version, file_name) in releases {
        writeln!(
            generated,
            "    ({version:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../manifests/{file_name}\"))),"
        )?;
    }
    generated.push_str("];\n");
    let output = PathBuf::from(env::var("OUT_DIR")?).join("manifest_registry.rs");
    fs::write(output, generated)?;
    Ok(())
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let parts = version
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() >= 3).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn accepts_numeric_releases_only() {
        assert_eq!(parse_version("0.0.19"), Some(vec![0, 0, 19]));
        assert_eq!(parse_version("0.0.rc1"), None);
    }
}
