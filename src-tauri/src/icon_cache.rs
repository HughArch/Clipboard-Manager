use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc, RwLock};

pub struct IconCacheEntry {
    pub icon: Option<String>,
    pub access_time: std::time::Instant,
}

pub struct IconCache {
    pub cache: HashMap<String, IconCacheEntry>,
    pub access_order: BTreeMap<std::time::Instant, String>,
    pub max_size: usize,
}

impl IconCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: BTreeMap::new(),
            max_size,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<Option<String>> {
        if let Some(entry) = self.cache.get_mut(key) {
            // 更新访问时间
            self.access_order.remove(&entry.access_time);
            entry.access_time = std::time::Instant::now();
            self.access_order.insert(entry.access_time, key.to_string());
            Some(entry.icon.clone())
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: String, icon: Option<String>) {
        let now = std::time::Instant::now();
        
        // 如果缓存已满，移除最旧的条目
        while self.cache.len() >= self.max_size {
            if let Some((_, oldest_key)) = self.access_order.pop_first() {
                self.cache.remove(&oldest_key);
            } else {
                break;
            }
        }

        let entry = IconCacheEntry {
            icon,
            access_time: now,
        };

        self.cache.insert(key.clone(), entry);
        self.access_order.insert(now, key);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

// 使用改进的图标缓存
static ICON_CACHE: std::sync::OnceLock<Arc<RwLock<IconCache>>> = std::sync::OnceLock::new();

pub fn get_icon_cache() -> &'static Arc<RwLock<IconCache>> {
    ICON_CACHE.get_or_init(|| Arc::new(RwLock::new(IconCache::new(10)))) // 减少到10个条目
}

// 更严格的图标缓存清理
pub fn cleanup_icon_cache() {
    let cache = get_icon_cache();
    if let Ok(mut cache_guard) = cache.write() {
        if cache_guard.len() > 5 {  // 只保留5个最新的
            // 清空一半缓存
            let to_clear = cache_guard.len() / 2;
            for _ in 0..to_clear {
                if let Some((_, oldest_key)) = cache_guard.access_order.pop_first() {
                    cache_guard.cache.remove(&oldest_key);
                } else {
                    break;
                }
            }
            tracing::info!("清理图标缓存，保留 {} 项", cache_guard.len());
        }
    }
} 