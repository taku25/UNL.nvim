-- Weighted stage progress aggregation (moved from UNL.progress.aggregator)
local Aggregator = {}
Aggregator.__index = Aggregator

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
function Aggregator:update(name, done)
  local s = self.stages[name]
  -- ステージが未定義の場合は何もしない
  if not s then
    return
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

return Aggregator
