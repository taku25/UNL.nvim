use rusqlite::{Connection};
use serde_json::{json, Value};
use crate::types::QueryRequest;

pub mod asset;
pub mod class;
pub mod module;
pub mod buffer;
pub mod config;
pub mod file;
pub mod search;
pub mod util;
pub mod goto;
pub mod usage;

pub fn process_query(conn: &Connection, request: QueryRequest) -> anyhow::Result<Value> {
    match request {
        QueryRequest::GetFilesInModules { modules, extensions, filter } => 
            file::get_files_in_modules(conn, modules, extensions, filter),
        QueryRequest::GetDependFiles { file_path, recursive, game_only } => 
            Ok(json!(file::get_depend_files(conn, &file_path, recursive, game_only)?)),
        
        QueryRequest::SearchSymbols { pattern, limit } => 
            search::search_symbols(conn, &pattern, limit),
        QueryRequest::GetStructsOnly => 
            search::get_structs(conn),
        
        QueryRequest::GetFileSymbols { file_path } => 
            class::get_file_symbols(conn, &file_path),
        QueryRequest::GetClassMembers { class_name } => 
            class::get_class_members(conn, &class_name),
        QueryRequest::FindSymbolUsages { symbol_name, file_path } =>
            usage::find_symbol_usages(conn, &symbol_name, file_path.as_deref()),
        
        QueryRequest::FindIncluders { file_path } =>
            usage::find_includers(conn, &file_path),
        
        QueryRequest::GetModules => 
            module::get_modules(conn),
        QueryRequest::GetModuleByName { name } => 
            module::get_module_by_name(conn, &name),
        
        QueryRequest::GetClassFilePath { class_name } => 
            util::get_class_file_path(conn, &class_name),

        QueryRequest::GetClassesInModules { modules, symbol_type } =>
            class::get_classes_in_modules(conn, modules, symbol_type),

        QueryRequest::SearchFiles { part } =>
            file::search_files_by_path_part(conn, &part),
        QueryRequest::SearchFilesByPathPart { part } =>
            file::search_files_by_path_part(conn, &part),
        
        QueryRequest::ParseBuffer { content, file_path, line, character } => 
            buffer::parse_buffer(content, file_path, line, character),

        // Goto definition / symbol search
        QueryRequest::GotoDefinition { content, line, character, file_path } =>
            goto::goto_definition(conn, content, line, character, file_path),
        QueryRequest::FindSymbolInInheritanceChain { class_name, symbol_name, .. } =>
            Ok(goto::find_symbol_in_inheritance_chain(conn, &class_name, &symbol_name)?
                .unwrap_or(Value::Null)),
        QueryRequest::FindSymbolInModule { module, symbol } =>
            Ok(goto::find_symbol_in_module(conn, &module, &symbol)?
                .unwrap_or(Value::Null)),

        // Assets / Components
        QueryRequest::GetAssets => asset::get_assets(conn),
        QueryRequest::GetComponents => crate::db::get_components(conn),
        QueryRequest::GrepAssets { pattern } => asset::grep_assets(conn, pattern, |_| Ok(())),

        QueryRequest::GetConfigData { .. } => Err(anyhow::anyhow!("GetConfigData must be handled by server state")),

        QueryRequest::GetEnumValues { enum_name } =>
            class::get_enum_values(conn, &enum_name),

        QueryRequest::GetTargetFiles => file::get_target_files(conn),
        QueryRequest::GetAllFilePaths => file::get_all_file_paths(conn),
        QueryRequest::GetAllFilesMetadata => file::get_all_files_metadata(conn),

        QueryRequest::GetFilesInFavoritePaths { dirs, exact_files } =>
            file::get_files_in_favorite_paths(conn, &dirs, &exact_files),

        _ => Err(anyhow::anyhow!("Query type not yet implemented in new structure: {:?}", request)),
    }
}

pub fn process_query_streaming<F>(conn: &Connection, request: QueryRequest, on_items: F) -> anyhow::Result<Value> 
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
    match request {
        QueryRequest::GrepAssets { pattern } => asset::grep_assets(conn, pattern, on_items),
        
        QueryRequest::GetFilesInModulesAsync { modules, extensions, filter } => 
            file::get_files_in_modules_async(conn, modules, extensions, filter, on_items),
            
        QueryRequest::SearchFilesInModulesAsync { modules, filter, limit } => 
            asset::search_files_in_modules_async(conn, modules, filter, limit, on_items),

        QueryRequest::SearchFilesByPathPartAsync { part } =>
            file::search_files_by_path_part_async(conn, &part, on_items),

        QueryRequest::GetClassesInModulesAsync { modules, symbol_type } =>

            class::get_classes_in_modules_async(conn, modules, symbol_type, on_items),

        QueryRequest::FindSymbolUsagesAsync { symbol_name, file_path } =>
            usage::find_symbol_usages_async(conn, &symbol_name, file_path.as_deref(), on_items),

        QueryRequest::FindIncludersAsync { file_path } =>
            usage::find_includers_async(conn, &file_path, on_items),
            
        _ => process_query(conn, request)
    }
}
