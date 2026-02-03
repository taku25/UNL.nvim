use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;
use notify::{Watcher, RecursiveMode, EventKind};
use unl_core::types::{RefreshRequest, ScanRequest, ParseResult, InputFile, WatchRequest, QueryRequest, SetupRequest, Progress, ProgressReporter};
use unl_core::{scanner, db, refresh};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use sysinfo::{Pid, System};

struct RpcProgressReporter {
    tx: mpsc::Sender<Vec<u8>>,
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
struct ProjectContext {
    db_path: String,
    #[serde(default)]
    vcs_hash: Option<String>,
    #[serde(skip, default = "Instant::now")]
    _last_refresh: Instant,
}

struct AppState {
    projects: Mutex<HashMap<PathBuf, ProjectContext>>,
    watcher: Mutex<notify::RecommendedWatcher>,
    registry_path: Option<PathBuf>,
    active_clients: Mutex<HashSet<u32>>,
    last_activity: Mutex<Instant>,
}

impl AppState {
    fn save_registry(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.registry_path {
            let projects = self.projects.lock().unwrap();
            let json = serde_json::to_string_pretty(&*projects)?;
            std::fs::write(path, json)?;
        }
        Ok(())
    }
    fn load_registry(path: &Path) -> HashMap<PathBuf, ProjectContext> {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(projects) = serde_json::from_str(&data) { return projects; }
        }
        HashMap::new()
    }
    fn register_client(&self, pid: u32) {
        let mut clients = self.active_clients.lock().unwrap();
        if clients.insert(pid) {
            info!("Registered new client PID: {}", pid);
        }
        *self.last_activity.lock().unwrap() = Instant::now();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30110);
    let registry_path = args.get(2).map(PathBuf::from);
    let log_path = if let Some(ref p) = registry_path { p.parent().unwrap().join("unl-server.log") } else { PathBuf::from("unl-server.log") };
    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    tracing_subscriber::fmt().with_writer(Arc::new(log_file)).init();
    info!("--- UNL Server Starting (MsgPack) ---");

    let (tx, mut rx) = mpsc::channel::<PathBuf>(100);
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                for path in event.paths { let _ = tx.blocking_send(path); }
            }
        }
    })?;

    let initial_projects = registry_path.as_ref().map(|p| AppState::load_registry(p)).unwrap_or_default();
    let state = Arc::new(AppState {
        projects: Mutex::new(initial_projects),
        watcher: Mutex::new(watcher),
        registry_path,
        active_clients: Mutex::new(HashSet::new()),
        last_activity: Mutex::new(Instant::now()),
    });

    {
        let projects = state.projects.lock().unwrap();
        let mut watcher = state.watcher.lock().unwrap();
        for root in projects.keys() { let _ = watcher.watch(root, RecursiveMode::Recursive); }
    }

    let state_for_watcher = Arc::clone(&state);
    tokio::spawn(async move {
        let mut last_event: HashMap<PathBuf, Instant> = HashMap::new();
        while let Some(path) = rx.recv().await {
            if let Some(last) = last_event.get(&path) { if last.elapsed() < Duration::from_millis(200) { continue; } }
            last_event.insert(path.clone(), Instant::now());
            handle_file_change(&state_for_watcher, path).await;
        }
    });

    let state_for_lifecycle = Arc::clone(&state);
    tokio::spawn(async move {
        let mut sys = System::new_all();
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            sys.refresh_processes();
            let mut clients = state_for_lifecycle.active_clients.lock().unwrap();
            let mut to_remove = Vec::new();
            for &pid in clients.iter() {
                if sys.process(Pid::from(pid as usize)).is_none() {
                    to_remove.push(pid);
                }
            }
            for pid in to_remove {
                info!("Client process {} disconnected (not found)", pid);
                clients.remove(&pid);
            }
            if clients.is_empty() {
                let last = *state_for_lifecycle.last_activity.lock().unwrap();
                if last.elapsed() > Duration::from_secs(600) {
                    info!("No active clients for 600s. Shutting down UNL Server...");
                    std::process::exit(0);
                }
            } else {
                *state_for_lifecycle.last_activity.lock().unwrap() = Instant::now();
            }
        }
    });

    let addr = format!("127.0.0.1:{}", port);
    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            info!("UNL Server listening on {}", addr);
            loop {
                let (socket, _) = listener.accept().await?;
                let state = Arc::clone(&state);
                tokio::spawn(async move { handle_connection(socket, state).await; });
            }
        }
        Err(e) => {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            return Err(e.into());
        }
    }
}

async fn handle_connection(socket: TcpStream, state: Arc<AppState>) {
    let (mut read_half, mut write_half) = socket.into_split();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(2000);
    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if let Err(_) = write_half.write_all(&data).await { break; }
            let _ = write_half.flush().await;
        }
    });
    let mut buffer = Vec::new();
    let mut temp_buf = [0u8; 8192];
    loop {
        match read_half.read(&mut temp_buf).await {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&temp_buf[..n]);
                while buffer.len() >= 4 {
                    let len = u32::from_be_bytes(buffer[0..4].try_into().unwrap()) as usize;
                    if buffer.len() < 4 + len { break; }
                    let data = buffer[4..4+len].to_vec();
                    buffer.drain(..4+len);
                    let state_clone = state.clone();
                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        if let Ok((_msg_type, msgid, method, params)) = rmp_serde::from_slice::<(u32, u64, String, Value)>(&data) {
                            process_msg(msgid, method, params, state_clone, tx_clone).await;
                        }
                    });
                }
            }
            Err(_) => break,
        }
    }
}

async fn process_msg(msgid: u64, method: String, params: Value, state: Arc<AppState>, tx: mpsc::Sender<Vec<u8>>) {
    let result = match method.as_str() {
        "ping" => handle_ping(&state, &params).await,
        "setup" => handle_setup(&state, &params).await,
        "refresh" => handle_refresh(&state, &params, tx.clone()).await,
        "watch" => handle_watch(&state, &params).await,
        "query" => handle_query(&state, &params).await,
        "scan" => handle_scan(&state, &params).await,
        "status" => get_status(&state).await,
        "list_projects" => list_projects(&state).await,
        "delete_project" => handle_delete_project(&state, &params).await,
        _ => Err(anyhow::anyhow!("Unknown method")),
    };
    let (err_val, res_val) = match result {
        Ok(res) => (Value::Null, res),
        Err(e) => (Value::String(e.to_string()), Value::Null),
    };
    let response = (1, msgid, err_val, res_val);
    if let Ok(vec) = rmp_serde::to_vec(&response) {
        let mut out = Vec::with_capacity(vec.len() + 4);
        out.extend_from_slice(&(vec.len() as u32).to_be_bytes());
        out.extend_from_slice(&vec);
        let _ = tx.send(out).await;
    }
}

fn convert_params<T: serde::de::DeserializeOwned>(val: &Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(val.clone())?)
}

fn normalize_to_unix(s: &str) -> String {
    s.replace('\\', "/")
}

fn normalize_to_native(s: &str) -> String {
    if cfg!(target_os = "windows") {
        s.replace('/', "\\")
    } else {
        s.replace('\\', "/")
    }
}

#[derive(Deserialize)]
struct DeleteProjectRequest { project_root: String }

async fn handle_delete_project(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: DeleteProjectRequest = convert_params(params)?;
    let root_unix = normalize_to_unix(&req.project_root);
    
    let removed = {
        let mut projects = state.projects.lock().unwrap();
        let mut found_key = None;
        for root in projects.keys() {
            if normalize_to_unix(&root.to_string_lossy()) == root_unix {
                found_key = Some(root.clone());
                break;
            }
        }
        if let Some(key) = found_key {
            projects.remove(&key).is_some()
        } else {
            false
        }
    };
    
    if removed {
        let _ = state.save_registry();
        info!("Deleted project: {}", root_unix);
        Ok(Value::String("Deleted".to_string()))
    } else {
        Err(anyhow::anyhow!("Project not found"))
    }
}

async fn handle_ping(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: PingRequest = convert_params(params)?;
    state.register_client(req.pid);
    Ok(Value::String("pong".to_string()))
}

#[derive(Deserialize)]
struct PingRequest { pid: u32 }

async fn handle_setup(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: SetupRequest = convert_params(params)?;
    let db_path_native = normalize_to_native(&req.db_path);
    let root_unix = normalize_to_unix(&req.project_root);
    let root_path_unix = PathBuf::from(&root_unix);
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(&db_path_native)?;
        unl_core::db::init_db(&conn)?;
        Ok::<_, anyhow::Error>(())
    }).await??;
    {
        let mut projects = state.projects.lock().unwrap();
        projects.insert(root_path_unix, ProjectContext { db_path: normalize_to_unix(&req.db_path), vcs_hash: req.vcs_hash.clone(), _last_refresh: Instant::now() });
    }
    let _ = state.save_registry();
    Ok(serde_json::json!({ "status": "ok" }))
}

async fn handle_refresh(state: &AppState, params: &Value, tx: mpsc::Sender<Vec<u8>>) -> anyhow::Result<Value> {
    let mut req: RefreshRequest = convert_params(params)?;
    let root_unix = normalize_to_unix(&req.project_root);
    let root_path_unix = PathBuf::from(&root_unix);
    let db_path_unix = {
        let mut projects = state.projects.lock().unwrap();
        let mut found_key = None;
        for root in projects.keys() {
            if normalize_to_unix(&root.to_string_lossy()) == root_unix {
                found_key = Some(root.clone());
                break;
            }
        }
        if let Some(path) = &req.db_path {
             let path_u = normalize_to_unix(path);
             projects.insert(root_path_unix.clone(), ProjectContext { db_path: path_u.clone(), vcs_hash: req.vcs_hash.clone(), _last_refresh: Instant::now() });
             path_u
        } else if let Some(key) = found_key {
             let ctx = projects.get_mut(&key).unwrap();
             ctx.vcs_hash = req.vcs_hash.clone();
             ctx.db_path.clone()
        } else { return Err(anyhow::anyhow!("Project not found")); }
    };
    req.db_path = Some(db_path_unix);
    let _ = state.save_registry();
    let reporter = Arc::new(RpcProgressReporter { tx });
    tokio::task::spawn_blocking(move || { refresh::run_refresh(req, reporter) }).await??;
    Ok(Value::String("Refresh success".to_string()))
}

async fn handle_watch(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: WatchRequest = convert_params(params)?;
    let root_native = normalize_to_native(&req.project_root);
    let root_path_native = PathBuf::from(&root_native);
    let mut watcher = state.watcher.lock().unwrap();
    watcher.watch(&root_path_native, RecursiveMode::Recursive)?;
    Ok(Value::String("Watch started".to_string()))
}

#[derive(serde::Deserialize)]
struct ServerQueryRequest { project_root: String, #[serde(flatten)] query: QueryRequest }

async fn handle_query(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: ServerQueryRequest = convert_params(params)?;
    let root_unix = normalize_to_unix(&req.project_root);
    let root_path_unix = PathBuf::from(&root_unix);
    let db_path_unix = {
        let projects = state.projects.lock().unwrap();
        let ctx = projects.get(&root_path_unix).ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        ctx.db_path.clone()
    };
    let db_path_native = normalize_to_native(&db_path_unix);
    tokio::task::spawn_blocking(move || unl_core::query::process_query(&db_path_native, req.query)).await?
}

async fn handle_scan(_state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: ScanRequest = convert_params(params)?;
    let db_path = req.files.get(0).and_then(|f| f.db_path.clone()).ok_or_else(|| anyhow::anyhow!("No DB path"))?;
    tokio::task::spawn_blocking(move || {
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = tree_sitter::Query::new(&language, scanner::QUERY_STR).unwrap();
        let results: Vec<ParseResult> = req.files.into_iter().filter_map(|input| scanner::process_file(&input, &language, &query).ok()).collect();
        let mut conn = rusqlite::Connection::open(&db_path)?;
        db::save_to_db(&mut conn, &results, Arc::new(unl_core::types::StdoutReporter))?;
        Ok(serde_json::json!(results.len()))
    }).await?
}

async fn get_status(state: &AppState) -> anyhow::Result<Value> {
    let projects = state.projects.lock().unwrap();
    let project_list: Vec<Value> = projects.keys().map(|p| serde_json::json!(p.to_string_lossy())).collect();
    let clients = state.active_clients.lock().unwrap();
    let client_list: Vec<Value> = clients.iter().map(|&pid| serde_json::json!(pid)).collect();
    Ok(serde_json::json!({ "status": "running", "active_projects": project_list, "active_clients": client_list }))
}

async fn list_projects(state: &AppState) -> anyhow::Result<Value> {
    let projects = state.projects.lock().unwrap();
    let list: Vec<Value> = projects.iter().map(|(root, ctx)| {
        serde_json::json!({ "root": root.to_string_lossy(), "db_path": ctx.db_path, "vcs_hash": ctx.vcs_hash })
    }).collect();
    Ok(json!(list))
}

async fn handle_file_change(state: &AppState, path: PathBuf) {
    if !path.exists() { return; }
    let target = {
        let projects = state.projects.lock().unwrap();
        let mut res = None;
        for (root, ctx) in projects.iter() { if path.starts_with(root) { res = Some((root.clone(), ctx.db_path.clone())); break; } }
        res
    };
    if let Some((_root, db_path)) = target {
        let path_str = path.to_string_lossy().replace("\\", "/");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !["h", "cpp", "hpp", "cs"].contains(&ext) { return; }
        let db_path_clone = db_path.clone(); let path_str_clone = path_str.clone();
        tokio::task::spawn_blocking(move || {
            if let Ok(Some(mod_id)) = db::get_module_id_for_path(&db_path_clone, &path_str_clone) {
                let language = tree_sitter_unreal_cpp::LANGUAGE.into();
                let query = tree_sitter::Query::new(&language, scanner::QUERY_STR).unwrap();
                let mtime = std::fs::metadata(&path_str_clone).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                let input = InputFile { path: path_str_clone, mtime, old_hash: None, module_id: Some(mod_id), db_path: Some(db_path_clone.clone()) };
                if let Ok(res) = scanner::process_file(&input, &language, &query) { 
                    if let Ok(mut conn) = rusqlite::Connection::open(&db_path_clone) {
                        let _ = db::save_to_db(&mut conn, &[res], Arc::new(unl_core::types::StdoutReporter));
                    }
                }
            }
        });
    }
}
