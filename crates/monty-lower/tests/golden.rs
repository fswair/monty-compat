use monty_compat::{CapabilityIndex, lower_source};
use ruff_python_parser::parse_module;

const MANIFEST: &str = include_str!("fixtures/manifest.json");

#[test]
fn lowering_matches_golden_sources_and_remains_parseable() -> Result<(), Box<dyn std::error::Error>>
{
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let fixtures = [
        (
            include_str!("fixtures/function_decorator.input.py"),
            include_str!("fixtures/function_decorator.expected.py"),
        ),
        (
            include_str!("fixtures/for_attribute.input.py"),
            include_str!("fixtures/for_attribute.expected.py"),
        ),
        (
            include_str!("fixtures/with_attribute.input.py"),
            include_str!("fixtures/with_attribute.expected.py"),
        ),
    ];

    for (input, expected) in fixtures {
        let output = lower_source(input, &capabilities)?;
        assert_eq!(output.code, expected);
        assert!(parse_module(&output.code).is_ok());
    }
    Ok(())
}
