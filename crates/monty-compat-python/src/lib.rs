//! Private `PyO3` boundary for the public `monty_compat.transpiler` function.

use std::{
    borrow::Cow,
    collections::HashMap,
    env, fmt,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use monty_compat::{
    CapabilityIndex, DiagnosticDisposition, LoweringDiagnostic, Transpiler, lowering_coverage,
};
use monty_compat_extract::{
    download_limited, extract_github, extract_local, extract_zip, to_json_pretty,
};
use pyo3::{
    Bound, PyResult, Python, create_exception,
    exceptions::PyException,
    prelude::PyModule,
    pyfunction, pymodule,
    types::{PyBytes, PyBytesMethods, PyModuleMethods},
    wrap_pyfunction,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/manifest_registry.rs"));

const DEFAULT_MANIFEST_CHANNEL_URL: &str =
    "https://fswair.github.io/monty-compat/manifest-channel.json";
const MANIFEST_CHANNEL_URL_ENV: &str = "MONTY_COMPAT_MANIFEST_CHANNEL_URL";
const MAX_CHANNEL_BYTES: usize = 256 * 1024;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const CHANNEL_SCHEMA_VERSION: u8 = 1;

static TRANSPILERS: OnceLock<Mutex<HashMap<String, Arc<Transpiler>>>> = OnceLock::new();
static REMOTE_LATEST: OnceLock<Mutex<Option<RemoteManifest>>> = OnceLock::new();

create_exception!(_native, TranspilationError, PyException);
create_exception!(_native, ExtractionError, PyException);

#[derive(Debug)]
enum BindingError {
    UnsupportedRelease(String),
    Latest(String),
    Manifest(String),
    Lowering(String),
    UnsafeDiagnostics(Vec<LoweringDiagnostic>),
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRelease(release) => write!(
                formatter,
                "Monty release {release:?} is not bundled; available bundled releases: {}",
                RELEASE_MANIFESTS
                    .iter()
                    .map(|(version, _)| *version)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Latest(message) => write!(formatter, "cannot resolve latest manifest: {message}"),
            Self::Manifest(message) => write!(formatter, "cannot load Monty manifest: {message}"),
            Self::Lowering(message) => {
                write!(formatter, "cannot transpile Python source: {message}")
            }
            Self::UnsafeDiagnostics(diagnostics) => {
                write!(formatter, "Python semantics cannot be preserved")?;
                for diagnostic in diagnostics {
                    write!(
                        formatter,
                        "; {} [{}] at bytes {}..{}: {}",
                        diagnostic.rule,
                        disposition_name(diagnostic.disposition),
                        diagnostic.start,
                        diagnostic.end,
                        diagnostic.message
                    )?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ManifestChannel {
    schema_version: u8,
    latest: ChannelRelease,
}

#[derive(Debug, Deserialize)]
struct ChannelRelease {
    release: String,
    manifest_url: String,
    sha256: String,
    compatible_transpilers: Vec<String>,
}

#[derive(Clone)]
struct RemoteManifest {
    release: String,
    source: String,
}

struct ManifestSelection {
    release: String,
    source: Cow<'static, str>,
}

fn disposition_name(disposition: DiagnosticDisposition) -> &'static str {
    match disposition {
        DiagnosticDisposition::Applied => "applied",
        DiagnosticDisposition::NeedsReview => "needs_review",
        DiagnosticDisposition::NotLowerable => "not_lowerable",
    }
}

fn embedded_manifest(release: &str) -> Result<ManifestSelection, BindingError> {
    let normalized = release
        .strip_prefix('v')
        .filter(|version| is_numeric_release(version))
        .unwrap_or(release);
    let resolved = if normalized == "verified" {
        VERIFIED_RELEASE
    } else {
        normalized
    };
    RELEASE_MANIFESTS
        .iter()
        .find(|(version, _)| *version == resolved)
        .copied()
        .map(|(version, source)| ManifestSelection {
            release: version.to_owned(),
            source: Cow::Borrowed(source),
        })
        .ok_or_else(|| BindingError::UnsupportedRelease(release.to_owned()))
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Transpiler>>> {
    TRANSPILERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_registry() -> MutexGuard<'static, HashMap<String, Arc<Transpiler>>> {
    match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn remote_latest_cache() -> &'static Mutex<Option<RemoteManifest>> {
    REMOTE_LATEST.get_or_init(|| Mutex::new(None))
}

fn lock_remote_latest() -> MutexGuard<'static, Option<RemoteManifest>> {
    match remote_latest_cache().lock() {
        Ok(cache) => cache,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn is_numeric_release(release: &str) -> bool {
    let mut parts = release.split('.');
    let valid = parts
        .by_ref()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    valid && release.matches('.').count() >= 2
}

fn validate_latest_documents(
    channel_source: &str,
    manifest_bytes: &[u8],
) -> Result<RemoteManifest, BindingError> {
    let channel: ManifestChannel = serde_json::from_str(channel_source)
        .map_err(|error| BindingError::Latest(format!("invalid channel JSON: {error}")))?;
    if channel.schema_version != CHANNEL_SCHEMA_VERSION {
        return Err(BindingError::Latest(format!(
            "channel schema {} is unsupported; expected {CHANNEL_SCHEMA_VERSION}",
            channel.schema_version
        )));
    }
    let latest = channel.latest;
    if !is_numeric_release(&latest.release) {
        return Err(BindingError::Latest(format!(
            "channel release {:?} is not a numeric Monty version",
            latest.release
        )));
    }
    if !latest
        .compatible_transpilers
        .iter()
        .any(|version| version == env!("CARGO_PKG_VERSION"))
    {
        return Err(BindingError::Latest(format!(
            "Monty {} requires a newer compatible monty-compat wheel; this engine is {}",
            latest.release,
            env!("CARGO_PKG_VERSION")
        )));
    }
    if !latest.manifest_url.starts_with("https://") {
        return Err(BindingError::Latest(
            "manifest URL must use HTTPS".to_owned(),
        ));
    }
    let actual_hash = sha256_hex(manifest_bytes);
    if actual_hash != latest.sha256.to_ascii_lowercase() {
        return Err(BindingError::Latest(format!(
            "manifest SHA-256 mismatch for Monty {}",
            latest.release
        )));
    }
    let manifest_source = std::str::from_utf8(manifest_bytes)
        .map_err(|error| BindingError::Latest(format!("manifest is not UTF-8: {error}")))?;
    let capabilities = CapabilityIndex::from_json(manifest_source)
        .map_err(|error| BindingError::Latest(format!("invalid manifest: {error}")))?;
    let target = capabilities.target();
    let target_version = target
        .runtime_version
        .as_deref()
        .unwrap_or_else(|| target.tag.strip_prefix('v').unwrap_or(&target.tag));
    if target_version != latest.release {
        return Err(BindingError::Latest(format!(
            "channel says Monty {}, but manifest targets {target_version}",
            latest.release
        )));
    }
    let known_features: std::collections::HashSet<_> = lowering_coverage()
        .iter()
        .map(|entry| entry.feature)
        .collect();
    let unknown_feature = capabilities
        .feature_statuses()
        .find_map(|(feature, status)| {
            (status != "supported" && !known_features.contains(feature)).then_some(feature)
        });
    if let Some(feature) = unknown_feature {
        return Err(BindingError::Latest(format!(
            "manifest contains unsupported feature {feature:?} unknown to this lowering engine"
        )));
    }
    Ok(RemoteManifest {
        release: latest.release,
        source: manifest_source.to_owned(),
    })
}

fn download_latest_manifest() -> Result<RemoteManifest, BindingError> {
    if let Some(cached) = lock_remote_latest().clone() {
        return Ok(cached);
    }

    let channel_url = env::var(MANIFEST_CHANNEL_URL_ENV)
        .unwrap_or_else(|_| DEFAULT_MANIFEST_CHANNEL_URL.to_owned());
    let channel_bytes = download_limited(&channel_url, "application/json", MAX_CHANNEL_BYTES)
        .map_err(|error| BindingError::Latest(error.to_string()))?;
    let channel_source = std::str::from_utf8(&channel_bytes)
        .map_err(|error| BindingError::Latest(format!("channel is not UTF-8: {error}")))?;
    let channel: ManifestChannel = serde_json::from_str(channel_source)
        .map_err(|error| BindingError::Latest(format!("invalid channel JSON: {error}")))?;
    let manifest_bytes = download_limited(
        &channel.latest.manifest_url,
        "application/json",
        MAX_MANIFEST_BYTES,
    )
    .map_err(|error| BindingError::Latest(error.to_string()))?;
    let latest = validate_latest_documents(channel_source, &manifest_bytes)?;

    let mut cache = lock_remote_latest();
    Ok(cache.get_or_insert_with(|| latest.clone()).clone())
}

fn release_manifest(release: &str) -> Result<ManifestSelection, BindingError> {
    if release == "latest" {
        let latest = download_latest_manifest()?;
        Ok(ManifestSelection {
            release: latest.release,
            source: Cow::Owned(latest.source),
        })
    } else {
        embedded_manifest(release)
    }
}

fn release_transpiler(release: &str) -> Result<Arc<Transpiler>, BindingError> {
    let selection = release_manifest(release)?;
    if let Some(transpiler) = lock_registry().get(&selection.release).cloned() {
        return Ok(transpiler);
    }

    // Build outside the registry lock. Concurrent first calls may duplicate
    // manifest parsing, but the first inserted Arc becomes canonical.
    let candidate = Arc::new(
        Transpiler::from_manifest_json(&selection.source)
            .map_err(|error| BindingError::Manifest(error.to_string()))?,
    );
    let mut registry = lock_registry();
    Ok(Arc::clone(
        registry.entry(selection.release).or_insert(candidate),
    ))
}

fn transpile_impl(code: &str, release: &str) -> Result<String, BindingError> {
    let transpiler = release_transpiler(release)?;
    let output = transpiler
        .transpile(code)
        .map_err(|error| BindingError::Lowering(error.to_string()))?;
    let unsafe_diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.disposition != DiagnosticDisposition::Applied)
        .cloned()
        .collect();
    if unsafe_diagnostics.is_empty() {
        Ok(output.code.clone())
    } else {
        Err(BindingError::UnsafeDiagnostics(unsafe_diagnostics))
    }
}

/// Transpile Python source for a verified, latest, or exact Monty release.
#[pyfunction(signature = (code, release = "verified"))]
fn transpiler(py: Python<'_>, code: &str, release: &str) -> PyResult<String> {
    let code = code.to_owned();
    let release = release.to_owned();
    py.detach(move || transpile_impl(&code, &release))
        .map_err(|error| TranspilationError::new_err(error.to_string()))
}

/// Private bridge used while Python's source scanner remains the parity oracle.
#[pyfunction]
fn _extract_local_json(py: Python<'_>, root: PathBuf) -> PyResult<String> {
    py.detach(move || extract_local(root).and_then(|graph| to_json_pretty(&graph)))
        .map_err(|error| ExtractionError::new_err(error.to_string()))
}

/// Private bridge for in-memory GitHub-style source archives.
#[pyfunction]
fn _extract_archive_json(py: Python<'_>, archive: &Bound<'_, PyBytes>) -> PyResult<String> {
    let archive = archive.as_bytes().to_owned();
    py.detach(move || extract_zip(&archive).and_then(|graph| to_json_pretty(&graph)))
        .map_err(|error| ExtractionError::new_err(error.to_string()))
}

/// Private bridge for bounded native HTTP download and source extraction.
#[pyfunction]
fn _extract_github_json(py: Python<'_>, url: &str, only_released: bool) -> PyResult<String> {
    let url = url.to_owned();
    py.detach(move || extract_github(&url, only_released).and_then(|graph| to_json_pretty(&graph)))
        .map_err(|error| ExtractionError::new_err(error.to_string()))
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(transpiler, module)?)?;
    module.add_function(wrap_pyfunction!(_extract_local_json, module)?)?;
    module.add_function(wrap_pyfunction!(_extract_archive_json, module)?)?;
    module.add_function(wrap_pyfunction!(_extract_github_json, module)?)?;
    module.add(
        "TranspilationError",
        module.py().get_type::<TranspilationError>(),
    )?;
    module.add("ExtractionError", module.py().get_type::<ExtractionError>())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_share_one_release_transpiler() {
        let verified = release_transpiler("verified").expect("bundled manifest should load");
        let version = release_transpiler("v0.0.19").expect("bundled manifest should load");
        assert!(Arc::ptr_eq(&verified, &version));
    }

    fn channel_for(release: &str, manifest: &[u8], compatible: &[&str]) -> String {
        serde_json::json!({
            "schema_version": CHANNEL_SCHEMA_VERSION,
            "latest": {
                "release": release,
                "manifest_url": format!("https://example.test/monty-v{release}.json"),
                "sha256": sha256_hex(manifest),
                "compatible_transpilers": compatible,
            }
        })
        .to_string()
    }

    #[test]
    fn accepts_hash_checked_engine_compatible_latest_manifest() {
        let manifest = include_bytes!("../../../manifests/monty-v0.0.19.json");
        let channel = channel_for("0.0.19", manifest, &[env!("CARGO_PKG_VERSION")]);
        let latest = validate_latest_documents(&channel, manifest)
            .expect("published manifest should validate");
        assert_eq!(latest.release, "0.0.19");
    }

    #[test]
    fn committed_channel_matches_the_published_manifest() {
        let channel = include_str!("../../../manifests/channel.json");
        let manifest = include_bytes!("../../../manifests/monty-v0.0.19.json");
        let latest = validate_latest_documents(channel, manifest)
            .expect("committed channel and manifest should stay in lockstep");
        assert_eq!(latest.release, VERIFIED_RELEASE);
    }

    #[test]
    fn rejects_latest_manifest_with_wrong_hash() {
        let manifest = include_bytes!("../../../manifests/monty-v0.0.19.json");
        let mut channel: serde_json::Value = serde_json::from_str(&channel_for(
            "0.0.19",
            manifest,
            &[env!("CARGO_PKG_VERSION")],
        ))
        .expect("test channel should parse");
        channel["latest"]["sha256"] = serde_json::Value::String("00".repeat(32));
        assert!(matches!(
            validate_latest_documents(&channel.to_string(), manifest),
            Err(BindingError::Latest(message)) if message.contains("SHA-256 mismatch")
        ));
    }

    #[test]
    fn rejects_latest_manifest_for_an_incompatible_engine() {
        let manifest = include_bytes!("../../../manifests/monty-v0.0.19.json");
        let channel = channel_for("0.0.19", manifest, &["999.0.0"]);
        assert!(matches!(
            validate_latest_documents(&channel, manifest),
            Err(BindingError::Latest(message)) if message.contains("newer compatible")
        ));
    }

    #[test]
    fn rejects_unknown_non_supported_latest_features() {
        let manifest = br#"{
            "target": {"tag": "v0.0.19", "runtime_version": "0.0.19"},
            "behavioral_capabilities": {
                "features": {"future.unknown": {"status": "unsupported_parse"}}
            }
        }"#;
        let channel = channel_for("0.0.19", manifest, &[env!("CARGO_PKG_VERSION")]);
        assert!(matches!(
            validate_latest_documents(&channel, manifest),
            Err(BindingError::Latest(message)) if message.contains("future.unknown")
        ));
    }

    #[test]
    fn rejects_unknown_releases_without_fallback() {
        assert!(matches!(
            release_transpiler("0.0.20"),
            Err(BindingError::UnsupportedRelease(_))
        ));
    }

    #[test]
    fn refuses_non_representable_source() {
        let error = transpile_impl("def values():\n    yield 1\n", "0.0.19")
            .expect_err("generator suspension must not be silently rewritten");
        assert!(matches!(error, BindingError::UnsafeDiagnostics(_)));
    }
}
