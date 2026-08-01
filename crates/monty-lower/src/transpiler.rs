use std::{
    collections::HashMap,
    mem::size_of,
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;

use crate::{
    CapabilityIndex, LoweringDiagnostic, LoweringError, LoweringOutput, ManifestError,
    TargetFingerprint, lower_source,
};

const DEFAULT_MAX_ENTRIES: usize = 256;
const DEFAULT_MAX_BYTES: usize = 32 * 1024 * 1024;

/// Bounds for the in-memory exact-source transpilation cache.
///
/// `max_bytes` accounts for the retained source, output buffers, diagnostics,
/// and their fixed-size cache metadata. Hash table control bytes and allocator
/// bookkeeping are implementation-specific and intentionally excluded. Setting
/// either limit to zero disables the cache without changing transpilation semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl CacheConfig {
    /// Construct explicit cache bounds.
    #[must_use]
    pub const fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }

    /// Disable result caching while retaining the same `Transpiler` API.
    #[must_use]
    pub const fn disabled() -> Self {
        Self::new(0, 0)
    }

    #[must_use]
    const fn is_enabled(self) -> bool {
        self.max_entries > 0 && self.max_bytes > 0
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES)
    }
}

/// Point-in-time counters for one `Transpiler` instance.
///
/// Counters are cumulative; `entries` and `bytes` describe the current cache.
/// `bytes` uses the same retained-size estimate as [`CacheConfig::max_bytes`].
/// Under concurrent use, the snapshot is observational rather than transactional.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub insertions: u64,
    pub evictions: u64,
    pub skipped: u64,
    pub bypasses: u64,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Debug)]
struct CacheEntry {
    output: Arc<LoweringOutput>,
    last_access: u64,
    weight: usize,
}

#[derive(Debug, Default)]
struct CacheState {
    entries: HashMap<String, CacheEntry>,
    bytes: usize,
    clock: u64,
}

impl CacheState {
    fn next_access(&mut self) -> u64 {
        // Saturation preserves memory safety and boundedness even after an
        // unrealistic number of calls; ties are acceptable for LRU eviction.
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn get(&mut self, source: &str) -> Option<Arc<LoweringOutput>> {
        let access = self.next_access();
        let entry = self.entries.get_mut(source)?;
        entry.last_access = access;
        Some(Arc::clone(&entry.output))
    }

    fn evict_lru(&mut self) -> bool {
        let lru_key = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(source, _)| source.clone());
        let Some(lru_key) = lru_key else {
            return false;
        };
        let Some(entry) = self.entries.remove(&lru_key) else {
            return false;
        };
        self.bytes = self.bytes.saturating_sub(entry.weight);
        true
    }

    fn insert(
        &mut self,
        source: &str,
        output: Arc<LoweringOutput>,
        config: CacheConfig,
    ) -> InsertOutcome {
        // Another thread may have populated the cache while this caller was
        // lowering. Prefer the existing canonical Arc in that case.
        if let Some(existing) = self.get(source) {
            return InsertOutcome::Existing(existing);
        }

        let Some(weight) = cache_weight(source, &output) else {
            return InsertOutcome::Skipped(output);
        };
        if weight > config.max_bytes {
            return InsertOutcome::Skipped(output);
        }

        let mut evictions = 0u64;
        while self.entries.len() >= config.max_entries
            || self.bytes.saturating_add(weight) > config.max_bytes
        {
            if !self.evict_lru() {
                return InsertOutcome::Skipped(output);
            }
            evictions = evictions.saturating_add(1);
        }

        let access = self.next_access();
        let entry = CacheEntry {
            output: Arc::clone(&output),
            last_access: access,
            weight,
        };
        if let Some(previous) = self.entries.insert(source.to_owned(), entry) {
            // This cannot occur while the mutex is held after the lookup above,
            // but handle it defensively without panicking or corrupting totals.
            self.bytes = self.bytes.saturating_sub(previous.weight);
        }
        self.bytes = self.bytes.saturating_add(weight);
        InsertOutcome::Inserted { output, evictions }
    }
}

enum InsertOutcome {
    Existing(Arc<LoweringOutput>),
    Inserted {
        output: Arc<LoweringOutput>,
        evictions: u64,
    },
    Skipped(Arc<LoweringOutput>),
}

#[derive(Debug, Default)]
struct CacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: AtomicU64,
    evictions: AtomicU64,
    skipped: AtomicU64,
    bypasses: AtomicU64,
}

impl CacheMetrics {
    fn increment(counter: &AtomicU64) {
        // Atomic integer overflow wraps by definition and cannot panic. These
        // counters are telemetry only and never affect cache correctness.
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn add(counter: &AtomicU64, value: u64) {
        counter.fetch_add(value, Ordering::Relaxed);
    }
}

/// Long-lived, release-pinned lowering engine with a bounded exact-source cache.
///
/// Cache entries cannot cross Monty releases because every `Transpiler` owns one
/// immutable `CapabilityIndex`. Only successful lowering results are cached.
#[derive(Debug)]
pub struct Transpiler {
    capabilities: CapabilityIndex,
    config: CacheConfig,
    cache: Mutex<CacheState>,
    metrics: CacheMetrics,
}

impl Transpiler {
    /// Build a transpiler with the default bounded cache.
    #[must_use]
    pub fn new(capabilities: CapabilityIndex) -> Self {
        Self::with_cache_config(capabilities, CacheConfig::default())
    }

    /// Build a transpiler with explicit cache bounds.
    #[must_use]
    pub fn with_cache_config(capabilities: CapabilityIndex, config: CacheConfig) -> Self {
        Self {
            capabilities,
            config,
            cache: Mutex::new(CacheState::default()),
            metrics: CacheMetrics::default(),
        }
    }

    /// Parse a manifest and build a transpiler with the default cache.
    pub fn from_manifest_json(manifest: &str) -> Result<Self, ManifestError> {
        CapabilityIndex::from_json(manifest).map(Self::new)
    }

    /// Read a manifest and build a transpiler with the default cache.
    pub fn from_manifest_path(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        CapabilityIndex::from_path(path).map(Self::new)
    }

    /// Return the immutable Monty release identity for this cache namespace.
    #[must_use]
    pub const fn target(&self) -> &TargetFingerprint {
        self.capabilities.target()
    }

    /// Lower Python source, returning a shared cached artifact on an exact hit.
    pub fn transpile(&self, source: &str) -> Result<Arc<LoweringOutput>, LoweringError> {
        if !self.config.is_enabled() {
            CacheMetrics::increment(&self.metrics.bypasses);
            return lower_source(source, &self.capabilities).map(Arc::new);
        }

        if let Some(output) = self.lock_cache().get(source) {
            CacheMetrics::increment(&self.metrics.hits);
            return Ok(output);
        }
        CacheMetrics::increment(&self.metrics.misses);

        // Do not hold the mutex during parsing and lowering. Concurrent misses
        // may duplicate work, but they do not serialize unrelated source code.
        let output = Arc::new(lower_source(source, &self.capabilities)?);
        let outcome = self.lock_cache().insert(source, output, self.config);
        match outcome {
            InsertOutcome::Existing(output) => Ok(output),
            InsertOutcome::Inserted { output, evictions } => {
                CacheMetrics::increment(&self.metrics.insertions);
                CacheMetrics::add(&self.metrics.evictions, evictions);
                Ok(output)
            }
            InsertOutcome::Skipped(output) => {
                CacheMetrics::increment(&self.metrics.skipped);
                Ok(output)
            }
        }
    }

    /// Remove all cached artifacts while retaining cumulative counters.
    pub fn clear_cache(&self) {
        let mut cache = self.lock_cache();
        cache.entries.clear();
        cache.bytes = 0;
    }

    /// Return cumulative counters plus the current bounded cache size.
    #[must_use]
    pub fn cache_stats(&self) -> CacheStats {
        let cache = self.lock_cache();
        CacheStats {
            hits: self.metrics.hits.load(Ordering::Relaxed),
            misses: self.metrics.misses.load(Ordering::Relaxed),
            insertions: self.metrics.insertions.load(Ordering::Relaxed),
            evictions: self.metrics.evictions.load(Ordering::Relaxed),
            skipped: self.metrics.skipped.load(Ordering::Relaxed),
            bypasses: self.metrics.bypasses.load(Ordering::Relaxed),
            entries: cache.entries.len(),
            bytes: cache.bytes,
        }
    }

    fn lock_cache(&self) -> MutexGuard<'_, CacheState> {
        match self.cache.lock() {
            Ok(cache) => cache,
            // No production path panics while holding this lock. Recovery also
            // keeps the API safe if a caller-induced unwind poisons the mutex.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

fn cache_weight(source: &str, output: &LoweringOutput) -> Option<usize> {
    let diagnostics = output
        .diagnostics
        .capacity()
        .checked_mul(size_of::<LoweringDiagnostic>())?;
    let fixed = size_of::<String>()
        .checked_add(size_of::<CacheEntry>())?
        .checked_add(size_of::<LoweringOutput>())?;
    let buffers = source
        .len()
        .checked_add(output.code.capacity())?
        .checked_add(output.target_tag.capacity())?
        .checked_add(diagnostics)?;
    output
        .diagnostics
        .iter()
        .try_fold(fixed.checked_add(buffers)?, |weight, diagnostic| {
            weight.checked_add(diagnostic.message.capacity())
        })
}
