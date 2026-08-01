use monty_compat::{CapabilityIndex, DiagnosticDisposition, lower_source};

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");

#[test]
fn non_representable_seams_are_never_silent() -> Result<(), Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let cases = [
        "async def run():\n    async for item in items:\n        pass\n",
        "async def run():\n    async with context:\n        pass\n",
        "class First:\n    pass\nclass Second:\n    pass\nitem = First()\nitem.__class__ = Second\n",
        "values = (value for value in range(3))\ntype(values) is list\n",
        "def values():\n    yield 1\nlist(values())\n",
        "def values():\n    yield from [1, 2]\n",
        "try:\n    raise ValueError()\nexcept* ValueError:\n    pass\n",
        "try:\n    raise ValueError()\nexcept ValueError as error:\n    raise TypeError() from error\n",
        "value = 1\ndel value\n",
        "class Item:\n    pass\nitem = Item()\ndel item.value\n",
        "values = map(str, [1, 2])\nlist(values)\n",
        "class Context:\n    def __enter__(self):\n        return self\n    def __exit__(self, exc_type, exc, tb):\n        return True\nwith Context():\n    raise ValueError('bad')\n",
        "with Context():\n    pass\n",
        "class Box:\n    def __rsub__(self, other):\n        return 1\n1 - Box()\n",
        "class Box:\n    def __int__(self):\n        return 1\nint(Box())\n",
        "class Box:\n    def __float__(self):\n        return 1.0\nfloat(Box())\n",
        "class Box:\n    def __index__(self):\n        return 1\nhex(Box())\n",
    ];
    for source in cases {
        let output = lower_source(source, &capabilities)?;
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.disposition == DiagnosticDisposition::NotLowerable),
            "missing not_lowerable diagnostic for {source:?}"
        );
    }
    Ok(())
}
