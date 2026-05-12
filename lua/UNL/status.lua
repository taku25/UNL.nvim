--- UNL.status — generic statusline state provider for unl-server.
--
-- Works with any statusline plugin (lualine, heirline, feline, mini.statusline …).
-- The module maintains a background polling timer and progress event hooks so
-- that callers always see up-to-date server state with near-zero overhead.
--
-- ──────────────────────────────────────────────────────────────────────────────
-- Quick-start
-- ──────────────────────────────────────────────────────────────────────────────
--
-- lualine:
--   sections = { lualine_x = { require("UNL.status").lualine() } }
--
-- heirline:
--   { provider = require("UNL.status").get_text }
--
-- feline / any other:
--   provider = function() return require("UNL.status").get_text() end
--
-- Raw state (for fully custom rendering):
--   local st = require("UNL.status").get_state()
--   -- st.server_state   : "offline"|"idle"|"refreshing"|"scanning"|"updating"|"completing"|"querying"|"busy"
--   -- st.project_count  : number
--   -- st.active_project : string|nil
--   -- st.detail         : { refreshes, asset_scans, file_updates, completions, queries }

local M = {}

-- ──────────────────────────────────────────────────────────────────────────────
-- Internal state
-- ──────────────────────────────────────────────────────────────────────────────

local _state = {
  server_state   = "offline",
  project_count  = 0,
  active_project = nil,
  detail         = { refreshes = 0, asset_scans = 0, file_updates = 0, completions = 0, queries = 0 },
  timer          = nil,   -- uv_timer_t (excluded from get_state())
  initialized    = false,
}

local SPINNER_FRAMES = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }

local STATE_CONF = {
  offline    = { icon = "󰅖", label = ""        },
  idle       = { icon = "󱘰", label = ""        },
  refreshing = { icon = "󰑓", label = "Refresh" },
  scanning   = { icon = "󰄉", label = "Scan"    },
  updating   = { icon = "󰝩", label = "Update"  },
  completing = { icon = "⚡",  label = "Compl"  },
  querying   = { icon = "󰆧",  label = "Query"  },
  busy       = { icon = "󰄉", label = "Busy"    },
}

-- ──────────────────────────────────────────────────────────────────────────────
-- Polling
-- ──────────────────────────────────────────────────────────────────────────────

local function poll()
  local ok_srv, server_manager = pcall(require, "UNL.scanner.server")
  if not ok_srv or not server_manager.is_running() then
    _state.server_state   = "offline"
    _state.project_count  = 0
    _state.active_project = nil
    return
  end

  local ok_rpc, rpc = pcall(require, "UNL.rpc")
  if not ok_rpc then return end

  rpc.request("simple_status", {}, nil, function(success, result)
    if not success or type(result) ~= "table" then
      _state.server_state = "offline"
      return
    end
    _state.server_state   = result.state         or "idle"
    _state.project_count  = result.project_count or 0
    _state.active_project = result.active_project
    if type(result.detail) == "table" then
      _state.detail = {
        refreshes    = result.detail.refreshes    or 0,
        asset_scans  = result.detail.asset_scans  or 0,
        file_updates = result.detail.file_updates or 0,
        completions  = result.detail.completions  or 0,
        queries      = result.detail.queries      or 0,
      }
    end
  end, 5000)
end

-- ──────────────────────────────────────────────────────────────────────────────
-- Progress event hooks (instant refresh start/end, no RPC round-trip needed)
-- ──────────────────────────────────────────────────────────────────────────────

local function setup_event_hooks()
  local ok_ev, events = pcall(require, "UNL.backend.progress.events")
  if ok_ev then
    events.set_category_hook("refresh", function(ev)
      if ev.phase == "stage_update" or ev.phase == "define_from_plan" then
        if _state.server_state == "idle" or _state.server_state == "offline" then
          _state.server_state = "refreshing"
        end
      elseif ev.phase == "finish" then
        if _state.server_state == "refreshing" or _state.server_state == "busy" then
          _state.server_state = "idle"
        end
      end
    end)
  end

  local ok_t, event_types = pcall(require, "UNL.event.types")
  local ok_e, unl_events  = pcall(require, "UNL.event.events")
  if ok_t and ok_e then
    unl_events.subscribe(event_types.ON_AFTER_REFRESH_COMPLETED, function(_)
      if _state.server_state == "refreshing" or _state.server_state == "busy" then
        _state.server_state = "idle"
      end
    end)
  end
end

-- ──────────────────────────────────────────────────────────────────────────────
-- Initialisation
-- ──────────────────────────────────────────────────────────────────────────────

--- Start the background polling timer.
--- Called automatically on the first `get_text()` call, but can be invoked
--- explicitly in your plugin setup if you prefer eager initialisation.
---@param opts? { interval_ms?: integer }
function M.start(opts)
  if _state.initialized then return end
  _state.initialized = true

  local interval_ms = (opts and opts.interval_ms) or 5000
  setup_event_hooks()
  vim.defer_fn(poll, 500)

  local timer = vim.loop.new_timer()
  timer:start(interval_ms, interval_ms, vim.schedule_wrap(poll))
  _state.timer = timer
end

-- ──────────────────────────────────────────────────────────────────────────────
-- Public API
-- ──────────────────────────────────────────────────────────────────────────────

--- Returns the display string for the current server state.
--- Can be used directly as a provider with any statusline plugin.
---
---@param opts? { show_idle?: boolean, show_offline?: boolean, show_project?: boolean, interval_ms?: integer }
---@return string
function M.get_text(opts)
  M.start(opts)

  local st   = _state.server_state
  local conf = STATE_CONF[st] or STATE_CONF.offline

  if st == "idle" and not (opts and opts.show_idle) then
    return ""
  end
  if st == "offline" then
    return (opts and opts.show_offline) and conf.icon or ""
  end

  -- Spinner advances naturally as the statusline redraws (~1 s interval).
  local frame = SPINNER_FRAMES[(math.floor(vim.loop.now() / 100) % #SPINNER_FRAMES) + 1]

  local parts = { conf.icon }
  if conf.label ~= "" then
    parts[#parts + 1] = frame
    parts[#parts + 1] = conf.label
  end

  -- Append non-zero detail counts for debugging visibility.
  local d = _state.detail
  local counts = {}
  if (d.completions  or 0) > 0 then counts[#counts+1] = "Compl×" .. d.completions  end
  if (d.file_updates or 0) > 0 then counts[#counts+1] = "Upd×"   .. d.file_updates end
  if (d.queries      or 0) > 0 then counts[#counts+1] = "Qry×"   .. d.queries      end
  if (d.refreshes    or 0) > 0 then counts[#counts+1] = "Ref×"   .. d.refreshes    end
  if (d.asset_scans  or 0) > 0 then counts[#counts+1] = "Scan×"  .. d.asset_scans  end
  if #counts > 0 then
    parts[#parts+1] = "[" .. table.concat(counts, " ") .. "]"
  end

  if opts and opts.show_project and _state.active_project then
    local short = vim.fn.fnamemodify(_state.active_project, ":t")
    if short ~= "" then
      parts[#parts + 1] = "(" .. short .. ")"
    end
  end

  return table.concat(parts, " ")
end

--- Returns a plain snapshot of the current state (no userdata; safe for deepcopy).
---@return { server_state: string, project_count: integer, active_project: string|nil, detail: table, initialized: boolean }
function M.get_state()
  return {
    server_state   = _state.server_state,
    project_count  = _state.project_count,
    active_project = _state.active_project,
    detail         = vim.deepcopy(_state.detail),
    initialized    = _state.initialized,
  }
end

--- Force an immediate poll (useful after `:UNL start` / `:UNL refresh`).
function M.refresh()
  vim.schedule(poll)
end

return M
