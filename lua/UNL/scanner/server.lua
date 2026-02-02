-- lua/UNL/scanner/server.lua (Robust Shared Server Manager)
local log = require("UNL.logging").get("UNL")
local scanner_core = require("UNL.scanner")
local unl_config = require("UNL.config")
local unl_events = require("UNL.event.events")
local unl_event_types = require("UNL.event.types")

local M = {}

local server_job_id = nil
local stdout_buf = ""
local last_known_status = false
local is_starting = false -- 起動試行中フラグ

---サーバーが稼働中かどうかを即座に返す (キャッシュまたは管理下のジョブから)
function M.is_running()
  if server_job_id then return true end
  return last_known_status
end

---サーバーが稼働していることを保証し、準備ができたらcallbackを呼ぶ
function M.ensure_running(callback)
  if M.is_running() then
    if callback then callback(true) end
    return
  end

  if is_starting then
    -- 既に起動処理中なら、完了を待ってからcallbackを呼ぶためのポーリングだけ行う
    local retries = 30
    local function wait_for_boot()
      if M.is_running() then
        if callback then callback(true) end
      elseif retries > 0 then
        retries = retries - 1
        vim.defer_fn(wait_for_boot, 200)
      else
        if callback then callback(false) end
      end
    end
    wait_for_boot()
    return
  end

  -- 起動を試みる
  M.start(function(success)
    if callback then callback(success) end
  end)
end

local rpc = require("UNL.rpc")

---自身のPIDをサーバーに登録する
function M.register_self(callback)
  local pid = vim.fn.getpid()
  log.debug("Registering Neovim PID %d with UNL Server...", pid)
  
  rpc.request("ping", { pid = pid }, nil, function(success, result)
    if success then
      log.debug("Successfully registered with UNL Server: %s", result)
    end
    if callback then callback(success) end
  end)
end

function M.start(on_complete)
  if server_job_id then
    log.debug("Server is already managed by this instance (job_id: %d)", server_job_id)
    M.register_self(on_complete)
    return
  end

  if is_starting then return end
  is_starting = true

  local conf = unl_config.get("UNL").remote
  
  -- 既存のインスタンスが他で動いているかチェック
  M.get_status(function(status)
    if status and status.status == "running" then
      log.debug("UNL Server is already running on port %d. Reusing existing instance.", conf.port)
      last_known_status = true
      is_starting = false
      M.register_self(function(ok)
        if on_complete then on_complete(ok) end
      end)
      return
    end

    -- 新しく起動
    local binary = scanner_core.get_binary_path()
    if not binary then 
      is_starting = false
      if on_complete then on_complete(false) end
      return 
    end
    local server_binary = binary:gsub("unl%-scanner", "unl-server")
    if vim.fn.executable(server_binary) == 0 then
      log.warn("unl-server binary not found at: %s", server_binary)
      is_starting = false
      if on_complete then on_complete(false) end
      return
    end

    local cache_dir = vim.fn.stdpath("cache") .. "/UNL"
    if vim.fn.isdirectory(cache_dir) == 0 then vim.fn.mkdir(cache_dir, "p") end
    local registry_path = cache_dir .. "/registered_projects.json"

    local cmd = { server_binary, tostring(conf.port), registry_path }
    log.debug("Starting new UNL Server instance on port %d...", conf.port)

    stdout_buf = ""
    server_job_id = vim.fn.jobstart(cmd, {
      on_stdout = function(_, data)
        if not data then return end
        for i, line in ipairs(data) do
          if i == 1 then
            stdout_buf = stdout_buf .. line
          else
            M.process_line(stdout_buf)
            stdout_buf = line
          end
        end
      end,
      on_stderr = function(_, data)
        if data then 
          for _, line in ipairs(data) do 
            if line ~= "" then 
              -- ポート使用中エラーなどの致命的なもの以外はDEBUGに下げる
              if line:find("os error 10048") then
                log.debug("Server failed to bind (already in use): %s", line)
              else
                log.error("[Server Error] %s", line) 
              end
            end 
          end 
        end
      end,
      on_exit = function(_, code)
        log.debug("Managed UNL Server stopped with code: %d", code)
        server_job_id = nil
        last_known_status = false
        is_starting = false
      end,
    })
    
    -- 起動完了を待つ
    local retries = 20
    local function check_boot()
      M.get_status(function(s)
        if s and s.status == "running" then
          last_known_status = true
          is_starting = false
          M.register_self(function()
            if on_complete then on_complete(true) end
          end)
        elseif retries > 0 then
          retries = retries - 1
          vim.defer_fn(check_boot, 300)
        else
          is_starting = false
          if on_complete then on_complete(false) end
        end
      end)
    end
    vim.defer_fn(check_boot, 500)
  end)
end

function M.process_line(line)
  if not line or line == "" then return end
  local ok, msg = pcall(vim.json.decode, line)
  if ok and type(msg) == "table" and msg.method == "progress" then
    local p = msg.params
    if p and p.type == "progress" then
      unl_events.publish(unl_event_types.ON_SERVER_PROGRESS, p)
    end
  else
    -- 通常時はデバッグログのみ
    log.debug("[Server] %s", line)
  end
end

function M.stop()
  if server_job_id then
    vim.fn.jobstop(server_job_id)
    server_job_id = nil
  end
end

function M.get_status(callback)
  local conf = unl_config.get("UNL").remote
  local uv = vim.loop
  local client = uv.new_tcp()
  
  client:connect("127.0.0.1", conf.port, function(err)
    if not client then return end
    client:close()
    
    -- Fast event context対策: vim.scheduleを使ってメインスレッドで実行する
    vim.schedule(function()
      if err then
        last_known_status = false
        if callback then callback(nil) end
      else
        last_known_status = true
        if callback then callback({ status = "running", port = conf.port }) end
      end
    end)
  end)
end

return M
