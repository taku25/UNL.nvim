-- lua/UNL/db/init.lua (Comprehensive RPC API)
local M = {}
local remote = require("UNL.db.remote")

--- クラス一覧を取得
function M.get_classes(opts, callback)
    opts = opts or {}
    remote.get_classes(opts.extra_where, opts.params, callback)
end

--- 構造体一覧を取得
function M.get_structs(opts, callback)
    opts = opts or {}
    remote.get_structs(opts.extra_where, opts.params, callback)
end

--- 全ての構造体を取得
function M.get_structs_only(callback)
    remote.get_structs_only(callback)
end

--- Enum詳細を取得
function M.get_enum_values(enum_name, callback)
    remote.get_enum_values(enum_name, callback)
end

--- クラスのメンバー（関数・変数）を再帰的に取得
function M.get_members_recursive(class_name, namespace, callback)
    remote.get_class_members_recursive(class_name, namespace, callback)
end
M.get_class_members_recursive = M.get_members_recursive -- Alias

--- クラスのメンバー（非再帰）を取得
function M.get_class_members(class_name, callback)
    remote.get_class_members(class_name, callback)
end

--- ファイル検索 (ファイル名)
function M.search_files(part, callback)
    remote.search_files(part, callback)
end

--- 指定したクラスを継承しているクラスを取得
function M.get_derived_classes(base_class, callback)
    remote.get_recursive_derived_classes(base_class, callback)
end

--- 指定したクラスの継承チェーンを取得
function M.get_inheritance_chain(child_class, callback)
    remote.get_recursive_parent_classes(child_class, callback)
end

--- プロジェクトのコンポーネント一覧を取得
function M.get_components(callback)
    remote.get_components(callback)
end

--- プロジェクトのモジュール一覧を取得
function M.get_modules(callback)
    remote.get_modules(callback)
end

--- モジュール詳細を取得 (ファイル一覧含む)
function M.get_module_by_name(name, callback)
    remote.get_module_by_name(name, callback)
end

--- モジュール内のファイル一覧を一括取得
function M.get_files_in_modules(modules, extensions, filter, callback)
    if type(extensions) == "function" then
        callback = extensions
        extensions = nil
        filter = nil
    elseif type(filter) == "function" then
        callback = filter
        filter = nil
    end
    remote.get_files_in_modules(modules, extensions, filter, callback)
end

--- モジュールリスト内からファイルを検索
function M.search_files_in_modules(modules, filter, limit, callback)
    remote.search_files_in_modules(modules, filter, limit, callback)
end

--- モジュールリスト内からシンボルを検索
function M.search_symbols_in_modules(modules, symbol_type, filter, limit, callback)
    remote.search_symbols_in_modules(modules, symbol_type, filter, limit, callback)
end

--- 全てのファイルパスを取得
function M.get_all_file_paths(callback)
    remote.get_all_file_paths(callback)
end

--- 全てのファイルのメタデータを取得 (filename, path, module_name)
function M.get_all_files_metadata(callback)
    remote.get_all_files_metadata(callback)
end

--- クラス名から定義情報の詳細を取得
function M.find_class_by_name(name, callback)
    remote.find_class_by_name(name, callback)
end

--- クラス名から定義ファイルのパスを取得
function M.get_class_file_path(class_name, callback)
    remote.get_class_file_path(class_name, callback)
end

--- 指定したファイルの全シンボルを取得
function M.get_file_symbols(file_path, callback)
    remote.get_file_symbols(file_path, callback)
end

--- クラス名の前方一致検索
function M.search_classes_prefix(prefix, limit, callback)
    remote.search_classes_prefix(prefix, limit, callback)
end

--- パスの一部からファイルを検索
function M.search_files_by_path_part(part, callback)
    remote.search_files_by_path_part(part, callback)
end

--- 特定モジュール内のシンボルを検索
function M.find_symbol_in_module(module, symbol, callback)
    remote.find_symbol_in_module(module, symbol, callback)
end

--- *.Target.cs ファイルの一覧を取得
function M.get_target_files(callback)
    remote.get_target_files(callback)
end

--- メンバーの戻り値型を更新 (書き込み操作)
function M.update_member_return_type(class_name, member_name, return_type, callback)
    remote.update_member_return_type(class_name, member_name, return_type, callback)
end

--- 指定したモジュールリスト内のクラスを取得 (純粋なデータ取得)
function M.get_classes_in_modules(modules, callback)
    remote.get_classes_in_modules(modules, function(rows, err)
        if err then return callback(nil, err) end
        local merged = {}
        for _, row in ipairs(rows or {}) do
            -- Response without symbol_type is an array: [name, line, path, type, base]
            local name = row[1]
            local line = row[2]
            local p = row[3]
            local type_ = row[4]
            local base = row[5]
            
            if p then
                if not merged[p] then merged[p] = { classes = {} } end
                table.insert(merged[p].classes, { name = name, base_class = base, line = line, type = type_ })
            end
        end
        callback(merged)
    end)
end

--- プロジェクト内のビルドターゲット一覧を取得
function M.get_build_targets(callback)
    M.get_target_files(function(rows, err)
        if err or not rows then return callback(nil, err) end
        local targets = {}
        for _, row in ipairs(rows) do
            local name = row.filename:gsub("%.Target%.cs$", "")
            local type = "Game"
            if name:match("Editor$") then type = "Editor"
            elseif name:match("Server$") then type = "Server"
            elseif name:match("Client$") then type = "Client" end
            table.insert(targets, { name = name, path = row.path, type = type })
        end
        callback(targets)
    end)
end

--- プロジェクト内のクラス一覧を取得 (スコープ・依存関係フィルタリング)
function M.get_project_classes(opts, callback)
    local project = require("UNL.project")
    project.get_modules_by_scope(opts, function(modules, maps)
        if not modules then return callback(nil, maps) end
        M.get_classes_in_modules(modules, callback)
    end)
end

--- プロジェクト内の全アイテムを取得 (ファイル・ディレクトリ)
function M.get_project_items(opts, callback)
    local project = require("UNL.project")
    project.get_modules_by_scope(opts, function(modules, maps)
        if not modules then return callback(nil, maps) end
        
        M.get_files_in_modules(modules, function(raw_files, err)
            if err then return callback(nil, err) end
            local items = {}
            local utils = require("UNL.utils")
            for _, file in ipairs(raw_files or {}) do
                local display = file.file_path
                if file.module_root and file.module_name then
                    display = file.module_name .. "/" .. utils.create_relative_path(file.file_path, file.module_root)
                end
                table.insert(items, { path = file.file_path, display = display, type = "file" })
            end
            callback(items)
        end)
    end)
end

return M