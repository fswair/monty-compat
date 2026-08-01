use monty_compat::{CapabilityIndex, DiagnosticDisposition, lower_source};
use ruff_python_parser::parse_module;

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");

#[test]
fn lowers_gather_return_exceptions_through_supported_gather()
-> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "import asyncio\n",
        "async def fail():\n",
        "    raise ValueError('bad')\n",
        "async def main():\n",
        "    return await asyncio.gather(fail(), return_exceptions=True)\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(
        output
            .code
            .contains("async def _monty_compat_gather_return_exceptions_0")
    );
    assert!(
        output
            .code
            .contains("asyncio.gather(_monty_compat_gather_return_exceptions_0((fail())))")
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "async_gather_return_exceptions"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}

#[test]
fn hoists_module_class_comprehensions_out_of_class_scope() -> Result<(), Box<dyn std::error::Error>>
{
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "try:\n",
        "    class Item:\n",
        "        offset = 10\n",
        "        values = [value + offset for value in [1, 2]]\n",
        "except NameError:\n",
        "    result = 'name-error'\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(
        output
            .code
            .contains("def _monty_compat_class_comprehension_0():")
    );
    assert!(
        output
            .code
            .contains("return [value + offset for value in [1, 2]]")
    );
    assert!(
        output
            .code
            .contains("values = _monty_compat_class_comprehension_0()")
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "class_body_comprehension_scope"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}

#[test]
fn snapshots_exit_for_a_statically_non_raising_body() -> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "events = []\n",
        "def replacement(self, exc_type, exc, tb):\n",
        "    events.append('new')\n",
        "class Context:\n",
        "    def __enter__(self):\n",
        "        return self\n",
        "    def __exit__(self, exc_type, exc, tb):\n",
        "        events.append('old')\n",
        "with Context():\n",
        "    Context.__exit__ = replacement\n",
        "events\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(
        output
            .code
            .contains("_monty_compat_target_1 = _monty_compat_target_0.__exit__")
    );
    assert!(
        output
            .code
            .contains("_monty_compat_target_1(None, None, None)")
    );
    assert!(!output.code.contains("with Context():"));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "with_exit_bound_once"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}

#[test]
fn boxes_identity_comprehension_lambdas_in_one_shared_cell()
-> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "functions = [lambda: value for value in range(3)]\n",
        "[function() for function in functions]\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(
        output
            .code
            .contains("def _monty_compat_late_bound_identity_lambdas_0(iterable):")
    );
    assert!(output.code.contains("cell[0] = value"));
    assert!(
        output
            .code
            .contains("functions = _monty_compat_late_bound_identity_lambdas_0((range(3)))")
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "closure_late_binding"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}

#[test]
fn preserves_construction_laziness_for_statically_dead_builtin_results()
-> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let sources = [
        "calls = []\ndef visit(value):\n    calls.append(value)\n    return value\nvalues = map(visit, [1, 2, 3])\ncalls\n",
        "calls = []\ndef visit(value):\n    calls.append(value)\n    return True\nvalues = filter(visit, [1, 2, 3])\ncalls\n",
        "calls = []\nvalues = [1, 2, None]\ndef take():\n    value = values.pop(0)\n    calls.append(value)\n    return value\nindexed = enumerate(iter(take, None))\ncalls\n",
        "calls = []\nvalues = [1, 2, None]\ndef take():\n    value = values.pop(0)\n    calls.append(value)\n    return value\npairs = zip(iter(take, None), [10, 20])\ncalls\n",
    ];

    for source in sources {
        let output = lower_source(source, &capabilities)?;
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == "dead_lazy_builtin"
                && diagnostic.disposition == DiagnosticDisposition::Applied
        }));
        assert!(
            !output.diagnostics.iter().any(|diagnostic| {
                diagnostic.disposition == DiagnosticDisposition::NotLowerable
            })
        );
        assert!(parse_module(&output.code).is_ok());
    }
    Ok(())
}

#[test]
fn preserves_a_statically_dead_generator_body_without_eager_evaluation()
-> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "events = []\n",
        "def visit(value):\n",
        "    events.append(value)\n",
        "    return value\n",
        "values = (visit(value) for value in range(3))\n",
        "events\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(output.code.contains("values = (iter((range(3))),)"));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "dead_generator_expression"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.disposition == DiagnosticDisposition::NotLowerable })
    );
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}

#[test]
fn lowers_a_non_raising_async_with_return_without_faking_tracebacks()
-> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let source = concat!(
        "import asyncio\n",
        "class Context:\n",
        "    async def __aenter__(self):\n",
        "        return 3\n",
        "    async def __aexit__(self, exc_type, exc, tb):\n",
        "        return False\n",
        "async def main():\n",
        "    async with Context() as value:\n",
        "        return value\n",
        "asyncio.run(main())\n",
    );
    let output = lower_source(source, &capabilities)?;

    assert!(!output.code.contains("async with Context()"));
    assert!(
        output
            .code
            .contains("await _monty_compat_target_1(None, None, None)")
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule == "async_with_non_raising_return"
            && diagnostic.disposition == DiagnosticDisposition::Applied
    }));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.disposition == DiagnosticDisposition::NotLowerable })
    );
    assert!(parse_module(&output.code).is_ok());
    Ok(())
}
