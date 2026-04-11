-- Weighted stage progress aggregation (moved from UNL.progress.aggregator)
local Aggregator = {}
Aggregator.__index = Aggregator

-- ユーザーに表示されるステージ名（英語）
local STAGE_LABELS = {
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
    stages = {},
    weights = normalize(vim.deepcopy(weights or {
      scan = 0.1, direct = 0.55, transitive = 0.3, finalize = 0.05
    })),
  }, Aggregator)
end

function Aggregator:define(name, total)
  self.stages[name] = self.stages[name] or { total = 1, done = 0 }
  self.stages[name].total = math.max(1, total or 1)
end

-- function Aggregator:update(name, done)
--   local s = self.stages[name]
--   if not s then
--     self:define(name, done or 1)
--     s = self.stages[name]
--   end
--   s.done = math.min(s.total, math.max(done or s.done, 0))
-- end
function Aggregator:update(name, done, total)
  local s = self.stages[name]
  -- ステージが未定義の場合は何もしない
  if not s then return end
  -- total が提供された場合はステージの合計値を更新する（Rust 側から実際のファイル数が来る）
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

-- "Analysis: 1234/5678 (42%)" または "Discovery (5%)" 形式の文字列を返す
function Aggregator:format(name, done, total)
  local label = STAGE_LABELS[name] or name
  local pct   = self:percentage()
  local s     = self.stages[name]
  local t     = total or (s and s.total) or 0
  local d     = done  or (s and s.done)  or 0
  if t > 0 then
    return string.format("%s: %d/%d (%d%%)", label, d, t, pct)
  else
    return string.format("%s (%d%%)", label, pct)
  end
end

function Aggregator:current_stage_info()
  local active_stage = nil
  local max_done_ratio = -1
  
  -- 進行中のステージ（完了しておらず、かつ進捗があるもの）を探す
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
