use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::time::{Instant};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tracing::info;
use serde::{Serialize, Deserialize};
use crate::types::{Progress, ProgressReporter, ConfigCache};
use crate::db;

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
    pub references: HashMap<String, HashSet<String>>,
    pub derived: HashMap<String, HashSet<String>>,
    pub functions: HashMap<String, HashSet<String>>,
}

pub struct AppState {
    pub projects: Mutex<HashMap<String, ProjectContext>>,
    pub connections: Mutex<HashMap<String, Arc<Mutex<rusqlite::Connection>>>>,
    pub active_refreshes: Mutex<HashSet<String>>,
    pub active_asset_scans: Mutex<HashSet<String>>,
    pub watcher: Mutex<notify::RecommendedWatcher>,
    pub registry_path: Option<PathBuf>,
    pub active_clients: Mutex<HashSet<u32>>,
    pub last_activity: Mutex<Instant>,
    pub asset_graphs: Mutex<HashMap<String, AssetGraph>>,
    pub config_caches: Mutex<HashMap<String, ConfigCache>>,
}

impl AppState {
    pub fn save_registry(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.registry_path {
            let projects = self.projects.lock().unwrap();
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
        let mut clients = self.active_clients.lock().unwrap();
        if clients.insert(pid) {
            info!("Registered new client PID: {}", pid);
        }
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    pub fn get_connection(&self, db_path_native: &str) -> anyhow::Result<Arc<Mutex<rusqlite::Connection>>> {
        let mut conns = self.connections.lock().unwrap();
        if let Some(conn) = conns.get(db_path_native) {
            return Ok(Arc::clone(conn));
        }

        info!("Opening database connection: {}", db_path_native);
        db::ensure_correct_version(db_path_native)?;
        
        let conn = rusqlite::Connection::open(db_path_native)?;

        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        let _ = conn.pragma_update(None, "cache_size", "-800000");
        let _ = conn.pragma_update(None, "mmap_size", "1073741824");
        let _ = conn.pragma_update(None, "temp_store", "MEMORY");
        
        let conn_arc = Arc::new(Mutex::new(conn));
        
        let conn_for_warmup = Arc::clone(&conn_arc);
        tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            let conn = conn_for_warmup.lock().unwrap();
            info!("Shadow warm-up (aggressive) started...");
            
            let _ = conn.query_row(
                "SELECT SUM(LENGTH(c.name) + LENGTH(f.path) + LENGTH(m.name)) 
                 FROM classes c 
                 JOIN files f ON c.file_id = f.id 
                 JOIN modules m ON f.module_id = m.id", 
                [], |_| Ok(())
            );
            
            let _ = conn.query_row("SELECT SUM(LENGTH(name) + LENGTH(COALESCE(detail, ''))) FROM members", [], |_| Ok(()));
            let _ = conn.query_row("SELECT SUM(LENGTH(parent_name)) FROM inheritance", [], |_| Ok(()));
            
            info!("Shadow warm-up (aggressive) completed in {:?}.", start.elapsed());
        });

        conns.insert(db_path_native.to_string(), Arc::clone(&conn_arc));
        Ok(conn_arc)
    }
}
