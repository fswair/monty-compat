use std::{collections::HashMap, error::Error, fmt, fs, path::Path};

use serde::{Deserialize, Serialize};

/// Release identity tying lowering decisions to the probed Monty runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetFingerprint {
    pub tag: String,
    pub runtime_version: Option<String>,
}

/// Minimal manifest view needed by the lowering engine.
#[derive(Debug, Clone)]
pub struct CapabilityIndex {
    target: TargetFingerprint,
    feature_statuses: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ManifestDocument {
    target: TargetFingerprint,
    behavioral_capabilities: BehavioralCapabilities,
}

#[derive(Debug, Deserialize)]
struct BehavioralCapabilities {
    features: HashMap<String, FeatureEvidence>,
}

#[derive(Debug, Deserialize)]
struct FeatureEvidence {
    status: String,
}

/// Failure while loading or validating a capability manifest.
#[derive(Debug)]
pub enum ManifestError {
    Io(std::io::Error),
    Json(serde_json::Error),
    VersionMismatch {
        tag: String,
        runtime_version: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot read capability manifest: {error}"),
            Self::Json(error) => write!(formatter, "invalid capability manifest: {error}"),
            Self::VersionMismatch {
                tag,
                runtime_version,
            } => write!(
                formatter,
                "manifest target {tag} does not match runtime version {runtime_version}"
            ),
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::VersionMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for ManifestError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ManifestError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl CapabilityIndex {
    /// Load the capability evidence used to gate lowering rules.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let source = fs::read_to_string(path)?;
        Self::from_json(&source)
    }

    /// Deserialize capability evidence from a discovery manifest.
    pub fn from_json(source: &str) -> Result<Self, ManifestError> {
        let document: ManifestDocument = serde_json::from_str(source)?;
        if let Some(runtime_version) = &document.target.runtime_version {
            let tag_version = document
                .target
                .tag
                .strip_prefix('v')
                .unwrap_or(&document.target.tag);
            if tag_version != runtime_version {
                return Err(ManifestError::VersionMismatch {
                    tag: document.target.tag,
                    runtime_version: runtime_version.clone(),
                });
            }
        }
        Ok(Self {
            target: document.target,
            feature_statuses: document
                .behavioral_capabilities
                .features
                .into_iter()
                .map(|(feature, evidence)| (feature, evidence.status))
                .collect(),
        })
    }

    /// Monty release and runtime identity for this evidence set.
    #[must_use]
    pub const fn target(&self) -> &TargetFingerprint {
        &self.target
    }

    /// Return the probe status for a stable feature identifier.
    #[must_use]
    pub fn feature_status(&self, feature: &str) -> Option<&str> {
        self.feature_statuses.get(feature).map(String::as_str)
    }

    /// Iterate over all feature identifiers and their discovered statuses.
    pub fn feature_statuses(&self) -> impl Iterator<Item = (&str, &str)> {
        self.feature_statuses
            .iter()
            .map(|(feature, status)| (feature.as_str(), status.as_str()))
    }

    /// Whether discovery proved that Monty's parser rejects this feature.
    #[must_use]
    pub fn is_parse_unsupported(&self, feature: &str) -> bool {
        self.feature_status(feature) == Some("unsupported_parse")
    }

    /// Whether discovery found any non-supported outcome for this feature.
    #[must_use]
    pub fn is_not_supported(&self, feature: &str) -> bool {
        self.feature_status(feature)
            .is_some_and(|status| status != "supported")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_release_and_runtime_identity() {
        let result = CapabilityIndex::from_json(
            r#"{
                "target": {"tag": "v0.0.19", "runtime_version": "0.0.18"},
                "behavioral_capabilities": {"features": {}}
            }"#,
        );
        assert!(matches!(result, Err(ManifestError::VersionMismatch { .. })));
    }
}
