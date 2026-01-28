use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum RawRequest {
    Refresh(RefreshRequest),
    Scan(Vec<InputFile>),
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct RefreshRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub project_root: String,
    pub engine_root: Option<String>,
    pub db_path: String,
    pub config: UEPConfig,
    pub scope: Option<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct UEPConfig {
    pub excludes_directory: Vec<String>,
    pub include_extensions: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct InputFile {
    pub path: String,
    pub mtime: u64,
    pub old_hash: Option<String>,
    pub module_id: Option<i64>,
    pub db_path: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ParseResult {
    pub path: String,
    pub status: String,
    pub mtime: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ParseData>,
    #[serde(skip)]
    pub module_id: Option<i64>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ParseData {
    pub classes: Vec<ClassInfo>,
    pub parser: String,
    pub new_hash: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ClassInfo {
    pub class_name: String,
    pub namespace: Option<String>,
    pub base_classes: Vec<String>,
    pub symbol_type: String,
    pub line: usize,
    #[serde(skip)]
    pub range_start: usize,
    #[serde(skip)]
    pub range_end: usize,
    pub members: Vec<MemberInfo>,
    pub is_final: bool,
    pub is_interface: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct MemberInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub mem_type: String,
    pub flags: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct Progress {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub stage: String,
    pub current: usize,
    pub total: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ModuleDef {
    pub name: String,
    pub path: PathBuf,
    pub root: PathBuf,
    pub public_deps: Vec<String>,
    pub private_deps: Vec<String>,
    pub mod_type: String,
    pub owner_name: String,
    pub component_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ComponentDef {
    pub name: String,
    pub display_name: String,
    pub comp_type: String, // "Game", "Engine", "Plugin"
    pub root_path: PathBuf,
    pub uproject_path: Option<PathBuf>,
    pub uplugin_path: Option<PathBuf>,
    pub owner_name: String,
}

#[derive(Deserialize)]
pub struct UProjectPluginJson {
    #[serde(rename = "Modules")]
    pub modules: Option<Vec<UModuleJson>>,
}

#[derive(Deserialize)]
pub struct UModuleJson {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Type")]
    pub mod_type: String,
}

use std::io::{self, Write};

pub fn report_progress(stage: &str, current: usize, total: usize, message: &str) {
    let p = Progress {
        msg_type: "progress".to_string(),
        stage: stage.to_string(),
        current,
        total,
        message: message.to_string(),
    };
    if let Ok(mut json) = serde_json::to_string(&p) {
        json.push('\n');
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(json.as_bytes());
        let _ = stdout.flush();
    }
}