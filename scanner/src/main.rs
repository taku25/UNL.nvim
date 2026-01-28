use std::io::{self, Read};
use std::sync::Arc;
use rayon::prelude::*;
use tree_sitter::Query;

mod types;
mod scanner;
mod db;
mod refresh;

use crate::types::RawRequest;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "refresh" {
        let arg = args.get(2).expect("Missing refresh config (JSON string or file path)");
        
        let input_str = if std::path::Path::new(arg).exists() {
            std::fs::read_to_string(arg)?
        } else {
            arg.clone()
        };

        let req: types::RefreshRequest = serde_json::from_str(&input_str)?;
        refresh::run_refresh(req)
    } else {
        run_scan_command()
    }
}

fn run_scan_command() -> anyhow::Result<()> {
    let language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));

    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    if buffer.trim().is_empty() { return Ok(()); }

    let request: RawRequest = serde_json::from_str(&buffer)?;

    match request {
        RawRequest::Scan(req) => {
            let inputs = req.files;
            let db_path = inputs.get(0).and_then(|i| i.db_path.clone());
            let results: Vec<types::ParseResult> = inputs.into_par_iter().filter_map(|input| {
                scanner::process_file(&input, &language, &query).ok()
            }).collect();

            if let Some(path) = db_path {
                let _ = db::save_to_db(&path, &results);
            }

            for res in results {
                if let Ok(json) = serde_json::to_string(&res) {
                    println!("{}", json);
                }
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
        },
        RawRequest::Refresh(req) => {
            refresh::run_refresh(req)?;
        }
    }

    Ok(())
}