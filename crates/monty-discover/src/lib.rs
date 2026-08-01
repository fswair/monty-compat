mod worker;

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Write as _},
    path::Path,
    time::Duration,
};

use monty_types::MontyObject;
use serde::{Deserialize, Serialize};

pub use worker::{
    EnvironmentInfo, GenerateResponse, MinimizeCandidate, MinimizeResponse, MontyResponse,
    MontyWorker, OracleResponse, PythonWorker, WorkerError, run_monty_worker_stdio,
};

pub const GENERATED_SCHEMA_VERSION: u64 = 1;
pub const MINIMIZATION_SCHEMA_VERSION: u64 = 1;
pub const PROMOTION_SCHEMA_VERSION: u64 = 1;
pub const PROBE_SCHEMA_VERSION: u64 = 3;
pub const LINKED_MONTY_VERSION: &str = env!("MONTY_COMPAT_LINKED_MONTY_VERSION");
const MAX_GENERATED_SEEDS: u64 = 100_000;
const BIGINT_WIRE_KEY: &str = "__monty_compat_bigint__";
const NONFINITE_WIRE_KEY: &str = "__monty_compat_nonfinite__";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub id: String,
    pub category: String,
    pub source: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Supported,
    UnsupportedParse,
    UnsupportedTypeCheck,
    UnsupportedRuntime,
    SemanticMismatch,
    Crash,
    Timeout,
    InvalidProbe,
    UnknownError,
}

impl ProbeStatus {
    const ALL: [Self; 9] = [
        Self::Supported,
        Self::UnsupportedParse,
        Self::UnsupportedTypeCheck,
        Self::UnsupportedRuntime,
        Self::SemanticMismatch,
        Self::Crash,
        Self::Timeout,
        Self::InvalidProbe,
        Self::UnknownError,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::UnsupportedParse => "unsupported_parse",
            Self::UnsupportedTypeCheck => "unsupported_type_check",
            Self::UnsupportedRuntime => "unsupported_runtime",
            Self::SemanticMismatch => "semantic_mismatch",
            Self::Crash => "crash",
            Self::Timeout => "timeout",
            Self::InvalidProbe => "invalid_probe",
            Self::UnknownError => "unknown_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub id: String,
    pub category: String,
    pub description: String,
    pub status: ProbeStatus,
    pub ast_nodes: Vec<String>,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BehavioralReport {
    pub probe_schema_version: u64,
    pub summary: BTreeMap<String, usize>,
    pub ast_node_coverage: BTreeMap<String, usize>,
    pub features: BTreeMap<String, ProbeResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedProbeConfig {
    pub seed_start: u64,
    pub seed_count: u64,
    pub node_limit: usize,
    pub depth_limit: usize,
    pub max_source_bytes: usize,
    pub max_ast_nodes: usize,
}

impl Default for GeneratedProbeConfig {
    fn default() -> Self {
        Self {
            seed_start: 0,
            seed_count: 100,
            node_limit: 100,
            depth_limit: 5,
            max_source_bytes: 100_000,
            max_ast_nodes: 2_000,
        }
    }
}

impl GeneratedProbeConfig {
    pub fn validate(&self) -> Result<(), DiscoverError> {
        if self.node_limit == 0 {
            return Err(DiscoverError::InvalidConfig(
                "node_limit must be positive".to_owned(),
            ));
        }
        if self.depth_limit == 0 {
            return Err(DiscoverError::InvalidConfig(
                "depth_limit must be positive".to_owned(),
            ));
        }
        if self.max_source_bytes == 0 {
            return Err(DiscoverError::InvalidConfig(
                "max_source_bytes must be positive".to_owned(),
            ));
        }
        if self.max_ast_nodes == 0 {
            return Err(DiscoverError::InvalidConfig(
                "max_ast_nodes must be positive".to_owned(),
            ));
        }
        if self.seed_count > MAX_GENERATED_SEEDS {
            return Err(DiscoverError::InvalidConfig(format!(
                "seed_count exceeds the {MAX_GENERATED_SEEDS}-seed limit"
            )));
        }
        self.seed_start
            .checked_add(self.seed_count)
            .ok_or_else(|| DiscoverError::InvalidConfig("seed range overflows u64".to_owned()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedProbeStatus {
    Completed,
    UnsupportedParse,
    UnsupportedTypeCheck,
    UnsupportedRuntime,
    SemanticMismatch,
    Crash,
    Timeout,
    GenerationError,
    GuardRejected,
    UnknownError,
}

impl GeneratedProbeStatus {
    const ALL: [Self; 10] = [
        Self::Completed,
        Self::UnsupportedParse,
        Self::UnsupportedTypeCheck,
        Self::UnsupportedRuntime,
        Self::SemanticMismatch,
        Self::Crash,
        Self::Timeout,
        Self::GenerationError,
        Self::GuardRejected,
        Self::UnknownError,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::UnsupportedParse => "unsupported_parse",
            Self::UnsupportedTypeCheck => "unsupported_type_check",
            Self::UnsupportedRuntime => "unsupported_runtime",
            Self::SemanticMismatch => "semantic_mismatch",
            Self::Crash => "crash",
            Self::Timeout => "timeout",
            Self::GenerationError => "generation_error",
            Self::GuardRejected => "guard_rejected",
            Self::UnknownError => "unknown_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedProbeResult {
    pub seed: u64,
    pub status: GeneratedProbeStatus,
    pub fully_accepted: bool,
    pub ast_nodes: Vec<String>,
    pub ast_node_count: usize,
    pub source_sha256: Option<String>,
    pub source: Option<String>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratorIdentity {
    pub distribution: &'static str,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratedSafety {
    pub mode: &'static str,
    pub raw_generated_code_executed: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratedReport {
    pub schema_version: u64,
    pub generator: GeneratorIdentity,
    pub safety: GeneratedSafety,
    pub config: GeneratedProbeConfig,
    pub summary: BTreeMap<String, usize>,
    pub fully_accepted: usize,
    pub ast_node_outcomes: BTreeMap<String, BTreeMap<String, usize>>,
    pub results: Vec<GeneratedProbeResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinimizationConfig {
    pub enabled: bool,
    pub max_checks: u64,
}

impl Default for MinimizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_checks: 1_000,
        }
    }
}

impl MinimizationConfig {
    fn validate(&self) -> Result<(), DiscoverError> {
        if self.enabled && self.max_checks == 0 {
            return Err(DiscoverError::InvalidConfig(
                "minimizer max_checks must be positive when enabled".to_owned(),
            ));
        }
        Ok(())
    }

    const fn disabled() -> Self {
        Self {
            enabled: false,
            max_checks: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinimizationOutcome {
    Minimized,
    Unchanged,
    Error,
}

impl MinimizationOutcome {
    const ALL: [Self; 3] = [Self::Minimized, Self::Unchanged, Self::Error];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Minimized => "minimized",
            Self::Unchanged => "unchanged",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinimizedFailure {
    pub seed: u64,
    pub original_status: GeneratedProbeStatus,
    pub outcome: MinimizationOutcome,
    pub original_source_sha256: String,
    pub original_bytes: usize,
    pub original_ast_node_count: usize,
    pub original_error_type: String,
    pub original_error_message: String,
    pub minimized_source: Option<String>,
    pub minimized_source_sha256: Option<String>,
    pub minimized_bytes: Option<usize>,
    pub minimized_ast_nodes: Vec<String>,
    pub minimized_ast_node_count: Option<usize>,
    pub checker_calls: u64,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MinimizationSafety {
    pub raw_generated_code_executed: bool,
    pub candidate_execution_mode: &'static str,
    pub predicate: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MinimizationReport {
    pub schema_version: u64,
    pub minimizer: GeneratorIdentity,
    pub safety: MinimizationSafety,
    pub config: MinimizationConfig,
    pub eligible_failures: usize,
    pub attempted: usize,
    pub unique_minimized_cases: usize,
    pub summary: BTreeMap<String, usize>,
    pub total_checker_calls: u64,
    pub results: Vec<MinimizedFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromotionCandidate {
    pub id: String,
    pub status: GeneratedProbeStatus,
    pub error_type: String,
    pub error_message: String,
    pub source: String,
    pub source_sha256: String,
    pub ast_nodes: Vec<String>,
    pub ast_node_count: usize,
    pub seeds: Vec<u64>,
    pub occurrences: usize,
    pub disposition: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromotionReport {
    pub schema_version: u64,
    pub policy: &'static str,
    pub candidate_count: usize,
    pub candidates: Vec<PromotionCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratedDiscoveryReport {
    pub generated_corpus: GeneratedReport,
    pub minimized_failures: MinimizationReport,
    pub promotion_candidates: PromotionReport,
}

#[derive(Debug)]
pub enum DiscoverError {
    InvalidConfig(String),
    Worker(WorkerError),
}

impl fmt::Display for DiscoverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => {
                write!(formatter, "invalid discovery config: {message}")
            }
            Self::Worker(error) => error.fmt(formatter),
        }
    }
}

impl Error for DiscoverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfig(_) => None,
            Self::Worker(error) => Some(error),
        }
    }
}

impl From<WorkerError> for DiscoverError {
    fn from(error: WorkerError) -> Self {
        Self::Worker(error)
    }
}

pub fn run_baseline_probes(
    python: &str,
    monty_worker_executable: &Path,
    python_timeout: Duration,
    monty_timeout: Duration,
) -> Result<BehavioralReport, DiscoverError> {
    let mut python_worker = PythonWorker::start(python, python_timeout)?;
    let catalog = python_worker.catalog()?;
    validate_catalog(&catalog)?;
    let mut monty_worker = Some(MontyWorker::start(monty_worker_executable, monty_timeout)?);
    let mut results = Vec::with_capacity(catalog.len());

    for spec in catalog {
        let oracle = python_worker.oracle(&spec.source)?;
        let result = match oracle {
            OracleResponse::Raise {
                error_type,
                error_message,
                ast_nodes,
            } => ProbeResult {
                id: spec.id,
                category: spec.category,
                description: spec.description,
                status: ProbeStatus::InvalidProbe,
                ast_nodes,
                expected: serde_json::Value::Null,
                actual: serde_json::Value::Null,
                error_type: Some(error_type),
                error_message: Some(error_message),
            },
            OracleResponse::Return { value, ast_nodes } => {
                if monty_worker.is_none() {
                    monty_worker =
                        Some(MontyWorker::start(monty_worker_executable, monty_timeout)?);
                }
                let Some(worker) = monty_worker.as_mut() else {
                    return Err(DiscoverError::InvalidConfig(
                        "Monty worker could not be initialized".to_owned(),
                    ));
                };
                let outcome = worker.run(&spec.source);
                if outcome.is_err() {
                    monty_worker = None;
                }
                baseline_result(spec, ast_nodes, decode_wire_json(value), outcome)
            }
        };
        results.push(result);
    }
    Ok(build_behavioral_report(results))
}

fn validate_catalog(catalog: &[ProbeSpec]) -> Result<(), DiscoverError> {
    let mut seen = BTreeSet::new();
    for spec in catalog {
        if spec.id.is_empty() || spec.category.is_empty() || spec.source.is_empty() {
            return Err(DiscoverError::InvalidConfig(format!(
                "probe {:?} has an empty required field",
                spec.id
            )));
        }
        if !seen.insert(&spec.id) {
            return Err(DiscoverError::InvalidConfig(format!(
                "duplicate probe id {:?}",
                spec.id
            )));
        }
    }
    Ok(())
}

fn baseline_result(
    spec: ProbeSpec,
    ast_nodes: Vec<String>,
    expected: serde_json::Value,
    outcome: Result<MontyResponse, WorkerError>,
) -> ProbeResult {
    let mut result = ProbeResult {
        id: spec.id,
        category: spec.category,
        description: spec.description,
        status: ProbeStatus::UnknownError,
        ast_nodes,
        expected,
        actual: serde_json::Value::Null,
        error_type: None,
        error_message: None,
    };
    match outcome {
        Ok(MontyResponse::Return { value, .. }) => {
            result.actual = decode_wire_json(value);
            result.status = if wire_strict_equal(&result.actual, &result.expected) {
                ProbeStatus::Supported
            } else {
                ProbeStatus::SemanticMismatch
            };
        }
        Ok(MontyResponse::CompileError {
            error_type,
            error_message,
        }) => {
            result.status = baseline_compile_error_status(&error_type, &error_message);
            let (wrapper_type, wrapper_message) =
                normalize_monty_error(result.status, error_type, error_message);
            result.error_type = Some(wrapper_type);
            result.error_message = Some(wrapper_message);
        }
        Ok(MontyResponse::RuntimeError {
            error_type,
            error_message,
        }) => {
            result.status = ProbeStatus::UnsupportedRuntime;
            let (wrapper_type, wrapper_message) =
                normalize_monty_error(result.status, error_type, error_message);
            result.error_type = Some(wrapper_type);
            result.error_message = Some(wrapper_message);
        }
        Err(error) => {
            result.status = if matches!(error, WorkerError::Timeout(_)) {
                ProbeStatus::Timeout
            } else {
                ProbeStatus::Crash
            };
            result.error_type = Some("MontyWorkerError".to_owned());
            result.error_message = Some(error.to_string());
        }
    }
    result
}

fn baseline_compile_error_status(error_type: &str, error_message: &str) -> ProbeStatus {
    let message = error_message.to_ascii_lowercase();
    if error_type == "SyntaxError" || message.contains("syntax parser does not yet support") {
        ProbeStatus::UnsupportedParse
    } else if matches!(error_type, "ImportError" | "ModuleNotFoundError") {
        ProbeStatus::UnsupportedRuntime
    } else {
        ProbeStatus::UnsupportedTypeCheck
    }
}

fn normalize_monty_error(
    status: ProbeStatus,
    error_type: String,
    error_message: String,
) -> (String, String) {
    match status {
        ProbeStatus::UnsupportedParse if error_type == "SyntaxError" => {
            ("MontySyntaxError".to_owned(), error_message)
        }
        ProbeStatus::UnsupportedTypeCheck => ("MontyTypingError".to_owned(), error_message),
        _ if error_message.is_empty() => ("MontyRuntimeError".to_owned(), error_type),
        _ => (
            "MontyRuntimeError".to_owned(),
            format!("{error_type}: {error_message}"),
        ),
    }
}

pub(crate) fn monty_wire_json_safe(value: &MontyObject) -> serde_json::Value {
    match value {
        MontyObject::None => serde_json::Value::Null,
        MontyObject::Bool(value) => serde_json::Value::Bool(*value),
        MontyObject::Int(value) => serde_json::Value::Number((*value).into()),
        MontyObject::BigInt(_) => serde_json::json!({BIGINT_WIRE_KEY: value.py_repr()}),
        MontyObject::Float(value) if value.is_nan() => {
            serde_json::json!({NONFINITE_WIRE_KEY: "nan"})
        }
        MontyObject::Float(value) if value.is_infinite() => serde_json::json!({
            NONFINITE_WIRE_KEY: if value.is_sign_positive() { "inf" } else { "-inf" }
        }),
        MontyObject::Float(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        MontyObject::String(value) => serde_json::Value::String(value.clone()),
        MontyObject::Bytes(bytes) => {
            let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
            for byte in bytes {
                let _ = write!(&mut hex, "{byte:02x}");
            }
            serde_json::json!({"type": "bytes", "hex": hex})
        }
        MontyObject::List(values) => {
            serde_json::Value::Array(values.iter().map(monty_wire_json_safe).collect())
        }
        MontyObject::Tuple(values) | MontyObject::NamedTuple { values, .. } => {
            serde_json::json!({
                "type": "tuple",
                "items": values.iter().map(monty_wire_json_safe).collect::<Vec<_>>(),
            })
        }
        MontyObject::Dict(pairs) => {
            let mut output = serde_json::Map::new();
            for (key, item) in pairs {
                output.insert(monty_key_string(key), monty_wire_json_safe(item));
            }
            serde_json::Value::Object(output)
        }
        _ => fallback_monty_json(value),
    }
}

fn decode_wire_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(decode_wire_json).collect())
        }
        serde_json::Value::Object(mut object) => {
            if object.len() == 1
                && let Some(serde_json::Value::String(integer)) = object.get(BIGINT_WIRE_KEY)
                && let Ok(number) = integer.parse::<serde_json::Number>()
            {
                return serde_json::Value::Number(number);
            }
            for item in object.values_mut() {
                *item = decode_wire_json(std::mem::take(item));
            }
            serde_json::Value::Object(object)
        }
        scalar => scalar,
    }
}

fn wire_strict_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    left == right && !contains_nan_marker(left) && !contains_nan_marker(right)
}

fn contains_nan_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_nan_marker),
        serde_json::Value::Object(object) => {
            object
                .get(NONFINITE_WIRE_KEY)
                .and_then(serde_json::Value::as_str)
                == Some("nan")
                || object.values().any(contains_nan_marker)
        }
        _ => false,
    }
}

fn monty_key_string(value: &MontyObject) -> String {
    match value {
        MontyObject::String(value) => value.clone(),
        MontyObject::None => "None".to_owned(),
        MontyObject::Bool(true) => "True".to_owned(),
        MontyObject::Bool(false) => "False".to_owned(),
        _ => value.to_string(),
    }
}

fn fallback_monty_json(value: &MontyObject) -> serde_json::Value {
    serde_json::json!({"type": value.type_name(), "repr": value.py_repr()})
}

fn build_behavioral_report(results: Vec<ProbeResult>) -> BehavioralReport {
    let mut summary = BTreeMap::new();
    for status in ProbeStatus::ALL {
        summary.insert(status.as_str().to_owned(), 0);
    }
    let mut ast_node_coverage = BTreeMap::new();
    let mut features = BTreeMap::new();
    for result in results {
        if let Some(count) = summary.get_mut(result.status.as_str()) {
            *count += 1;
        }
        for node in &result.ast_nodes {
            *ast_node_coverage.entry(node.clone()).or_default() += 1;
        }
        features.insert(result.id.clone(), result);
    }
    BehavioralReport {
        probe_schema_version: PROBE_SCHEMA_VERSION,
        summary,
        ast_node_coverage,
        features,
    }
}

pub fn run_generated_probes(
    config: &GeneratedProbeConfig,
    python: &str,
    monty_worker_executable: &std::path::Path,
    python_timeout: Duration,
    monty_timeout: Duration,
) -> Result<GeneratedReport, DiscoverError> {
    Ok(run_generated_internal(
        config,
        &MinimizationConfig::disabled(),
        python,
        monty_worker_executable,
        python_timeout,
        monty_timeout,
    )?
    .generated_corpus)
}

pub fn run_generated_discovery(
    config: &GeneratedProbeConfig,
    minimization: &MinimizationConfig,
    python: &str,
    monty_worker_executable: &std::path::Path,
    python_timeout: Duration,
    monty_timeout: Duration,
) -> Result<GeneratedDiscoveryReport, DiscoverError> {
    run_generated_internal(
        config,
        minimization,
        python,
        monty_worker_executable,
        python_timeout,
        monty_timeout,
    )
}

fn run_generated_internal(
    config: &GeneratedProbeConfig,
    minimization: &MinimizationConfig,
    python: &str,
    monty_worker_executable: &std::path::Path,
    python_timeout: Duration,
    monty_timeout: Duration,
) -> Result<GeneratedDiscoveryReport, DiscoverError> {
    config.validate()?;
    minimization.validate()?;
    let mut python_worker = PythonWorker::start(python, python_timeout)?;
    let mut monty_worker = MontyWorker::start(monty_worker_executable, monty_timeout)?;
    let generator_version = python_worker.load_generator()?.map(str::to_owned);
    let minimizer_version = if minimization.enabled {
        python_worker.load_minimizer()?.map(str::to_owned)
    } else {
        None
    };
    let end = config.seed_start + config.seed_count;
    let mut results = Vec::with_capacity(usize::try_from(config.seed_count).unwrap_or(1024));
    let mut minimized_failures = Vec::new();
    for seed in config.seed_start..end {
        let result = match python_worker.generate(seed, config) {
            Ok(response) => run_generated_response(response, &mut monty_worker),
            Err(error) => GeneratedProbeResult {
                seed,
                status: if matches!(error, WorkerError::Timeout(_)) {
                    GeneratedProbeStatus::Timeout
                } else {
                    GeneratedProbeStatus::GenerationError
                },
                fully_accepted: false,
                ast_nodes: Vec::new(),
                ast_node_count: 0,
                source_sha256: None,
                source: None,
                error_type: Some("WorkerError".to_owned()),
                error_message: Some(error.to_string()),
            },
        };
        if minimization.enabled
            && let Some(minimized) = minimize_generated_failure(
                &result,
                config,
                minimization,
                &mut python_worker,
                &mut monty_worker,
            )?
        {
            minimized_failures.push(minimized);
        }
        let worker_failed = matches!(
            result.status,
            GeneratedProbeStatus::Timeout | GeneratedProbeStatus::GenerationError
        ) && result.error_type.as_deref() == Some("WorkerError");
        results.push(result);
        if worker_failed {
            break;
        }
    }
    let eligible_failures = results
        .iter()
        .filter(|result| generated_result_fingerprint(result).is_some())
        .count();
    let minimized_failures = build_minimization_report(
        minimization.clone(),
        minimizer_version,
        eligible_failures,
        minimized_failures,
    );
    let promotion_candidates = build_promotion_report(&minimized_failures);
    Ok(GeneratedDiscoveryReport {
        generated_corpus: build_generated_report(config.clone(), generator_version, results),
        minimized_failures,
        promotion_candidates,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailureFingerprint {
    status: GeneratedProbeStatus,
    error_type: String,
    error_message: String,
}

fn generated_result_fingerprint(result: &GeneratedProbeResult) -> Option<FailureFingerprint> {
    if !matches!(
        result.status,
        GeneratedProbeStatus::UnsupportedParse
            | GeneratedProbeStatus::UnsupportedTypeCheck
            | GeneratedProbeStatus::UnsupportedRuntime
    ) {
        return None;
    }
    Some(FailureFingerprint {
        status: result.status,
        error_type: result.error_type.clone()?,
        error_message: result.error_message.clone()?,
    })
}

fn monty_response_fingerprint(response: MontyResponse) -> Option<FailureFingerprint> {
    let (status, error_type, error_message) = match response {
        MontyResponse::CompileError {
            error_type,
            error_message,
        } => (
            compile_error_status(&error_type, &error_message),
            error_type,
            error_message,
        ),
        MontyResponse::RuntimeError {
            error_type,
            error_message,
        } => (
            GeneratedProbeStatus::UnsupportedRuntime,
            error_type,
            error_message,
        ),
        MontyResponse::Return { .. } => return None,
    };
    let (error_type, error_message) =
        normalize_generated_monty_error(status, error_type, error_message);
    Some(FailureFingerprint {
        status,
        error_type,
        error_message,
    })
}

fn minimize_generated_failure(
    result: &GeneratedProbeResult,
    generated_config: &GeneratedProbeConfig,
    minimization_config: &MinimizationConfig,
    python_worker: &mut PythonWorker,
    monty_worker: &mut MontyWorker,
) -> Result<Option<MinimizedFailure>, DiscoverError> {
    let Some(fingerprint) = generated_result_fingerprint(result) else {
        return Ok(None);
    };
    let (Some(source), Some(source_sha256)) =
        (result.source.as_deref(), result.source_sha256.as_deref())
    else {
        return Ok(None);
    };
    let response = python_worker.minimize(
        source,
        generated_config,
        minimization_config.max_checks,
        |candidate| {
            let response = monty_worker.run(&candidate.inert_source)?;
            Ok(monty_response_fingerprint(response).as_ref() == Some(&fingerprint))
        },
    )?;
    let original = MinimizedFailure {
        seed: result.seed,
        original_status: result.status,
        outcome: MinimizationOutcome::Unchanged,
        original_source_sha256: source_sha256.to_owned(),
        original_bytes: source.len(),
        original_ast_node_count: result.ast_node_count,
        original_error_type: fingerprint.error_type,
        original_error_message: fingerprint.error_message,
        minimized_source: None,
        minimized_source_sha256: None,
        minimized_bytes: None,
        minimized_ast_nodes: Vec::new(),
        minimized_ast_node_count: None,
        checker_calls: 0,
        error_type: None,
        error_message: None,
    };
    Ok(Some(match response {
        MinimizeResponse::Minimized {
            source,
            source_sha256,
            ast_nodes,
            ast_node_count,
            checks,
        } => MinimizedFailure {
            outcome: MinimizationOutcome::Minimized,
            minimized_bytes: Some(source.len()),
            minimized_source: Some(source),
            minimized_source_sha256: Some(source_sha256),
            minimized_ast_nodes: ast_nodes,
            minimized_ast_node_count: Some(ast_node_count),
            checker_calls: checks,
            ..original
        },
        MinimizeResponse::Unchanged { checks } => MinimizedFailure {
            checker_calls: checks,
            ..original
        },
        MinimizeResponse::MinimizationError {
            checks,
            error_type,
            error_message,
        } => MinimizedFailure {
            outcome: MinimizationOutcome::Error,
            checker_calls: checks,
            error_type: Some(error_type),
            error_message: Some(error_message),
            ..original
        },
    }))
}

fn build_promotion_report(report: &MinimizationReport) -> PromotionReport {
    type Key = (GeneratedProbeStatus, String, String, String);
    let mut grouped: BTreeMap<Key, PromotionCandidate> = BTreeMap::new();
    for failure in &report.results {
        if failure.outcome != MinimizationOutcome::Minimized {
            continue;
        }
        let (Some(source), Some(source_sha256), Some(ast_node_count)) = (
            failure.minimized_source.as_ref(),
            failure.minimized_source_sha256.as_ref(),
            failure.minimized_ast_node_count,
        ) else {
            continue;
        };
        let key = (
            failure.original_status,
            failure.original_error_type.clone(),
            failure.original_error_message.clone(),
            source_sha256.clone(),
        );
        let candidate = grouped.entry(key).or_insert_with(|| PromotionCandidate {
            id: format!(
                "generated.{}.{}",
                failure.original_status.as_str(),
                &source_sha256[..source_sha256.len().min(16)]
            ),
            status: failure.original_status,
            error_type: failure.original_error_type.clone(),
            error_message: failure.original_error_message.clone(),
            source: source.clone(),
            source_sha256: source_sha256.clone(),
            ast_nodes: failure.minimized_ast_nodes.clone(),
            ast_node_count,
            seeds: Vec::new(),
            occurrences: 0,
            disposition: "needs_semantic_probe",
        });
        candidate.seeds.push(failure.seed);
        candidate.occurrences += 1;
    }
    let candidates = grouped.into_values().collect::<Vec<_>>();
    PromotionReport {
        schema_version: PROMOTION_SCHEMA_VERSION,
        policy: "minimized failures are review candidates, never automatic capabilities",
        candidate_count: candidates.len(),
        candidates,
    }
}

fn build_minimization_report(
    config: MinimizationConfig,
    minimizer_version: Option<String>,
    eligible_failures: usize,
    results: Vec<MinimizedFailure>,
) -> MinimizationReport {
    let mut summary = BTreeMap::new();
    for outcome in MinimizationOutcome::ALL {
        summary.insert(outcome.as_str().to_owned(), 0);
    }
    let mut total_checker_calls = 0_u64;
    for result in &results {
        if let Some(count) = summary.get_mut(result.outcome.as_str()) {
            *count += 1;
        }
        total_checker_calls = total_checker_calls.saturating_add(result.checker_calls);
    }
    let unique_minimized_cases = results
        .iter()
        .filter_map(|result| result.minimized_source_sha256.as_deref())
        .collect::<BTreeSet<_>>()
        .len();
    MinimizationReport {
        schema_version: MINIMIZATION_SCHEMA_VERSION,
        minimizer: GeneratorIdentity {
            distribution: "pysource-minimize",
            version: minimizer_version,
        },
        safety: MinimizationSafety {
            raw_generated_code_executed: false,
            candidate_execution_mode: "dead_branch",
            predicate: "exact_status_error_type_and_message",
        },
        config,
        eligible_failures,
        attempted: results.len(),
        unique_minimized_cases,
        summary,
        total_checker_calls,
        results,
    }
}

fn run_generated_response(
    response: GenerateResponse,
    monty_worker: &mut MontyWorker,
) -> GeneratedProbeResult {
    match response {
        GenerateResponse::Prepared {
            seed,
            source,
            source_sha256,
            inert_source,
            ast_nodes,
            ast_node_count,
        } => {
            let prepared = PreparedGeneratedProbe {
                seed,
                source,
                source_sha256,
                ast_nodes,
                ast_node_count,
            };
            run_inert_monty(monty_worker, &inert_source, prepared)
        }
        GenerateResponse::GenerationError {
            seed,
            source,
            source_sha256,
            error_type,
            error_message,
        } => failed_generated_result(
            seed,
            GeneratedProbeStatus::GenerationError,
            source,
            source_sha256,
            error_type,
            error_message,
        ),
        GenerateResponse::GuardRejected {
            seed,
            source,
            source_sha256,
            error_type,
            error_message,
        } => failed_generated_result(
            seed,
            GeneratedProbeStatus::GuardRejected,
            source,
            source_sha256,
            error_type,
            error_message,
        ),
    }
}

struct PreparedGeneratedProbe {
    seed: u64,
    source: String,
    source_sha256: String,
    ast_nodes: Vec<String>,
    ast_node_count: usize,
}

fn run_inert_monty(
    monty_worker: &mut MontyWorker,
    inert_source: &str,
    prepared: PreparedGeneratedProbe,
) -> GeneratedProbeResult {
    let PreparedGeneratedProbe {
        seed,
        source,
        source_sha256,
        ast_nodes,
        ast_node_count,
    } = prepared;
    match monty_worker.run(inert_source) {
        Ok(MontyResponse::Return { is_none: true, .. }) => GeneratedProbeResult {
            seed,
            status: GeneratedProbeStatus::Completed,
            fully_accepted: true,
            ast_nodes,
            ast_node_count,
            source_sha256: Some(source_sha256),
            source: Some(source),
            error_type: None,
            error_message: None,
        },
        Ok(MontyResponse::Return { repr, .. }) => GeneratedProbeResult {
            seed,
            status: GeneratedProbeStatus::SemanticMismatch,
            fully_accepted: true,
            ast_nodes,
            ast_node_count,
            source_sha256: Some(source_sha256),
            source: Some(source),
            error_type: None,
            error_message: Some(format!("inert source returned {repr}")),
        },
        Ok(MontyResponse::CompileError {
            error_type,
            error_message,
        }) => monty_failed_result(
            compile_error_status(&error_type, &error_message),
            PreparedGeneratedProbe {
                seed,
                source,
                source_sha256,
                ast_nodes,
                ast_node_count,
            },
            error_type,
            error_message,
        ),
        Ok(MontyResponse::RuntimeError {
            error_type,
            error_message,
        }) => monty_failed_result(
            GeneratedProbeStatus::UnsupportedRuntime,
            PreparedGeneratedProbe {
                seed,
                source,
                source_sha256,
                ast_nodes,
                ast_node_count,
            },
            error_type,
            error_message,
        ),
        Err(error) => GeneratedProbeResult {
            seed,
            status: if matches!(error, WorkerError::Timeout(_)) {
                GeneratedProbeStatus::Timeout
            } else {
                GeneratedProbeStatus::Crash
            },
            fully_accepted: false,
            ast_nodes,
            ast_node_count,
            source_sha256: Some(source_sha256),
            source: Some(source),
            error_type: Some("MontyWorkerError".to_owned()),
            error_message: Some(error.to_string()),
        },
    }
}

fn compile_error_status(error_type: &str, error_message: &str) -> GeneratedProbeStatus {
    let message = error_message.to_ascii_lowercase();
    if error_type == "SyntaxError" || message.contains("syntax parser does not yet support") {
        GeneratedProbeStatus::UnsupportedParse
    } else if matches!(error_type, "ImportError" | "ModuleNotFoundError") {
        GeneratedProbeStatus::UnsupportedRuntime
    } else {
        GeneratedProbeStatus::UnsupportedTypeCheck
    }
}

fn normalize_generated_monty_error(
    status: GeneratedProbeStatus,
    error_type: String,
    error_message: String,
) -> (String, String) {
    let probe_status = match status {
        GeneratedProbeStatus::UnsupportedParse => ProbeStatus::UnsupportedParse,
        GeneratedProbeStatus::UnsupportedTypeCheck => ProbeStatus::UnsupportedTypeCheck,
        _ => ProbeStatus::UnsupportedRuntime,
    };
    normalize_monty_error(probe_status, error_type, error_message)
}

fn monty_failed_result(
    status: GeneratedProbeStatus,
    prepared: PreparedGeneratedProbe,
    error_type: String,
    error_message: String,
) -> GeneratedProbeResult {
    let PreparedGeneratedProbe {
        seed,
        source,
        source_sha256,
        ast_nodes,
        ast_node_count,
    } = prepared;
    let (error_type, error_message) =
        normalize_generated_monty_error(status, error_type, error_message);
    GeneratedProbeResult {
        seed,
        status,
        fully_accepted: false,
        ast_nodes,
        ast_node_count,
        source_sha256: Some(source_sha256),
        source: Some(source),
        error_type: Some(error_type),
        error_message: Some(error_message),
    }
}

fn failed_generated_result(
    seed: u64,
    status: GeneratedProbeStatus,
    source: Option<String>,
    source_sha256: Option<String>,
    error_type: String,
    error_message: String,
) -> GeneratedProbeResult {
    GeneratedProbeResult {
        seed,
        status,
        fully_accepted: false,
        ast_nodes: Vec::new(),
        ast_node_count: 0,
        source_sha256,
        source,
        error_type: Some(error_type),
        error_message: Some(error_message),
    }
}

fn build_generated_report(
    config: GeneratedProbeConfig,
    generator_version: Option<String>,
    results: Vec<GeneratedProbeResult>,
) -> GeneratedReport {
    let mut summary = BTreeMap::new();
    for status in GeneratedProbeStatus::ALL {
        summary.insert(status.as_str().to_owned(), 0);
    }
    let mut ast_node_outcomes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut fully_accepted = 0;
    for result in &results {
        if let Some(count) = summary.get_mut(result.status.as_str()) {
            *count += 1;
        }
        fully_accepted += usize::from(result.fully_accepted);
        for node in &result.ast_nodes {
            *ast_node_outcomes
                .entry(node.clone())
                .or_default()
                .entry(result.status.as_str().to_owned())
                .or_default() += 1;
        }
    }
    GeneratedReport {
        schema_version: GENERATED_SCHEMA_VERSION,
        generator: GeneratorIdentity {
            distribution: "pysource-codegen",
            version: generator_version,
        },
        safety: GeneratedSafety {
            mode: "dead_branch",
            raw_generated_code_executed: false,
            description: "generated module body is parsed under `if False`",
        },
        config,
        summary,
        fully_accepted,
        ast_node_outcomes,
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_zero_limits_and_overflowing_seed_ranges() {
        assert!(
            GeneratedProbeConfig {
                node_limit: 0,
                ..GeneratedProbeConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            GeneratedProbeConfig {
                seed_start: u64::MAX,
                seed_count: 1,
                ..GeneratedProbeConfig::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn empty_report_contains_every_stable_status() {
        let report = build_generated_report(GeneratedProbeConfig::default(), None, Vec::new());
        assert_eq!(report.summary.len(), GeneratedProbeStatus::ALL.len());
        assert!(report.summary.values().all(|count| *count == 0));
    }
}
