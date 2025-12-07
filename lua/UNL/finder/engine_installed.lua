local M = {}
local fs = require("vim.fs")

-- ==========================================================
-- Windows Implementation (Registry)
-- ==========================================================

local function reg_query(key, args)
    local cmd = string.format('reg query "%s" %s', key, args or "")
    local output = vim.fn.system(cmd)
    if vim.v.shell_error ~= 0 then return nil end
    return output
end

local function find_windows_engines()
    local engines = {}
    -- ... (Windowsの実装は変更なし、前回のまま) ...
    -- 1. ランチャー版 (HKLM)
    local hklm_base = "HKEY_LOCAL_MACHINE\\SOFTWARE\\EpicGames\\Unreal Engine"
    local versions_output = reg_query(hklm_base)
    
    if versions_output then
        for line in versions_output:gmatch("[^\r\n]+") do
            local version = line:match("Unreal Engine\\([%d%.]+)$")
            if version then
                local val_output = reg_query(line, '/v InstalledDirectory')
                if val_output then
                    local path = val_output:match("InstalledDirectory%s+REG_SZ%s+(.*)")
                    if path then
                        path = vim.trim(path)
                        table.insert(engines, { version = version, path = path, type = "Launcher", label = "UE " .. version })
                    end
                end
            end
        end
    end

    -- 2. ソース/カスタムビルド版 (HKCU)
    local hkcu_base = "HKEY_CURRENT_USER\\Software\\Epic Games\\Unreal Engine\\Builds"
    local builds_output = reg_query(hkcu_base)
    
    if builds_output then
        for line in builds_output:gmatch("[^\r\n]+") do
            if not line:find("HKEY_") then
                local path = line:match("REG_SZ%s+(.*)")
                if path then
                    path = vim.trim(path)
                    local name = vim.fn.fnamemodify(path, ":t")
                    table.insert(engines, { version = name, path = path, type = "Source", label = "Source: " .. name })
                end
            end
        end
    end

    return engines
end

-- ==========================================================
-- Linux Implementation (Install.ini & Common Paths)
-- ==========================================================

-- 指定したディレクトリ内に "Engine/Binaries" があるか確認する
local function is_valid_engine_root(path)
    if not path or path == "" then return false end
    return vim.fn.isdirectory(fs.joinpath(path, "Engine", "Binaries")) == 1
end

local function find_linux_engines()
    local engines = {}
    local seen_paths = {}

    -- 1. Install.ini をチェック (登録済みエンジン)
    -- Linuxの標準パス: ~/.config/Epic/UnrealEngine/Install.ini
    local home = vim.loop.os_homedir()
    local ini_path = fs.joinpath(home, ".config", "Epic", "UnrealEngine", "Install.ini")

    if vim.fn.filereadable(ini_path) == 1 then
        local lines = vim.fn.readfile(ini_path)
        for _, line in ipairs(lines) do
            -- Format: Identifier=Path
            -- 例: UE_5.3=/home/user/UnrealEngine/UE_5.3
            -- 例: {GUID}=/home/user/Source/UnrealEngine
            local id, path = line:match("^([^=]+)=(.*)$")
            if id and path then
                path = vim.trim(path)
                if is_valid_engine_root(path) and not seen_paths[path] then
                    local label = id
                    -- IDが "UE_5.3" のような形式なら "UE 5.3" に整形
                    if id:match("^UE_") then
                        label = id:gsub("_", " ")
                    end
                    
                    table.insert(engines, {
                        version = id,
                        path = path,
                        type = "Registered",
                        label = label
                    })
                    seen_paths[path] = true
                end
            end
        end
    end

    -- 2. 一般的なインストール場所をスキャン (フォールバック)
    -- ~/UnrealEngine, ~/Epic Games, /opt/UnrealEngine など
    local search_roots = {
        fs.joinpath(home, "UnrealEngine"),
        fs.joinpath(home, "Epic Games"),
        "/opt/UnrealEngine",
        "/usr/local/share/UnrealEngine"
    }

    for _, root in ipairs(search_roots) do
        if vim.fn.isdirectory(root) == 1 then
            local handle = vim.loop.fs_scandir(root)
            if handle then
                while true do
                    local name, type = vim.loop.fs_scandir_next(handle)
                    if not name then break end
                    
                    -- ディレクトリかつ "UE_" で始まるものを候補とする
                    if type == "directory" and (name:match("^UE_") or name:match("^UnrealEngine")) then
                        local full_path = fs.joinpath(root, name)
                        if is_valid_engine_root(full_path) and not seen_paths[full_path] then
                            table.insert(engines, {
                                version = name,
                                path = full_path,
                                type = "Discovered",
                                label = "Found: " .. name
                            })
                            seen_paths[full_path] = true
                        end
                    end
                end
            end
        end
    end

    return engines
end

-- ==========================================================
-- Mac Implementation (Placeholder)
-- ==========================================================

local function find_mac_engines()
    -- MacもLinuxと同様に Install.ini (~/Library/Application Support/Epic/...) を見ればOK
    -- 必要になったら実装します
    return {}
end

-- ==========================================================
-- Public API
-- ==========================================================

function M.find()
    local engines = {}
    
    if vim.fn.has("win32") == 1 then
        engines = find_windows_engines()
    elseif vim.fn.has("unix") == 1 then
        local uname = vim.loop.os_uname().sysname
        if uname == "Darwin" then
            engines = find_mac_engines()
        else
            -- Linux
            engines = find_linux_engines()
        end
    end
    
    -- ソート (新しいバージョン順, Launcher優先)
    table.sort(engines, function(a, b)
        if a.type ~= b.type then return a.type < b.type end
        return a.version > b.version
    end)

    return engines
end

return M
