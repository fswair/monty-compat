//! Fallible, source-backed extraction of Monty's static capability graph.

mod scanner;

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    io::{Cursor, Read, Seek},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub use scanner::extract_sources;

const SOURCE_DIR_REL: &str = "crates/monty/src";
const MODULES_REL: &str = "crates/monty/src/modules/mod.rs";
const TYPES_REL: &str = "crates/monty/src/types/type.rs";
const INTERN_REL: &str = "crates/monty/src/intern.rs";
const BUILTINS_RELS: &[&str] = &[
    "crates/monty-types/src/builtins.rs",
    "crates/monty/src/builtins/mod.rs",
];
const EXCEPTIONS_RELS: &[&str] = &[
    "crates/monty-types/src/exceptions.rs",
    "crates/monty/src/exception_private.rs",
];
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_ARCHIVE_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RELEASE_METADATA_BYTES: usize = 1024 * 1024;
const HTTP_TIMEOUT_SECONDS: u64 = 30;
const HTTP_MAX_REDIRECTS: usize = 5;
const HTTP_MAX_HEADERS_BYTES: usize = 32 * 1024;
const GITHUB_RELEASE_API: &str = "https://api.github.com/repos/pydantic/monty/releases/latest";
const GITHUB_RELEASE_TAG_API_PREFIX: &str =
    "https://api.github.com/repos/pydantic/monty/releases/tags/";
const GITHUB_TAG_ARCHIVE_PREFIX: &str = "https://github.com/pydantic/monty/archive/refs/tags/";
const MONTY_REPOSITORY: &str = "https://github.com/pydantic/monty";

/// Immutable identity of a published Monty release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseMetadata {
    pub repository: String,
    pub tag: String,
    pub runtime_version: String,
    pub published_at: Option<String>,
    pub release_url: Option<String>,
    pub archive_url: String,
}

/// Deterministic JSON-compatible view of Monty's source-backed capabilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGraph {
    pub builtin_functions: BTreeSet<String>,
    pub type_constructors: BTreeSet<String>,
    pub exception_types: BTreeSet<String>,
    pub modules: BTreeSet<String>,
    pub module_attributes: BTreeMap<String, BTreeSet<String>>,
    pub type_attributes: BTreeMap<String, BTreeSet<String>>,
}

/// All source text needed by the extractor, including dispatch helper files.
#[derive(Debug, Clone)]
pub struct SourceBundle {
    pub builtins: String,
    pub modules: String,
    pub types: String,
    pub exceptions: String,
    pub intern: String,
    pub rust_files: BTreeMap<String, String>,
}

/// Failure while loading or scanning a Monty source tree.
#[derive(Debug)]
pub enum ExtractError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    MissingSource {
        candidates: Vec<PathBuf>,
    },
    Archive(zip::result::ZipError),
    ArchiveRead(std::io::Error),
    Http(minreq::Error),
    HttpRead(std::io::Error),
    HttpStatus {
        url: String,
        status: u16,
        reason: String,
    },
    DownloadTooLarge {
        url: String,
        limit: usize,
    },
    InvalidReleaseResponse(String),
    InvalidArchive(String),
    InvalidArchiveUtf8 {
        path: String,
        source: std::string::FromUtf8Error,
    },
    InvalidPattern {
        pattern: &'static str,
        message: String,
    },
    Serialize(serde_json::Error),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::MissingSource { candidates } => write!(
                formatter,
                "Monty source is missing all of: {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Archive(error) => write!(formatter, "cannot read ZIP archive: {error}"),
            Self::ArchiveRead(error) => {
                write!(formatter, "cannot read a file from ZIP archive: {error}")
            }
            Self::Http(error) => write!(formatter, "HTTP request failed: {error}"),
            Self::HttpRead(error) => write!(formatter, "cannot read HTTP response: {error}"),
            Self::HttpStatus {
                url,
                status,
                reason,
            } => write!(
                formatter,
                "HTTP request to {url} returned {status} {reason}"
            ),
            Self::DownloadTooLarge { url, limit } => write!(
                formatter,
                "HTTP response from {url} exceeds the {limit}-byte limit"
            ),
            Self::InvalidReleaseResponse(message) => {
                write!(formatter, "invalid GitHub release response: {message}")
            }
            Self::InvalidArchive(message) => write!(formatter, "invalid ZIP archive: {message}"),
            Self::InvalidArchiveUtf8 { path, source } => {
                write!(formatter, "ZIP source {path:?} is not UTF-8: {source}")
            }
            Self::InvalidPattern { pattern, message } => {
                write!(
                    formatter,
                    "invalid internal scanner pattern {pattern:?}: {message}"
                )
            }
            Self::Serialize(error) => write!(formatter, "cannot serialize capabilities: {error}"),
        }
    }
}

impl Error for ExtractError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Archive(error) => Some(error),
            Self::ArchiveRead(error) | Self::HttpRead(error) => Some(error),
            Self::Http(error) => Some(error),
            Self::InvalidArchiveUtf8 { source, .. } => Some(source),
            Self::Serialize(error) => Some(error),
            Self::MissingSource { .. }
            | Self::HttpStatus { .. }
            | Self::DownloadTooLarge { .. }
            | Self::InvalidReleaseResponse(_)
            | Self::InvalidArchive(_)
            | Self::InvalidPattern { .. } => None,
        }
    }
}

impl From<serde_json::Error> for ExtractError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}

impl From<zip::result::ZipError> for ExtractError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Archive(error)
    }
}

impl SourceBundle {
    /// Load the exact source files used by Python's current local extractor.
    pub fn from_local(root: impl AsRef<Path>) -> Result<Self, ExtractError> {
        let root = root.as_ref();
        let crates = root.join("crates");
        let mut rust_paths = Vec::new();
        collect_rust_files(&crates, &mut rust_paths)?;
        rust_paths.sort();

        let mut rust_files = BTreeMap::new();
        for path in rust_paths {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            rust_files.insert(relative, read_source(&path)?);
        }

        Self::from_rust_files(rust_files)
    }

    /// Load Monty sources directly from GitHub-style ZIP bytes without extracting them.
    pub fn from_zip_bytes(bytes: &[u8]) -> Result<Self, ExtractError> {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(ExtractError::InvalidArchive(format!(
                "compressed input is {} bytes; limit is {MAX_ARCHIVE_BYTES}",
                bytes.len()
            )));
        }
        let declared_entries = standard_zip_entry_count(bytes)?;
        Self::from_zip(Cursor::new(bytes), declared_entries)
    }

    fn from_zip<R: Read + Seek>(reader: R, declared_entries: usize) -> Result<Self, ExtractError> {
        let mut archive = zip::ZipArchive::new(reader)?;
        if archive.len() != declared_entries {
            return Err(ExtractError::InvalidArchive(format!(
                "central directory declares {declared_entries} entries but only {} unique paths exist; duplicate entry paths are not allowed",
                archive.len()
            )));
        }
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(ExtractError::InvalidArchive(format!(
                "archive contains {} entries; limit is {MAX_ARCHIVE_ENTRIES}",
                archive.len()
            )));
        }

        let mut root_component: Option<String> = None;
        let mut seen_paths = BTreeSet::new();
        let mut rust_files = BTreeMap::new();
        let mut expanded_bytes = 0_u64;

        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let raw_name = file.name_raw();
            let name = std::str::from_utf8(raw_name).map_err(|error| {
                ExtractError::InvalidArchive(format!("entry {index} has a non-UTF-8 path: {error}"))
            })?;
            let components = validate_archive_path(name)?;
            let Some((root, relative_components)) = components.split_first() else {
                continue;
            };
            match &root_component {
                Some(expected) if expected != root => {
                    return Err(ExtractError::InvalidArchive(format!(
                        "entries use multiple top-level directories: {expected:?} and {root:?}"
                    )));
                }
                None => root_component = Some((*root).to_owned()),
                Some(_) => {}
            }

            let normalized = components.join("/");
            if !seen_paths.insert(normalized.clone()) {
                return Err(ExtractError::InvalidArchive(format!(
                    "duplicate entry path {normalized:?}"
                )));
            }

            let size = file.size();
            if size > MAX_ARCHIVE_FILE_BYTES {
                return Err(ExtractError::InvalidArchive(format!(
                    "entry {normalized:?} expands to {size} bytes; per-file limit is {MAX_ARCHIVE_FILE_BYTES}"
                )));
            }
            expanded_bytes = expanded_bytes.checked_add(size).ok_or_else(|| {
                ExtractError::InvalidArchive("expanded size overflowed u64".to_owned())
            })?;
            if expanded_bytes > MAX_ARCHIVE_EXPANDED_BYTES {
                return Err(ExtractError::InvalidArchive(format!(
                    "entries expand to more than {MAX_ARCHIVE_EXPANDED_BYTES} bytes"
                )));
            }

            if file.is_dir()
                || relative_components.first().copied() != Some("crates")
                || relative_components.last().is_none_or(|part| {
                    Path::new(part)
                        .extension()
                        .is_none_or(|extension| !extension.eq_ignore_ascii_case("rs"))
                })
            {
                continue;
            }

            let relative = relative_components.join("/");
            let capacity = usize::try_from(size).map_err(|_| {
                ExtractError::InvalidArchive(format!(
                    "entry {normalized:?} does not fit in memory on this platform"
                ))
            })?;
            let mut bytes = Vec::with_capacity(capacity);
            file.by_ref()
                .take(size.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(ExtractError::ArchiveRead)?;
            if bytes.len() != capacity {
                return Err(ExtractError::InvalidArchive(format!(
                    "entry {normalized:?} declared {size} bytes but yielded {}",
                    bytes.len()
                )));
            }
            let source =
                String::from_utf8(bytes).map_err(|source| ExtractError::InvalidArchiveUtf8 {
                    path: relative.clone(),
                    source,
                })?;
            if rust_files.insert(relative.clone(), source).is_some() {
                return Err(ExtractError::InvalidArchive(format!(
                    "duplicate Rust source path {relative:?}"
                )));
            }
        }

        if root_component.is_none() {
            return Err(ExtractError::InvalidArchive(
                "archive contains no entries".to_owned(),
            ));
        }
        Self::from_rust_files(rust_files)
    }

    fn from_rust_files(rust_files: BTreeMap<String, String>) -> Result<Self, ExtractError> {
        Ok(Self {
            builtins: read_first_from_map(&rust_files, BUILTINS_RELS)?,
            modules: read_from_map(&rust_files, MODULES_REL)?,
            types: read_from_map(&rust_files, TYPES_REL)?,
            exceptions: read_first_from_map(&rust_files, EXCEPTIONS_RELS)?,
            intern: read_from_map(&rust_files, INTERN_REL)?,
            rust_files,
        })
    }
}

/// Extract a capability graph from a local Monty repository checkout.
pub fn extract_local(root: impl AsRef<Path>) -> Result<CapabilityGraph, ExtractError> {
    extract_sources(&SourceBundle::from_local(root)?)
}

/// Extract a capability graph from an in-memory GitHub-style ZIP archive.
pub fn extract_zip(bytes: &[u8]) -> Result<CapabilityGraph, ExtractError> {
    extract_sources(&SourceBundle::from_zip_bytes(bytes)?)
}

/// Download and extract the released or explicitly addressed Monty source archive.
pub fn extract_github(url: &str, only_released: bool) -> Result<CapabilityGraph, ExtractError> {
    let resolved_url = if only_released {
        resolve_release("latest")?.archive_url
    } else {
        url.to_owned()
    };
    let archive = download_limited(&resolved_url, "application/zip", MAX_ARCHIVE_BYTES)?;
    extract_zip(&archive)
}

/// Resolve `latest`, a bare version, or a `v`-prefixed tag through GitHub's release API.
pub fn resolve_release(release: &str) -> Result<ReleaseMetadata, ExtractError> {
    let requested_tag = normalize_release_tag(release)?;
    let api_url = requested_tag.as_ref().map_or_else(
        || GITHUB_RELEASE_API.to_owned(),
        |tag| format!("{GITHUB_RELEASE_TAG_API_PREFIX}{tag}"),
    );
    let bytes = download_limited(
        &api_url,
        "application/vnd.github+json",
        MAX_RELEASE_METADATA_BYTES,
    )?;
    let metadata = parse_release_metadata(&bytes)?;
    if let Some(requested_tag) = requested_tag
        && metadata.tag != requested_tag
    {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "requested tag {requested_tag:?} but GitHub returned {:?}",
            metadata.tag
        )));
    }
    Ok(metadata)
}

/// Download and extract the exact archive identified by resolved release metadata.
pub fn extract_release(metadata: &ReleaseMetadata) -> Result<CapabilityGraph, ExtractError> {
    if !is_safe_release_tag(&metadata.tag) {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "tag {:?} contains unsupported characters",
            metadata.tag
        )));
    }
    let expected_url = format!("{GITHUB_TAG_ARCHIVE_PREFIX}{}.zip", metadata.tag);
    if metadata.archive_url != expected_url {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "archive URL {:?} does not match release tag {:?}",
            metadata.archive_url, metadata.tag
        )));
    }
    let archive = download_limited(&metadata.archive_url, "application/zip", MAX_ARCHIVE_BYTES)?;
    extract_zip(&archive)
}

/// Serialize a graph using the same deterministic shape as Python's `to_dict()`.
pub fn to_json_pretty(graph: &CapabilityGraph) -> Result<String, ExtractError> {
    Ok(serde_json::to_string_pretty(graph)? + "\n")
}

fn read_source(path: &Path) -> Result<String, ExtractError> {
    fs::read_to_string(path).map_err(|source| ExtractError::Io {
        path: path.to_owned(),
        source,
    })
}

/// Download a bounded HTTP response without buffering beyond `limit` bytes.
///
/// This is hidden from the rendered public API because it exists for the
/// Python binding's hash-checked manifest-channel transport rather than extraction.
#[doc(hidden)]
pub fn download_limited(url: &str, accept: &str, limit: usize) -> Result<Vec<u8>, ExtractError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "unsupported URL scheme in {url:?}"
        )));
    }
    let mut response = minreq::get(url)
        .with_header("Accept", accept)
        .with_header(
            "User-Agent",
            concat!("monty-compat/", env!("CARGO_PKG_VERSION")),
        )
        .with_timeout(HTTP_TIMEOUT_SECONDS)
        .with_max_redirects(HTTP_MAX_REDIRECTS)
        .with_max_headers_size(HTTP_MAX_HEADERS_BYTES)
        .send_lazy()
        .map_err(ExtractError::Http)?;
    if !(200..300).contains(&response.status_code) {
        return Err(ExtractError::HttpStatus {
            url: response.url,
            status: response.status_code,
            reason: response.reason_phrase,
        });
    }
    if response_content_length(&response.headers).is_some_and(|length| length > limit) {
        return Err(ExtractError::DownloadTooLarge {
            url: response.url,
            limit,
        });
    }

    let read_limit = u64::try_from(limit)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| ExtractError::DownloadTooLarge {
            url: response.url.clone(),
            limit,
        })?;
    let mut body = Vec::with_capacity(
        response_content_length(&response.headers)
            .unwrap_or_default()
            .min(limit),
    );
    response
        .by_ref()
        .take(read_limit)
        .read_to_end(&mut body)
        .map_err(ExtractError::HttpRead)?;
    if body.len() > limit {
        return Err(ExtractError::DownloadTooLarge {
            url: response.url,
            limit,
        });
    }
    Ok(body)
}

fn response_content_length(headers: &[(String, String)]) -> Option<usize> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse().ok())
}

fn is_safe_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn normalize_release_tag(release: &str) -> Result<Option<String>, ExtractError> {
    if release == "latest" {
        return Ok(None);
    }
    let tag = if release.starts_with('v') {
        release.to_owned()
    } else {
        format!("v{release}")
    };
    if !is_safe_release_tag(&tag) {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "release {release:?} contains unsupported characters"
        )));
    }
    Ok(Some(tag))
}

fn parse_release_metadata(bytes: &[u8]) -> Result<ReleaseMetadata, ExtractError> {
    let payload: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| ExtractError::InvalidReleaseResponse(error.to_string()))?;
    let tag = payload
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ExtractError::InvalidReleaseResponse("response has no string tag_name".to_owned())
        })?;
    if !is_safe_release_tag(tag) {
        return Err(ExtractError::InvalidReleaseResponse(format!(
            "tag_name {tag:?} contains unsupported characters"
        )));
    }
    let runtime_version = tag.strip_prefix('v').unwrap_or(tag);
    if runtime_version.is_empty() {
        return Err(ExtractError::InvalidReleaseResponse(
            "release tag has no runtime version".to_owned(),
        ));
    }
    Ok(ReleaseMetadata {
        repository: MONTY_REPOSITORY.to_owned(),
        tag: tag.to_owned(),
        runtime_version: runtime_version.to_owned(),
        published_at: payload
            .get("published_at")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        release_url: payload
            .get("html_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        archive_url: format!("{GITHUB_TAG_ARCHIVE_PREFIX}{tag}.zip"),
    })
}

fn read_from_map(
    sources: &BTreeMap<String, String>,
    relative: &str,
) -> Result<String, ExtractError> {
    sources
        .get(relative)
        .cloned()
        .ok_or_else(|| ExtractError::MissingSource {
            candidates: vec![PathBuf::from(relative)],
        })
}

fn read_first_from_map(
    sources: &BTreeMap<String, String>,
    candidates: &[&str],
) -> Result<String, ExtractError> {
    candidates
        .iter()
        .find_map(|candidate| sources.get(*candidate).cloned())
        .ok_or_else(|| ExtractError::MissingSource {
            candidates: candidates.iter().map(PathBuf::from).collect(),
        })
}

fn validate_archive_path(name: &str) -> Result<Vec<&str>, ExtractError> {
    if name.is_empty() {
        return Err(ExtractError::InvalidArchive(
            "an entry has an empty path".to_owned(),
        ));
    }
    if name.contains('\\') || name.contains('\0') || name.starts_with('/') {
        return Err(ExtractError::InvalidArchive(format!(
            "unsafe entry path {name:?}"
        )));
    }

    let without_trailing_slash = name.strip_suffix('/').unwrap_or(name);
    let components: Vec<_> = without_trailing_slash.split('/').collect();
    if components
        .iter()
        .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return Err(ExtractError::InvalidArchive(format!(
            "unsafe entry path {name:?}"
        )));
    }
    if components
        .first()
        .is_some_and(|component| component.contains(':'))
    {
        return Err(ExtractError::InvalidArchive(format!(
            "unsafe entry path {name:?}"
        )));
    }
    Ok(components)
}

fn standard_zip_entry_count(bytes: &[u8]) -> Result<usize, ExtractError> {
    const END_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const END_RECORD_BYTES: usize = 22;
    const MAX_COMMENT_BYTES: usize = u16::MAX as usize;

    let search_start = bytes
        .len()
        .saturating_sub(END_RECORD_BYTES + MAX_COMMENT_BYTES);
    let offset = bytes[search_start..]
        .windows(END_SIGNATURE.len())
        .enumerate()
        .rev()
        .find_map(|(relative_offset, window)| {
            if window != END_SIGNATURE {
                return None;
            }
            let offset = search_start + relative_offset;
            let record = bytes.get(offset..)?;
            if record.len() < END_RECORD_BYTES {
                return None;
            }
            let comment_bytes = usize::from(u16::from_le_bytes([record[20], record[21]]));
            (record.len() == END_RECORD_BYTES + comment_bytes).then_some(offset)
        })
        .ok_or_else(|| {
            ExtractError::InvalidArchive("end-of-central-directory record is missing".to_owned())
        })?;
    let record = bytes.get(offset..).ok_or_else(|| {
        ExtractError::InvalidArchive("end-of-central-directory offset is invalid".to_owned())
    })?;
    if record.len() < END_RECORD_BYTES {
        return Err(ExtractError::InvalidArchive(
            "end-of-central-directory record is truncated".to_owned(),
        ));
    }

    let read_u16 = |start: usize| -> u16 { u16::from_le_bytes([record[start], record[start + 1]]) };
    let disk_number = read_u16(4);
    let central_directory_disk = read_u16(6);
    let entries_on_disk = read_u16(8);
    let total_entries = read_u16(10);
    let comment_bytes = usize::from(read_u16(20));
    if record.len() != END_RECORD_BYTES + comment_bytes {
        return Err(ExtractError::InvalidArchive(
            "end-of-central-directory comment length is inconsistent".to_owned(),
        ));
    }
    if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries {
        return Err(ExtractError::InvalidArchive(
            "multi-disk ZIP archives are not supported".to_owned(),
        ));
    }
    if total_entries == u16::MAX {
        return Err(ExtractError::InvalidArchive(
            "ZIP64 archives are not accepted by the bounded source loader".to_owned(),
        ));
    }
    Ok(usize::from(total_entries))
}

fn collect_rust_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), ExtractError> {
    let entries = fs::read_dir(directory).map_err(|source| ExtractError::Io {
        path: directory.to_owned(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ExtractError::Io {
            path: directory.to_owned(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| ExtractError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_rust_files(&path, output)?;
        } else if file_type.is_file() && path.extension().is_some_and(|suffix| suffix == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

#[must_use]
pub const fn source_directory_relative() -> &'static str {
    SOURCE_DIR_REL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tags_are_restricted_to_url_safe_ascii() {
        assert!(is_safe_release_tag("v0.0.19"));
        assert!(is_safe_release_tag("release_candidate-1"));
        assert!(!is_safe_release_tag(""));
        assert!(!is_safe_release_tag("../main"));
        assert!(!is_safe_release_tag("v1?download=true"));
    }

    #[test]
    fn release_metadata_is_normalized_to_an_exact_archive() {
        let metadata = parse_release_metadata(
            br#"{
                "tag_name": "v0.0.19",
                "published_at": "2025-01-02T03:04:05Z",
                "html_url": "https://github.com/pydantic/monty/releases/tag/v0.0.19"
            }"#,
        )
        .expect("release metadata should parse");
        assert_eq!(metadata.runtime_version, "0.0.19");
        assert_eq!(
            metadata.archive_url,
            "https://github.com/pydantic/monty/archive/refs/tags/v0.0.19.zip"
        );
    }

    #[test]
    fn release_aliases_are_normalized_without_accepting_paths() {
        assert_eq!(
            normalize_release_tag("0.0.19").expect("version should normalize"),
            Some("v0.0.19".to_owned())
        );
        assert_eq!(
            normalize_release_tag("latest").expect("latest should normalize"),
            None
        );
        assert!(normalize_release_tag("../main").is_err());
    }

    #[test]
    fn downloader_rejects_non_http_schemes_before_io() {
        assert!(matches!(
            extract_github("file:///tmp/monty.zip", false),
            Err(ExtractError::InvalidReleaseResponse(_))
        ));
    }
}
