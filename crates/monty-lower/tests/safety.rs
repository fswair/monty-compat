use std::panic::catch_unwind;

use monty_compat::{CapabilityIndex, lower_source};

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");

#[test]
fn arbitrary_source_never_unwinds_through_the_public_api() -> Result<(), Box<dyn std::error::Error>>
{
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let corpus = [
        "",
        "é = 1\né",
        "🙂",
        "def",
        "@decorator\ndef value():\n    return 1\n",
        "match value:\n    case [first, *rest]:\n        result = rest\n",
        "x = {'a': 1}\ndel x['a']\n",
        "del unknown\n",
        "async def run():\n    async for value in values:\n        pass\n",
        "def values():\n    yield from [1, 2]\n",
        "f'{value:🙂}'",
        "class Outer:\n    class Inner:\n        pass\n",
        "\0",
    ];
    for source in corpus {
        let guarded = catch_unwind(|| lower_source(source, &capabilities));
        assert!(guarded.is_ok(), "lowering unwound for {source:?}");
    }
    Ok(())
}

#[test]
fn generated_utf8_and_truncated_inputs_never_unwind() -> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let alphabet = ['a', ' ', '\n', ':', '[', ']', '\'', 'é', '١', '🙂'];
    let mut source = String::new();
    for (index, character) in alphabet.iter().cycle().take(512).enumerate() {
        source.push(*character);
        if index % 7 == 0 {
            let guarded = catch_unwind(|| lower_source(&source, &capabilities));
            assert!(
                guarded.is_ok(),
                "lowering unwound at generated prefix {index}"
            );
        }
    }
    Ok(())
}
