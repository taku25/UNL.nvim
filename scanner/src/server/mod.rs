pub mod state;
pub mod handlers;
pub mod asset;
pub mod utils;
pub mod watcher;

use std::sync::{Arc};
use tokio::net::{TcpStream};
use tokio::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde_json::{Value};
use crate::server::state::{AppState};

pub async fn handle_connection(socket: TcpStream, state: Arc<AppState>) {
    let (mut read_half, mut write_half) = socket.into_split();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(2000);
    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if (write_half.write_all(&data).await).is_err() { break; }
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
    tracing::info!("Received RPC request: method={}, msgid={}", method, msgid);
    let result = match method.as_str() {
        "ping" => handlers::handle_ping(&state, &params).await,
        "setup" => handlers::handle_setup(state.clone(), &params).await,
        "refresh" => handlers::handle_refresh(&state, &params, tx.clone()).await,
        "watch" => handlers::handle_watch(&state, &params).await,
        "query" => handlers::handle_query(state.clone(), &params, tx.clone(), msgid).await,
        "scan" => handlers::handle_scan(&state, &params).await,
        "status" => handlers::get_status(&state).await,
        "list_projects" => handlers::list_projects(&state).await,
        "delete_project" => handlers::handle_delete_project(&state, &params).await,
        "rescan_assets" => handlers::handle_rescan_assets(state.clone(), &params).await,
        "modify_uproject_add_module" => handlers::handle_modify_uproject_add_module(&params).await,
        "modify_target_add_module" => handlers::handle_modify_target_add_module(&params).await,
        _ => Err(anyhow::anyhow!("Unknown method")),
    };
    let (err_val, res_val) = match result {
        Ok(res) => (Value::Null, res),
        Err(e) => {
            tracing::error!("Method '{}' failed: {}. Params: {}", method, e, params);
            (Value::String(e.to_string()), Value::Null)
        },
    };
    let response = (1, msgid, err_val, res_val);
    if let Ok(vec) = rmp_serde::to_vec(&response) {
        let mut out = Vec::with_capacity(vec.len() + 4);
        out.extend_from_slice(&(vec.len() as u32).to_be_bytes());
        out.extend_from_slice(&vec);
        let _ = tx.send(out).await;
    }
}
