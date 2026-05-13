use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::time::{Instant};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tracing::info;
use serde::{Serialize, Deserialize};
use crate::types::{Progress, ProgressPlan, PhaseInfo, ProgressReporter, ConfigCache};
use crate::server::watch_filter::WatcherFilter;
use crate::db;
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct RpcProgressReporter {
    pub tx: mpsc::Sender<Vec<u8>>,
}

impl ProgressReporter for RpcProgressReporter {
    fn report(&self, stage: &str, current: usize, total: usize, message: &str) {
        let p = Progress {
            msg_type: "progress".to_string(),
            stage: stage.to_string(),
            current,
            total,
            message: message.to_string(),
        };
        let notification = (2, "progress", p);
        if let Ok(vec) = rmp_serde::to_vec(&notification) {
            let mut out = Vec::with_capacity(vec.len() + 4);
            let len = vec.len() as u32;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(&vec);
            let _ = self.tx.blocking_send(out);
        }
    }

    fn report_plan(&self, phases: &[PhaseInfo]) {
        let plan = ProgressPlan {
            msg_type: "progress_plan".to_string(),
            phases: phases.to_vec(),
        };
        let notification = (2, "progress_plan", plan);
        if let Ok(vec) = rmp_serde::to_vec(&notification) {
            let mut out = Vec::with_capacity(vec.len() + 4);
            let len = vec.len() as u32;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(&vec);
            let _ = self.tx.blocking_send(out);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub db_path: String,
    #[serde(default)]
    pub vcs_hash: Option<String>,
    #[serde(skip, default = "Instant::now")]
    pub _last_refresh: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct AssetGraph {
    pub references: HashMap<Arc<str>, HashSet<Arc<str>>>,
    pub derived: HashMap<Arc<str>, HashSet<Arc<str>>>,
    pub functions: HashMap<Arc<str>, HashSet<Arc<str>>>,
}

/// cache_key -> (completion_results, hit_count)
/// cache_key format: "{class_name}:{prefix}:{accessor_class}"
pub struct CompletionCache {
    /// LRU Cache: cache_key -> (JSON Value, hit_count). Limit to 50,000 entries.
    pub lru: LruCache<String, (serde_json::Value, u64)>,
    /// Map of class_name -> set of cache_keys in LRU, for O(1) class-level invalidation.
    pub class_to_keys: HashMap<String, HashSet<String>>,
}

impl Default for CompletionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionCache {
    pub fn new() -> Self {
        Self {
            lru: LruCache::new(NonZeroUsize::new(50000).unwrap()),
            class_to_keys: HashMap::new(),
        }
    }

    pub fn get(&mut self, cache_key: &str) -> Option<serde_json::Value> {
        if let Some((val, hit)) = self.lru.get_mut(cache_key) {
            *hit += 1;
            return Some(val.clone());
        }
        None
    }

    /// Store a result in the cache.
    /// `class_name` is the actual class name (for invalidation grouping).
    /// `cache_key` is the full compound lookup key `"{class_name}:{prefix}:{accessor}"`.
    pub fn put(&mut self, class_name: &str, cache_key: &str, results: serde_json::Value) {
        // Register this cache_key under the class_name for O(1) invalidation
        self.class_to_keys.entry(class_name.to_string())
            .or_default()
            .insert(cache_key.to_string());
            
        self.lru.put(cache_key.to_string(), (results, 1));
    }

    pub fn invalidate_class(&mut self, class_name: &str) {
        if let Some(keys) = self.class_to_keys.remove(class_name) {
            info!("Invalidating completion cache for class: {} ({} entries)", class_name, keys.len());
            for key in keys {
                self.lru.pop(&key);
            }
        }
    }

    pub fn clear(&mut self) {
        self.lru.clear();
        self.class_to_keys.clear();
    }
}

pub struct AppState {
    pub projects: Mutex<HashMap<String, ProjectContext>>,
    pub connections: Mutex<HashMap<String, Arc<Mutex<rusqlite::Connection>>>>,
    pub read_only_connections: Mutex<HashMap<String, Arc<Mutex<rusqlite::Connection>>>>,
    pub active_refreshes: Mutex<HashSet<String>>,
    pub active_asset_scans: Mutex<HashSet<String>>,
    pub watcher: Mutex<notify::RecommendedWatcher>,
    pub registry_path: Option<PathBuf>,
    pub active_clients: Mutex<HashSet<u32>>,
    pub last_activity: Mutex<Instant>,
    pub asset_graphs: Mutex<HashMap<String, AssetGraph>>,
    pub config_caches: Mutex<HashMap<String, ConfigCache>>,
    /// project_root -> CompletionCache
    pub completion_caches: Mutex<HashMap<String, Arc<Mutex<CompletionCache>>>>,
    /// Number of in-progress watcher-triggered single-file updates
    pub active_file_updates: AtomicU32,
    /// Number of in-flight GetCompletions queries (Tree-sitter parse + DB lookup)
    pub active_completions: AtomicU32,
    /// Number of in-flight non-completion DB queries
    pub active_queries: AtomicU32,
    /// per-project watch filters
    pub watch_filters: Mutex<HashMap<String, Arc<WatcherFilter>>>,
}

impl AppState {
    pub fn save_registry(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.registry_path {
            let projects = self.projects.lock();
            let json = serde_json::to_string_pretty(&*projects)?;
            std::fs::write(path, json)?;
        }
        Ok(())
    }

    pub fn load_registry(path: &Path) -> HashMap<String, ProjectContext> {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(projects) = serde_json::from_str(&data) { return projects; }
        }
        HashMap::new()
    }

    pub fn register_client(&self, pid: u32) {
        {
            let mut clients = self.active_clients.lock();
            if clients.insert(pid) {
                info!("Registered new client PID: {}", pid);
            }
        }
        *self.last_activity.lock() = Instant::now();
    }

    pub fn get_connection(&self, db_path_native: &str) -> anyhow::Result<Arc<Mutex<rusqlite::Connection>>> {
        let mut conns = self.connections.lock();
        if let Some(conn) = conns.get(db_path_native) {
            return Ok(Arc::clone(conn));
        }

        info!("Opening primary database connection (Read/Write): {}", db_path_native);
        let _ = db::ensure_correct_version(db_path_native)?;

        let conn = rusqlite::Connection::open(db_path_native)?;        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // プライマリ接続（書き込み・リフレッシュ用）
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        let _ = conn.pragma_update(None, "cache_size", "-500000");   // 約512MBキャッシュ
        let _ = conn.pragma_update(None, "mmap_size", "1073741824"); // 1GB mmap
        let _ = conn.pragma_update(None, "temp_store", "MEMORY");
        
        let conn_arc = Arc::new(Mutex::new(conn));
        conns.insert(db_path_native.to_string(), Arc::clone(&conn_arc));
        Ok(conn_arc)
    }

    /// 読み取り専用の新しい接続を取得する（並列アクセス用）
    pub fn get_read_only_connection(&self, db_path_native: &str) -> anyhow::Result<rusqlite::Connection> {
        let conn = rusqlite::Connection::open_with_flags(
            db_path_native,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // 各接続のメモリ消費を厳格に制限しつつ、並列性を確保
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "cache_size", "-4000"); // 約4MB (インデックス用)
        let _ = conn.pragma_update(None, "mmap_size", "0");      // メモリマップ無効 (20GBリークの主因)
        let _ = conn.pragma_update(None, "temp_store", "FILE");  // 一時データはディスクへ
        let _ = conn.pragma_update(None, "query_only", "ON");

        Ok(conn)
    }

    pub fn get_completion_cache(&self, project_root: &str) -> Arc<Mutex<CompletionCache>> {
        let mut caches = self.completion_caches.lock();
        if let Some(cache) = caches.get(project_root) {
            return Arc::clone(cache);
        }
        let cache = Arc::new(Mutex::new(CompletionCache::new()));
        caches.insert(project_root.to_string(), Arc::clone(&cache));
        cache
    }

}
