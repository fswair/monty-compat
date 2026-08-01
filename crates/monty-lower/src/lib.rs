//! Manifest-driven source lowering for the Monty Python interpreter.

mod coverage;
mod edit;
mod facts;
mod lower;
mod manifest;
mod match_lower;
mod transpiler;

pub use coverage::{FeatureLowering, LOWERING_COVERAGE, LoweringAvailability, lowering_coverage};
pub use edit::EditError;
pub use lower::{
    DiagnosticDisposition, LoweringDiagnostic, LoweringError, LoweringOutput, lower_source,
};
pub use manifest::{CapabilityIndex, ManifestError, TargetFingerprint};
pub use transpiler::{CacheConfig, CacheStats, Transpiler};
