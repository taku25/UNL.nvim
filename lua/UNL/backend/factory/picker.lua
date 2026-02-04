-- lua/UNL/backend/factory/picker.lua
-- A generic factory for loading and running backend providers with fallback.

local M = {}

-- ... (M.load_providers は変更なし) ...
function M.load_providers(registry, provider_modules, opts)
  local opts = opts or {}
  local logging = require("UNL.logging")
  local log = logging.get(opts.logger_name or "UNL")
  for _, mod_name in ipairs(provider_modules) do
    local ok, provider = pcall(require, mod_name)
    if ok and provider.name then
      registry.register(provider)
    else
      log.debug("Failed to load provider module: %s", mod_name)
    end
  end
end

---
-- Tries to run a provider from a preference chain with waterfall fallback.
-- @param opts table
--   - picker_type_name (string)
--   - registry (table)
--   - conf (table) -- (変更) prefer_chain の代わりに conf を受け取る
--   - spec (table)
--   - logger_name (string)
function M.run_with_fallback(opts)
  opts = opts or {}
  local logging = require("UNL.logging")
  local log = logging.get(opts.logger_name or "UNL")

  -- ▼▼▼ ここからが新しいロジック ▼▼▼
  
  -- 1. 設定オブジェクトを取得
  local conf = opts.conf or {}
  local mode = conf.mode or "auto"
  
  -- 2. mode に基づいて、試行するプロバイダーのリストを構築
  local prefer_chain = {}
  if mode == "auto" then
    prefer_chain = conf.prefer or {}
  elseif mode ~= "none" and mode ~= "dummy" then
    -- mode で特定のプロバイダーが指定されている場合、それが唯一の候補になる
    prefer_chain = { mode }
  end

  -- ▲▲▲ 新しいロジックここまで ▲▲▲

  local success = false
  -- 3. 構築したリストを元に、フォールバックを実行 (このループ自体は変更なし)
  for _, provider_name in ipairs(prefer_chain) do
    local provider = opts.registry.get(provider_name)
    
    if provider and provider.available and provider.available() then
      log.debug("%s: Attempting to use provider '%s'...", opts.picker_type_name, provider_name)
      
      local ok, err = pcall(provider.run, opts.spec)
      
      if ok then
        log.debug("%s: Provider '%s' executed successfully.", opts.picker_type_name, provider_name)
        success = true
        break
      else
        log.warn("%s: Provider '%s' failed to run, trying next. Reason: %s", opts.picker_type_name, provider_name, tostring(err))
      end
    end
  end

  if not success then
    log.error("%s: All preferred providers failed. Falling back to dummy.", opts.picker_type_name)
    local dummy_provider = opts.registry.get("dummy")
    if dummy_provider and dummy_provider.run then
      pcall(dummy_provider.run, opts.spec)
    end
  end
end

return M
