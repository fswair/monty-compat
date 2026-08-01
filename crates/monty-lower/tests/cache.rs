use std::{sync::Arc, thread};

use monty_compat::{CacheConfig, CapabilityIndex, Transpiler};

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");
const MIB: usize = 1024 * 1024;

fn transpiler(config: CacheConfig) -> Result<Transpiler, Box<dyn std::error::Error>> {
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    Ok(Transpiler::with_cache_config(capabilities, config))
}

#[test]
fn exact_source_hit_reuses_the_lowered_artifact() -> Result<(), Box<dyn std::error::Error>> {
    let transpiler = transpiler(CacheConfig::new(8, MIB))?;
    let source = "value = 2\nmatch value:\n    case 2:\n        result = 'two'\n    case _:\n        result = 'other'\nresult\n";

    let first = transpiler.transpile(source)?;
    let after_miss = transpiler.cache_stats();
    let second = transpiler.transpile(source)?;
    let after_hit = transpiler.cache_stats();

    assert!(first.changed);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(after_miss.misses, 1);
    assert_eq!(after_miss.insertions, 1);
    assert_eq!(after_miss.hits, 0);
    assert_eq!(after_miss.entries, 1);
    assert!(after_miss.bytes > source.len() + first.code.len());
    assert!(after_miss.bytes <= MIB);
    assert_eq!(after_hit.hits, 1);
    assert_eq!(after_hit.misses, 1);
    assert_eq!(after_hit.entries, 1);
    Ok(())
}

#[test]
fn lru_eviction_retains_the_recent_entry() -> Result<(), Box<dyn std::error::Error>> {
    let transpiler = transpiler(CacheConfig::new(2, MIB))?;
    let first = "value = 1\nvalue\n";
    let second = "value = 2\nvalue\n";
    let third = "value = 3\nvalue\n";

    transpiler.transpile(first)?;
    transpiler.transpile(second)?;
    transpiler.transpile(first)?;
    transpiler.transpile(third)?;
    let before_second_lookup = transpiler.cache_stats();
    transpiler.transpile(second)?;
    let after_second_lookup = transpiler.cache_stats();

    assert_eq!(before_second_lookup.hits, 1);
    assert_eq!(before_second_lookup.evictions, 1);
    assert_eq!(before_second_lookup.entries, 2);
    assert_eq!(after_second_lookup.misses, before_second_lookup.misses + 1);
    assert_eq!(after_second_lookup.evictions, 2);
    assert_eq!(after_second_lookup.entries, 2);
    Ok(())
}

#[test]
fn oversized_artifact_is_returned_but_not_cached() -> Result<(), Box<dyn std::error::Error>> {
    let transpiler = transpiler(CacheConfig::new(8, 16))?;
    let source = "a_long_variable_name = 123\na_long_variable_name\n";

    let first = transpiler.transpile(source)?;
    let second = transpiler.transpile(source)?;
    let stats = transpiler.cache_stats();

    assert_eq!(first, second);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.skipped, 2);
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.bytes, 0);
    Ok(())
}

#[test]
fn disabled_cache_always_bypasses_storage() -> Result<(), Box<dyn std::error::Error>> {
    let transpiler = transpiler(CacheConfig::disabled())?;
    let source = "value = 1\nvalue\n";

    let first = transpiler.transpile(source)?;
    let second = transpiler.transpile(source)?;
    let stats = transpiler.cache_stats();

    assert_eq!(first, second);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(stats.bypasses, 2);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.entries, 0);
    Ok(())
}

#[test]
fn clear_removes_entries_without_changing_release_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let transpiler = transpiler(CacheConfig::new(8, MIB))?;
    transpiler.transpile("value = 1\nvalue\n")?;

    transpiler.clear_cache();
    let stats = transpiler.cache_stats();

    assert_eq!(transpiler.target().tag, "v0.0.19");
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.bytes, 0);
    assert_eq!(stats.insertions, 1);
    Ok(())
}

#[test]
fn concurrent_callers_receive_one_canonical_cached_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    let transpiler = Arc::new(transpiler(CacheConfig::new(8, MIB))?);
    let source = "value = 2\nmatch value:\n    case 2:\n        result = 'two'\n    case _:\n        result = 'other'\nresult\n";
    let mut handles = Vec::new();

    for _ in 0..8 {
        let transpiler = Arc::clone(&transpiler);
        handles.push(thread::spawn(move || transpiler.transpile(source)));
    }

    let mut outputs = Vec::new();
    for handle in handles {
        let result = handle.join().map_err(|_| "transpiler thread panicked")?;
        outputs.push(result?);
    }
    let Some(first) = outputs.first() else {
        return Err("concurrency test produced no output".into());
    };
    assert!(outputs.iter().all(|output| Arc::ptr_eq(first, output)));
    assert_eq!(transpiler.cache_stats().entries, 1);
    Ok(())
}
