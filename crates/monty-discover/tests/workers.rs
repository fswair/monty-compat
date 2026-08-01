use std::{path::PathBuf, time::Duration};

use monty_compat_discover::{
    GeneratedProbeConfig, MinimizationConfig, MinimizationOutcome, MontyResponse, MontyWorker,
    OracleResponse, PythonWorker, WorkerError, run_baseline_probes, run_generated_discovery,
    run_generated_probes,
};

fn discovery_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_monty-discover"))
}

#[test]
fn generated_failure_is_minimized_with_the_exact_monty_fingerprint() {
    let python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.venv/bin/python");
    if !python.is_file() {
        return;
    }
    let config = GeneratedProbeConfig {
        seed_start: 3,
        seed_count: 1,
        node_limit: 20,
        depth_limit: 3,
        ..GeneratedProbeConfig::default()
    };
    let report = run_generated_discovery(
        &config,
        &MinimizationConfig::default(),
        &python.to_string_lossy(),
        &discovery_binary(),
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
    .expect("generated minimization should run");
    assert_eq!(report.generated_corpus.summary["unsupported_parse"], 1);
    assert_eq!(report.minimized_failures.summary["minimized"], 1);
    assert_eq!(report.promotion_candidates.candidate_count, 1);
    let minimized = &report.minimized_failures.results[0];
    assert_eq!(minimized.outcome, MinimizationOutcome::Minimized);
    assert!(minimized.checker_calls > 0);
    assert!(
        minimized
            .minimized_bytes
            .is_some_and(|bytes| bytes < minimized.original_bytes)
    );
    assert_eq!(
        report.promotion_candidates.candidates[0].disposition,
        "needs_semantic_probe"
    );
    assert!(
        minimized
            .minimized_source
            .as_deref()
            .is_some_and(|source| source.contains("match"))
    );
}

#[test]
fn generated_minimization_stops_at_the_candidate_budget() {
    let python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.venv/bin/python");
    if !python.is_file() {
        return;
    }
    let config = GeneratedProbeConfig {
        seed_start: 3,
        seed_count: 1,
        node_limit: 20,
        depth_limit: 3,
        ..GeneratedProbeConfig::default()
    };
    let report = run_generated_discovery(
        &config,
        &MinimizationConfig {
            enabled: true,
            max_checks: 1,
        },
        &python.to_string_lossy(),
        &discovery_binary(),
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
    .expect("a minimizer budget exhaustion should remain reportable");
    assert_eq!(report.minimized_failures.summary["error"], 1);
    assert_eq!(report.minimized_failures.results[0].checker_calls, 1);
    assert_eq!(
        report.minimized_failures.results[0].error_type.as_deref(),
        Some("_MinimizationBudgetExceeded")
    );
}

#[test]
fn rust_baseline_matches_the_release_019_capability_counts() {
    let python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.venv/bin/python");
    if !python.is_file() {
        return;
    }
    let report = run_baseline_probes(
        &python.to_string_lossy(),
        &discovery_binary(),
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
    .expect("baseline discovery should run");
    assert_eq!(report.features.len(), 269);
    assert_eq!(report.summary["supported"], 160);
    assert_eq!(report.summary["semantic_mismatch"], 32);
    assert_eq!(report.summary["unsupported_parse"], 30);
    assert_eq!(report.summary["unsupported_runtime"], 47);
    assert_eq!(
        report.features["expression.large_integer"].actual,
        serde_json::from_str::<serde_json::Value>("1000000000000000000000000000000")
            .expect("large JSON integer should parse")
    );
}

#[test]
fn monty_worker_runs_in_a_child_and_enforces_timeout() {
    let binary = discovery_binary();
    let mut worker =
        MontyWorker::start(&binary, Duration::from_secs(10)).expect("Monty worker should start");
    let response = worker.run("1 + 2");
    assert!(
        matches!(
            &response,
            Ok(MontyResponse::Return {
                value: serde_json::Value::Number(value),
                is_none: false,
                ..
            })
            if value.as_i64() == Some(3)
        ),
        "unexpected Monty worker response: {response:?}"
    );

    let mut worker =
        MontyWorker::start(&binary, Duration::from_millis(100)).expect("Monty worker should start");
    assert!(matches!(
        worker.run("while True:\n    pass"),
        Err(WorkerError::Timeout(_))
    ));
}

#[test]
fn python_worker_exposes_catalog_oracle_and_generated_parity_slice() {
    let python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.venv/bin/python");
    if !python.is_file() {
        return;
    }
    let python = python.to_string_lossy();
    let mut worker =
        PythonWorker::start(&python, Duration::from_secs(10)).expect("Python worker should start");
    let catalog = worker.catalog().expect("catalog should load");
    let environment = worker
        .environment_info()
        .expect("environment info should load");
    assert_eq!(environment.implementation, "cpython");
    assert!(catalog.len() >= 220);
    assert!(matches!(
        worker.oracle("1 + 2"),
        Ok(OracleResponse::Return {
            value: serde_json::Value::Number(value),
            ..
        }) if value.as_i64() == Some(3)
    ));
    assert!(matches!(
        worker.oracle("float('nan')"),
        Ok(OracleResponse::Return {
            value: serde_json::Value::Object(value),
            ..
        }) if value.get("__monty_compat_nonfinite__").and_then(serde_json::Value::as_str)
            == Some("nan")
    ));
    drop(worker);

    let config = GeneratedProbeConfig {
        seed_count: 5,
        node_limit: 20,
        depth_limit: 3,
        ..GeneratedProbeConfig::default()
    };
    let report = run_generated_probes(
        &config,
        &python,
        &discovery_binary(),
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
    .expect("generated discovery should run");
    assert_eq!(report.summary["completed"], 2, "{:#?}", report.results);
    assert_eq!(report.summary["unsupported_parse"], 2);
    assert_eq!(report.summary["unsupported_runtime"], 1);
    assert_eq!(report.results.len(), 5);
    assert_eq!(
        report.results[3].error_type.as_deref(),
        Some("MontyRuntimeError")
    );
    assert!(
        report.results[3]
            .error_message
            .as_deref()
            .is_some_and(|message| message.starts_with("NotImplementedError: "))
    );
}
