local M = {}

local function _normalize(p)
  if not p or p == "" then return p end
  local ok, np = pcall(vim.fs.normalize, p)
  p = ok and np or p
  -- UNC の先頭はそのまま (//server/share) を許容
  -- バックスラッシュ統一 (normalize で大半は変わるが念のため)
  p = p:gsub("\\", "/")
  -- 末尾スラッシュ除去（ルートは残す）
  if #p > 1 then
    p = p:gsub("/+$", "")
  end
  return p
end

function M.normalize(p)
  return _normalize(p)
end

-- テスト向け比較 (返り値: boolean)
function M.equal(a, b)
  return _normalize(a) == _normalize(b)
end

-- 結合 (vim.fs.normalize に任せる前提で "/" 連結 → normalize)
function M.join(...)
  local parts = { ... }
  local raw = table.concat(parts, "/")
  return _normalize(raw)
end



-- 絶対パスを “安全な” ベースファイル名 (拡張子抜き) に変換
function M.path_to_cache_filename(p)
  local normalized_path = M.normalize(p)
  local safe_string = fn.substitute(normalized_path, '[\\/:]', '_', 'g')
  return safe_string  -- 拡張子はここでは付けない
end

-- 拡張子を冪等に .json にするヘルパ（他所でも使えるよう公開）
function M.ensure_json(name)
  if not name or name == "" then return name end
  if name:lower():sub(-5) == ".json" then
    return name
  end
  return name .. ".json"
end
return M
