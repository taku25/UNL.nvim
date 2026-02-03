use std::io::{self, Read, Write};
use std::sync::Arc;
use rayon::prelude::*;
use tree_sitter::Query;
use unl_core::types::{RawRequest, ParseResult};
use unl_core::{scanner, db, refresh};
use std::net::TcpStream;
use serde_json::Value;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let server_port: u16 = std::env::var("UNL_SERVER_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(30110);
    let is_server_running = TcpStream::connect(format!("127.0.0.1:{}", server_port)).is_ok();

    if args.len() > 1 {
        let cmd = &args[1];
        match cmd.as_str() {
            "refresh" | "watch" | "query" | "setup" => {
                let arg = args.get(2).ok_or_else(|| anyhow::anyhow!("Missing config"))?;
                let input_str = if arg.starts_with('{') { arg.clone() } else if std::path::Path::new(arg).exists() { std::fs::read_to_string(arg)? } else { arg.clone() };
                if is_server_running {
                    return proxy_to_server(server_port, cmd, &input_str);
                } else if cmd == "refresh" {
                    let req: unl_core::types::RefreshRequest = serde_json::from_str(&input_str)?;
                    return refresh::run_refresh(req, Arc::new(unl_core::types::StdoutReporter));
                } else {
                    return Err(anyhow::anyhow!("Server not running"));
                }
            },
            "status" | "list_projects" => {
                if is_server_running {
                    return proxy_to_server(server_port, cmd, "{}");
                } else {
                    println!("Server not running.");
                    return Ok(());
                }
            },
            _ => return run_scan_command(server_port, is_server_running)
        }
    }
    run_scan_command(server_port, is_server_running)
}

fn proxy_to_server(port: u16, method: &str, json_payload: &str) -> anyhow::Result<()> {
    let params: Value = serde_json::from_str(json_payload).map_err(|e| anyhow::anyhow!("JSON Parse Error: {}", e))?;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
    
    // Request: [0, msgid, method, params]
    let req = (0, 1, method, params);
    let buf = rmp_serde::to_vec(&req)?;
    
    // Length prefix (4 bytes)
    stream.write_all(&(buf.len() as u32).to_be_bytes())?;
    stream.write_all(&buf)?;
    
    let mut read_buf = Vec::new();
    let mut temp = [0u8; 4096];
    
    loop {
        let n = stream.read(&mut temp)?;
        if n == 0 { break; }
        read_buf.extend_from_slice(&temp[..n]);
        
        while read_buf.len() >= 4 {
            let len = u32::from_be_bytes(read_buf[0..4].try_into().unwrap()) as usize;
            if read_buf.len() < 4 + len { break; }
            let data = read_buf[4..4+len].to_vec();
            read_buf.drain(..4+len);
            
            // Response: [1, msgid, error, result] OR Notification: [2, method, params]
            // We use generic Value for array structure
            if let Ok(msg) = rmp_serde::from_slice::<Vec<Value>>(&data) {
                if let Some(msg_type) = msg.get(0).and_then(|v| v.as_u64()) {
                    if msg_type == 1 { // Response
                        if let Some(err) = msg.get(2).filter(|v| !v.is_null()) {
                            return Err(anyhow::anyhow!("Server Error: {}", err));
                        } else if let Some(res) = msg.get(3) {
                            println!("{}", serde_json::to_string_pretty(res)?);
                            return Ok(());
                        }
                    } else if msg_type == 2 { // Notification (progress)
                        // Ignore progress in CLI or print it?
                        // println!("Progress: {:?}", msg.get(2));
                    }
                }
            }
        }
    }
    Ok(())
}

fn run_scan_command(port: u16, is_server_running: bool) -> anyhow::Result<()> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    if buffer.trim().is_empty() { return Ok(()); }

    if is_server_running {
        return proxy_to_server(port, "scan", &buffer);
    }

    let language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));
    let request: RawRequest = serde_json::from_str(&buffer)?;

    match request {
        RawRequest::Scan(req) => {
            let inputs = req.files;
            let db_path = inputs.get(0).and_then(|i| i.db_path.clone());
            let results: Vec<ParseResult> = inputs.into_par_iter().filter_map(|input| {
                scanner::process_file(&input, &language, &query).ok()
            }).collect();

            if let Some(path) = db_path {
                if let Ok(mut conn) = rusqlite::Connection::open(&path) {
                    let _ = db::save_to_db(&mut conn, &results, Arc::new(unl_core::types::StdoutReporter));
                }
            }

            for res in results {
                if let Ok(json) = serde_json::to_string(&res) {
                    println!("{}", json);
                }
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
        },
        _ => {}
    }

    Ok(())
}