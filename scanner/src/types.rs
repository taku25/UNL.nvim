use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum RawRequest {
    #[serde(rename = "refresh")]
    Refresh(RefreshRequest),
    #[serde(rename = "scan")]
    Scan(ScanRequest),
}

#[derive(Deserialize, Debug)]
pub struct ScanRequest {
    pub files: Vec<InputFile>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct RefreshRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub project_root: String,
    pub engine_root: Option<String>,
    pub db_path: Option<String>,
    pub config: UEPConfig,
    pub scope: Option<String>,
    pub vcs_hash: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct WatchRequest {
    pub project_root: String,
    pub db_path: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SetupRequest {
    pub project_root: String,
    pub db_path: String,
    pub config: UEPConfig,
    pub vcs_hash: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
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
    pub access: String,
    pub line: usize,
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

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "kind")]
pub enum QueryRequest {
    FindDerivedClasses { base_class: String },
    SearchFiles { part: String },
    LoadComponentData { component: String },
    GetModuleByName { name: String },
    GetClassesInModules { modules: Vec<String>, #[serde(default)] symbol_type: Option<String> },
    GetRecursiveDerivedClasses { base_class: String },
    GetRecursiveParentClasses { child_class: String },
    FindSymbolInInheritanceChain { class_name: String, symbol_name: String, #[serde(default)] mode: Option<String> },
    GetVirtualFunctionsInInheritanceChain { class_name: String },
    GetProgramFiles,
    GetAllIniFiles,
    FindSymbolInModule { module: String, symbol: String },
    FindClassByName { name: String },
    SearchClassesPrefix { prefix: String, limit: Option<usize> },
    GetClasses { extra_where: Option<String>, params: Option<Vec<String>> },
    GetStructs { extra_where: Option<String>, params: Option<Vec<String>> },
    GetStructsOnly,
    GetClassMembersById { class_id: i64 },
    GetClassMembers { class_name: String },
    GetClassMethods { class_name: String },
    GetClassProperties { class_name: String },
    GetClassMembersRecursive { class_name: String, namespace: Option<String> },
    SearchFilesByPathPart { part: String },
    GetEnumValues { enum_name: String },
    GetComponents,
    GetModules,
    GetModuleIdByName { name: String },
    GetModuleRootPath { name: String },
    GetFilesInModule { module_id: i64 },
    GetFilesInModules { 
        modules: Vec<String>,
        #[serde(default)]
        extensions: Option<Vec<String>>,
        #[serde(default)]
        filter: Option<String>,
    },
    SearchFilesInModules { modules: Vec<String>, filter: String, limit: Option<usize> },
    SearchSymbolsInModules { modules: Vec<String>, symbol_type: Option<String>, filter: String, limit: Option<usize> },
    GetDirectoriesInModule { module_id: i64 },
    GetModuleFilesByNameAndRoot { name: String, root: String },
    GetModuleDirsByNameAndRoot { name: String, root: String },
    GetClassFilePath { class_name: String },
    GetFileSymbols { file_path: String },
    UpdateMemberReturnType { class_name: String, member_name: String, return_type: String },
    GetTargetFiles,
    GetAllFilePaths,
    GetAllFilesMetadata,
}

use std::io::{self, Write};

pub trait ProgressReporter: Send + Sync {

    fn report(&self, stage: &str, current: usize, total: usize, message: &str);

}



pub struct StdoutReporter;

impl ProgressReporter for StdoutReporter {

    fn report(&self, stage: &str, current: usize, total: usize, message: &str) {

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

}



pub fn report_progress(stage: &str, current: usize, total: usize, message: &str) {

    StdoutReporter.report(stage, current, total, message);

}
