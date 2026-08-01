use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{Mutex, MutexGuard, OnceLock},
};

use regex::Regex;

use crate::{CapabilityGraph, ExtractError, SourceBundle};

type Result<T> = std::result::Result<T, ExtractError>;

static REGEX_CACHE: OnceLock<Mutex<HashMap<&'static str, Regex>>> = OnceLock::new();

fn lock_regex_cache() -> MutexGuard<'static, HashMap<&'static str, Regex>> {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    match cache.lock() {
        Ok(cache) => cache,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn regex(pattern: &'static str) -> Result<Regex> {
    if let Some(compiled) = lock_regex_cache().get(pattern).cloned() {
        return Ok(compiled);
    }
    let compiled = Regex::new(pattern).map_err(|error| ExtractError::InvalidPattern {
        pattern,
        message: error.to_string(),
    })?;
    let mut cache = lock_regex_cache();
    Ok(cache
        .entry(pattern)
        .or_insert_with(|| compiled.clone())
        .clone())
}

fn pascal_to_snake(name: &str) -> Result<String> {
    let acronym = regex(r"([A-Z]+)([A-Z][a-z])")?;
    let boundary = regex(r"([a-z\d])([A-Z])")?;
    let first = acronym.replace_all(name, "${1}_${2}");
    Ok(boundary.replace_all(&first, "${1}_${2}").to_lowercase())
}

fn matching_delimiter(source: &str, start: usize, opening: u8, closing: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start).copied() != Some(opening) {
        return None;
    }

    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        if bytes.get(index..index.saturating_add(2)) == Some(b"//") {
            index = bytes[index.saturating_add(2)..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |relative| index + 2 + relative + 1);
            continue;
        }
        if bytes.get(index..index.saturating_add(2)) == Some(b"/*") {
            index = bytes[index.saturating_add(2)..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |relative| index + 2 + relative + 2);
            continue;
        }
        if bytes[index] == b'"' {
            index = index.saturating_add(1);
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = index.saturating_add(2);
                } else if bytes[index] == b'"' {
                    index = index.saturating_add(1);
                    break;
                } else {
                    index = index.saturating_add(1);
                }
            }
            continue;
        }
        if bytes[index] == opening {
            depth = depth.saturating_add(1);
        } else if bytes[index] == closing {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index);
            }
        }
        index = index.saturating_add(1);
    }
    None
}

fn delimited_contents(source: &str, start: usize, opening: u8, closing: u8) -> Option<&str> {
    let end = matching_delimiter(source, start, opening, closing)?;
    source.get(start.saturating_add(1)..end)
}

fn rust_function_bodies(source: &str) -> Result<Vec<(String, String)>> {
    let function = regex(r"\bfn\s+([A-Za-z_]\w*)\b")?;
    let mut functions = Vec::new();
    for capture in function.captures_iter(source) {
        let (Some(whole), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        let Some(params_relative) = source.get(whole.end()..).and_then(|tail| tail.find('('))
        else {
            continue;
        };
        let params_start = whole.end().saturating_add(params_relative);
        let Some(params_end) = matching_delimiter(source, params_start, b'(', b')') else {
            continue;
        };
        let Some(body_relative) = source
            .get(params_end.saturating_add(1)..)
            .and_then(|tail| tail.find('{'))
        else {
            continue;
        };
        let body_start = params_end.saturating_add(1).saturating_add(body_relative);
        let Some(body) = delimited_contents(source, body_start, b'{', b'}') else {
            continue;
        };
        functions.push((name.as_str().to_owned(), body.to_owned()));
    }
    Ok(functions)
}

fn function_bodies(source: &str, target: &str) -> Result<Vec<String>> {
    Ok(rust_function_bodies(source)?
        .into_iter()
        .filter_map(|(name, body)| (name == target).then_some(body))
        .collect())
}

fn parse_static_strings_map(source: &str) -> Result<BTreeMap<String, String>> {
    let enum_start = regex(r"\bpub(?:\([^)]*\))?\s+enum\s+StaticStrings\s*\{")?;
    let Some(found) = enum_start.find(source) else {
        return Ok(BTreeMap::new());
    };
    let Some(open) = source.get(found.start()..).and_then(|tail| tail.find('{')) else {
        return Ok(BTreeMap::new());
    };
    let Some(body) = delimited_contents(source, found.start() + open, b'{', b'}') else {
        return Ok(BTreeMap::new());
    };

    let serialize = regex(r#"#\[strum\(serialize\s*=\s*"([^"]*)"\)\]"#)?;
    let variant = regex(r"([A-Z]\w*)")?;
    let mut names = BTreeMap::new();
    let mut pending = None;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(capture) = serialize.captures(line)
            && let Some(name) = capture.get(1)
        {
            pending = Some(name.as_str().to_owned());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(capture) = variant.captures(line)
            && let Some(name) = capture.get(1)
        {
            let variant = name.as_str().to_owned();
            let python_name = match pending.take() {
                Some(name) => name,
                None => pascal_to_snake(&variant)?,
            };
            names.insert(variant, python_name);
        }
    }
    Ok(names)
}

fn parse_enum_variants(source: &str, enum_name: &str) -> Result<Vec<String>> {
    let enum_pattern = regex(r"\bpub(?:\([^)]*\))?\s+enum\s+([A-Za-z_]\w*)\s*\{")?;
    let variant_pattern = regex(r"([A-Z][A-Za-z0-9_]*)")?;
    for capture in enum_pattern.captures_iter(source) {
        let (Some(whole), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        if name.as_str() != enum_name {
            continue;
        }
        let Some(open_relative) = source.get(whole.start()..).and_then(|tail| tail.find('{'))
        else {
            return Ok(Vec::new());
        };
        let Some(body) = delimited_contents(source, whole.start() + open_relative, b'{', b'}')
        else {
            return Ok(Vec::new());
        };
        return Ok(body
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                    return None;
                }
                variant_pattern
                    .captures(line)
                    .and_then(|capture| capture.get(1))
                    .map(|name| name.as_str().to_owned())
            })
            .collect());
    }
    Ok(Vec::new())
}

fn parse_builtin_functions(source: &str) -> Result<BTreeSet<String>> {
    Ok(parse_enum_variants(source, "BuiltinsFunctions")?
        .into_iter()
        .map(|variant| variant.to_lowercase())
        .collect())
}

fn parse_builtin_type_variants(source: &str) -> Result<BTreeMap<String, String>> {
    let mapping = regex(r#""([^"]+)"\s*=>\s*Some\(Self::(\w+)\)"#)?;
    let mut names = BTreeMap::new();
    for body in function_bodies(source, "from_builtin_name")? {
        for capture in mapping.captures_iter(&body) {
            let (Some(python_name), Some(variant)) = (capture.get(1), capture.get(2)) else {
                continue;
            };
            names.insert(variant.as_str().to_owned(), python_name.as_str().to_owned());
        }
    }
    Ok(names)
}

fn without_feature_gated_items(source: &str) -> Result<String> {
    let cfg = regex(r"#\[cfg\s*\(\s*feature\s*=\s*[^)]+\)\]")?;
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();

    for found in cfg.find_iter(source) {
        let start = found.start();
        let mut item_start = found.end();
        while bytes.get(item_start).is_some_and(u8::is_ascii_whitespace) {
            item_start = item_start.saturating_add(1);
        }
        while bytes.get(item_start..item_start.saturating_add(2)) == Some(b"#[") {
            let Some(attribute_end) =
                matching_delimiter(source, item_start.saturating_add(1), b'[', b']')
            else {
                break;
            };
            item_start = attribute_end.saturating_add(1);
            while bytes.get(item_start).is_some_and(u8::is_ascii_whitespace) {
                item_start = item_start.saturating_add(1);
            }
        }

        let mut item_end = item_start;
        let mut index = item_start;
        let (mut paren_depth, mut bracket_depth, mut brace_depth) = (0usize, 0usize, 0usize);
        while index < bytes.len() {
            if bytes.get(index..index.saturating_add(2)) == Some(b"//") {
                index = bytes[index.saturating_add(2)..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |relative| index + 2 + relative + 1);
                continue;
            }
            if bytes.get(index..index.saturating_add(2)) == Some(b"/*") {
                index = bytes[index.saturating_add(2)..]
                    .windows(2)
                    .position(|window| window == b"*/")
                    .map_or(bytes.len(), |relative| index + 2 + relative + 2);
                continue;
            }
            if bytes[index] == b'"' {
                index = index.saturating_add(1);
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = index.saturating_add(2);
                    } else if bytes[index] == b'"' {
                        index = index.saturating_add(1);
                        break;
                    } else {
                        index = index.saturating_add(1);
                    }
                }
                continue;
            }
            match bytes[index] {
                b'(' => paren_depth = paren_depth.saturating_add(1),
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth = bracket_depth.saturating_add(1),
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth = brace_depth.saturating_add(1),
                b'}' if brace_depth == 1 => {
                    item_end = index.saturating_add(1);
                    break;
                }
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b';' | b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    item_end = index.saturating_add(1);
                    break;
                }
                _ => {}
            }
            index = index.saturating_add(1);
        }
        if index >= bytes.len() {
            item_end = bytes.len();
        }
        let masked_len = masked.len();
        for byte in masked
            .get_mut(start..item_end.min(masked_len))
            .into_iter()
            .flatten()
        {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    Ok(String::from_utf8_lossy(&masked).into_owned())
}

fn parse_builtin_modules(
    source: &str,
    static_strings: &BTreeMap<String, String>,
) -> Result<BTreeSet<String>> {
    let dispatch = regex(r"StaticStrings::(\w+)\s*=>\s*Some\(Self::")?;
    let names: BTreeSet<_> = dispatch
        .captures_iter(source)
        .filter_map(|capture| capture.get(1))
        .filter_map(|variant| static_strings.get(variant.as_str()).cloned())
        .collect();
    if !names.is_empty() {
        return Ok(names);
    }
    for enum_name in ["StandardLib", "BuiltinModule"] {
        let variants = parse_enum_variants(source, enum_name)?;
        if !variants.is_empty() {
            return Ok(variants
                .into_iter()
                .map(|variant| variant.to_lowercase())
                .collect());
        }
    }
    Ok(BTreeSet::new())
}

fn parse_exception_types(source: &str) -> Result<BTreeSet<String>> {
    Ok(parse_enum_variants(source, "ExcType")?
        .into_iter()
        .collect())
}

fn module_set_attr_calls(source: &str) -> Result<Vec<(String, String)>> {
    let set_attr = regex(r"\bmodule\.set_attr\s*\(")?;
    let first_argument = regex(r"^\s*\*?StaticStrings::(\w+)")?;
    let mut calls = Vec::new();
    for found in set_attr.find_iter(source) {
        let Some(open_relative) = source.get(found.start()..).and_then(|tail| tail.find('('))
        else {
            continue;
        };
        let open = found.start().saturating_add(open_relative);
        let Some(arguments) = delimited_contents(source, open, b'(', b')') else {
            continue;
        };
        let Some(variant) = first_argument
            .captures(arguments)
            .and_then(|capture| capture.get(1))
        else {
            continue;
        };
        calls.push((variant.as_str().to_owned(), arguments.to_owned()));
    }
    Ok(calls)
}

fn static_string_arrays(source: &str) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let declaration = regex(r"\b(?:const|static|let)\s+(\w+)\b")?;
    let tuple_key = regex(r"\(\s*StaticStrings::(\w+)")?;
    let any_value = regex(r"StaticStrings::(\w+)")?;
    let mut arrays = BTreeMap::new();
    for capture in declaration.captures_iter(source) {
        let (Some(whole), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        let Some(assignment_relative) = source.get(whole.end()..).and_then(|tail| tail.find('='))
        else {
            continue;
        };
        let assignment = whole.end().saturating_add(assignment_relative);
        let Some(start_relative) = source.get(assignment..).and_then(|tail| tail.find('[')) else {
            continue;
        };
        let start = assignment.saturating_add(start_relative);
        let Some(values) = delimited_contents(source, start, b'[', b']') else {
            continue;
        };
        let tuple_keys: BTreeSet<_> = tuple_key
            .captures_iter(values)
            .filter_map(|item| item.get(1))
            .map(|item| item.as_str().to_owned())
            .collect();
        let variants = if tuple_keys.is_empty() {
            any_value
                .captures_iter(values)
                .filter_map(|item| item.get(1))
                .map(|item| item.as_str().to_owned())
                .collect()
        } else {
            tuple_keys
        };
        arrays.insert(name.as_str().to_owned(), variants);
    }
    Ok(arrays)
}

fn registered_module_attributes(
    create_body: &str,
    source: &str,
    static_strings: &BTreeMap<String, String>,
) -> Result<BTreeSet<String>> {
    let mut variants: BTreeSet<_> = module_set_attr_calls(create_body)?
        .into_iter()
        .map(|(variant, _)| variant)
        .collect();
    let arrays = static_string_arrays(source)?;
    let loops = regex(r"\bfor\b[^{};]*\bin\s+(\w+)\s*\{")?;
    for capture in loops.captures_iter(create_body) {
        let (Some(whole), Some(array_name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        let Some(open_relative) = create_body
            .get(whole.start()..)
            .and_then(|tail| tail.find('{'))
        else {
            continue;
        };
        let Some(body) = delimited_contents(
            create_body,
            whole.start().saturating_add(open_relative),
            b'{',
            b'}',
        ) else {
            continue;
        };
        if body.contains("module.set_attr")
            && let Some(values) = arrays.get(array_name.as_str())
        {
            variants.extend(values.iter().cloned());
        }
    }
    Ok(variants
        .into_iter()
        .filter_map(|variant| static_strings.get(&variant).cloned())
        .collect())
}

fn module_type_bindings(
    create_body: &str,
    static_strings: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let runtime_type = regex(r"Builtins::Type\(Type::(\w+)\)")?;
    let mut bindings = BTreeMap::new();
    for (variant, arguments) in module_set_attr_calls(create_body)? {
        let Some(type_variant) = runtime_type
            .captures(&arguments)
            .and_then(|capture| capture.get(1))
        else {
            continue;
        };
        if let Some(name) = static_strings.get(&variant) {
            bindings.insert(name.clone(), type_variant.as_str().to_owned());
        }
    }
    Ok(bindings)
}

#[derive(Debug)]
struct RegisteredModule {
    name: String,
    attributes: BTreeSet<String>,
    bindings: BTreeMap<String, String>,
}

fn registered_modules(
    source: &str,
    static_strings: &BTreeMap<String, String>,
) -> Result<Vec<RegisteredModule>> {
    let module = regex(r"Module::new\(StaticStrings::(\w+)\)")?;
    let mut registered = Vec::new();
    for create_body in function_bodies(source, "create_module")? {
        let Some(module_variant) = module
            .captures(&create_body)
            .and_then(|capture| capture.get(1))
        else {
            continue;
        };
        let Some(module_name) = static_strings.get(module_variant.as_str()) else {
            continue;
        };
        registered.push(RegisteredModule {
            name: module_name.clone(),
            attributes: registered_module_attributes(&create_body, source, static_strings)?,
            bindings: module_type_bindings(&create_body, static_strings)?,
        });
    }
    Ok(registered)
}

fn pytrait_implementation_bodies(source: &str) -> Result<Vec<String>> {
    let implementation = regex(r"\bimpl(?:<[^{}]*>)?\s+PyTrait(?:<[^{}]*>)?\s+for\s+[^{}]*\{")?;
    let mut bodies = Vec::new();
    for found in implementation.find_iter(source) {
        let Some(open_relative) = source.get(found.start()..).and_then(|tail| tail.find('{'))
        else {
            continue;
        };
        let Some(body) = delimited_contents(
            source,
            found.start().saturating_add(open_relative),
            b'{',
            b'}',
        ) else {
            continue;
        };
        bodies.push(body.to_owned());
    }
    Ok(bodies)
}

fn runtime_type_variants(implementation: &str, source: &str) -> Result<BTreeSet<String>> {
    let runtime_type = regex(r"\bType::(\w+)")?;
    let call = regex(r"\b([A-Za-z_]\w*)\s*\(")?;
    let local_functions: HashMap<_, _> = rust_function_bodies(source)?.into_iter().collect();
    let mut variants = BTreeSet::new();
    for body in function_bodies(implementation, "py_type")? {
        variants.extend(
            runtime_type
                .captures_iter(&body)
                .filter_map(|capture| capture.get(1))
                .map(|variant| variant.as_str().to_owned()),
        );
        for capture in call.captures_iter(&body) {
            let Some(name) = capture.get(1) else {
                continue;
            };
            if name.as_str() == "py_type" {
                continue;
            }
            if let Some(helper) = local_functions.get(name.as_str()) {
                variants.extend(
                    runtime_type
                        .captures_iter(helper)
                        .filter_map(|capture| capture.get(1))
                        .map(|variant| variant.as_str().to_owned()),
                );
            }
        }
    }
    Ok(variants)
}

fn delegated_dispatch_calls(body: &str) -> Result<BTreeSet<String>> {
    let call = regex(r"\b([A-Za-z_]\w*)\s*\(")?;
    let selector = regex(r"\b(?:method\w*|attr\w*|ss)\b")?;
    let mut calls = BTreeSet::new();
    for capture in call.captures_iter(body) {
        let (Some(whole), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        if matches!(name.as_str(), "py_call_attr" | "py_getattr") {
            continue;
        }
        let Some(open_relative) = body.get(whole.start()..).and_then(|tail| tail.find('(')) else {
            continue;
        };
        let Some(arguments) = delimited_contents(
            body,
            whole.start().saturating_add(open_relative),
            b'(',
            b')',
        ) else {
            continue;
        };
        if selector.is_match(arguments) {
            calls.insert(name.as_str().to_owned());
        }
    }
    Ok(calls)
}

fn normalized_variant(name: &str) -> String {
    name.chars()
        .filter(|character| *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn guarded_type_variants(
    guard: Option<&str>,
    variants: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let Some(guard) = guard else {
        return Ok(variants.clone());
    };
    let predicates = regex(r"(!?)\s*is_([a-z_]+)\s*\(")?;
    let matches: Vec<_> = predicates.captures_iter(guard).collect();
    if matches.is_empty() {
        return Ok(variants.clone());
    }

    let mut selected = BTreeSet::new();
    for capture in matches {
        let (Some(negated), Some(predicate)) = (capture.get(1), capture.get(2)) else {
            continue;
        };
        let predicate = normalized_variant(predicate.as_str());
        let matching: BTreeSet<_> = variants
            .iter()
            .filter(|variant| normalized_variant(variant) == predicate)
            .cloned()
            .collect();
        if matching.is_empty() {
            continue;
        }
        if negated.as_str().is_empty() {
            selected.extend(matching);
        } else {
            selected.extend(variants.difference(&matching).cloned());
        }
    }
    Ok(if selected.is_empty() {
        variants.clone()
    } else {
        selected
    })
}

fn static_variants(source: &str) -> Result<Vec<String>> {
    let variant = regex(r"StaticStrings::(\w+)")?;
    Ok(variant
        .captures_iter(source)
        .filter_map(|capture| capture.get(1))
        .map(|name| name.as_str().to_owned())
        .collect())
}

fn attributes_from_dispatch(
    body: &str,
    variants: &BTreeSet<String>,
    static_strings: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let selector = regex(r"\b(?:method\w*|attr\w*|ss)\b")?;
    let match_header = regex(r"\bmatch\s+([^{};]+?)\s*\{")?;
    let arm_pattern = regex(
        r"(?s)(?P<pattern>(?:Some\()?StaticStrings::\w+\)?(?:\s*\|\s*(?:Some\()?StaticStrings::\w+\)?)*)(?:\s+if\s+(?P<guard>.*?))?\s*=>",
    )?;
    let equality_arm = regex(r"Some\(\w+\)\s+if\s+\w+\s*==\s*StaticStrings::(\w+)\s*=>")?;
    let matches_macro = regex(r"\bmatches!\s*\(")?;
    let mut attributes: BTreeMap<_, BTreeSet<_>> = variants
        .iter()
        .cloned()
        .map(|variant| (variant, BTreeSet::new()))
        .collect();
    let mut decisions: BTreeMap<(String, String), bool> = BTreeMap::new();

    for capture in match_header.captures_iter(body) {
        let (Some(whole), Some(subject)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        if !selector.is_match(subject.as_str()) {
            continue;
        }
        let Some(open_relative) = body.get(whole.start()..).and_then(|tail| tail.find('{')) else {
            continue;
        };
        let Some(match_body) = delimited_contents(
            body,
            whole.start().saturating_add(open_relative),
            b'{',
            b'}',
        ) else {
            continue;
        };
        let arms: Vec<_> = arm_pattern.captures_iter(match_body).collect();
        for (index, arm) in arms.iter().enumerate() {
            let Some(whole_arm) = arm.get(0) else {
                continue;
            };
            let arm_end = arms
                .get(index.saturating_add(1))
                .and_then(|next| next.get(0))
                .map_or(match_body.len(), |next| next.start());
            let arm_body = match_body.get(whole_arm.end()..arm_end).unwrap_or_default();
            let supported = !arm_body.contains("not_implemented(");
            let targets =
                guarded_type_variants(arm.name("guard").map(|guard| guard.as_str()), variants)?;
            let Some(pattern) = arm.name("pattern") else {
                continue;
            };
            for static_variant in static_variants(pattern.as_str())? {
                let Some(name) = static_strings.get(&static_variant) else {
                    continue;
                };
                for variant in &targets {
                    decisions
                        .entry((variant.clone(), name.clone()))
                        .or_insert(supported);
                }
            }
        }

        for direct in equality_arm.captures_iter(match_body) {
            let Some(static_variant) = direct.get(1) else {
                continue;
            };
            if let Some(name) = static_strings.get(static_variant.as_str()) {
                for variant in variants {
                    decisions
                        .entry((variant.clone(), name.clone()))
                        .or_insert(true);
                }
            }
        }
    }

    for found in matches_macro.find_iter(body) {
        let Some(open_relative) = body.get(found.start()..).and_then(|tail| tail.find('(')) else {
            continue;
        };
        let Some(contents) = delimited_contents(
            body,
            found.start().saturating_add(open_relative),
            b'(',
            b')',
        ) else {
            continue;
        };
        let first_argument = contents
            .split_once(',')
            .map_or(contents, |(first, _)| first);
        if !selector.is_match(first_argument) {
            continue;
        }
        for static_variant in static_variants(contents)? {
            if let Some(name) = static_strings.get(&static_variant) {
                for variant in variants {
                    decisions
                        .entry((variant.clone(), name.clone()))
                        .or_insert(true);
                }
            }
        }
    }

    for line in body.lines() {
        if !line.contains("StaticStrings::") || !line.contains("if") || !selector.is_match(line) {
            continue;
        }
        let targets = guarded_type_variants(Some(line), variants)?;
        for static_variant in static_variants(line)? {
            if let Some(name) = static_strings.get(&static_variant) {
                for variant in &targets {
                    decisions
                        .entry((variant.clone(), name.clone()))
                        .or_insert(true);
                }
            }
        }
    }

    for ((variant, name), supported) in decisions {
        if supported && let Some(names) = attributes.get_mut(&variant) {
            names.insert(name);
        }
    }
    Ok(attributes)
}

fn type_dispatch_attributes(
    implementation: &str,
    function_index: &BTreeMap<String, Vec<String>>,
    static_strings: &BTreeMap<String, String>,
    variants: &BTreeSet<String>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut pending: Vec<_> = rust_function_bodies(implementation)?
        .into_iter()
        .filter_map(|(name, body)| {
            matches!(name.as_str(), "py_call_attr" | "py_getattr").then_some(body)
        })
        .collect();
    let mut visited = HashSet::new();
    let mut pending_helpers = BTreeSet::new();
    let mut attributes: BTreeMap<_, BTreeSet<_>> = variants
        .iter()
        .cloned()
        .map(|variant| (variant, BTreeSet::new()))
        .collect();

    while let Some(body) = pending.pop() {
        if !visited.insert(body.clone()) {
            continue;
        }
        for (variant, names) in attributes_from_dispatch(&body, variants, static_strings)? {
            attributes.entry(variant).or_default().extend(names);
        }
        pending_helpers.extend(delegated_dispatch_calls(&body)?);
        while let Some(helper) = pending_helpers.pop_first() {
            if let Some(helper_bodies) = function_index.get(&helper) {
                for helper_body in helper_bodies {
                    if !visited.contains(helper_body) && helper_body.contains("StaticStrings::") {
                        pending.push(helper_body.clone());
                    }
                }
            }
        }
    }
    Ok(attributes)
}

fn class_method_attributes(
    source: &str,
    static_strings: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let class_method =
        regex(r"\(\s*Self::(\w+)\s*,\s*(\w+)\s*\)\s*if\s+(\w+)\s*==\s*StaticStrings::(\w+)")?;
    let mut attributes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for body in function_bodies(source, "call_class_method")? {
        for capture in class_method.captures_iter(&body) {
            let (Some(whole), Some(variant), Some(binding), Some(guard_binding), Some(name)) = (
                capture.get(0),
                capture.get(1),
                capture.get(2),
                capture.get(3),
                capture.get(4),
            ) else {
                continue;
            };
            if binding.as_str() != guard_binding.as_str() {
                continue;
            }
            let lookahead_end = whole.end().saturating_add(500).min(body.len());
            if body
                .get(whole.end()..lookahead_end)
                .is_some_and(|tail| tail.contains("not_implemented"))
            {
                continue;
            }
            if let Some(python_name) = static_strings.get(name.as_str()) {
                attributes
                    .entry(variant.as_str().to_owned())
                    .or_default()
                    .insert(python_name.clone());
            }
        }
    }
    Ok(attributes)
}

/// Build the complete static capability graph from an already loaded source bundle.
pub fn extract_sources(sources: &SourceBundle) -> Result<CapabilityGraph> {
    let static_strings = parse_static_strings_map(&without_feature_gated_items(&sources.intern)?)?;
    let active_types = without_feature_gated_items(&sources.types)?;
    let builtin_type_variants = parse_builtin_type_variants(&active_types)?;
    let active_modules = without_feature_gated_items(&sources.modules)?;
    let active_files: BTreeMap<_, _> = sources
        .rust_files
        .iter()
        .map(|(path, source)| Ok((path.clone(), without_feature_gated_items(source)?)))
        .collect::<Result<_>>()?;

    let mut registered_attributes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut registered_bindings: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for source in active_files.values() {
        for module in registered_modules(source, &static_strings)? {
            registered_attributes
                .entry(module.name.clone())
                .or_default()
                .extend(module.attributes);
            registered_bindings
                .entry(module.name)
                .or_default()
                .extend(module.bindings);
        }
    }

    let parsed_modules = parse_builtin_modules(&active_modules, &static_strings)?;
    let modules = if parsed_modules.is_empty() {
        registered_attributes.keys().cloned().collect()
    } else {
        parsed_modules
    };
    let module_attributes: BTreeMap<_, _> = modules
        .iter()
        .cloned()
        .map(|name| {
            let attributes = registered_attributes
                .get(&name)
                .cloned()
                .unwrap_or_default();
            (name, attributes)
        })
        .collect();

    let mut type_paths: BTreeMap<String, BTreeSet<String>> = builtin_type_variants
        .iter()
        .map(|(variant, name)| (variant.clone(), BTreeSet::from([name.clone()])))
        .collect();
    for module in &modules {
        if let Some(bindings) = registered_bindings.get(module) {
            for (export, variant) in bindings {
                type_paths
                    .entry(variant.clone())
                    .or_default()
                    .insert(format!("{module}.{export}"));
            }
        }
    }

    let mut function_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for source in active_files.values() {
        for (name, body) in rust_function_bodies(source)? {
            function_index.entry(name).or_default().push(body);
        }
    }

    let mut type_variant_attributes = class_method_attributes(&active_types, &static_strings)?;
    for source in active_files.values() {
        for implementation in pytrait_implementation_bodies(source)? {
            let mut variants = runtime_type_variants(&implementation, source)?;
            variants.remove("Type");
            if variants.is_empty() {
                continue;
            }
            for (variant, attributes) in type_dispatch_attributes(
                &implementation,
                &function_index,
                &static_strings,
                &variants,
            )? {
                type_variant_attributes
                    .entry(variant)
                    .or_default()
                    .extend(attributes);
            }
        }
    }

    let mut type_attributes = BTreeMap::new();
    for (variant, paths) in type_paths {
        let attributes = type_variant_attributes
            .get(&variant)
            .cloned()
            .unwrap_or_default();
        for path in paths {
            type_attributes.insert(path, attributes.clone());
        }
    }

    Ok(CapabilityGraph {
        builtin_functions: parse_builtin_functions(&sources.builtins)?,
        type_constructors: builtin_type_variants.values().cloned().collect(),
        exception_types: parse_exception_types(&sources.exceptions)?,
        modules,
        module_attributes,
        type_attributes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiter_scanner_ignores_strings_and_comments() {
        let source = r#"fn demo() { let text = "}"; /* } */ value() }"#;
        let start = source.find('{').unwrap_or(source.len());
        let body = delimited_contents(source, start, b'{', b'}');
        assert!(body.is_some_and(|body| body.contains("value()")));
    }

    #[test]
    fn truncated_delimiters_are_rejected_without_unwinding() {
        assert_eq!(delimited_contents("fn demo() {", 10, b'{', b'}'), None);
        assert_eq!(delimited_contents("", 0, b'{', b'}'), None);
    }

    #[test]
    fn feature_gated_items_are_masked_but_newlines_survive() {
        let source = "before\n#[cfg(feature = \"test\")]\nvalue(),\nafter\n";
        let masked = without_feature_gated_items(source).expect("static pattern should compile");
        assert!(!masked.contains("value"));
        assert_eq!(masked.lines().count(), source.lines().count());
        assert!(masked.contains("before"));
        assert!(masked.contains("after"));
    }
}
