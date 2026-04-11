-- Weighted stage progress aggregation (moved from UNL.progress.aggregator)
local Aggregator = {}
Aggregator.__index = Aggregator

-- Fallback labels used when no plan has been received from the server.
local FALLBACK_LABELS = {
  discovery  = "Discovery",
  db_sync    = "DB Sync",
  analysis   = "Analysis",
  finalizing = "Finalizing",
  file_scan  = "File Scan",
  complete   = "Complete",
}

local function normalize(weights)
  local sum = 0
  for _, v in pairs(weights) do sum = sum + v end
  if sum <= 0 then return weights end
  if math.abs(sum - 1.0) < 1e-6 then return weights end
  for k, v in pairs(weights) do weights[k] = v / sum end
  return weights
end

function Aggregator.new(weights)
  return setmetatable({
    stages  = {},
    labels  = {},   -- populated by define_from_plan or falls back to FALLBACK_LABELS
    weights = normalize(vim.deepcopy(weights or {
      discovery = 0.05, db_sync = 0.15, analysis = 0.65, finalizing = 0.15,
    })),
  }, Aggregator)
end

--- Called once when the server sends the progress_plan notification.
--- phases: array of { name, label, weight } (map-style) or {[1]=name, [2]=label, [3]=weight} (array-style from rmp_serde)
function Aggregator:define_from_plan(phases)
  local new_weights = {}
  for _, p in ipairs(phases) do
    -- rmp_serde はデフォルトで struct を配列にシリアライズするため両方に対応する
    local name   = p.name   or p[1]
    local label  = p.label  or p[2]
    local weight = p.weight or p[3]
    if name then
      new_weights[name] = weight
      self.labels[name] = label
      self:define(name, 1)
    end
  end
  self.weights = normalize(new_weights)
end

function Aggregator:define(name, total)
  self.stages[name] = self.stages[name] or { total = 1, done = 0 }
  self.stages[name].total = math.max(1, total or 1)
end

function Aggregator:update(name, done, total)
  local s = self.stages[name]
  if not s then return end
  -- Use actual total provided by the server when available.
  if total and total > 0 then
    s.total = total
  end
  s.done = math.min(s.total, math.max(done or s.done, 0))
end

function Aggregator:percentage()
  local pct = 0
  for name, st in pairs(self.stages) do
    local w = self.weights[name]
    if w and st.total > 0 then
      pct = pct + w * (st.done / st.total)
    end
  end
  return math.floor(pct * 100 + 0.5)
end

-- Returns "Analysis: 1234/5678 (42%)" or "Discovery (5%)" depending on whether
-- a meaningful count is available.
function Aggregator:format(name, done, total)
  local label = self.labels[name] or FALLBACK_LABELS[name] or name
  local pct   = self:percentage()
  local s     = self.stages[name]
  local t     = total or (s and s.total) or 0
  local d     = done  or (s and s.done)  or 0
  if t > 1 then
    return string.format("%s: %d/%d (%d%%)", label, d, t, pct)
  else
    return string.format("%s (%d%%)", label, pct)
  end
end

-- Returns format WITHOUT the trailing "(pct%)" — use when the UI (e.g. fidget)
-- already renders the percentage field separately.
function Aggregator:format_no_pct(name, done, total)
  local label = self.labels[name] or FALLBACK_LABELS[name] or name
  local s     = self.stages[name]
  local t     = total or (s and s.total) or 0
  local d     = done  or (s and s.done)  or 0
  if t > 1 then
    return string.format("%s: %d/%d", label, d, t)
  else
    return label
  end
end

function Aggregator:current_stage_info()
  local active_stage = nil

  -- Find the stage that is in progress (started but not finished).
  for name, st in pairs(self.stages) do
    local ratio = st.done / st.total
    if ratio > 0 and ratio < 1.0 then
      active_stage = { name = name, done = st.done, total = st.total, ratio = ratio }
      break
    end
  end

  return active_stage
end

return Aggregator
