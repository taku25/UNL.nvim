use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use parking_lot::Mutex;
use std::path::{PathBuf};
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use tokio::net::{TcpListener};
use tokio::sync::mpsc;
use tracing::info;
use unl_core::server::state::{AppState};
use unl_core::server::watcher::{handle_file_change};
use unl_core::server::watch_filter::{should_ignore_fast};
use unl_core::server::{handle_connection};
use sysinfo::{Pid, System};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30110);
    let registry_path = args.get(2).map(PathBuf::from);
    let log_path = if let Some(ref p) = registry_path {
        p.parent().unwrap_or(&PathBuf::from(".")).join("unl-server.log")
    } else {
        PathBuf::from("unl-server.log")
    };
    
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Arc::new(log_file))
        .init();
    info!("--- UNL Server Starting (MsgPack) ---");
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("PANIC: {}", info);
    }));

    let (tx, mut rx) = mpsc::channel::<PathBuf>(100);
    let _watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            tracing::debug!("Watcher event: {:?}", event);
            for path in event.paths { let _ = tx.blocking_send(path); }
        }
    })?;

    // Do NOT restore projects from registry at startup.
    // Each Neovim client registers its own project via the "setup" RPC call on connect.
    // Restoring all past projects would activate projects from previous sessions
    // that are no longer open (e.g. watcher, asset scans, DB connections for stale projects).
    // The registry file is kept only as a write-target so db_path associations persist
    // across server restarts for informational purposes.

    let state = Arc::new(AppState {
        projects: Mutex::new(HashMap::new()),
        connections: Mutex::new(HashMap::new()),
        read_only_connections: Mutex::new(HashMap::new()),
        persistent_cache_connections: Mutex::new(HashMap::new()),
        active_refreshes: Mutex::new(HashSet::new()),
        active_asset_scans: Mutex::new(HashSet::new()),
        watcher: Mutex::new(_watcher),
        registry_path,
        active_clients: Mutex::new(HashSet::new()),
        last_activity: Mutex::new(Instant::now()),
        asset_graphs: Mutex::new(HashMap::new()),
        config_caches: Mutex::new(HashMap::new()),
        completion_caches: Mutex::new(HashMap::new()),
        active_file_updates: AtomicU32::new(0),
        active_completions: AtomicU32::new(0),
        active_queries: AtomicU32::new(0),
        watch_filters: Mutex::new(HashMap::new()),
    });

    let state_for_watcher = Arc::clone(&state);
    tokio::spawn(async move {
        let mut last_event: HashMap<PathBuf, Instant> = HashMap::new();
        let mut event_count: u32 = 0;
        while let Some(path) = rx.recv().await {
            // Stateless fast-path: drop events for build/generated directories immediately,
            // before any HashMap lookup or lock acquisition.
            if should_ignore_fast(&path) { continue; }

            if let Some(last) = last_event.get(&path) { if last.elapsed() < Duration::from_millis(500) { continue; } }
            last_event.insert(path.clone(), Instant::now());
            handle_file_change(state_for_watcher.clone(), path).await;

            // Periodically evict stale entries from `last_event` so it does not grow
            // unboundedly when Unreal generates thousands of unique paths during a build.
            event_count += 1;
            if event_count >= 500 {
                event_count = 0;
                let cutoff = Instant::now() - Duration::from_secs(60);
                last_event.retain(|_, t| *t > cutoff);
            }
        }
    });

    let state_for_lifecycle = Arc::clone(&state);
    tokio::spawn(async move {
        let mut sys = System::new_all();
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            sys.refresh_processes();
            let mut clients = state_for_lifecycle.active_clients.lock();
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
                let last = *state_for_lifecycle.last_activity.lock();
                if last.elapsed() > Duration::from_secs(600) {
                    info!("No active clients for 600s. Shutting down UNL Server...");
                    std::process::exit(0);
                }
            } else {
                *state_for_lifecycle.last_activity.lock() = Instant::now();
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
