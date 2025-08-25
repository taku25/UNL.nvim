-- lua/UNL/config/loader.lua (完全版)
-- 設定レイヤーを明確に分離し、マージの優先順位を保証する修正版。
--
-- 優先順位:
-- 1. UNL自身のデフォルト (最低)
-- 2. プラグイン(UEP等)のデフォルト
-- 3. ユーザーがsetup()で渡す設定
-- 4. プロジェクトルートの .unlrc.json
-- 5. ランタイムの動的オーバーライド (最高)

local unl_defaults = require("UNL.config.defaults") -- UNL自身のデフォルト
local schema = require("UNL.config.schema")

local M = {}

-- 設定レイヤーごとに状態変数を明確に分離
local _configs = {}

-- Deep merge (bがaを上書きする)
local function deep_merge(a, b)
  for k, v in pairs(b) do
    if type(v) == "table" and type(a[k]) == "table" then
      deep_merge(a[k], v)
    else
      a[k] = vim.deepcopy(v)
    end
  end
  return a
end

local function read_file(path)
  local ok, lines = pcall(vim.fn.readfile, path)
  if not ok then return nil end
  return table.concat(lines, "\n")
end

local function decode_json(str)
  if not str then return nil end
  local ok, data = pcall(vim.json.decode or vim.fn.json_decode, str)
  if not ok or type(data) ~= "table" then return nil end
  return data
end

local function is_fs_root(dir)
  if dir == "/" then return true end
  if vim.loop.os_uname().version:match("Windows") then
    return dir:match("^%a:[/\\]$") ~= nil
  end
  return false
end

local function parent_dir(dir)
  local p = vim.fs.dirname(dir)
  if not p or p == "" or p == dir then return dir end
  return p
end

-- Upward search for rc file
local function find_local_rc(start_path, fname, stop_at_home)
  if not start_path or start_path == "" then
    start_path = vim.loop.cwd()
  end
  local stat = vim.loop.fs_stat(start_path)
  local dir = stat and stat.type == "file" and vim.fs.dirname(start_path) or start_path
  local home = stop_at_home and vim.loop.os_homedir() or nil

  while dir and dir ~= "" do
    local cand = dir .. "/" .. fname
    if vim.fn.filereadable(cand) == 1 then
      return cand
    end
    if (home and dir == home) or is_fs_root(dir) then
      break
    end
    local nextd = parent_dir(dir)
    if nextd == dir then break end
    dir = nextd
  end
  return nil
end


-- 最終的な設定を構築するコア関数
local function build(name, start_path, override)
  if not _configs[name] then
    -- まだsetupが呼ばれていない場合でも、最低限のデータで動作するようにする
    _configs[name] = { plugin_defaults = {}, user = {} }
  end
  local stored_cfg = _configs[name]

  -- 1. UNLのデフォルト設定をベースにする
  local final_cfg = vim.deepcopy(unl_defaults)
  -- 2. プラグイン(UEP等)のデフォルト設定をマージする
  deep_merge(final_cfg, stored_cfg.plugin_defaults)
  -- 3. ユーザーがsetup()で渡した設定をマージする
  deep_merge(final_cfg, stored_cfg.user)

  -- 4. プロジェクトローカルの設定(.unlrc.json)を探してマージする
  local rc_path = find_local_rc(start_path, final_cfg.project.localrc_filename, final_cfg.project.search_stop_at_home)
  if rc_path then
    local content = read_file(rc_path)
    local data = decode_json(content)
    if data then
      deep_merge(final_cfg, data)
    end
  end

  -- 5. ランタイムの動的オーバーライドをマージする
  if override then
    deep_merge(final_cfg, override)
  end

  -- 6. (任意)スキーマ検証
  -- local ok, res = schema.validate(final_cfg) ...

  return final_cfg
end

--- Public API ---

---
-- 設定システムを初期化する
-- @param plugin_defaults table UEPのような利用側プラグインのデフォルト設定
-- @param user_cfg table ユーザーがinit.lua等で渡す設定
--
function M.setup(name, plugin_defaults, user_cfg)

  _configs[name] = {
    plugin_defaults = vim.deepcopy(plugin_defaults or {}),
    user = vim.deepcopy(user_cfg or {}),
    materialized = nil, -- マージ済み設定のキャッシュをクリア
    last_root = nil,
  }
end

---
-- 現在の有効な設定を取得する
--
function M.get(name, start_path, override)
  if not _configs[name] then
    M.setup(name, {}, {})
  end

  local stored_cfg = _configs[name]
  local current_root = start_path or vim.loop.cwd()

  if not stored_cfg.materialized or stored_cfg.last_root ~= current_root or override then
    stored_cfg.materialized = build(name, current_root, override)
    stored_cfg.last_root = current_root
  end
  
  return stored_cfg.materialized
end

---
-- 設定をファイルから再読み込みする
--
function M.reload(name, start_path)
  if _configs[name] then
    _configs[name].materialized = nil
    _configs[name].last_root = nil
  end
  return M.get(name, start_path or vim.loop.cwd())
end

---
-- 全ての設定をリセットする (主にテスト用)
--
function M.reset_single(name)
  _configs[name] = {}
end

---
-- 設定の診断情報を表示する
--
function M.diagnose(start_path)
  local cfg = M.get(start_path)
  local lines = {
    "UNL.config diagnose:",
    ("  cwd: %s"):format(vim.loop.cwd()),
    ("  start_path: %s"):format(start_path or "(nil)"),
    ("  project localrc filename: %s"):format(cfg.project.localrc_filename),
    ("  cache.dirname: %s"):format(cfg.cache.dirname), -- ★ 確認用に追加
    ("  ui.progress.mode: %s"):format(cfg.ui.progress.mode),
    ("  logging.level: %s"):format(cfg.logging.level),
  }
  return table.concat(lines, "\n")
end

return M
