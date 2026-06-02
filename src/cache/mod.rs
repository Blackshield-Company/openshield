use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// A cached response entry with expiration timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub response: String,
    pub expires_at: u64,
}

/// In-memory + disk response cache with TTL-based expiration.
#[derive(Clone, Debug)]
pub struct ResponseCache {
    inner: Arc<Mutex<HashMap<String, CachedResponse>>>,
    cache_file: PathBuf,
    stats: Arc<Mutex<CacheStats>>,
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub sets: u64,
}

impl ResponseCache {
    /// Create a new cache, loading existing entries from disk if present.
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?
            .join("openshark");

        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache dir: {:?}", cache_dir))?;

        let cache_file = cache_dir.join("response_cache.json");
        Self::with_file(&cache_file)
    }

    /// Create a cache with a specific file path (useful for testing).
    pub fn with_file(cache_file: &std::path::Path) -> Result<Self> {
        let cache_dir = cache_file.parent().unwrap_or(std::path::Path::new("."));
        fs::create_dir_all(cache_dir)
            .with_context(|| format!("Failed to create cache dir: {:?}", cache_dir))?;

        let inner = if cache_file.exists() {
            match fs::read_to_string(cache_file) {
                Ok(data) => {
                    if data.trim().is_empty() {
                        Arc::new(Mutex::new(HashMap::new()))
                    } else {
                        match serde_json::from_str::<HashMap<String, CachedResponse>>(&data) {
                            Ok(parsed) => {
                                let now = current_timestamp_secs();
                                let valid: HashMap<String, CachedResponse> = parsed
                                    .into_iter()
                                    .filter(|(_, v)| v.expires_at > now)
                                    .collect();
                                Arc::new(Mutex::new(valid))
                            }
                            Err(_) => Arc::new(Mutex::new(HashMap::new())),
                        }
                    }
                }
                Err(_) => Arc::new(Mutex::new(HashMap::new())),
            }
        } else {
            Arc::new(Mutex::new(HashMap::new()))
        };

        Ok(Self {
            inner,
            cache_file: cache_file.to_path_buf(),
            stats: Arc::new(Mutex::new(CacheStats::default())),
        })
    }

    /// Retrieve a cached response if it exists and has not expired.
    pub fn get(&self, key: &str) -> Option<CachedResponse> {
        let map = self.inner.lock().ok()?;
        let entry = map.get(key)?;
        let now = current_timestamp_secs();
        let is_valid = entry.expires_at > now;
        let result = entry.clone();
        drop(map);
        if is_valid {
            if let Ok(mut stats) = self.stats.lock() {
                stats.hits += 1;
            }
            Some(result)
        } else {
            if let Ok(mut stats) = self.stats.lock() {
                stats.misses += 1;
            }
            None
        }
    }

    /// Store a response in the cache with a TTL in seconds.
    /// Persistence is done asynchronously to avoid blocking the caller.
    pub fn set(&self, key: &str, response: &str, ttl_secs: u64) -> Result<()> {
        let expires_at = current_timestamp_secs().saturating_add(ttl_secs);
        let entry = CachedResponse {
            response: response.to_string(),
            expires_at,
        };
        {
            let mut map = self
                .inner
                .lock()
                .map_err(|e| anyhow::anyhow!("Cache lock poisoned: {}", e))?;
            map.insert(key.to_string(), entry);
        }
        if let Ok(mut stats) = self.stats.lock() {
            stats.sets += 1;
        }
        // Persist asynchronously to avoid blocking the hot path
        let cache_file = self.cache_file.clone();
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let _ = Self::persist_sync(&inner, &cache_file);
        });
        Ok(())
    }

    /// Synchronous persist helper (called from async context).
    fn persist_sync(
        inner: &Arc<Mutex<HashMap<String, CachedResponse>>>,
        cache_file: &std::path::Path,
    ) -> Result<()> {
        let map = inner
            .lock()
            .map_err(|e| anyhow::anyhow!("Cache lock poisoned: {}", e))?;
        let json =
            serde_json::to_string_pretty(&*map).with_context(|| "Failed to serialize cache")?;
        let cache_file = cache_file.to_path_buf();
        drop(map);
        fs::write(&cache_file, json)
            .with_context(|| format!("Failed to write cache file: {:?}", cache_file))?;
        Ok(())
    }

    /// Remove a single entry from the cache.
    #[allow(dead_code)]
    pub fn invalidate(&self, key: &str) -> Result<()> {
        {
            let mut map = self
                .inner
                .lock()
                .map_err(|e| anyhow::anyhow!("Cache lock poisoned: {}", e))?;
            map.remove(key);
        }
        // Async persist
        let cache_file = self.cache_file.clone();
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let _ = Self::persist_sync(&inner, &cache_file);
        });
        Ok(())
    }

    /// Clear all cached entries.
    #[allow(dead_code)]
    pub fn clear(&self) -> Result<()> {
        {
            let mut map = self
                .inner
                .lock()
                .map_err(|e| anyhow::anyhow!("Cache lock poisoned: {}", e))?;
            map.clear();
        }
        if let Ok(mut stats) = self.stats.lock() {
            *stats = CacheStats::default();
        }
        // Async persist
        let cache_file = self.cache_file.clone();
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let _ = Self::persist_sync(&inner, &cache_file);
        });
        Ok(())
    }

    /// Get cache statistics.
    pub fn get_stats(&self) -> CacheStats {
        match self.stats.lock() {
            Ok(stats) => stats.clone(),
            Err(_) => CacheStats::default(),
        }
    }

    /// Get the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Check if the cache is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Compute a cache key from model name and serialized messages.
///
/// This intentionally does NOT include API keys so that cache entries
/// are portable across different key configurations.
pub fn compute_cache_key(model: &str, messages_json: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    model.hash(&mut hasher);
    messages_json.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{}:{:x}", model, hash)
}

fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_set_and_get() {
        let cache_dir = dirs::cache_dir().unwrap().join("openshark");
        let cache_file = cache_dir.join(format!("test_cache_set_get_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&cache_file);

        let cache = ResponseCache::with_file(&cache_file).unwrap();
        let key = "test_key";
        let response = "Hello, cache!";

        assert!(cache.get(key).is_none());
        cache.set(key, response, 3600).unwrap();
        let cached = cache.get(key).unwrap();
        assert_eq!(cached.response, response);

        let _ = std::fs::remove_file(&cache_file);
    }

    #[test]
    fn test_cache_expiration() {
        let cache = ResponseCache::new().unwrap();
        let key = "expiring_key";
        cache.set(key, "short lived", 1).unwrap();
        assert!(cache.get(key).is_some());
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(cache.get(key).is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = ResponseCache::new().unwrap();
        let key = "invalidate_key";
        cache.set(key, "value", 3600).unwrap();
        assert!(cache.get(key).is_some());
        cache.invalidate(key).unwrap();
        assert!(cache.get(key).is_none());
    }

    #[test]
    fn test_cache_clear() {
        let cache = ResponseCache::new().unwrap();
        cache.set("a", "1", 3600).unwrap();
        cache.set("b", "2", 3600).unwrap();
        cache.clear().unwrap();
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_none());
    }

    #[test]
    fn test_compute_cache_key() {
        let key1 = compute_cache_key("gpt-4", "[{\"role\":\"user\",\"content\":\"hi\"}]");
        let key2 = compute_cache_key("gpt-4", "[{\"role\":\"user\",\"content\":\"hi\"}]");
        let key3 = compute_cache_key("gpt-3.5", "[{\"role\":\"user\",\"content\":\"hi\"}]");
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_cache_persistence() {
        let cache_dir = dirs::cache_dir().unwrap().join("openshark");
        let cache_file = cache_dir.join(format!("test_cache_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&cache_file);

        let key = format!("persist_key_{}", std::process::id());
        let cache = ResponseCache::with_file(&cache_file).unwrap();
        cache.set(&key, "persist_value", 3600).unwrap();

        let cache2 = ResponseCache::with_file(&cache_file).unwrap();
        let cached = cache2.get(&key);
        assert!(cached.is_some(), "Expected cached value to be persisted");
        assert_eq!(cached.unwrap().response, "persist_value");

        let _ = cache2.invalidate(&key);
        let _ = std::fs::remove_file(&cache_file);
    }
}
